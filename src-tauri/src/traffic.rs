//! What goes through a session's channels, for the lights and speeds the frontend shows:
//! the bytes each terminal sends and receives, those on each connection's SFTP channels, and
//! those each forwarding rule carries. They are counted where a channel's data is read and
//! written, so SSH's own traffic (keepalives, window adjustments, the encryption and the
//! packet headers) is in none of them: it belongs to no channel. The transport's own count
//! of an SSH connection is kept apart (`ssh::abortable`).
//!
//! A [`Reporter`] sends a count to the tabs showing it after it changes: at once after a quiet
//! spell, then at most every [`REPORT_INTERVAL`] while bytes keep coming. That is shorter than
//! the frontend keeps a light on after the last byte, so a steady stream keeps it steady.

use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use ts_rs::TS;

use crate::session::{SessionEvent, SessionId, SessionSink};

/// The least time between two reports of a count.
pub const REPORT_INTERVAL: Duration = Duration::from_millis(50);

/// What a meter has counted so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub sent: u64,
    pub received: u64,
    /// When something was last sent and received, in milliseconds since the Unix epoch (as
    /// JavaScript's `Date.now()`), if it has been.
    pub last_sent: Option<u64>,
    pub last_received: Option<u64>,
}

/// Wakes a [`Reporter`]: shared by the meters it reports, and by whatever else it reports
/// (the channels a count is on).
#[derive(Clone, Default)]
pub struct Wake(Arc<Notify>);

impl Wake {
    pub fn wake(&self) {
        self.0.notify_one();
    }
}

/// Counts bytes sent and received, waking its reporter on each change.
pub struct Meter {
    sent: AtomicU64,
    received: AtomicU64,
    /// As in [`Counts`]; 0 if never.
    last_sent: AtomicU64,
    last_received: AtomicU64,
    wake: Wake,
}

impl Meter {
    pub fn new(wake: &Wake) -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            last_sent: AtomicU64::new(0),
            last_received: AtomicU64::new(0),
            wake: wake.clone(),
        }
    }

    pub fn sent(&self, bytes: usize) {
        if bytes > 0 {
            self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
            self.last_sent.store(now(), Ordering::Relaxed);
            self.wake.wake();
        }
    }

    pub fn received(&self, bytes: usize) {
        if bytes > 0 {
            self.received.fetch_add(bytes as u64, Ordering::Relaxed);
            self.last_received.store(now(), Ordering::Relaxed);
            self.wake.wake();
        }
    }

    /// Wakes its reporter for a change other than in the counts (the channel they are on).
    pub fn changed(&self) {
        self.wake.wake();
    }

    pub fn counts(&self) -> Counts {
        let at = |time: &AtomicU64| Some(time.load(Ordering::Relaxed)).filter(|&time| time > 0);
        Counts {
            sent: self.sent.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
            last_sent: at(&self.last_sent),
            last_received: at(&self.last_received),
        }
    }
}

/// Milliseconds since the Unix epoch.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

/// Calls its report each time it is woken, at most every [`REPORT_INTERVAL`]: wakes that come
/// meanwhile make one more report once the interval has passed. Stops when dropped.
pub struct Reporter(JoinHandle<()>);

impl Reporter {
    pub fn start(wake: &Wake, mut report: impl FnMut() + Send + 'static) -> Self {
        let woken = wake.0.clone();
        Self(tauri::async_runtime::spawn(async move {
            loop {
                woken.notified().await;
                report();
                tokio::time::sleep(REPORT_INTERVAL).await;
            }
        }))
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A stream whose reads count as received on `meter`, and its writes as sent.
pub struct Counted<S> {
    inner: S,
    meter: Arc<Meter>,
}

impl<S> Counted<S> {
    pub fn new(inner: S, meter: Arc<Meter>) -> Self {
        Self { inner, meter }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Counted<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        self.meter.received(buf.filled().len() - before);
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counted<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = poll {
            self.meter.sent(written);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// An SSH connection, as the frontend names it next to a count: a number given in the order
/// connections are made since ZShell started, and the port of its TCP connection on this
/// computer (to the first jump host or the proxy, if it goes through one), unless it runs
/// through a proxy command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub id: u32,
    pub local_port: Option<u16>,
}

impl ConnectionInfo {
    /// A new connection's.
    pub fn next(local_port: Option<u16>) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Self { id: NEXT.fetch_add(1, Ordering::Relaxed), local_port }
    }
}

/// A channel on an SSH connection: the number this side gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub connection: ConnectionInfo,
    pub channel: u32,
}

/// The tabs a count is reported to.
#[derive(Clone, Default)]
pub struct Viewers(Arc<Mutex<Vec<(SessionId, SessionSink)>>>);

impl Viewers {
    pub fn add(&self, id: SessionId, sink: SessionSink) {
        self.0.lock().unwrap().push((id, sink));
    }

    pub fn remove(&self, id: SessionId) {
        self.0.lock().unwrap().retain(|(viewer, _)| *viewer != id);
    }

    pub fn send(&self, event: &SessionEvent) {
        for (_, sink) in self.0.lock().unwrap().iter() {
            sink.event(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_counts_and_notes_when() {
        let meter = Meter::new(&Wake::default());
        assert_eq!(meter.counts(), Counts::default());
        meter.sent(3);
        meter.sent(0);
        meter.received(5);
        let counts = meter.counts();
        assert_eq!((counts.sent, counts.received), (3, 5));
        assert!(counts.last_sent.is_some_and(|at| at.abs_diff(now()) < 1000));
        assert!(counts.last_received.is_some());
    }

    #[tokio::test]
    async fn reports_come_at_once_then_at_most_every_interval() {
        let wake = Wake::default();
        let meter = Arc::new(Meter::new(&wake));
        let (reports_tx, mut reports) = tokio::sync::mpsc::unbounded_channel();
        let counted = meter.clone();
        let _reporter = Reporter::start(&wake, move || {
            let _ = reports_tx.send((std::time::Instant::now(), counted.counts().sent));
        });
        let started = std::time::Instant::now();
        meter.sent(1);
        let (first, sent) = reports.recv().await.unwrap();
        assert_eq!(sent, 1);
        assert!(first - started < REPORT_INTERVAL);
        // Many changes within the interval make one report, with all of them.
        for _ in 0..10 {
            meter.sent(1);
        }
        let (second, sent) = reports.recv().await.unwrap();
        assert_eq!(sent, 11);
        assert!(second - first >= REPORT_INTERVAL);
        assert!(tokio::time::timeout(REPORT_INTERVAL * 3, reports.recv()).await.is_err());
    }

    #[tokio::test]
    async fn a_counted_stream_counts_both_ways() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let meter = Arc::new(Meter::new(&Wake::default()));
        let (ours, mut theirs) = tokio::io::duplex(64);
        let mut counted = Counted::new(ours, meter.clone());
        counted.write_all(b"hello").await.unwrap();
        theirs.write_all(b"hi").await.unwrap();
        let mut read = [0; 2];
        counted.read_exact(&mut read).await.unwrap();
        assert_eq!((meter.counts().sent, meter.counts().received), (5, 2));
    }
}
