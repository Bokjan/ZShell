//! Uploads: the local files and folders are listed, the folders made on the server, then
//! the files copied (see [`copy_files`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio_util::sync::CancellationToken;

use super::left::{self, Left};
use super::{copy, copy_files, uploaded_mode, with_file_context, Plan, Reporter, Run, CONCURRENT_DIRS, CREATE_BATCH};
use crate::error::Error;
use crate::sftp::batch::{each, levels, Order};
use crate::sftp::replace::RemoteTarget;
use crate::sftp::{file_name, join};

/// Uploads into `remote_dir`, replacing files there; what it created goes into `left`.
pub async fn upload(
    sftp: &SftpSession,
    local_paths: &[PathBuf],
    remote_dir: &str,
    reporter: &mut Reporter,
    left: &mut Left<String>,
) -> Result<()> {
    let mut plan = Plan::new();
    for local in local_paths {
        let name = local.file_name().context(Error::new("transfer.invalidPath").param("path", local.display()))?.to_string_lossy();
        let remote = join(remote_dir, &name);
        let metadata = tokio::fs::metadata(local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        if metadata.is_dir() {
            scan_local_dir(local, remote, &mut plan, reporter).await?;
        } else {
            reporter.progress.total += metadata.len();
            plan.add_file(local.clone(), remote, metadata.len(), uploaded_mode(&metadata));
        }
    }
    reporter.progress.files_total = plan.files.len();

    let mut created = HashSet::new();
    let made = create_remote_dirs(sftp, &plan.dirs, &plan.dir_modes, &mut created, &reporter.cancelled).await;
    left.dirs = created.iter().cloned().collect();
    made?;
    let turns = same_name_turns(&plan.files);
    // The files it created, and how many it replaced.
    let files = Mutex::new((Vec::new(), 0));
    let run = Run::new(reporter);
    let sizes: Vec<u64> = plan.files.iter().map(|(_, _, size)| *size).collect();
    let result = copy_files(&sizes, &run, |i| {
        let ((local, remote, _), mode) = (&plan.files[i], plan.file_modes[i]);
        let (run, turn, files) = (&run, turns.get(&remote.to_lowercase()), &files);
        // In a folder this upload created, nothing is there to replace, unless another file of
        // the same name went there first.
        let new = turn.is_none() && remote.rsplit_once('/').is_some_and(|(dir, _)| created.contains(dir));
        async move {
            // After the file of the same name before it, which this one replaces.
            let _turn = match turn {
                Some(turn) => Some(turn.lock().await),
                None => None,
            };
            run.start_file(file_name(remote));
            let mut src = tokio::fs::File::open(local).await.context(Error::new("transfer.openFailed").param("path", local.display()))?;
            let mut dst = match new {
                true => RemoteTarget::create_new(sftp, remote, mode).await?,
                false => RemoteTarget::create(sftp, remote, mode).await?,
            };
            let replaces = dst.replaces();
            let result = match copy(&mut src, &mut dst.file, run).await {
                Ok(()) => dst.finish(sftp).await,
                Err(e) => {
                    dst.abandon(sftp).await;
                    Err(e)
                }
            };
            result.map_err(|e| with_file_context(e, Error::new("transfer.uploadFailed").param("path", local.display())))?;
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
    let (files, replaced) = files.into_inner().unwrap();
    (left.files, left.replaced) = (left::stamp_remote(sftp, files).await, replaced);
    result
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
    plan: &mut Plan<PathBuf, String>,
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
                plan.add_file(path, child, metadata.len(), uploaded_mode(&metadata));
            }
        }
    }
    Ok(())
}
