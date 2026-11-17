//! Recursive uploads and downloads with progress reporting and cancellation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use futures_util::{stream, StreamExt};
use russh_sftp::protocol::FileAttributes;
use serde::Serialize;
use std::future::Future;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

mod download;
mod job;
mod left;
mod resume;
mod upload;

pub use download::{download_to, download_to_edit};
pub use job::Job;
pub use left::{delete_local, delete_remote, Deletion, Ended, Kept, Left, Server};
pub use resume::{ended_by_loss, Resume};

use left::Stamp;
use resume::Copied;

use crate::error::Error;

const BUFFER_SIZE: usize = 256 * 1024;
pub(super) const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

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

/// Cancellation of running transfers, what ended ones left (see [`left`]), and what it takes
/// to go on with failed ones (see [`resume`]), by the webview that started each and the id its
/// page chose. A transfer belongs to its page: only that page cancels it, goes on with it or
/// deletes what it left, and when the page is gone (reloaded, or its window closed), with
/// nobody left to show it, it is cancelled and what it left forgotten.
#[derive(Default)]
pub struct Transfers {
    running: Mutex<HashMap<(String, String), CancellationToken>>,
    /// None while it is being deleted, until what couldn't be is put back.
    left: Mutex<HashMap<(String, String), Option<Kept>>>,
    /// The failed transfers that can be gone on with.
    paused: Mutex<HashMap<(String, String), Job>>,
}

impl Transfers {
    pub fn start(&self, webview: &str, id: &str) -> CancellationToken {
        let cancelled = CancellationToken::new();
        self.running.lock().unwrap().insert((webview.to_owned(), id.to_owned()), cancelled.clone());
        cancelled
    }

    pub fn finish(&self, webview: &str, id: &str) {
        self.running.lock().unwrap().remove(&(webview.to_owned(), id.to_owned()));
    }

    /// Cancels a transfer: one running, or one that failed and was kept to go on with, as it
    /// may have failed (its connection lost) just before the user cancelled it. Returns the
    /// latter, to discard.
    pub fn cancel(&self, webview: &str, id: &str) -> Option<Job> {
        let key = (webview.to_owned(), id.to_owned());
        // Held while the job is taken: `pause` checks the cancellation under it.
        let running = self.running.lock().unwrap();
        if let Some(cancelled) = running.get(&key) {
            cancelled.cancel();
        }
        self.paused.lock().unwrap().remove(&key)
    }

    /// Cancels the transfers of `webview`, whose page is gone, and forgets what its ended ones
    /// left. Returns the failed ones it could have gone on with, to discard.
    pub fn cancel_webview(&self, webview: &str) -> Vec<Job> {
        self.running.lock().unwrap().retain(|(owner, _), cancelled| {
            let theirs = owner == webview;
            if theirs {
                cancelled.cancel();
            }
            !theirs
        });
        self.left.lock().unwrap().retain(|(owner, _), _| owner != webview);
        let mut paused = self.paused.lock().unwrap();
        let theirs: Vec<_> = paused.keys().filter(|(owner, _)| owner == webview).cloned().collect();
        theirs.into_iter().filter_map(|key| paused.remove(&key)).collect()
    }

    /// Keeps a failed transfer's `job`, to go on with, unless it was `cancelled` meanwhile (the
    /// failure, a lost connection say, coming first); returns it then, to discard.
    pub fn pause(&self, webview: &str, id: &str, job: Job, cancelled: &CancellationToken) -> Option<Job> {
        // `cancel` takes the token, then the job, under this lock: it can't come in between.
        let _running = self.running.lock().unwrap();
        if cancelled.is_cancelled() {
            return Some(job);
        }
        self.paused.lock().unwrap().insert((webview.to_owned(), id.to_owned()), job);
        None
    }

    /// Takes a failed transfer's job to go on with it; none if it can't be gone on with (it
    /// finished, was cancelled or forgotten, or is going on already).
    pub fn take_paused(&self, webview: &str, id: &str) -> Option<Job> {
        self.paused.lock().unwrap().remove(&(webview.to_owned(), id.to_owned()))
    }

    /// Takes what a transfer being gone on with left so far, to add to it; none while it is
    /// being deleted.
    pub fn take_left(&self, webview: &str, id: &str) -> Option<Kept> {
        let mut left = self.left.lock().unwrap();
        let key = (webview.to_owned(), id.to_owned());
        match left.get(&key) {
            Some(Some(_)) => left.remove(&key).flatten(),
            _ => None,
        }
    }

    /// Ends a transfer with `result`: lets go of its cancellation, and keeps what it `left`.
    /// Returns how it ended, with the local `paths` to show, and how it can be gone on with.
    pub fn end(
        &self,
        webview: &str,
        id: &str,
        left: Kept,
        result: crate::error::Result<()>,
        paths: Vec<PathBuf>,
        resume: Option<Resume>,
    ) -> Ended {
        self.finish(webview, id);
        match &result {
            Ok(()) => log::debug!("transfer {id}: done"),
            Err(e) => log::warn!("transfer {id}: {}", e.logged()),
        }
        let ended = left.ended(result.err(), paths, resume);
        self.keep(webview, id, left);
        ended
    }

    /// Keeps what an ended transfer left, unless there is nothing to delete.
    pub fn keep(&self, webview: &str, id: &str, kept: Kept) {
        if !kept.is_empty() {
            self.left.lock().unwrap().insert((webview.to_owned(), id.to_owned()), Some(kept));
        }
    }

    /// Takes what a transfer left, to delete it, until [`Transfers::put_back`]; none while it
    /// is being deleted already.
    pub fn take(&self, webview: &str, id: &str) -> Option<Kept> {
        self.left.lock().unwrap().get_mut(&(webview.to_owned(), id.to_owned()))?.take()
    }

    /// Puts back what deleting a transfer's files couldn't delete, unless the list stopped
    /// showing it meanwhile.
    pub fn put_back(&self, webview: &str, id: &str, kept: Kept) {
        let mut left = self.left.lock().unwrap();
        let key = (webview.to_owned(), id.to_owned());
        match left.get_mut(&key) {
            Some(_) if kept.is_empty() => drop(left.remove(&key)),
            Some(slot) => *slot = Some(kept),
            None => {}
        }
    }

    /// Forgets what the transfers `ids` left, which the list no longer shows. Returns the failed
    /// ones it could have gone on with, to discard.
    pub fn forget(&self, webview: &str, ids: &[String]) -> Vec<Job> {
        let mut left = self.left.lock().unwrap();
        let mut paused = self.paused.lock().unwrap();
        let mut jobs = Vec::new();
        for id in ids {
            let key = (webview.to_owned(), id.clone());
            left.remove(&key);
            jobs.extend(paused.remove(&key));
        }
        jobs
    }
}

/// What the transfers on one connection copy files at once by (see [`copy_files`]): the files'
/// slots, and the turn of the large ones. A large file waits for its turn before it waits for
/// its slots, so that only one waits for them at a time: the semaphore gives one waiting for
/// several slots those free at once and queues the others behind it, so large ones waiting
/// side by side would each take the slot left to small files.
pub struct TransferSlots {
    files: Semaphore,
    large: Semaphore,
}

impl Default for TransferSlots {
    fn default() -> Self {
        Self { files: Semaphore::new(CONCURRENT_FILES), large: Semaphore::new(1) }
    }
}

pub struct Reporter {
    send: Box<dyn Fn(Progress) + Send + Sync>,
    cancelled: CancellationToken,
    /// Shared with the connection's other transfers ([`Reporter::sharing`]), else its own.
    slots: Arc<TransferSlots>,
    progress: Progress,
    /// When progress was last sent; `None` before the first time, which is at once.
    last_emit: Option<Instant>,
}

impl Reporter {
    pub fn new(channel: Channel<Progress>, cancelled: CancellationToken) -> Self {
        Self::with_sink(move |progress| drop(channel.send(progress)), cancelled)
    }

    /// Reports progress to `send` instead of a channel of its own.
    pub fn with_sink(send: impl Fn(Progress) + Send + Sync + 'static, cancelled: CancellationToken) -> Self {
        Self { send: Box::new(send), cancelled, slots: Arc::default(), progress: Progress::default(), last_emit: None }
    }

    /// Copies files at once by `slots`, those of the connection's other transfers too.
    pub fn sharing(self, slots: Arc<TransferSlots>) -> Self {
        Self { slots, ..self }
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.is_cancelled() {
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

/// What a transfer copies, and how far it has got; `T` is where a file cut short was left
/// (see [`resume`]).
struct Plan<S, D, T> {
    /// Directories to create, parents before children.
    dirs: Vec<D>,
    /// Each file, where it goes and its size.
    files: Vec<(S, D, u64)>,
    /// The permission bits each of `dirs` is created with, where they are known.
    dir_modes: Vec<Option<u32>>,
    /// Those each of `files` is created with.
    file_modes: Vec<Option<u32>>,
    /// What each of `files` was like when listed: one changed since is copied again from the
    /// start.
    sources: Vec<Option<Stamp>>,
    /// How far each of `files` has been copied.
    copied: Vec<Copied<D, T>>,
}

impl<S, D, T> Plan<S, D, T> {
    fn new() -> Self {
        Self { dirs: Vec::new(), files: Vec::new(), dir_modes: Vec::new(), file_modes: Vec::new(), sources: Vec::new(), copied: Vec::new() }
    }

    fn add_dir(&mut self, dir: D, mode: Option<u32>) {
        self.dirs.push(dir);
        self.dir_modes.push(mode);
    }

    fn add_file(&mut self, from: S, to: D, size: u64, mode: Option<u32>, source: Option<Stamp>) {
        self.files.push((from, to, size));
        self.file_modes.push(mode);
        self.sources.push(source);
        self.copied.push(Copied::No);
    }

    /// The files still to copy, wholly or in part, by index.
    fn to_copy(&self) -> Vec<usize> {
        (0..self.files.len()).filter(|&i| !matches!(self.copied[i], Copied::Done(_))).collect()
    }

    /// Where a transfer going on with the plan starts from: the files copied whole are done.
    fn progress(&self) -> Progress {
        let done = || (0..self.files.len()).filter(|&i| matches!(self.copied[i], Copied::Done(_)));
        Progress {
            transferred: done().map(|i| self.files[i].2).sum(),
            total: self.files.iter().map(|(_, _, size)| size).sum(),
            files_done: done().count(),
            files_total: self.files.len(),
            current: String::new(),
        }
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
/// Files larger than this are copied without another large one: one keeps the channel's
/// window full by itself, and with others beside it the requests waiting at the server could
/// outlast their timeout on a slow link (see `channels`). They leave one of the slots to small
/// files, so that a large download doesn't hold up a small file of another transfer (such as
/// a drag) until it is done.
const SMALL_FILE: u64 = 256 << 10;
/// The slots a large file takes.
const LARGE_FILE_SLOTS: u32 = CONCURRENT_FILES as u32 - 1;
/// Folders listed, created or looked up at once.
const CONCURRENT_DIRS: usize = 8;
/// Folders created between looks at whether the upload was cancelled.
const CREATE_BATCH: usize = 64;

/// A transfer copying files: its reporter, shared by the files copied at once, and whether
/// one of them failed, after which no other starts.
struct Run<'a> {
    reporter: Mutex<&'a mut Reporter>,
    slots: Arc<TransferSlots>,
    cancelled: CancellationToken,
    failed: AtomicBool,
}

impl<'a> Run<'a> {
    fn new(reporter: &'a mut Reporter) -> Self {
        let (slots, cancelled) = (reporter.slots.clone(), reporter.cancelled.clone());
        Self { slots, cancelled, reporter: Mutex::new(reporter), failed: AtomicBool::new(false) }
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

/// `future`, unless the transfer is cancelled first: what it waits for (the server, or the
/// connection's other transfers to let go of their slots) may take minutes. Not for requests
/// that open a file or folder at the server, which would stay open if dropped midway.
async fn unless_cancelled<T>(cancelled: &CancellationToken, future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        result = future => result,
        () = cancelled.cancelled() => bail!(Error::new("transfer.cancelled")),
    }
}

/// Copies the files of `sizes` with `copy_file` (given each one's index), in order, up to
/// [`CONCURRENT_FILES`] small ones at once, or a large one and a small one, counting those that the
/// connection's other transfers copy (see [`Reporter::sharing`]). After a failure no other
/// starts, those being copied finish (as they would have before it, one at a time), and the
/// first failure is returned.
async fn copy_files<F>(sizes: &[u64], run: &Run<'_>, copy_file: impl Fn(usize) -> F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    let (slots, copy_file) = (&*run.slots, &copy_file);
    // By index: closures given references make futures the compiler can't prove `Send`.
    let mut copies = stream::iter(0..sizes.len())
        .map(|i| async move {
            let large = sizes[i] > SMALL_FILE;
            let _slots = unless_cancelled(&run.cancelled, async {
                let turn = if large { Some(slots.large.acquire().await.expect("never closed")) } else { None };
                let weight = if large { LARGE_FILE_SLOTS } else { 1 };
                Ok((turn, slots.files.acquire_many(weight).await.expect("never closed")))
            })
            .await?;
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

async fn copy<R, W>(src: &mut R, dst: &mut W, run: &Run<'_>) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    run.check_cancelled()?;
    pump(src, dst, anyhow::Error::from, |n| {
        run.advance(n as u64);
        run.check_cancelled()
    })
    .await
}

/// Copies `src` to `dst` a buffer at a time, with `copied` told each one's length once it is
/// written; it stops the copy by failing. A read's error is passed through `read_failed`.
/// Not flushed here: the targets do as they finish. A remote file's flush also has the server
/// sync it to disk (`fsync@openssh.com`), a round trip and a wait for every file.
pub(super) async fn pump<R, W>(
    src: &mut R,
    dst: &mut W,
    read_failed: impl Fn(std::io::Error) -> anyhow::Error,
    mut copied: impl FnMut(usize) -> Result<()>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0; BUFFER_SIZE];
    loop {
        let n = src.read(&mut buf).await.map_err(&read_failed)?;
        if n == 0 {
            return Ok(());
        }
        dst.write_all(&buf[..n]).await?;
        copied(n)?;
    }
}

/// Adds which file failed, except for cancellation, which is reported as is.
fn with_file_context(e: anyhow::Error, context: Error) -> anyhow::Error {
    if is_cancelled(&e) {
        e
    } else {
        e.context(context)
    }
}

/// Whether `e` is the transfer's cancellation.
pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Error>().is_some_and(|e| e.code() == "transfer.cancelled")
}

#[cfg(test)]
mod tests;
