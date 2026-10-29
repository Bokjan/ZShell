//! A connection's transport, which can be ended from outside russh and tells what goes on in
//! it.
//!
//! A handshake given up on ends it: russh starts its connection task once the server's version
//! line arrives, and `connect_stream` waits for the key exchange to finish, so dropping that
//! future leaves the task running, reading from a server that may never say more. Ending the
//! stream makes the task's reads and writes fail, so it stops and the connection (or the
//! proxy command) goes with it.
//!
//! So does a connection found dead (see `super::liveness`): russh's connection task may itself
//! be stuck writing to the stream, and a disconnect message would wait behind that write. And so
//! does a write left waiting for room too long (see `super::stall_limit`), which russh would
//! wait for with no limit.
//!
//! It also keeps what the server sends first, up to its version line, for the debug log: russh
//! skips the lines before it, and keeps neither (see [`AbortOnDrop::greeting`]).

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;

/// Wraps `stream`; the guard ends it when dropped, unless kept first. A write left waiting for
/// room for `stall` (if given), the peer taking nothing meanwhile, fails and ends the stream.
pub fn abortable<S>(stream: S, stall: Option<Duration>) -> (Abortable<S>, AbortOnDrop) {
    let shared = Arc::new(Shared::default());
    (Abortable { inner: stream, shared: shared.clone(), stall, stalled: None }, AbortOnDrop(Some(shared)))
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// The signs of the peer: reads that brought something, and writes that went on after
    /// waiting for room (which only the peer's acknowledgements make, once the system's send
    /// buffer is full).
    heard: AtomicU64,
    /// The writes that took something.
    sent: AtomicU64,
    /// The bytes read and written: all of the connection's traffic, as it goes over the
    /// network (see `crate::traffic`).
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    /// The last read, and the last write, were left waiting for the peer.
    reading: AtomicBool,
    writing: AtomicBool,
    /// What the peer sent first, until its version line has come or [`GREETING_LIMIT`] bytes
    /// have (`greeted`).
    greeting: Mutex<Vec<u8>>,
    greeted: AtomicBool,
}

/// The most of what the server sends first that is kept, as RFC 4253 allows lines before the
/// version line (russh reads at most 256 of them).
const GREETING_LIMIT: usize = 2048;

/// What the server sent before the SSH key exchange: its version line, once it has come, and
/// whatever came before it, as text with its control characters escaped.
#[derive(Debug, Default, PartialEq)]
pub struct Greeting {
    pub version: Option<String>,
    pub before: Option<String>,
}

/// Reads [`Greeting`] from the bytes the server sent first.
fn greeting(bytes: &[u8]) -> Greeting {
    let text = String::from_utf8_lossy(bytes);
    let escaped = |before: &str| (!before.is_empty()).then(|| before.escape_debug().to_string());
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("SSH-") && line.ends_with('\n') {
            return Greeting { version: Some(line.trim_end().to_owned()), before: escaped(text[..start].trim_end()) };
        }
        start += line.len();
    }
    Greeting { version: None, before: escaped(text.trim_end()) }
}

impl Shared {
    /// Keeps the first of `read`, until the version line has come.
    fn note_greeting(&self, read: &[u8]) {
        let mut kept = self.greeting.lock().unwrap();
        let room = GREETING_LIMIT.saturating_sub(kept.len());
        kept.extend_from_slice(&read[..read.len().min(room)]);
        if kept.len() >= GREETING_LIMIT || greeting(&kept).version.is_some() {
            self.greeted.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
struct State {
    /// Why the stream ended, once it has.
    ended: Option<io::ErrorKind>,
    /// The tasks waiting to read and to write, woken when the stream ends.
    reader: Option<Waker>,
    writer: Option<Waker>,
}

impl Shared {
    fn end(&self, why: io::ErrorKind) {
        let (reader, writer) = {
            let mut state = self.state.lock().unwrap();
            state.ended.get_or_insert(why);
            (state.reader.take(), state.writer.take())
        };
        for waker in [reader, writer].into_iter().flatten() {
            waker.wake();
        }
    }
}

/// Fails once the stream has ended; otherwise remembers who to wake when it does.
fn check(shared: &Shared, cx: &Context<'_>, waker: fn(&mut State) -> &mut Option<Waker>) -> io::Result<()> {
    let mut state = shared.state.lock().unwrap();
    if let Some(why) = state.ended {
        return Err(why.into());
    }
    let slot = waker(&mut state);
    if !slot.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
        *slot = Some(cx.waker().clone());
    }
    Ok(())
}

/// Notes in `waiting` whether `poll` was left waiting for the peer.
fn noting<T>(waiting: &AtomicBool, poll: Poll<T>) -> Poll<T> {
    waiting.store(poll.is_pending(), Ordering::Relaxed);
    poll
}

pub struct Abortable<S> {
    inner: S,
    shared: Arc<Shared>,
    /// How long a write may wait for room.
    stall: Option<Duration>,
    /// Since the write waiting for room began to wait, when one is: until when it may.
    stalled: Option<Pin<Box<Sleep>>>,
}

impl<S> Abortable<S> {
    /// Fails a write (or flush) left waiting for room for [`Self::stall`], which ends the
    /// stream; one that goes on, or that may wait longer, is left as it is. The wait counts
    /// from the first write that found no room since the last that made progress, and wakes
    /// the task when it is up: a peer gone silently wakes nobody.
    fn limit_stall<T>(&mut self, cx: &mut Context<'_>, poll: Poll<io::Result<T>>) -> Poll<io::Result<T>> {
        let Some(stall) = self.stall else { return poll };
        if poll.is_ready() {
            self.stalled = None;
            return poll;
        }
        let until = self.stalled.get_or_insert_with(|| Box::pin(tokio::time::sleep(stall)));
        if until.as_mut().poll(cx).is_pending() {
            return poll;
        }
        self.stalled = None;
        log::warn!("a write waited {stall:?} for room, the server taking nothing meanwhile: the connection is taken for lost");
        self.shared.end(io::ErrorKind::TimedOut);
        Poll::Ready(Err(io::ErrorKind::TimedOut.into()))
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Abortable<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        check(&self.shared, cx, |s| &mut s.reader)?;
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        let read = buf.filled().len() - before;
        if read > 0 {
            if !self.shared.greeted.load(Ordering::Relaxed) {
                self.shared.note_greeting(&buf.filled()[before..]);
            }
            self.shared.heard.fetch_add(1, Ordering::Relaxed);
            self.shared.bytes_read.fetch_add(read as u64, Ordering::Relaxed);
        }
        noting(&self.shared.reading, poll)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Abortable<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        check(&self.shared, cx, |s| &mut s.writer)?;
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        let poll = self.limit_stall(cx, poll);
        if let Poll::Ready(Ok(written @ 1..)) = poll {
            self.shared.sent.fetch_add(1, Ordering::Relaxed);
            self.shared.bytes_written.fetch_add(written as u64, Ordering::Relaxed);
            if self.shared.writing.load(Ordering::Relaxed) {
                self.shared.heard.fetch_add(1, Ordering::Relaxed);
            }
        }
        noting(&self.shared.writing, poll)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        check(&self.shared, cx, |s| &mut s.writer)?;
        let poll = Pin::new(&mut self.inner).poll_flush(cx);
        let poll = self.limit_stall(cx, poll);
        noting(&self.shared.writing, poll)
    }

    /// Done at once once ended: dropping the stream closes it, and a shutdown could wait on
    /// the very peer that went silent.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.shared.state.lock().unwrap().ended.is_some() {
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Ends the stream when dropped, unless [kept](Self::keep).
pub struct AbortOnDrop(Option<Arc<Shared>>);

impl AbortOnDrop {
    /// What the server has sent before the key exchange so far.
    pub fn greeting(&self) -> Greeting {
        self.0.as_ref().map(|shared| greeting(&shared.greeting.lock().unwrap())).unwrap_or_default()
    }

    /// Leaves the stream to run for as long as the connection does; the link ends it sooner.
    pub fn keep(mut self) -> Link {
        Link(self.0.take().expect("kept once"))
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(shared) = self.0.take() {
            shared.end(io::ErrorKind::ConnectionAborted);
        }
    }
}

/// What goes on in a kept stream, and a way to end it. Dropping it leaves the stream alone.
#[derive(Clone)]
pub struct Link(Arc<Shared>);

impl Link {
    /// Counts the signs of the peer so far: anything read from it, and room to write that it
    /// made by acknowledging what was sent, on a slow link where russh writes for long without
    /// reading.
    pub fn heard(&self) -> u64 {
        self.0.heard.load(Ordering::Relaxed)
    }

    /// Counts the writes that took something so far.
    pub fn sent(&self) -> u64 {
        self.0.sent.load(Ordering::Relaxed)
    }

    /// The bytes written and read so far: everything the connection sent and received.
    #[cfg_attr(not(test), expect(dead_code, reason = "kept for showing a connection's whole traffic beside what its channels carry"))]
    pub fn bytes(&self) -> (u64, u64) {
        (self.0.bytes_written.load(Ordering::Relaxed), self.0.bytes_read.load(Ordering::Relaxed))
    }

    /// Whether russh waits for the peer: to read something, or for room to write. Neither when
    /// its connection task is held up on this side, by a channel not read (see
    /// `super::bridge`).
    pub fn waiting(&self) -> bool {
        self.0.reading.load(Ordering::Relaxed) || self.writing()
    }

    /// Whether russh waits for room to write.
    pub fn writing(&self) -> bool {
        self.0.writing.load(Ordering::Relaxed)
    }

    /// Ends the stream: its reads and writes fail with `why` from now on.
    pub fn end(&self, why: io::ErrorKind) {
        self.0.end(why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn the_greeting_is_the_version_line_and_what_came_before() {
        assert_eq!(greeting(b""), Greeting::default());
        let version = greeting(b"SSH-2.0-OpenSSH_9.3\r\n\0\0\0");
        assert_eq!(version, Greeting { version: Some("SSH-2.0-OpenSSH_9.3".into()), before: None });
        let gateway = greeting(b"HTTP/1.1 403 Forbidden\r\nX-Rendered-By: gateway\r\n\r\nno\x1b[2J");
        assert_eq!(gateway.version, None);
        assert_eq!(gateway.before.as_deref(), Some(r"HTTP/1.1 403 Forbidden\r\nX-Rendered-By: gateway\r\n\r\nno\u{1b}[2J"));
        let banner = greeting(b"Welcome\r\nSSH-2.0-dropbear\r\n");
        assert_eq!((banner.version.as_deref(), banner.before.as_deref()), (Some("SSH-2.0-dropbear"), Some("Welcome")));
        // Not a whole line yet.
        assert_eq!(greeting(b"SSH-2.0-Open").version, None);
    }

    /// What the server sends first is kept until its version line, and no more.
    #[tokio::test]
    async fn the_greeting_is_kept_until_the_version_line() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream, None);
        peer.write_all(b"hi\r\nSSH-2.0-x\r\nmore").await.unwrap();
        let mut buf = [0; 32];
        let _ = stream.read(&mut buf).await.unwrap();
        peer.write_all(b"after").await.unwrap();
        let _ = stream.read(&mut buf).await.unwrap();
        assert_eq!(guard.greeting(), Greeting { version: Some("SSH-2.0-x".into()), before: Some("hi".into()) });
        let shared = guard.0.as_ref().unwrap();
        assert!(shared.greeted.load(Ordering::Relaxed));
        assert!(!shared.greeting.lock().unwrap().ends_with(b"after"));
    }

    /// A read waiting for data fails once the guard goes, and so does what follows.
    #[tokio::test]
    async fn dropping_the_guard_ends_a_waiting_read() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream, None);
        let reading = tokio::spawn(async move {
            let mut buf = [0; 8];
            let first = stream.read(&mut buf).await;
            (first.map_err(|e| e.kind()), stream.write_all(b"x").await.map_err(|e| e.kind()))
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(guard);
        let (read, write) = tokio::time::timeout(std::time::Duration::from_secs(5), reading).await.unwrap().unwrap();
        assert_eq!(read, Err(io::ErrorKind::ConnectionAborted));
        assert_eq!(write, Err(io::ErrorKind::ConnectionAborted));
        // Nothing reached the other side.
        let mut buf = [0; 8];
        assert_eq!(peer.read(&mut buf).await.unwrap(), 0);
    }

    /// A write that goes on once the peer has made room tells that the peer is there; one that
    /// finds room at once doesn't.
    #[tokio::test]
    async fn room_made_by_the_peer_is_heard() {
        let (stream, mut peer) = tokio::io::duplex(4);
        let (mut stream, guard) = abortable(stream, None);
        let link = guard.keep();
        stream.write_all(b"abcd").await.unwrap();
        assert_eq!(link.heard(), 0);
        let writing = tokio::spawn(async move { stream.write_all(b"efgh").await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(link.writing());
        let mut buf = [0; 8];
        peer.read_exact(&mut buf).await.unwrap();
        writing.await.unwrap();
        assert!(!link.writing());
        assert_eq!(link.heard(), 1);
        assert_eq!(link.bytes(), (8, 0));
    }

    /// A write left waiting for room fails once it has waited too long, which ends the stream;
    /// the wait starts again whenever the peer makes room.
    #[tokio::test(start_paused = true)]
    async fn a_write_left_waiting_for_room_ends_the_stream() {
        use std::time::Duration;
        use tokio::time::Instant;

        const STALL: Duration = Duration::from_secs(10);
        let (stream, mut peer) = tokio::io::duplex(4);
        let (mut stream, _guard) = abortable(stream, Some(STALL));
        stream.write_all(b"abcd").await.unwrap();
        let writing = tokio::spawn(async move {
            let written = stream.write_all(b"efgh").await;
            written.map(|()| stream)
        });
        tokio::time::sleep(STALL - Duration::from_secs(2)).await;
        let mut buf = [0; 4];
        peer.read_exact(&mut buf).await.unwrap();
        let mut stream = writing.await.unwrap().unwrap();

        let start = Instant::now();
        let written = stream.write_all(b"ijkl").await;
        assert_eq!(written.map_err(|e| e.kind()), Err(io::ErrorKind::TimedOut));
        assert!(start.elapsed() >= STALL);
        assert_eq!(stream.read(&mut buf).await.map_err(|e| e.kind()), Err(io::ErrorKind::TimedOut));
    }

    /// Kept, the stream works as before, and the link tells what it does until it ends it.
    #[tokio::test]
    async fn a_kept_stream_runs_until_its_link_ends_it() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream, None);
        let link = guard.keep();
        stream.write_all(b"hello").await.unwrap();
        let mut buf = [0; 5];
        peer.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        assert_eq!((link.heard(), link.sent()), (0, 1));
        peer.write_all(b"hi").await.unwrap();
        stream.read_exact(&mut buf[..2]).await.unwrap();
        assert_eq!(link.heard(), 1);
        assert!(!link.waiting());

        let reading = tokio::spawn(async move {
            let mut buf = [0; 8];
            stream.read(&mut buf).await.map_err(|e| e.kind())
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(link.waiting());
        link.end(io::ErrorKind::TimedOut);
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), reading).await.unwrap().unwrap();
        assert_eq!(read, Err(io::ErrorKind::TimedOut));
    }
}
