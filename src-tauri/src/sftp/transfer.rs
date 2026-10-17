//! Recursive uploads and downloads with progress reporting and cancellation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use futures_util::{stream, StreamExt};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use serde::Serialize;
use std::future::Future;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use ts_rs::TS;

use super::replace::{LocalTarget, RemoteTarget};
use super::{file_name, is_entry, join};
use crate::error::Error;
use crate::local_name::{create_unique, create_unique_file, local_file_name, numbered};

const BUFFER_SIZE: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Default, Serialize, TS)]
#[ts(rename = "TransferProgress")]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub transferred: u64,
    pub total: u64,
    pub files_done: usize,
    pub files_total: usize,
    pub current: String,
}

/// Cancellation flags of running transfers, keyed by frontend-chosen transfer id.
#[derive(Default)]
pub struct Transfers(Mutex<HashMap<String, Arc<AtomicBool>>>);

impl Transfers {
    pub fn start(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().insert(id.to_owned(), flag.clone());
        flag
    }

    pub fn finish(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.0.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

pub struct Reporter {
    send: Box<dyn Fn(Progress) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
    progress: Progress,
    /// When progress was last sent; `None` before the first time, which is at once.
    last_emit: Option<Instant>,
}

impl Reporter {
    pub fn new(channel: Channel<Progress>, cancelled: Arc<AtomicBool>) -> Self {
        Self::with_sink(move |progress| drop(channel.send(progress)), cancelled)
    }

    /// Reports progress to `send` instead of a channel of its own.
    pub fn with_sink(send: impl Fn(Progress) + Send + Sync + 'static, cancelled: Arc<AtomicBool>) -> Self {
        Self { send: Box::new(send), cancelled, progress: Progress::default(), last_emit: None }
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!(Error::new("transfer.cancelled"));
        }
        Ok(())
    }

    // Each of these sends progress at most every `PROGRESS_INTERVAL`: thousands of small
    // files start and finish faster than the file panel shows them.
    fn start_file(&mut self, name: &str) {
        name.clone_into(&mut self.progress.current);
        self.emit_due();
    }

    fn advance(&mut self, bytes: u64) {
        self.progress.transferred += bytes;
        self.emit_due();
    }

    fn finish_file(&mut self) {
        self.progress.files_done += 1;
        self.emit_due();
    }

    fn emit_due(&mut self) {
        if self.last_emit.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
            self.emit();
        }
    }

    /// Sends the progress as it is now (when a transfer ends).
    fn emit(&mut self) {
        self.last_emit = Some(Instant::now());
        (self.send)(self.progress.clone());
    }
}

struct Plan<S, D> {
    /// Directories to create, parents before children.
    dirs: Vec<D>,
    /// Each file, where it goes and its size.
    files: Vec<(S, D, u64)>,
}

/// Files copied at once. A small file costs a few round trips (opening it, its first read or
/// its writes' acknowledgements, closing it), which copying one file after another waits for
/// in turn: 10,000 small files took over an hour from a server 100 ms away.
const CONCURRENT_FILES: usize = 8;
/// Files larger than this are copied alone: one keeps the channel's window full by itself,
/// and with others beside it the requests waiting at the server could outlast their timeout
/// on a slow link (see `channels`).
const SMALL_FILE: u64 = 256 << 10;
/// Folders listed, created or looked up at once.
const CONCURRENT_DIRS: usize = 8;

/// A transfer copying files: its reporter, shared by the files copied at once, and whether
/// one of them failed, after which no other starts.
struct Run<'a> {
    reporter: Mutex<&'a mut Reporter>,
    failed: AtomicBool,
}

impl<'a> Run<'a> {
    fn new(reporter: &'a mut Reporter) -> Self {
        Self { reporter: Mutex::new(reporter), failed: AtomicBool::new(false) }
    }

    fn check_cancelled(&self) -> Result<()> {
        self.reporter.lock().unwrap().check_cancelled()
    }

    fn start_file(&self, name: &str) {
        self.reporter.lock().unwrap().start_file(name);
    }

    fn advance(&self, bytes: u64) {
        self.reporter.lock().unwrap().advance(bytes);
    }

    fn finish_file(&self) {
        self.reporter.lock().unwrap().finish_file();
    }

    fn end(self) {
        self.reporter.into_inner().unwrap().emit();
    }
}

/// Copies the files of `sizes` with `copy_file` (given each one's index), in order, up to
/// [`CONCURRENT_FILES`] small ones at once and a large one alone. After a failure no other
/// starts, those being copied finish (as they would have before it, one at a time), and the
/// first failure is returned.
async fn copy_files<F>(sizes: &[u64], run: &Run<'_>, copy_file: impl Fn(usize) -> F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    let slots = Semaphore::new(CONCURRENT_FILES);
    let (slots, copy_file) = (&slots, &copy_file);
    // By index: closures given references make futures the compiler can't prove `Send`.
    let mut copies = stream::iter(0..sizes.len())
        .map(|i| async move {
            let weight = if sizes[i] > SMALL_FILE { CONCURRENT_FILES } else { 1 };
            let _slots = slots.acquire_many(weight as u32).await.expect("never closed");
            if run.failed.load(Ordering::Relaxed) {
                bail!(Error::new("transfer.cancelled"));
            }
            run.check_cancelled()?;
            copy_file(i).await
        })
        .buffer_unordered(CONCURRENT_FILES);
    let mut first = None;
    while let Some(result) = copies.next().await {
        if let (Err(e), None) = (result, &first) {
            run.failed.store(true, Ordering::Relaxed);
            first = Some(e);
        }
    }
    first.map_or(Ok(()), Err)
}

pub async fn upload(sftp: &SftpSession, local_paths: &[PathBuf], remote_dir: &str, reporter: &mut Reporter) -> Result<()> {
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    for local in local_paths {
        let name = local.file_name().context(Error::new("transfer.invalidPath").param("path", local.display()))?.to_string_lossy();
        let remote = join(remote_dir, &name);
        let metadata = tokio::fs::metadata(local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        if metadata.is_dir() {
            scan_local_dir(local, remote, &mut plan, reporter).await?;
        } else {
            reporter.progress.total += metadata.len();
            plan.files.push((local.clone(), remote, metadata.len()));
        }
    }
    reporter.progress.files_total = plan.files.len();

    let created = create_remote_dirs(sftp, &plan.dirs).await?;
    let run = Run::new(reporter);
    let sizes: Vec<u64> = plan.files.iter().map(|(_, _, size)| *size).collect();
    let result = copy_files(&sizes, &run, |i| {
        let (local, remote, _) = &plan.files[i];
        // In a folder this upload created, nothing is there to replace.
        let new = remote.rsplit_once('/').is_some_and(|(dir, _)| created.contains(dir));
        let run = &run;
        async move {
            run.start_file(file_name(remote));
            let mut src = tokio::fs::File::open(local).await.context(Error::new("transfer.openFailed").param("path", local.display()))?;
            let mut dst = match new {
                true => RemoteTarget::create_new(sftp, remote).await?,
                false => RemoteTarget::create(sftp, remote).await?,
            };
            let result = match copy(&mut src, &mut dst.file, run).await {
                Ok(()) => dst.finish(sftp).await,
                Err(e) => {
                    dst.abandon(sftp).await;
                    Err(e)
                }
            };
            result.map_err(|e| with_file_context(e, Error::new("transfer.uploadFailed").param("path", local.display())))?;
            run.finish_file();
            Ok(())
        }
    })
    .await;
    run.end();
    result
}

/// Creates an upload's folders, parents first and those of one depth at once; returns those
/// that weren't there.
async fn create_remote_dirs(sftp: &SftpSession, dirs: &[String]) -> Result<HashSet<String>> {
    let depth = |dir: &&String| dir.matches('/').count();
    let mut by_depth: Vec<&String> = dirs.iter().collect();
    by_depth.sort_by_key(depth);
    let mut created = HashSet::new();
    for level in by_depth.chunk_by(|a, b| depth(a) == depth(b)) {
        let results: Vec<(usize, Result<bool>)> = stream::iter(0..level.len())
            .map(|i| async move { (i, create_remote_dir(sftp, level[i]).await) })
            .buffer_unordered(CONCURRENT_DIRS)
            .collect()
            .await;
        for (i, result) in results {
            if result? {
                created.insert(level[i].clone());
            }
        }
    }
    Ok(created)
}

/// Creates `dir` unless there is a folder there already; true if there wasn't.
async fn create_remote_dir(sftp: &SftpSession, dir: &str) -> Result<bool> {
    match sftp.create_dir(dir).await {
        Ok(()) => Ok(true),
        // Most often because it is there: asked only then, one round trip less for new ones.
        Err(e) => match sftp.metadata(dir).await {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(false),
            _ => Err(anyhow::Error::from(e).context(Error::new("transfer.createDirFailed").param("path", dir))),
        },
    }
}

async fn scan_local_dir(root: &Path, remote_root: String, plan: &mut Plan<PathBuf, String>, reporter: &mut Reporter) -> Result<()> {
    let mut pending = vec![(root.to_path_buf(), remote_root)];
    while let Some((local, remote)) = pending.pop() {
        reporter.check_cancelled()?;
        plan.dirs.push(remote.clone());
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
                pending.push((path, child));
            } else if metadata.is_file() {
                reporter.progress.total += metadata.len();
                plan.files.push((path, child, metadata.len()));
            }
        }
    }
    Ok(())
}

/// Downloads into `local_dir`; top-level items get a unique name instead of overwriting
/// existing files. Returns the local paths of the top-level items.
pub async fn download(sftp: &SftpSession, remote_paths: &[String], local_dir: &Path, reporter: &mut Reporter) -> Result<Vec<PathBuf>> {
    let mut items = Vec::with_capacity(remote_paths.len());
    let mut result = Ok(());
    for remote in remote_paths {
        match claim(sftp, remote, local_dir).await {
            Ok(local) => items.push((remote.clone(), local)),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let mut done = 0;
    if result.is_ok() {
        result = download_counting(sftp, &items, reporter, &mut done).await;
    }
    if let Err(e) = result {
        // Give back the names of the items that never arrived. Those that did stay, even an
        // empty file or folder.
        for (_, local) in &items[done..] {
            let _ = match std::fs::symlink_metadata(local) {
                Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(local),
                Ok(metadata) if metadata.len() == 0 => std::fs::remove_file(local),
                _ => Ok(()),
            };
        }
        return Err(e);
    }
    Ok(items.into_iter().map(|(_, local)| local).collect())
}

/// Takes a free local name for the remote item, creating an empty file or folder there.
async fn claim(sftp: &SftpSession, remote: &str, local_dir: &Path) -> Result<PathBuf> {
    let name = local_file_name(file_name(remote)).ok_or_else(|| Error::new("transfer.invalidName").param("name", remote))?;
    let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
    let path = local_dir.join(name);
    let created = if metadata.file_type().is_dir() {
        create_unique(&path, |path| std::fs::create_dir(path))
    } else {
        create_unique_file(&path).map(|(path, _)| (path, ()))
    };
    let (path, ()) = created.context(Error::new("transfer.createFailed").param("path", path.display()))?;
    Ok(path)
}

/// Downloads each remote file or directory to the given local path, replacing files there.
pub async fn download_to(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter) -> Result<()> {
    download_counting(sftp, items, reporter, &mut 0).await
}

/// [`download_to`], keeping in `done` how many of the items, from the first, have arrived
/// whole.
async fn download_counting(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter, done: &mut usize) -> Result<()> {
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    // The items of one download go to one folder, or one disk at least.
    let mut taken = Taken::new(items.first().is_some_and(|(_, local)| ignores_case(local)));
    // Where each item's files end in `plan.files`.
    let mut ends = Vec::with_capacity(items.len());
    for (remote, local) in items {
        let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), &mut plan, &mut taken, reporter).await?;
        } else {
            let size = metadata.size.unwrap_or(0);
            reporter.progress.total += size;
            plan.files.push((remote.clone(), local.clone(), size));
        }
        ends.push(plan.files.len());
    }
    // `+=`: the items of a drag are downloaded one call at a time with the same reporter.
    reporter.progress.files_total += plan.files.len();

    // The folders the items go into (for files given a path of their own), then the remote
    // folders' copies.
    let parents = items.iter().filter_map(|(_, local)| local.parent());
    for dir in parents.chain(plan.dirs.iter().map(PathBuf::as_path)) {
        tokio::fs::create_dir_all(dir).await.context(Error::new("transfer.createDirFailed").param("path", dir.display()))?;
    }
    let clashes = clashing(&plan.files, &mut taken);
    let taken = Mutex::new(taken);
    let arrived = Mutex::new(vec![false; plan.files.len()]);
    let run = Run::new(reporter);
    let sizes: Vec<u64> = plan.files.iter().map(|(_, _, size)| *size).collect();
    let result = copy_files(&sizes, &run, |i| {
        let (remote, local, _) = &plan.files[i];
        let (clashes, taken, arrived, run) = (clashes[i], &taken, &arrived, &run);
        async move {
            run.start_file(file_name(remote));
            let mut src = sftp.open(remote).await.context(Error::new("transfer.openFailed").param("path", remote))?;
            // A file that would land where another of this download (or a folder) did, or
            // where a folder is, gets a free name instead.
            let local = if clashes || local.is_dir() {
                let free = taken.lock().unwrap().create_free_file(local);
                free.context(Error::new("transfer.createFailed").param("path", local.display()))?.0
            } else {
                local.clone()
            };
            let mut dst = LocalTarget::create(&local).await?;
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
            arrived.lock().unwrap()[i] = true;
            run.finish_file();
            Ok(())
        }
    })
    .await;
    run.end();
    let arrived = arrived.into_inner().unwrap();
    let first_missing = arrived.iter().position(|&arrived| !arrived).unwrap_or(arrived.len());
    *done = ends.partition_point(|&end| end <= first_missing);
    result
}

/// The local paths a download has taken, as the local file system tells names apart: two
/// remote names can land on one (`README` and `readme` where case is ignored; `a:b` and `a?b`
/// on Windows, where `local_file_name` makes both `a_b`).
struct Taken {
    ignores_case: bool,
    keys: HashSet<String>,
}

impl Taken {
    fn new(ignores_case: bool) -> Self {
        Self { ignores_case, keys: HashSet::new() }
    }

    /// Takes `path`; false if it was taken already.
    fn take(&mut self, path: &Path) -> bool {
        let name = path.to_string_lossy();
        self.keys.insert(if self.ignores_case { name.to_lowercase() } else { name.into_owned() })
    }

    /// Takes `path`, or `name (1)`, `name (2)`, … if it is taken.
    fn take_free(&mut self, path: PathBuf) -> PathBuf {
        (0..).map(|n| numbered(&path, n)).find(|candidate| self.take(candidate)).unwrap()
    }

    /// Creates an empty file at `path`, or at `name (1)`, `name (2)`, …, taking a name that
    /// neither a file on disk nor another of this download's files has: a later file may be
    /// called `name (1)` on the server, and would replace this one.
    fn create_free_file(&mut self, path: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
        create_unique(path, |candidate| match self.take(candidate) {
            true => std::fs::OpenOptions::new().write(true).create_new(true).open(candidate),
            false => Err(std::io::ErrorKind::AlreadyExists.into()),
        })
    }
}

/// Whether the file system `path` is on (or would be on) ignores case: macOS and Windows
/// disks usually do, but not always, and Linux can mount ones that do (exFAT, NTFS, SMB).
/// Found out with a file of our own in the nearest existing folder; when that can't be
/// written, as the system usually is.
fn ignores_case(path: &Path) -> bool {
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
fn clashing(files: &[(String, PathBuf, u64)], taken: &mut Taken) -> Vec<bool> {
    files.iter().map(|(_, local, _)| !taken.take(local)).collect()
}

/// Lists a remote folder's files and folders, recursively, into `plan`. Two folders that would
/// land on one local path (see `Taken`) get one each, rather than being merged: the second is
/// named `name (1)`, and what is in it goes there.
async fn scan_remote_dir(
    sftp: &SftpSession,
    root: String,
    local_root: PathBuf,
    plan: &mut Plan<String, PathBuf>,
    taken: &mut Taken,
    reporter: &mut Reporter,
) -> Result<()> {
    // Its name was chosen by the caller.
    taken.take(&local_root);
    // A level of the tree at a time, its folders listed at once.
    let mut level = vec![(root, local_root)];
    while !level.is_empty() {
        reporter.check_cancelled()?;
        let listings: Vec<_> = stream::iter(0..level.len())
            .map(|i| {
                let remote = &level[i].0;
                async move { sftp.read_dir(remote).await.context(Error::new("transfer.readFailed").param("path", remote)) }
            })
            .buffered(CONCURRENT_DIRS)
            .collect()
            .await;
        let mut next = Vec::new();
        for ((remote, local), listing) in level.into_iter().zip(listings) {
            plan.dirs.push(local.clone());
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
                    next.push((child, taken.take_free(local_child)));
                } else if metadata.file_type().is_file() {
                    let size = metadata.size.unwrap_or(0);
                    reporter.progress.total += size;
                    plan.files.push((child, local_child, size));
                }
            }
        }
        level = next;
    }
    Ok(())
}

async fn copy<R, W>(src: &mut R, dst: &mut W, run: &Run<'_>) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0; BUFFER_SIZE];
    loop {
        run.check_cancelled()?;
        let n = src.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).await?;
        run.advance(n as u64);
    }
    // Not flushed here: the targets do as they finish. A remote file's flush also has the
    // server sync it to disk (`fsync@openssh.com`), a round trip and a wait for every file.
    Ok(())
}

/// Adds which file failed, except for cancellation, which is reported as is.
fn with_file_context(e: anyhow::Error, context: Error) -> anyhow::Error {
    if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "transfer.cancelled") {
        e
    } else {
        e.context(context)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upload_scan_skips_directory_links() {
        let root = std::env::temp_dir().join(format!("zshell-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "abc").unwrap();
        std::os::unix::fs::symlink("..", root.join("sub/loop")).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("sub/link.txt")).unwrap();

        let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result = scan_local_dir(&root, "/r".into(), &mut plan, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        assert_eq!(plan.dirs, ["/r", "/r/sub"]);
        let mut remote: Vec<_> = plan.files.iter().map(|(_, remote, _)| remote.as_str()).collect();
        remote.sort();
        assert_eq!(remote, ["/r/sub/a.txt", "/r/sub/link.txt"]);
        assert_eq!(reporter.progress.total, 6);
    }

    /// Folders of many files, some small (copied several at once) and one large (alone), go
    /// up and come back whole, into folders that are new and one that was there, and the last
    /// progress counts them all. Skipped without `sftp-server`.
    #[tokio::test]
    async fn copies_folders_of_many_files_both_ways() {
        let Some(sftp) = super::super::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-tree-{}", uuid::Uuid::new_v4()));
        let (source, remote, back) = (root.join("tree"), root.join("remote"), root.join("back"));
        let mut expected = Vec::new();
        for dir in ["", "a", "a/b", "c"] {
            std::fs::create_dir_all(source.join(dir)).unwrap();
            for n in 0..20 {
                let path = format!("{dir}/{n}.txt");
                std::fs::write(source.join(path.trim_start_matches('/')), path.repeat(n + 1)).unwrap();
                expected.push(path);
            }
        }
        let large: Vec<u8> = (0..SMALL_FILE as usize * 3).map(|i| (i % 251) as u8).collect();
        std::fs::write(source.join("a/large.bin"), &large).unwrap();
        // Already there: its files are looked for before they are written.
        std::fs::create_dir_all(remote.join("tree/c")).unwrap();
        std::fs::create_dir_all(&back).unwrap();

        let last = Arc::new(Mutex::new(Progress::default()));
        let mut reporter = {
            let last = last.clone();
            Reporter::with_sink(move |progress| *last.lock().unwrap() = progress, Arc::default())
        };
        let up = upload(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter).await;
        let uploaded = last.lock().unwrap().clone();
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let down = download(&sftp, &[remote.join("tree").to_str().unwrap().to_owned()], &back, &mut reporter).await;
        let same = |path: &str| std::fs::read(source.join(path)).ok() == std::fs::read(back.join("tree").join(path)).ok();
        let all_same = expected.iter().all(|path| same(path.trim_start_matches('/'))) && same("a/large.bin");
        std::fs::remove_dir_all(&root).unwrap();

        up.unwrap();
        down.unwrap();
        assert_eq!((uploaded.files_done, uploaded.files_total), (81, 81));
        assert!(all_same);
    }

    /// A failed download takes back only the names of the items that did not arrive: an
    /// empty file before the failure stays. Skipped without `sftp-server`.
    #[tokio::test]
    async fn a_failed_download_keeps_the_items_that_arrived() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = super::super::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-download-{}", uuid::Uuid::new_v4()));
        let (remote, local) = (root.join("remote"), root.join("local"));
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(remote.join("empty"), "").unwrap();
        std::fs::write(remote.join("secret"), "x").unwrap();
        std::fs::set_permissions(remote.join("secret"), std::fs::Permissions::from_mode(0o000)).unwrap();
        std::fs::write(remote.join("later"), "").unwrap();

        let paths: Vec<String> = ["empty", "secret", "later"].iter().map(|n| remote.join(n).to_str().unwrap().to_owned()).collect();
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result = download(&sftp, &paths, &local, &mut reporter).await;
        let mut left: Vec<_> = std::fs::read_dir(&local).unwrap().map(|e| e.unwrap().file_name()).collect();
        left.sort();
        std::fs::remove_dir_all(&root).unwrap();

        assert!(result.is_err());
        assert_eq!(left, ["empty"]);
    }

    #[test]
    fn finds_files_that_land_on_the_same_name() {
        let files: Vec<(String, PathBuf, u64)> =
            ["/d/README", "/d/x", "/d/readme", "/d/README", "/d/Sub"].iter().map(|&r| (r.to_owned(), PathBuf::from(r), 0)).collect();
        for ignores_case in [false, true] {
            let mut taken = Taken::new(ignores_case);
            // A folder took it.
            taken.take(Path::new("/d/sub"));
            assert_eq!(clashing(&files, &mut taken), [false, false, ignores_case, true, ignores_case]);
        }
        let mut taken = Taken::new(true);
        let dirs = ["/d/Dir", "/d/dir", "/d/DIR", "/d/dir (1)"].map(|d| taken.take_free(PathBuf::from(d)));
        assert_eq!(dirs.map(|d| d.to_string_lossy().into_owned()), ["/d/Dir", "/d/dir (1)", "/d/DIR (2)", "/d/dir (1) (1)"]);
    }

    /// A file renamed because its name clashed doesn't take a name that a later file of the
    /// download has on the server (`README`, `readme`, `readme (1)` where case is ignored).
    #[test]
    fn renamed_files_keep_clear_of_the_others() {
        let dir = std::env::temp_dir().join(format!("zshell-clash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<(String, PathBuf, u64)> =
            ["README", "readme", "readme (1)"].iter().map(|&n| (n.to_owned(), dir.join(n), 0)).collect();
        let mut taken = Taken::new(true);
        assert_eq!(clashing(&files, &mut taken), [false, true, false]);
        std::fs::write(&files[0].1, "").unwrap();
        let (renamed, _) = taken.create_free_file(&files[1].1).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(renamed, dir.join("readme (2)"));
    }

    /// The temporary folder's disk is told apart from one that ignores case.
    #[test]
    fn finds_whether_a_disk_ignores_case() {
        let dir = std::env::temp_dir().join(format!("zshell-case-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), "").unwrap();
        let expected = dir.join("A").exists();
        // A path that doesn't exist yet is on its nearest folder's disk.
        assert_eq!(ignores_case(&dir.join("new/sub/file")), expected);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two remote folders that land on one local name each get their own, with what is in
    /// them. Needs a disk that tells case apart (on the server's side), and `sftp-server`.
    #[tokio::test]
    async fn folders_that_land_on_one_name_are_not_merged() {
        let Some(sftp) = super::super::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-download-dirs-{}", uuid::Uuid::new_v4()));
        let (remote, local) = (root.join("remote"), root.join("local"));
        std::fs::create_dir_all(remote.join("Dir")).unwrap();
        if remote.join("dir").exists() {
            std::fs::remove_dir_all(&root).unwrap();
            return;
        }
        std::fs::create_dir_all(remote.join("dir")).unwrap();
        std::fs::write(remote.join("Dir/f"), "upper").unwrap();
        std::fs::write(remote.join("dir/f"), "lower").unwrap();
        std::fs::create_dir_all(&local).unwrap();

        let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
        let mut taken = Taken::new(true);
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result =
            scan_remote_dir(&sftp, remote.to_str().unwrap().to_owned(), local.join("r"), &mut plan, &mut taken, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        // Which of the two is `dir (1)` depends on the order the server lists them in.
        let mut dirs: Vec<_> = plan.dirs.iter().map(|d| d.strip_prefix(&local).unwrap().to_string_lossy().to_lowercase()).collect();
        dirs.sort();
        assert_eq!(dirs, ["r", "r/dir", "r/dir (1)"]);
        // Each file follows its folder, so neither replaces the other.
        let files: HashSet<_> = plan.files.iter().map(|(_, local, _)| local.clone()).collect();
        assert_eq!(files.len(), 2);
        assert!(clashing(&plan.files, &mut taken).iter().all(|&clashes| !clashes));
    }
}
