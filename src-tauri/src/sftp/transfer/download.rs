//! Downloads: the remote files and folders are listed (each item's top-level name claimed
//! first when they go to a folder), the folders made here, then the files copied (see
//! [`copy_files`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use futures_util::{stream, StreamExt, TryStreamExt};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;

use super::left::{Left, Stamp};
use super::{
    copy, copy_files, downloaded_dir_mode, downloaded_mode, unless_cancelled, with_file_context, Plan, Reporter, Run, CONCURRENT_DIRS,
};
use crate::error::Error;
use crate::local_name::{create_dir, create_unique, create_unique_file, local_file_name, new_file, numbered};
use crate::sftp::replace::LocalTarget;
use crate::sftp::{file_name, is_entry, join};

/// Downloads into `local_dir`; top-level items get a unique name instead of overwriting
/// existing files, so everything this creates is its own. What it created goes into `left`,
/// with the local paths of the top-level items that are there.
pub async fn download(
    sftp: &SftpSession,
    remote_paths: &[String],
    local_dir: &Path,
    reporter: &mut Reporter,
    left: &mut Left<PathBuf>,
) -> Result<()> {
    let names: Vec<&str> = remote_paths.iter().map(String::as_str).collect();
    let attributes = unless_cancelled(&reporter.cancelled, stat_all(sftp, &names)).await?;
    let mut items = Vec::with_capacity(remote_paths.len());
    let mut result = Ok(());
    // One at a time, in order: which of two names that clash locally gets `name (1)` stays
    // the same from one download to the next.
    for (remote, attributes) in remote_paths.iter().zip(&attributes) {
        match claim(remote, attributes, local_dir).await {
            Ok(local) => items.push((remote.clone(), local)),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let mut whole = Vec::new();
    if result.is_ok() {
        result = download_counting(sftp, &items, attributes, reporter, &mut whole, left, true).await;
    }
    if let Err(e) = result {
        // Give back the names of the items that never arrived. Those that did stay, even an
        // empty file or folder.
        let missing = items.iter().enumerate().filter(|(i, _)| !whole.get(*i).copied().unwrap_or(false));
        for (_, (_, local)) in missing {
            let _ = match tokio::fs::symlink_metadata(local).await {
                Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir(local).await,
                Ok(metadata) if metadata.len() == 0 => tokio::fs::remove_file(local).await,
                _ => Ok(()),
            };
        }
        for (_, local) in items {
            if tokio::fs::symlink_metadata(&local).await.is_ok() {
                left.items.push(local);
            }
        }
        return Err(e);
    }
    left.items = items.into_iter().map(|(_, local)| local).collect();
    Ok(())
}

/// The attributes of each remote item, in order, asked at once: one after another, choosing
/// 500 files to download waited a minute on a server 100 ms away before the first byte.
async fn stat_all(sftp: &SftpSession, remote_paths: &[&str]) -> Result<Vec<FileAttributes>> {
    // By index, as in `copy_files`.
    stream::iter(0..remote_paths.len())
        .map(|i| {
            let remote = remote_paths[i];
            async move { sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote)) }
        })
        .buffered(CONCURRENT_DIRS)
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
/// been there.
pub async fn download_to(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter, left: &mut Left<PathBuf>) -> Result<()> {
    let names: Vec<&str> = items.iter().map(|(remote, _)| remote.as_str()).collect();
    let attributes = unless_cancelled(&reporter.cancelled, stat_all(sftp, &names)).await?;
    download_counting(sftp, items, attributes, reporter, &mut Vec::new(), left, false).await
}

/// [`download_to`], given the items' `attributes`, keeping in `whole` which of the items have
/// arrived whole: those copied at the same time as one that failed may have. With `claimed`,
/// the items' paths were free before (see [`claim`]): all that arrives under them is new,
/// folders too.
async fn download_counting(
    sftp: &SftpSession,
    items: &[(String, PathBuf)],
    attributes: Vec<FileAttributes>,
    reporter: &mut Reporter,
    whole: &mut Vec<bool>,
    left: &mut Left<PathBuf>,
    claimed: bool,
) -> Result<()> {
    let mut plan = Plan::new();
    // The items of one download go to one folder, or one disk at least.
    let first = items.first().map(|(_, local)| local.clone());
    let ignores_case = tokio::task::spawn_blocking(move || first.is_some_and(|local| ignores_case(&local))).await?;
    let mut taken = Taken::new(ignores_case);
    // Where each item's files end in `plan.files`.
    let mut ends = Vec::with_capacity(items.len());
    for ((remote, local), metadata) in items.iter().zip(attributes) {
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), downloaded_dir_mode(&metadata), &mut plan, &mut taken, reporter).await?;
        } else {
            let size = metadata.size.unwrap_or(0);
            reporter.progress.total += size;
            plan.add_file(remote.clone(), local.clone(), size, downloaded_mode(&metadata));
        }
        ends.push(plan.files.len());
    }
    reporter.progress.files_total += plan.files.len();

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
    let clashes = clashing(&plan.files, &mut taken);
    let taken = Mutex::new(taken);
    /// Where a file arrived, whether it replaced one there, and how it was left.
    #[derive(Clone)]
    struct Arrived {
        local: PathBuf,
        replaces: bool,
        stamp: Option<Stamp>,
    }
    let arrived: Mutex<Vec<Option<Arrived>>> = Mutex::new(vec![None; plan.files.len()]);
    let run = Run::new(reporter);
    let sizes: Vec<u64> = plan.files.iter().map(|(_, _, size)| *size).collect();
    let result = copy_files(&sizes, &run, |i| {
        let ((remote, local, _), mode) = (&plan.files[i], plan.file_modes[i]);
        let (clashes, taken, arrived, run) = (clashes[i], &taken, &arrived, &run);
        async move {
            run.start_file(file_name(remote));
            let mut src = sftp.open(remote).await.context(Error::new("transfer.openFailed").param("path", remote))?;
            // A file that would land where another of this download (or a folder) did, or
            // where a folder is, gets a free name instead. Looked at off the async threads:
            // the download folder may be on a network drive.
            let at_folder = || async { tokio::fs::metadata(local).await.is_ok_and(|metadata| metadata.is_dir()) };
            let local = if clashes || at_folder().await {
                let free = taken.lock().unwrap().create_free_file(local, mode);
                free.context(Error::new("transfer.createFailed").param("path", local.display()))?.0
            } else {
                local.clone()
            };
            let mut dst = LocalTarget::create(&local, mode).await?;
            let replaces = dst.replaces();
            let result = copy(&mut src, &mut dst.file, run).await;
            // Closed without waiting for the server's answer: all of it has been read.
            drop(src);
            let result = match result {
                Ok(()) => dst.finish().await,
                Err(e) => {
                    dst.abandon().await;
                    Err(e)
                }
            };
            result.map_err(|e| with_file_context(e, Error::new("transfer.downloadFailed").param("path", remote)))?;
            let stamp = tokio::fs::symlink_metadata(&local).await.ok().and_then(|metadata| Stamp::local(&metadata));
            arrived.lock().unwrap()[i] = Some(Arrived { local, replaces, stamp });
            run.finish_file();
            Ok(())
        }
    })
    .await;
    run.end();
    let arrived = arrived.into_inner().unwrap();
    *whole = items_whole(&ends, &arrived.iter().map(Option::is_some).collect::<Vec<_>>());
    for Arrived { local, replaces, stamp } in arrived.into_iter().flatten() {
        match (replaces && !claimed, stamp) {
            (true, _) => left.replaced += 1,
            (false, Some(stamp)) => left.files.push((local, stamp)),
            // Not found as it was just written: not deleted.
            (false, None) => {}
        }
    }
    if claimed {
        left.dirs = plan.dirs;
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
    plan: &mut Plan<String, PathBuf>,
    taken: &mut Taken,
    reporter: &mut Reporter,
) -> Result<()> {
    // Its name was chosen by the caller.
    taken.take(&local_root);
    // A level of the tree at a time, its folders listed at once.
    let mut level = vec![(root, local_root, root_mode)];
    while !level.is_empty() {
        reporter.check_cancelled()?;
        // A level may have thousands of folders, listed for minutes on a distant server.
        let listings = stream::iter(0..level.len())
            .map(|i| {
                let remote = &level[i].0;
                async move { sftp.read_dir(remote).await.context(Error::new("transfer.readFailed").param("path", remote)) }
            })
            .buffered(CONCURRENT_DIRS)
            .collect::<Vec<_>>();
        let listings = unless_cancelled(&reporter.cancelled, async { Ok(listings.await) }).await?;
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
            let targets: Vec<_> =
                stream::iter(links.clone()).map(|i| sftp.metadata(&entries[i].0)).buffered(CONCURRENT_DIRS).collect().await;
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
                    plan.add_file(child, local_child, size, downloaded_mode(&metadata));
                }
            }
        }
        level = next;
    }
    Ok(())
}
