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

mod left;

pub use left::{delete_local, delete_remote, Deletion, Ended, Kept, Left, Server, Stamp};

use super::replace::{LocalTarget, RemoteTarget};
use super::{file_name, is_entry, join};
use crate::error::Error;
use crate::local_name::{create_dir, create_unique, create_unique_file, local_file_name, new_file, numbered};

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

/// Cancellation flags of running transfers, and what ended ones left (see [`left`]), by the
/// webview that started each and the id its page chose. A transfer belongs to its page: only
/// that page cancels it or deletes what it left, and when the page is gone (reloaded, or its
/// window closed), with nobody left to show it, it is cancelled and what it left forgotten.
#[derive(Default)]
pub struct Transfers {
    running: Mutex<HashMap<(String, String), Arc<AtomicBool>>>,
    left: Mutex<HashMap<(String, String), Kept>>,
}

impl Transfers {
    pub fn start(&self, webview: &str, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.running.lock().unwrap().insert((webview.to_owned(), id.to_owned()), flag.clone());
        flag
    }

    pub fn finish(&self, webview: &str, id: &str) {
        self.running.lock().unwrap().remove(&(webview.to_owned(), id.to_owned()));
    }

    pub fn cancel(&self, webview: &str, id: &str) {
        if let Some(flag) = self.running.lock().unwrap().get(&(webview.to_owned(), id.to_owned())) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Cancels the transfers of `webview`, whose page is gone, and forgets what its ended ones
    /// left.
    pub fn cancel_webview(&self, webview: &str) {
        self.running.lock().unwrap().retain(|(owner, _), flag| {
            let theirs = owner == webview;
            if theirs {
                flag.store(true, Ordering::Relaxed);
            }
            !theirs
        });
        self.left.lock().unwrap().retain(|(owner, _), _| owner != webview);
    }

    /// Keeps what an ended transfer left, unless there is nothing to delete.
    pub fn keep(&self, webview: &str, id: &str, kept: Kept) {
        if !kept.is_empty() {
            self.left.lock().unwrap().insert((webview.to_owned(), id.to_owned()), kept);
        }
    }

    /// Takes what a transfer left, to delete it; what can't be is kept again.
    pub fn take(&self, webview: &str, id: &str) -> Option<Kept> {
        self.left.lock().unwrap().remove(&(webview.to_owned(), id.to_owned()))
    }

    /// Forgets what the transfers `ids` left, which the list no longer shows.
    pub fn forget(&self, webview: &str, ids: &[String]) {
        let mut left = self.left.lock().unwrap();
        for id in ids {
            left.remove(&(webview.to_owned(), id.clone()));
        }
    }
}

/// What the transfers on one connection copy files at once by (see [`copy_files`]).
pub struct TransferSlots(Semaphore);

impl Default for TransferSlots {
    fn default() -> Self {
        Self(Semaphore::new(CONCURRENT_FILES))
    }
}

pub struct Reporter {
    send: Box<dyn Fn(Progress) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
    /// Shared with the connection's other transfers ([`Reporter::sharing`]), else its own.
    slots: Arc<TransferSlots>,
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
        Self { send: Box::new(send), cancelled, slots: Arc::default(), progress: Progress::default(), last_emit: None }
    }

    /// Copies files at once by `slots`, those of the connection's other transfers too.
    pub fn sharing(self, slots: Arc<TransferSlots>) -> Self {
        Self { slots, ..self }
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
    /// The permission bits each of `dirs` is created with, where they are known.
    dir_modes: Vec<Option<u32>>,
    /// Those each of `files` is created with.
    file_modes: Vec<Option<u32>>,
}

impl<S, D> Plan<S, D> {
    fn new() -> Self {
        Self { dirs: Vec::new(), files: Vec::new(), dir_modes: Vec::new(), file_modes: Vec::new() }
    }

    fn add_dir(&mut self, dir: D, mode: Option<u32>) {
        self.dirs.push(dir);
        self.dir_modes.push(mode);
    }

    fn add_file(&mut self, from: S, to: D, size: u64, mode: Option<u32>) {
        self.files.push((from, to, size));
        self.file_modes.push(mode);
    }
}

/// The permission bits a copy uploaded gets, as `sftp put` gives them: the file's, without the
/// set-id and sticky bits (and less the server's umask). None on Windows, which has none: the
/// server's default.
fn uploaded_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    return Some(std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o777);
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// Those a folder downloaded gets, as `sftp get -r` gives them: with the owner's read, write
/// and search.
fn downloaded_dir_mode(attributes: &FileAttributes) -> Option<u32> {
    attributes.permissions.map(|mode| mode & 0o777 | 0o700)
}

/// Those a file downloaded gets, as `sftp get` gives them: the remote one's, without the set-id
/// and sticky bits, and with the owner's write, which writes it (less the umask; Unix only).
fn downloaded_mode(attributes: &FileAttributes) -> Option<u32> {
    attributes.permissions.map(|mode| mode & 0o777 | 0o200)
}

/// Files copied at once on a connection, by all of its transfers together. A small file costs
/// a few round trips (opening it, its first read or its writes' acknowledgements, closing it),
/// which copying one file after another waits for in turn: 10,000 small files took over an
/// hour from a server 100 ms away.
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
    slots: Arc<TransferSlots>,
    failed: AtomicBool,
}

impl<'a> Run<'a> {
    fn new(reporter: &'a mut Reporter) -> Self {
        Self { slots: reporter.slots.clone(), reporter: Mutex::new(reporter), failed: AtomicBool::new(false) }
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
/// [`CONCURRENT_FILES`] small ones at once and a large one alone, counting those that the
/// connection's other transfers copy (see [`Reporter::sharing`]). After a failure no other
/// starts, those being copied finish (as they would have before it, one at a time), and the
/// first failure is returned.
async fn copy_files<F>(sizes: &[u64], run: &Run<'_>, copy_file: impl Fn(usize) -> F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    let (slots, copy_file) = (&run.slots.0, &copy_file);
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
    let made = create_remote_dirs(sftp, &plan.dirs, &plan.dir_modes, &mut created).await;
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
            // A file of the same name that this upload put there before is its own still.
            if !replaces {
                files.0.push(remote.clone());
            } else if !files.0.contains(remote) {
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
fn same_name_turns(files: &[(PathBuf, String, u64)]) -> HashMap<String, tokio::sync::Mutex<()>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, remote, _) in files {
        *counts.entry(remote.to_lowercase()).or_default() += 1;
    }
    counts.into_iter().filter(|(_, count)| *count > 1).map(|(remote, _)| (remote, tokio::sync::Mutex::new(()))).collect()
}

/// Creates an upload's folders, parents first and those of one depth at once, with the
/// permission bits `modes` gives each; those that weren't there go into `created`, also when
/// another fails.
async fn create_remote_dirs(sftp: &SftpSession, dirs: &[String], modes: &[Option<u32>], created: &mut HashSet<String>) -> Result<()> {
    let depth = |dir: &(&String, Option<u32>)| dir.0.matches('/').count();
    let mut by_depth: Vec<(&String, Option<u32>)> = dirs.iter().zip(modes.iter().copied()).collect();
    by_depth.sort_by_key(depth);
    for level in by_depth.chunk_by(|a, b| depth(a) == depth(b)) {
        let results: Vec<(usize, Result<bool>)> = stream::iter(0..level.len())
            .map(|i| async move { (i, create_remote_dir(sftp, level[i].0, level[i].1).await) })
            .buffer_unordered(CONCURRENT_DIRS)
            .collect()
            .await;
        let mut first = Ok(());
        for (i, result) in results {
            match result {
                Ok(true) => drop(created.insert(level[i].0.clone())),
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

async fn scan_local_dir(root: &Path, remote_root: String, plan: &mut Plan<PathBuf, String>, reporter: &mut Reporter) -> Result<()> {
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
    let mut whole = Vec::new();
    if result.is_ok() {
        result = download_counting(sftp, &items, reporter, &mut whole, left, true).await;
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

/// Takes a free local name for the remote item, creating an empty file or folder there.
async fn claim(sftp: &SftpSession, remote: &str, local_dir: &Path) -> Result<PathBuf> {
    let name = local_file_name(file_name(remote)).ok_or_else(|| Error::new("transfer.invalidName").param("name", remote))?;
    let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
    let path = local_dir.join(name);
    let (is_dir, dir_mode, file_mode) = (metadata.file_type().is_dir(), downloaded_dir_mode(&metadata), downloaded_mode(&metadata));
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
    download_counting(sftp, items, reporter, &mut Vec::new(), left, false).await
}

/// [`download_to`], keeping in `whole` which of the items have arrived whole: those copied at
/// the same time as one that failed may have. With `claimed`, the items' paths were free
/// before (see [`claim`]): all that arrives under them is new, folders too.
async fn download_counting(
    sftp: &SftpSession,
    items: &[(String, PathBuf)],
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
    for (remote, local) in items {
        let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), downloaded_dir_mode(&metadata), &mut plan, &mut taken, reporter).await?;
        } else {
            let size = metadata.size.unwrap_or(0);
            reporter.progress.total += size;
            plan.add_file(remote.clone(), local.clone(), size, downloaded_mode(&metadata));
        }
        ends.push(plan.files.len());
    }
    // `+=`: the items of a drag are downloaded one call at a time with the same reporter.
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
            // where a folder is, gets a free name instead.
            let local = if clashes || local.is_dir() {
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
fn items_whole(ends: &[usize], arrived: &[bool]) -> Vec<bool> {
    let starts = std::iter::once(0).chain(ends.iter().copied());
    starts.zip(ends).map(|(start, &end)| arrived[start..end].iter().all(|&arrived| arrived)).collect()
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
    fn create_free_file(&mut self, path: &Path, mode: Option<u32>) -> std::io::Result<(PathBuf, std::fs::File)> {
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
        let listings: Vec<_> = stream::iter(0..level.len())
            .map(|i| {
                let remote = &level[i].0;
                async move { sftp.read_dir(remote).await.context(Error::new("transfer.readFailed").param("path", remote)) }
            })
            .buffered(CONCURRENT_DIRS)
            .collect()
            .await;
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

#[cfg(test)]
mod tests;
