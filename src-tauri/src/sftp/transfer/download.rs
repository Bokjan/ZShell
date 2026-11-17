//! Downloads: the remote files and folders are listed (each item's top-level name claimed
//! first when they go to a folder), the folders made here, then the files copied (see
//! [`copy_files`]). A failed download can be gone on with (see [`super::resume`]):
//! [`Download`] keeps what it takes, the names it claimed too.

use std::collections::HashSet;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use futures_util::{StreamExt, TryStreamExt};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::AsyncSeekExt;

use super::left::{Left, Stamp};
use super::resume::{download_from, resumable, Copied};
use super::{
    copy, copy_files, downloaded_dir_mode, downloaded_mode, is_cancelled, unless_cancelled, with_file_context, Plan, Reporter, Run,
    CONCURRENT_DIRS,
};
use crate::error::Error;
use crate::local_name::{create_dir, create_unique, create_unique_file, local_file_name, new_file, numbered};
use crate::sftp::batch::each_in_order;
use crate::sftp::replace::{LocalPartial, LocalTarget};
use crate::sftp::{file_name, is_entry, join};

/// A download, and how far it has got.
pub struct Download {
    remote_paths: Vec<String>,
    /// The folder the items go to, under names of their own (see [`claim`]); none when each was
    /// given a path ([`Download::to`]).
    folder: Option<PathBuf>,
    /// Each item and where it goes, once its name is claimed.
    items: Option<Vec<(String, PathBuf)>>,
    /// Once the remote items are listed.
    planned: Option<Planned>,
    /// Whether its large files are written so that it can be gone on with.
    resumable: bool,
    /// Whether files keep the execute bits of the remote ones, as `sftp get` gives them.
    executable: bool,
}

/// What a download copies (see [`plan`]).
struct Planned {
    plan: Plan<String, PathBuf, LocalPartial>,
    /// Where each item's files end in `plan.files`.
    ends: Vec<usize>,
    /// The local paths the plan has taken, and which of its files land on one taken before them.
    taken: Arc<Mutex<Taken>>,
    clashes: Vec<bool>,
}

impl Download {
    /// Into `folder`; top-level items get a unique name instead of overwriting existing files,
    /// so everything this creates is its own.
    pub fn into(remote_paths: Vec<String>, folder: PathBuf) -> Self {
        Self { remote_paths, folder: Some(folder), items: None, planned: None, resumable: true, executable: true }
    }

    /// Each remote file or folder to the local path given, replacing files there. Without
    /// `resumable`, a large file cut short isn't kept to go on with.
    pub fn to(items: Vec<(String, PathBuf)>, resumable: bool) -> Self {
        let remote_paths = items.iter().map(|(remote, _)| remote.clone()).collect();
        Self { remote_paths, folder: None, items: Some(items), planned: None, resumable, executable: true }
    }

    /// The local items to show once it ends: into a folder, those of the items that are there
    /// (taken from what it `left`); else each item's path, once all arrived.
    pub(super) fn paths(&self, left: &mut Left<PathBuf>, done: bool) -> Vec<PathBuf> {
        match &self.folder {
            Some(_) => std::mem::take(&mut left.items),
            None if done => self.items.iter().flatten().map(|(_, local)| local.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// Removes what the files cut short left, and gives back the names claimed for the items
    /// that never arrived, as it won't be gone on with.
    pub(super) async fn discard(mut self) {
        let mut whole = Vec::new();
        if let Some(Planned { plan, ends, .. }) = self.planned.take() {
            whole = items_whole(&ends, &plan.copied.iter().map(|copied| matches!(copied, Copied::Done(_))).collect::<Vec<_>>());
            for copied in plan.copied {
                if let Copied::Partly(partial) = copied {
                    partial.remove().await;
                }
            }
        }
        if let (Some(_), Some(items)) = (&self.folder, &self.items) {
            give_back(items, &whole).await;
        }
    }
}

/// Gives back the names claimed for the `items` not `whole` (by index; all when it is short).
/// Those that arrived stay, even an empty file or folder.
async fn give_back(items: &[(String, PathBuf)], whole: &[bool]) {
    for (i, (_, local)) in items.iter().enumerate() {
        if whole.get(i).copied().unwrap_or(false) {
            continue;
        }
        let _ = match tokio::fs::symlink_metadata(local).await {
            Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir(local).await,
            Ok(metadata) if metadata.len() == 0 => tokio::fs::remove_file(local).await,
            _ => Ok(()),
        };
    }
}

/// Downloads, or goes on with a download; what it created goes into `left`, with the local
/// paths of the top-level items that are there (into a folder). With `check`, a file copied
/// whole before is copied again unless it is there as it was left (by its size).
pub(super) async fn download(
    sftp: &SftpSession,
    job: &mut Download,
    reporter: &mut Reporter,
    left: &mut Left<PathBuf>,
    check: bool,
) -> Result<()> {
    let planned = match &mut job.planned {
        Some(planned) => {
            go_on(sftp, &mut planned.plan, reporter, check).await?;
            planned
        }
        None => {
            let attributes = unless_cancelled(&reporter.cancelled, stat_all(sftp, &job.remote_paths)).await?;
            if let (None, Some(folder)) = (&job.items, &job.folder) {
                job.items = Some(claim_all(&job.remote_paths, &attributes, folder).await?);
            }
            let items = job.items.as_deref().expect("a download is given a folder or a path for each item");
            let mut planned = plan(sftp, items, attributes, reporter).await?;
            if !job.executable {
                for mode in planned.plan.file_modes.iter_mut().flatten() {
                    *mode &= !0o111;
                }
            }
            job.planned.insert(planned)
        }
    };
    let items = job.items.as_deref().unwrap_or_default();
    let claimed = job.folder.is_some();
    let result = copy_planned(planned, items, reporter, left, claimed, job.resumable, sftp).await;
    if claimed {
        left.items.clear();
        for (_, local) in items {
            if tokio::fs::symlink_metadata(local).await.is_ok() {
                left.items.push(local.clone());
            }
        }
    }
    result
}

/// Takes a free local name for each remote item, in order: which of two names that clash
/// locally gets `name (1)` stays the same from one download to the next. If one can't be
/// taken, gives back those taken.
async fn claim_all(remote_paths: &[String], attributes: &[FileAttributes], folder: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut items = Vec::with_capacity(remote_paths.len());
    for (remote, attributes) in remote_paths.iter().zip(attributes) {
        match claim(remote, attributes, folder).await {
            Ok(local) => items.push((remote.clone(), local)),
            Err(e) => {
                give_back(&items, &[]).await;
                return Err(e);
            }
        }
    }
    Ok(items)
}

/// The attributes of each remote item, in order, asked at once: one after another, choosing
/// 500 files to download waited a minute on a server 100 ms away before the first byte.
async fn stat_all(sftp: &SftpSession, remote_paths: &[String]) -> Result<Vec<FileAttributes>> {
    each_in_order(remote_paths, CONCURRENT_DIRS, |remote| async move {
        sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))
    })
    .try_collect()
    .await
}

/// Takes a free local name for the remote item, creating an empty file or folder there.
async fn claim(remote: &str, metadata: &FileAttributes, local_dir: &Path) -> Result<PathBuf> {
    let name = local_file_name(file_name(remote)).ok_or_else(|| Error::new("transfer.invalidName").param("name", remote))?;
    let path = local_dir.join(name);
    let (is_dir, dir_mode, file_mode) = (metadata.file_type().is_dir(), downloaded_dir_mode(metadata), downloaded_mode(metadata));
    let wanted = path.clone();
    let created = tokio::task::spawn_blocking(move || match is_dir {
        true => create_unique(&wanted, |path| create_dir(path, dir_mode, false)),
        false => create_unique_file(&wanted, file_mode).map(|(path, _)| (path, ())),
    })
    .await?;
    let (path, ()) = created.context(Error::new("transfer.createFailed").param("path", path.display()))?;
    Ok(path)
}

/// Downloads each remote file or directory to the given local path, replacing files there.
/// The files it created and those it replaced go into `left`; not the folders, which may have
/// been there. For a download that isn't kept to go on with (a file to edit, or dragged out).
pub async fn download_to(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter, left: &mut Left<PathBuf>) -> Result<()> {
    download(sftp, &mut Download::to(items.to_vec(), false), reporter, left, false).await
}

/// Downloads the remote file `remote` to `local` to open it in an editor, as [`download_to`]
/// does but never executable: the system's default application for an executable file
/// without an extension (Terminal on macOS) runs it rather than opening it.
pub async fn download_to_edit(sftp: &SftpSession, remote: &str, local: &Path, reporter: &mut Reporter) -> Result<()> {
    let mut job = Download { executable: false, ..Download::to(vec![(remote.to_owned(), local.to_owned())], false) };
    download(sftp, &mut job, reporter, &mut Left::default(), false).await
}

/// Lists the remote `items` (with their `attributes`) into a plan: their files, and the
/// folders to make.
async fn plan(
    sftp: &SftpSession,
    items: &[(String, PathBuf)],
    attributes: Vec<FileAttributes>,
    reporter: &mut Reporter,
) -> Result<Planned> {
    let mut plan = Plan::new();
    // The items of one download go to one folder, or one disk at least.
    let first = items.first().map(|(_, local)| local.clone());
    let ignores_case = tokio::task::spawn_blocking(move || first.is_some_and(|local| ignores_case(&local))).await?;
    let mut taken = Taken::new(ignores_case);
    let mut ends = Vec::with_capacity(items.len());
    for ((remote, local), metadata) in items.iter().zip(attributes) {
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), downloaded_dir_mode(&metadata), &mut plan, &mut taken, reporter).await?;
        } else {
            let size = metadata.size.unwrap_or(0);
            reporter.progress.total += size;
            plan.add_file(remote.clone(), local.clone(), size, downloaded_mode(&metadata), Stamp::remote(&metadata));
        }
        ends.push(plan.files.len());
    }
    reporter.progress.files_total += plan.files.len();
    let clashes = clashing(&plan.files, &mut taken);
    Ok(Planned { plan, ends, taken: Arc::new(Mutex::new(taken)), clashes })
}

/// Readies a download's `plan` to go on with it: a file cut short whose remote file changed
/// since is copied again from the start, and so is one copied whole (if `check`) that isn't
/// there as it was left. Starts the progress where the download got.
async fn go_on(sftp: &SftpSession, plan: &mut Plan<String, PathBuf, LocalPartial>, reporter: &mut Reporter, check: bool) -> Result<()> {
    for i in 0..plan.files.len() {
        if !matches!(plan.copied[i], Copied::Partly(_)) {
            continue;
        }
        let remote = &plan.files[i].0;
        let attributes = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
        let stamp = Stamp::remote(&attributes);
        if stamp != plan.sources[i] {
            if let Copied::Partly(partial) = std::mem::replace(&mut plan.copied[i], Copied::No) {
                partial.remove().await;
            }
            (plan.files[i].2, plan.sources[i]) = (attributes.size.unwrap_or(0), stamp);
        }
    }
    if check {
        for i in 0..plan.files.len() {
            let Copied::Done(local) = &plan.copied[i] else { continue };
            let size = tokio::fs::symlink_metadata(local).await.ok().filter(|metadata| metadata.is_file()).map(|metadata| metadata.len());
            if size != Some(plan.files[i].2) {
                plan.copied[i] = Copied::No;
            }
        }
    }
    reporter.check_cancelled()?;
    reporter.progress = plan.progress();
    Ok(())
}

/// Makes the folders of `planned` and copies its files still to copy, keeping how far each
/// got. With `claimed`, the items' paths were free before (see [`claim`]): all that arrives
/// under them is new, folders too. With `resumable`, a large file cut short is kept to go on
/// with.
async fn copy_planned(
    planned: &mut Planned,
    items: &[(String, PathBuf)],
    reporter: &mut Reporter,
    left: &mut Left<PathBuf>,
    claimed: bool,
    resumable_files: bool,
    sftp: &SftpSession,
) -> Result<()> {
    let Planned { plan, taken, clashes, .. } = planned;
    // The folders the items go into (for files given a path of their own), then the remote
    // folders' copies.
    let parents = items.iter().filter_map(|(_, local)| local.parent()).map(|dir| (dir, None));
    for (dir, mode) in parents.chain(plan.dirs.iter().map(PathBuf::as_path).zip(plan.dir_modes.iter().copied())) {
        let mut builder = tokio::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            builder.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        builder.create(dir).await.context(Error::new("transfer.createDirFailed").param("path", dir.display()))?;
    }
    /// Where a file arrived, whether it replaced one there, and how it was left.
    struct Arrived {
        local: PathBuf,
        replaces: bool,
        stamp: Option<Stamp>,
    }
    let arrived: Mutex<Vec<Arrived>> = Mutex::new(Vec::new());
    let to_copy = plan.to_copy();
    let copied = Mutex::new(std::mem::take(&mut plan.copied));
    let run = Run::new(reporter);
    let sizes: Vec<u64> = to_copy.iter().map(|&i| plan.files[i].2).collect();
    let (plan, taken, clashes) = (&*plan, &*taken, &*clashes);
    let result = copy_files(&sizes, &run, |n| {
        let i = to_copy[n];
        let ((remote, local, size), mode) = (&plan.files[i], plan.file_modes[i]);
        let (clashes, arrived, copied, run, size) = (clashes[i], &arrived, &copied, &run, *size);
        let failed = move |e| with_file_context(e, Error::new("transfer.downloadFailed").param("path", remote));
        async move {
            run.start_file(file_name(remote));
            let mut src = sftp.open(remote).await.context(Error::new("transfer.openFailed").param("path", remote))?;
            let aside = resumable_files && resumable(size);
            let partial = match std::mem::replace(&mut copied.lock().unwrap()[i], Copied::No) {
                Copied::Partly(partial) => Some(partial),
                _ => None,
            };
            // Where it went before, once it was given a name of its own.
            let named = partial.as_ref().map(|partial| partial.path.clone());
            let resumed = match partial {
                Some(partial) => {
                    let kept = partial.clone();
                    match LocalTarget::resume(partial, |written| download_from(written, size)).await {
                        Ok(resumed) => resumed,
                        Err(e) => {
                            copied.lock().unwrap()[i] = Copied::Partly(kept);
                            return Err(failed(e));
                        }
                    }
                }
                None => None,
            };
            let (local, mut dst, from) = match (resumed, named) {
                (Some((dst, from)), Some(named)) => (named, dst, from),
                (None, Some(named)) => {
                    let dst = LocalTarget::create(&named, mode, aside).await?;
                    (named, dst, 0)
                }
                (_, None) => {
                    // A file that would land where another of this download (or a folder) did,
                    // or where a folder is, gets a free name instead. Looked at off the async
                    // threads: the download folder may be on a network drive.
                    let at_folder = || async { tokio::fs::metadata(local).await.is_ok_and(|metadata| metadata.is_dir()) };
                    let local = if clashes || at_folder().await {
                        let (taken, path) = (taken.clone(), local.clone());
                        let free = tokio::task::spawn_blocking(move || taken.lock().unwrap().create_free_file(&path, mode)).await?;
                        free.context(Error::new("transfer.createFailed").param("path", local.display()))?.0
                    } else {
                        local.clone()
                    };
                    let dst = LocalTarget::create(&local, mode, aside).await?;
                    (local, dst, 0)
                }
            };
            let replaces = dst.replaces();
            let written = async {
                if from > 0 {
                    src.seek(SeekFrom::Start(from)).await.context(Error::new("transfer.readFailed").param("path", remote))?;
                    run.advance(from);
                }
                copy(&mut src, &mut dst.file, run).await
            };
            let result = written.await;
            // Closed without waiting for the server's answer: all of it has been read.
            drop(src);
            let result = match result {
                Ok(()) => dst.finish().await,
                Err(e) if aside && !is_cancelled(&e) => {
                    if let Some(partial) = dst.suspend().await {
                        copied.lock().unwrap()[i] = Copied::Partly(partial);
                    }
                    Err(e)
                }
                Err(e) => {
                    dst.abandon().await;
                    Err(e)
                }
            };
            result.map_err(failed)?;
            let stamp = tokio::fs::symlink_metadata(&local).await.ok().and_then(|metadata| Stamp::local(&metadata));
            copied.lock().unwrap()[i] = Copied::Done(local.clone());
            arrived.lock().unwrap().push(Arrived { local, replaces, stamp });
            run.finish_file();
            Ok(())
        }
    })
    .await;
    run.end();
    planned.plan.copied = copied.into_inner().unwrap();
    let mut files = Vec::new();
    for Arrived { local, replaces, stamp } in arrived.into_inner().unwrap() {
        match (replaces && !claimed, stamp) {
            (true, _) => left.replaced += 1,
            (false, Some(stamp)) => files.push((local, stamp)),
            // Not found as it was just written: not deleted.
            (false, None) => {}
        }
    }
    left.add_files(files);
    if claimed {
        left.dirs.clone_from(&planned.plan.dirs);
    }
    result
}
/// Which items arrived whole, from where each item's files end (`ends`) and which files
/// `arrived`: all of an item's (none for an empty folder).
pub(super) fn items_whole(ends: &[usize], arrived: &[bool]) -> Vec<bool> {
    let starts = std::iter::once(0).chain(ends.iter().copied());
    starts.zip(ends).map(|(start, &end)| arrived[start..end].iter().all(|&arrived| arrived)).collect()
}

/// The local paths a download has taken, as the local file system tells names apart: two
/// remote names can land on one (`README` and `readme` where case is ignored; `a:b` and `a?b`
/// on Windows, where `local_file_name` makes both `a_b`).
pub(super) struct Taken {
    ignores_case: bool,
    keys: HashSet<String>,
}

impl Taken {
    pub(super) fn new(ignores_case: bool) -> Self {
        Self { ignores_case, keys: HashSet::new() }
    }

    /// Takes `path`; false if it was taken already.
    pub(super) fn take(&mut self, path: &Path) -> bool {
        let name = path.to_string_lossy();
        self.keys.insert(if self.ignores_case { name.to_lowercase() } else { name.into_owned() })
    }

    /// Takes `path`, or `name (1)`, `name (2)`, … if it is taken.
    pub(super) fn take_free(&mut self, path: PathBuf) -> PathBuf {
        (0..).map(|n| numbered(&path, n)).find(|candidate| self.take(candidate)).unwrap()
    }

    /// Creates an empty file at `path`, or at `name (1)`, `name (2)`, …, taking a name that
    /// neither a file on disk nor another of this download's files has: a later file may be
    /// called `name (1)` on the server, and would replace this one.
    pub(super) fn create_free_file(&mut self, path: &Path, mode: Option<u32>) -> std::io::Result<(PathBuf, std::fs::File)> {
        create_unique(path, |candidate| match self.take(candidate) {
            true => new_file(mode).open(candidate),
            false => Err(std::io::ErrorKind::AlreadyExists.into()),
        })
    }
}

/// Whether the file system `path` is on (or would be on) ignores case: macOS and Windows
/// disks usually do, but not always, and Linux can mount ones that do (exFAT, NTFS, SMB).
/// Found out with a file of our own in the nearest existing folder; when that can't be
/// written, as the system usually is.
pub(super) fn ignores_case(path: &Path) -> bool {
    let usual = cfg!(any(windows, target_os = "macos"));
    let Some(dir) = path.ancestors().skip(1).find(|dir| dir.is_dir()) else {
        return usual;
    };
    let name = format!(".zshell-case-{}", uuid::Uuid::new_v4().simple());
    let probe = dir.join(&name);
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_err() {
        return usual;
    }
    let ignores = dir.join(name.to_uppercase()).exists();
    let _ = std::fs::remove_file(&probe);
    ignores
}

/// Which of a download's files would land on a path taken before them, by a folder or a file.
pub(super) fn clashing(files: &[(String, PathBuf, u64)], taken: &mut Taken) -> Vec<bool> {
    files.iter().map(|(_, local, _)| !taken.take(local)).collect()
}

/// Lists a remote folder's files and folders, recursively, into `plan`. Two folders that would
/// land on one local path (see `Taken`) get one each, rather than being merged: the second is
/// named `name (1)`, and what is in it goes there.
pub(super) async fn scan_remote_dir(
    sftp: &SftpSession,
    root: String,
    local_root: PathBuf,
    root_mode: Option<u32>,
    plan: &mut Plan<String, PathBuf, LocalPartial>,
    taken: &mut Taken,
    reporter: &mut Reporter,
) -> Result<()> {
    // Its name was chosen by the caller.
    taken.take(&local_root);
    // A level of the tree at a time, its folders listed at once.
    let mut level = vec![(root, local_root, root_mode)];
    while !level.is_empty() {
        reporter.check_cancelled()?;
        // A level may have thousands of folders, listed for minutes on a distant server, so
        // none starts once the transfer is cancelled. Those started are left to finish: one
        // dropped midway would leave the folder open at the server, which russh-sftp closes
        // only once the listing ends.
        let cancelled = &reporter.cancelled;
        let listings: Vec<_> = each_in_order(&level, CONCURRENT_DIRS, |(remote, _, _)| async move {
            if cancelled.is_cancelled() {
                bail!(Error::new("transfer.cancelled"));
            }
            sftp.read_dir(remote).await.context(Error::new("transfer.readFailed").param("path", remote))
        })
        .collect()
        .await;
        reporter.check_cancelled()?;
        let mut next = Vec::new();
        for ((remote, local, mode), listing) in level.into_iter().zip(listings) {
            plan.add_dir(local.clone(), mode);
            let mut entries = Vec::new();
            for entry in listing? {
                let name = entry.file_name();
                if !is_entry(&name) {
                    continue;
                }
                // The server chooses the names: one that is not a plain file name here (`../x`,
                // or `a\\b` on Windows) must not lead outside the folder.
                let Some(local_name) = local_file_name(&name) else {
                    continue;
                };
                entries.push((join(&remote, &name), local.join(&local_name), entry.metadata()));
            }
            // Follow symlinks to files only, so a link cycle can't recurse forever.
            let links: Vec<usize> = (0..entries.len()).filter(|&i| entries[i].2.file_type().is_symlink()).collect();
            let targets = each_in_order(&links, CONCURRENT_DIRS, |&i| sftp.metadata(&entries[i].0)).collect::<Vec<_>>();
            let targets = unless_cancelled(&reporter.cancelled, async { Ok(targets.await) }).await?;
            for (i, target) in links.into_iter().zip(targets) {
                entries[i].2 = match target {
                    Ok(target) if target.file_type().is_file() => target,
                    // Neither a file nor a folder: skipped.
                    _ => FileAttributes::empty(),
                };
            }
            for (child, local_child, metadata) in entries {
                if metadata.file_type().is_dir() {
                    next.push((child, taken.take_free(local_child), downloaded_dir_mode(&metadata)));
                } else if metadata.file_type().is_file() {
                    let size = metadata.size.unwrap_or(0);
                    reporter.progress.total += size;
                    plan.add_file(child, local_child, size, downloaded_mode(&metadata), Stamp::remote(&metadata));
                }
            }
        }
        level = next;
    }
    Ok(())
}
