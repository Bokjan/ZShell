//! A connection that a handshake given up on can end. russh starts its connection task once
//! the server's version line arrives, and `connect_stream` waits for the key exchange to
//! finish: dropping that future leaves the task running, reading from a server that may
//! never say more. Ending the stream makes the task's reads and writes fail, so it stops and
//! the connection (or the proxy command) goes with it.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Wraps `stream`; the guard ends it when dropped, unless disarmed first.
pub fn abortable<S>(stream: S) -> (Abortable<S>, AbortOnDrop) {
    let state = Arc::new(Mutex::new(State::default()));
    (Abortable { inner: stream, state: state.clone() }, AbortOnDrop(Some(state)))
}

#[derive(Default)]
struct State {
    aborted: bool,
    /// The tasks waiting to read and to write, woken when the stream ends.
    reader: Option<Waker>,
    writer: Option<Waker>,
}

/// Fails once the stream has ended; otherwise remembers who to wake when it does.
fn check(state: &Mutex<State>, cx: &Context<'_>, waker: fn(&mut State) -> &mut Option<Waker>) -> io::Result<()> {
    let mut state = state.lock().unwrap();
    if state.aborted {
        return Err(io::ErrorKind::ConnectionAborted.into());
    }
    let slot = waker(&mut state);
    if !slot.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
        *slot = Some(cx.waker().clone());
    }
    Ok(())
}

pub struct Abortable<S> {
    inner: S,
    state: Arc<Mutex<State>>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Abortable<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        check(&self.state, cx, |s| &mut s.reader)?;
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Abortable<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        check(&self.state, cx, |s| &mut s.writer)?;
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        check(&self.state, cx, |s| &mut s.writer)?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    /// Done at once once ended: dropping the stream closes it, and a shutdown could wait on
    /// the very peer that went silent.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.state.lock().unwrap().aborted {
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Ends the stream when dropped, unless [disarmed](Self::disarm).
pub struct AbortOnDrop(Option<Arc<Mutex<State>>>);

impl AbortOnDrop {
    /// Leaves the stream to run for as long as the connection does.
    pub fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        let Some(state) = self.0.take() else { return };
        let (reader, writer) = {
            let mut state = state.lock().unwrap();
            state.aborted = true;
            (state.reader.take(), state.writer.take())
        };
        for waker in [reader, writer].into_iter().flatten() {
            waker.wake();
        }
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

    /// Disarmed, the stream works as before.
    #[tokio::test]
    async fn a_disarmed_guard_leaves_the_stream_alone() {
        let (stream, mut peer) = tokio::io::duplex(64);
        let (mut stream, guard) = abortable(stream);
        guard.disarm();
        stream.write_all(b"hello").await.unwrap();
        let mut buf = [0; 5];
        peer.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }
}
