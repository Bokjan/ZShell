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
//! be stuck writing to the stream, and a disconnect message would wait behind that write.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Wraps `stream`; the guard ends it when dropped, unless kept first.
pub fn abortable<S>(stream: S) -> (Abortable<S>, AbortOnDrop) {
    let shared = Arc::new(Shared::default());
    (Abortable { inner: stream, shared: shared.clone() }, AbortOnDrop(Some(shared)))
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
}

impl<S: AsyncRead + Unpin> AsyncRead for Abortable<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        check(&self.shared, cx, |s| &mut s.reader)?;
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        let read = buf.filled().len() - before;
        if read > 0 {
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

    /// A read waiting for data fails once the guard goes, and so does what follows.
    #[tokio::test]
    async fn dropping_the_guard_ends_a_waiting_read() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream);
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
        let (mut stream, guard) = abortable(stream);
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

    /// Kept, the stream works as before, and the link tells what it does until it ends it.
    #[tokio::test]
    async fn a_kept_stream_runs_until_its_link_ends_it() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream);
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
