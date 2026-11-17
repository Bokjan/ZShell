//! Uploads: the local files and folders are listed, the folders made on the server, then
//! the files copied (see [`copy_files`]). A failed upload can be gone on with (see
//! [`super::resume`]): [`Upload`] keeps what it takes.

use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::AsyncSeekExt;
use tokio_util::sync::CancellationToken;

use super::left::{self, Left, Server, Stamp};
use super::resume::{resumable, upload_from, Copied};
use super::{copy, copy_files, is_cancelled, uploaded_mode, with_file_context, Plan, Reporter, Run, CONCURRENT_DIRS, CREATE_BATCH};
use crate::error::Error;
use crate::sftp::batch::{each, each_in_order, levels, Order};
use crate::sftp::replace::{RemotePartial, RemoteTarget};
use crate::sftp::{file_name, join};

/// An upload into a remote folder, replacing files there, and how far it has got.
pub struct Upload {
    local_paths: Vec<PathBuf>,
    remote_dir: String,
    /// The server it goes to: it is gone on with there only.
    pub(super) server: Server,
    /// Once the local files are listed.
    plan: Option<Plan<PathBuf, String, RemotePartial>>,
    /// The folders it created, where nothing was there to replace.
    created: HashSet<String>,
    /// The files it created that couldn't be looked up once copied (the connection lost
    /// meanwhile), to be when it goes on, so that they can be deleted too.
    unstamped: Vec<String>,
}

impl Upload {
    pub fn new(local_paths: Vec<PathBuf>, remote_dir: String, server: Server) -> Self {
        Self { local_paths, remote_dir, server, plan: None, created: HashSet::new(), unstamped: Vec::new() }
    }

    /// Removes what the files cut short left, through `sftp` (on the server it went to), as it
    /// won't be gone on with; left there without one.
    pub(super) async fn discard(self, sftp: Option<&SftpSession>) {
        let (Some(sftp), Some(plan)) = (sftp, self.plan) else { return };
        for copied in plan.copied {
            if let Copied::Partly(partial) = copied {
                partial.remove(sftp).await;
            }
        }
    }
}

/// Uploads, or goes on with an upload; what it created goes into `left`. With `check`, a file
/// copied whole before is copied again unless it is there as it was left (by its size).
pub(super) async fn upload(
    sftp: &SftpSession,
    job: &mut Upload,
    reporter: &mut Reporter,
    left: &mut Left<String>,
    check: bool,
) -> Result<()> {
    let plan = match &mut job.plan {
        Some(plan) => {
            go_on(sftp, plan, reporter, check).await?;
            plan
        }
        None => {
            let mut plan = Plan::new();
            for local in &job.local_paths {
                let name = local.file_name().context(Error::new("transfer.invalidPath").param("path", local.display()))?.to_string_lossy();
                let remote = join(&job.remote_dir, &name);
                let metadata =
                    tokio::fs::metadata(local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
                if metadata.is_dir() {
                    scan_local_dir(local, remote, &mut plan, reporter).await?;
                } else {
                    reporter.progress.total += metadata.len();
                    plan.add_file(local.clone(), remote, metadata.len(), uploaded_mode(&metadata), Stamp::local(&metadata));
                }
            }
            reporter.progress.files_total = plan.files.len();
            job.plan.insert(plan)
        }
    };

    let mut created = HashSet::new();
    let made = create_remote_dirs(sftp, &plan.dirs, &plan.dir_modes, &mut created, &reporter.cancelled).await;
    left.add_dirs(created.iter().cloned());
    job.created.extend(created);
    made?;
    let turns = same_name_turns(&plan.files);
    // The files it created, and how many it replaced.
    let files = Mutex::new((Vec::new(), 0));
    let to_copy = plan.to_copy();
    let copied = Mutex::new(std::mem::take(&mut plan.copied));
    let run = Run::new(reporter);
    let sizes: Vec<u64> = to_copy.iter().map(|&i| plan.files[i].2).collect();
    let (created, plan) = (&job.created, &*plan);
    let result = copy_files(&sizes, &run, |n| {
        let i = to_copy[n];
        let ((local, remote, size), mode) = (&plan.files[i], plan.file_modes[i]);
        let (run, turn, files, copied, size) = (&run, turns.get(&remote.to_lowercase()), &files, &copied, *size);
        // In a folder this upload created, nothing is there to replace, unless another file of
        // the same name went there first.
        let new = turn.is_none() && remote.rsplit_once('/').is_some_and(|(dir, _)| created.contains(dir));
        let failed = move |e| with_file_context(e, Error::new("transfer.uploadFailed").param("path", local.display()));
        async move {
            // After the file of the same name before it, which this one replaces.
            let _turn = match turn {
                Some(turn) => Some(turn.lock().await),
                None => None,
            };
            run.start_file(file_name(remote));
            let mut src = tokio::fs::File::open(local).await.context(Error::new("transfer.openFailed").param("path", local.display()))?;
            let aside = resumable(size);
            let partial = match std::mem::replace(&mut copied.lock().unwrap()[i], Copied::No) {
                Copied::Partly(partial) => Some(partial),
                _ => None,
            };
            let resumed = match partial {
                Some(partial) => {
                    let kept = partial.clone();
                    match RemoteTarget::resume(sftp, partial, |written| upload_from(written, size)).await {
                        Ok(resumed) => resumed,
                        Err(e) => {
                            copied.lock().unwrap()[i] = Copied::Partly(kept);
                            return Err(failed(e));
                        }
                    }
                }
                None => None,
            };
            let (mut dst, from) = match resumed {
                Some(resumed) => resumed,
                None if new => (RemoteTarget::create_new(sftp, remote, mode, aside).await?, 0),
                None => (RemoteTarget::create(sftp, remote, mode, aside).await?, 0),
            };
            let replaces = dst.replaces();
            let written = async {
                if from > 0 {
                    src.seek(SeekFrom::Start(from)).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
                    run.advance(from);
                }
                copy(&mut src, &mut dst.file, run).await
            };
            let result = match written.await {
                // The last writes (`TRANSFER_WRITES_IN_FLIGHT`) are only acknowledged as it
                // closes: a file cut short then is gone on with too.
                Ok(()) => match dst.finish_or_suspend(sftp, aside).await {
                    Ok(()) => Ok(()),
                    Err((e, partial)) => {
                        if let Some(partial) = partial {
                            copied.lock().unwrap()[i] = Copied::Partly(partial);
                        }
                        Err(e)
                    }
                },
                Err(e) if aside && !is_cancelled(&e) => {
                    if let Some(partial) = dst.suspend(sftp).await {
                        copied.lock().unwrap()[i] = Copied::Partly(partial);
                    }
                    Err(e)
                }
                Err(e) => {
                    dst.abandon(sftp).await;
                    Err(e)
                }
            };
            result.map_err(failed)?;
            copied.lock().unwrap()[i] = Copied::Done(remote.clone());
            let mut files = files.lock().unwrap();
            // A file of the same name that this upload put there before is its own still: only
            // then looked for, as going through all of them for each replaced file adds up.
            if !replaces {
                files.0.push(remote.clone());
            } else if turn.is_none() || !files.0.contains(remote) {
                files.1 += 1;
            }
            drop(files);
            run.finish_file();
            Ok(())
        }
    })
    .await;
    run.end();
    let plan = job.plan.as_mut().expect("planned above");
    plan.copied = copied.into_inner().unwrap();
    let (mut files, replaced) = files.into_inner().unwrap();
    files.append(&mut job.unstamped);
    let stamped = left::stamp_remote(sftp, files.clone()).await;
    let found: HashSet<&String> = stamped.iter().map(|(file, _)| file).collect();
    job.unstamped = files.into_iter().filter(|file| !found.contains(file)).collect();
    left.add_files(stamped);
    left.replaced += replaced;
    result
}

/// Readies an upload's `plan` to go on with it: a file cut short whose local file changed
/// since is copied again from the start, and so is one copied whole (if `check`) that isn't
/// there as it was left. Starts the progress where the upload got.
async fn go_on(sftp: &SftpSession, plan: &mut Plan<PathBuf, String, RemotePartial>, reporter: &mut Reporter, check: bool) -> Result<()> {
    for i in 0..plan.files.len() {
        if !matches!(plan.copied[i], Copied::Partly(_)) {
            continue;
        }
        let local = &plan.files[i].0;
        let metadata = tokio::fs::metadata(local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        let stamp = Stamp::local(&metadata);
        if stamp != plan.sources[i] {
            if let Copied::Partly(partial) = std::mem::replace(&mut plan.copied[i], Copied::No) {
                partial.remove(sftp).await;
            }
            (plan.files[i].2, plan.sources[i]) = (metadata.len(), stamp);
        }
    }
    if check {
        let done: Vec<usize> = (0..plan.files.len()).filter(|&i| matches!(plan.copied[i], Copied::Done(_))).collect();
        let files = &plan.files;
        let sizes: Vec<Option<u64>> = each_in_order(&done, left::CONCURRENT_STAMPS, |&i| async move {
            sftp.symlink_metadata(&files[i].1).await.ok().and_then(|attributes| attributes.size)
        })
        .collect()
        .await;
        for (i, size) in done.into_iter().zip(sizes) {
            if size != Some(plan.files[i].2) {
                plan.copied[i] = Copied::No;
            }
        }
    }
    reporter.check_cancelled()?;
    reporter.progress = plan.progress();
    Ok(())
}

/// The remote paths that more than one file of an upload goes to (the same name dropped from
/// different folders, or differing only in case, which a server may ignore), each with a lock
/// that has them copied one at a time, in order: copied at once, they would be written over
/// each other through two handles, and the file would be neither.
pub(super) fn same_name_turns(files: &[(PathBuf, String, u64)]) -> HashMap<String, tokio::sync::Mutex<()>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, remote, _) in files {
        *counts.entry(remote.to_lowercase()).or_default() += 1;
    }
    counts.into_iter().filter(|(_, count)| *count > 1).map(|(remote, _)| (remote, tokio::sync::Mutex::new(()))).collect()
}

/// Creates an upload's folders, parents first and those of one depth at once, with the
/// permission bits `modes` gives each; those that weren't there go into `created`, also when
/// another fails. Once `cancelled`, stops after those being created: each of them is noted,
/// to be deleted with the rest of what the upload left.
async fn create_remote_dirs(
    sftp: &SftpSession,
    dirs: &[String],
    modes: &[Option<u32>],
    created: &mut HashSet<String>,
    cancelled: &CancellationToken,
) -> Result<()> {
    let mut by_depth: Vec<(&String, Option<u32>)> = dirs.iter().zip(modes.iter().copied()).collect();
    // A level may have thousands of folders: a batch of them at a time.
    for level in levels(&mut by_depth, |dir| dir.0, Order::ParentsFirst).flat_map(|level| level.chunks(CREATE_BATCH)) {
        if cancelled.is_cancelled() {
            bail!(Error::new("transfer.cancelled"));
        }
        let results: Vec<(&String, Result<bool>)> =
            each(level, CONCURRENT_DIRS, |&(dir, mode)| async move { (dir, create_remote_dir(sftp, dir, mode).await) }).collect().await;
        let mut first = Ok(());
        for (dir, result) in results {
            match result {
                Ok(true) => drop(created.insert(dir.clone())),
                Ok(false) => {}
                Err(e) => first = first.and(Err(e)),
            }
        }
        first?;
    }
    Ok(())
}

/// Creates `dir` unless there is a folder there already; true if there wasn't. A new one gets
/// the local folder's permission bits `mode`, with the owner's write and search (which the
/// upload needs), as `sftp put -r` gives them; set once it is made, as russh-sftp makes
/// folders without any.
async fn create_remote_dir(sftp: &SftpSession, dir: &str, mode: Option<u32>) -> Result<bool> {
    match sftp.create_dir(dir).await {
        Ok(()) => {
            if let Some(mode) = mode {
                let attributes = FileAttributes { permissions: Some(mode | 0o300), ..FileAttributes::empty() };
                let _ = sftp.set_metadata(dir, attributes).await;
            }
            Ok(true)
        }
        // Most often because it is there: asked only then, one round trip less for new ones.
        Err(e) => match sftp.metadata(dir).await {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(false),
            _ => Err(anyhow::Error::from(e).context(Error::new("transfer.createDirFailed").param("path", dir))),
        },
    }
}

pub(super) async fn scan_local_dir(
    root: &Path,
    remote_root: String,
    plan: &mut Plan<PathBuf, String, RemotePartial>,
    reporter: &mut Reporter,
) -> Result<()> {
    let root_mode = tokio::fs::metadata(root).await.ok().and_then(|metadata| uploaded_mode(&metadata));
    let mut pending = vec![(root.to_path_buf(), remote_root, root_mode)];
    while let Some((local, remote, mode)) = pending.pop() {
        reporter.check_cancelled()?;
        plan.add_dir(remote.clone(), mode);
        let mut entries = tokio::fs::read_dir(&local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let child = join(&remote, &entry.file_name().to_string_lossy());
            // Follow symlinks to files only, as downloads do, so a link back up the tree
            // (`loop -> ..`, Wine's `z: -> /`) can't walk the whole disk. Skip anything that
            // is neither a file nor a directory.
            let Ok(file_type) = entry.file_type().await else { continue };
            let Ok(metadata) = tokio::fs::metadata(&path).await else { continue };
            if file_type.is_symlink() && !metadata.is_file() {
                continue;
            }
            if metadata.is_dir() {
                pending.push((path, child, uploaded_mode(&metadata)));
            } else if metadata.is_file() {
                reporter.progress.total += metadata.len();
                plan.add_file(path, child, metadata.len(), uploaded_mode(&metadata), Stamp::local(&metadata));
            }
        }
    }
    Ok(())
}
