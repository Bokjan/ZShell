//! The SFTP sessions of an SSH connection: one for browsing and editing, one for transfers,
//! each on a channel of its own, opened on first use and again if its channel ends while the
//! connection stays. What each channel carries is counted and reported to the connection's
//! tabs (see `crate::traffic`).

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::Context;
use russh::client::Msg;
use russh::{Channel, ChannelMsg};
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::transfer::TransferSlots;
use crate::error::Error;
use crate::session::{SessionEvent, SftpChannelTraffic, Viewers};
use crate::ssh;
use crate::traffic::{ConnectionInfo, Counted, Meter, Reporter, Wake};

/// How long an SFTP request may wait for its reply. Generous, since on a slow link a request
/// waits behind the reads queued before it: russh-sftp's default of 10 s failed downloads
/// below about 400 KB/s. A lost connection doesn't wait for it: once the keepalives (or TCP)
/// end the connection, the channel closes and the waiting requests fail.
const SFTP_REQUEST_TIMEOUT_SECS: u64 = 120;
/// How long the server may take to open an SFTP session's channel and start the subsystem on
/// it. Usually a moment, but the answers wait behind what the server already sends on the
/// connection: on a slow link, behind a transfer's full channel window (2 MiB), this is reached
/// below about 70 KB/s. One that never answers would otherwise hold every SFTP operation on the
/// connection, waiting for the session, until the connection ends. Longer than a check of the
/// connection (`ssh::liveness`), so that one gone dead ends as lost first; a tenth in tests, as
/// those checks are.
const SFTP_OPEN_TIMEOUT: Duration = Duration::from_millis(if cfg!(test) { 3000 } else { 30000 });
/// Reads kept in flight per file. The SSH channel window (2 MiB) already limits what is on
/// the way, so more only makes later requests (listing a folder) queue longer.
const SFTP_CONCURRENT_READS: usize = 8;
/// Writes kept in flight per file on the transfer channel: 64 of 32 KiB fill the server's
/// window (2 MiB on OpenSSH) as `sftp` does; russh-sftp's 16 kept uploads to a quarter of the
/// speed of downloads on a distant server. The browsing channel keeps 16, as a folder listed
/// while an edited file uploads waits behind its writes.
const SFTP_CONCURRENT_TRANSFER_WRITES: usize = 64;

/// An SFTP session, once opened, and whether its channel has ended since.
type Slot = tokio::sync::Mutex<Option<(Arc<SftpSession>, Arc<AtomicBool>)>>;

/// The SFTP sessions of one SSH connection.
pub struct Channels {
    browse: Slot,
    transfer: Slot,
    /// The files its transfers copy at once, all together (see `transfer::copy_files`).
    transfer_slots: Arc<TransferSlots>,
    traffic: Arc<Traffic>,
    _reporter: Reporter,
}

/// What the two channels carry, and which channels they are.
struct Traffic {
    browse: Arc<Meter>,
    transfer: Arc<Meter>,
    browse_channel: Mutex<Option<u32>>,
    transfer_channel: Mutex<Option<u32>>,
    /// Transfers go on the browsing channel: the server refused them their own.
    shared: AtomicBool,
    wake: Wake,
}

impl Traffic {
    fn new(wake: &Wake) -> Self {
        Self {
            browse: Arc::new(Meter::new(wake)),
            transfer: Arc::new(Meter::new(wake)),
            browse_channel: Mutex::default(),
            transfer_channel: Mutex::default(),
            shared: AtomicBool::new(false),
            wake: wake.clone(),
        }
    }

    fn event(&self, connection: ConnectionInfo) -> SessionEvent {
        let browse = SftpChannelTraffic { counts: self.browse.counts(), channel: *self.browse_channel.lock().unwrap() };
        let transfer = match self.shared.load(Ordering::Relaxed) {
            true => browse,
            false => SftpChannelTraffic { counts: self.transfer.counts(), channel: *self.transfer_channel.lock().unwrap() },
        };
        SessionEvent::SftpTraffic { connection, browse, transfer }
    }

    fn set_shared(&self, shared: bool) {
        if self.shared.swap(shared, Ordering::Relaxed) != shared {
            self.wake.wake();
        }
    }
}

impl Channels {
    /// For `connection`, whose tabs are `viewers`.
    pub fn new(connection: ConnectionInfo, viewers: Viewers) -> Self {
        let wake = Wake::default();
        let traffic = Arc::new(Traffic::new(&wake));
        let reported = traffic.clone();
        let reporter = Reporter::start(&wake, move || viewers.send(&reported.event(connection)));
        Self { browse: Slot::default(), transfer: Slot::default(), transfer_slots: Arc::default(), traffic, _reporter: reporter }
    }

    /// What the channels have carried so far, for a tab that starts showing it; `None` before
    /// any has opened.
    pub fn traffic(&self, connection: ConnectionInfo) -> Option<SessionEvent> {
        // Its own statement: a guard taken in the `then` chain would live through `event`,
        // which locks the same channel again.
        let opened = self.traffic.browse_channel.lock().unwrap().is_some();
        opened.then(|| self.traffic.event(connection))
    }

    /// What its transfers copy files at once by, shared: two large files downloaded at once
    /// would otherwise each keep the channel's window full, as if alone.
    pub fn transfer_slots(&self) -> Arc<TransferSlots> {
        self.transfer_slots.clone()
    }

    /// The SFTP session for browsing (and editing) on `connection`, opened on first use.
    pub async fn browse(&self, connection: &ssh::Connection) -> anyhow::Result<Arc<SftpSession>> {
        let traffic = &self.traffic;
        open(&self.browse, connection, config(), (&traffic.browse, &traffic.browse_channel)).await
    }

    /// The SFTP session for transfers (uploads, downloads, drags out of the window), on a
    /// channel of its own: requests on one channel are answered in order, so a folder
    /// listed during a large transfer would wait behind the transfer's reads. The server
    /// may refuse another channel (`MaxSessions`); transfers then share the browsing one.
    pub async fn transfer(&self, connection: &ssh::Connection) -> anyhow::Result<Arc<SftpSession>> {
        let config = SftpConfig { max_concurrent_writes: SFTP_CONCURRENT_TRANSFER_WRITES, ..config() };
        let traffic = &self.traffic;
        match open(&self.transfer, connection, config, (&traffic.transfer, &traffic.transfer_channel)).await {
            Ok(sftp) => {
                traffic.set_shared(false);
                Ok(sftp)
            }
            Err(_) => {
                traffic.set_shared(true);
                self.browse(connection).await
            }
        }
    }

    /// Closes the sessions, before the connection disconnects. A slot that is busy is
    /// opening a session, which may take a while (up to [`SFTP_OPEN_TIMEOUT`], then the
    /// SFTP handshake): the disconnection ends it anyway.
    pub async fn close(&self) {
        for slot in [&self.browse, &self.transfer] {
            if let Some((sftp, _)) = slot.try_lock().ok().and_then(|mut slot| slot.take()) {
                let _ = sftp.close().await;
            }
        }
    }
}

/// The SFTP session kept in `slot`, opened on first use, and again if its channel ended
/// while the connection stays (the server's `sftp-server` exited, say): otherwise every SFTP
/// operation would fail until the tab reconnects.
fn config() -> SftpConfig {
    SftpConfig { request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS, max_concurrent_reads: SFTP_CONCURRENT_READS, ..Default::default() }
}

/// `traffic`: the meter its channel counts on, and where to note which channel it is.
async fn open(
    slot: &Slot,
    connection: &ssh::Connection,
    config: SftpConfig,
    traffic: (&Arc<Meter>, &Mutex<Option<u32>>),
) -> anyhow::Result<Arc<SftpSession>> {
    let mut cached = slot.lock().await;
    if let Some((sftp, ended)) = cached.as_ref() {
        if !ended.load(Ordering::Relaxed) {
            return Ok(sftp.clone());
        }
    }
    let channel = subsystem(connection).await?;
    let ended = Arc::new(AtomicBool::new(false));
    let encoding = crate::encoding::for_profile(&connection.profile().encoding);
    let (meter, noted) = traffic;
    *noted.lock().unwrap() = Some(channel.id().into());
    meter.changed();
    let counted = Counted::new(channel.into_stream(), meter.clone());
    let stream = Watched { inner: super::names::convert(counted, encoding), ended: ended.clone() };
    let sftp = Arc::new(SftpSession::new_with_config(stream, config).await.context(Error::new("sftp.unsupported"))?);
    *cached = Some((sftp.clone(), ended));
    Ok(sftp)
}

/// A channel on `connection` with the SFTP subsystem started, within [`SFTP_OPEN_TIMEOUT`].
/// Fails with `sftp.openFailed` (russh's error underneath), `sftp.unsupported` or
/// `sftp.openTimeout`. A channel given up on is closed, rather than left open at the server,
/// where it counts against `MaxSessions`.
async fn subsystem(connection: &ssh::Connection) -> anyhow::Result<Channel<Msg>> {
    let deadline = tokio::time::Instant::now() + SFTP_OPEN_TIMEOUT;
    let handle = connection.handle().clone();
    let mut opening = Box::pin(async move { handle.channel_open_session().await });
    let mut channel = match tokio::time::timeout_at(deadline, &mut opening).await {
        Ok(opened) => opened.context(Error::new("sftp.openFailed"))?,
        Err(_) => {
            // The server may open it yet: closed then. Waiting ends with the connection at the
            // latest.
            tokio::spawn(async move {
                if let Ok(channel) = opening.await {
                    let _ = channel.close().await;
                }
            });
            return Err(Error::new("sftp.openTimeout").into());
        }
    };
    // Sent without waiting for the answer, which comes below: this fails only when the
    // connection is gone.
    channel.request_subsystem(true, "sftp").await.context(Error::new("net.connectionLost"))?;
    // A server without the subsystem answers with a failure but keeps the channel open, and
    // russh-sftp would wait for its greeting until the request times out.
    let started = tokio::time::timeout_at(deadline, async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => return true,
                Some(ChannelMsg::Failure | ChannelMsg::Close | ChannelMsg::Eof) | None => return false,
                Some(_) => {}
            }
        }
    })
    .await;
    if started == Ok(true) {
        return Ok(channel);
    }
    let _ = channel.close().await;
    Err(Error::new(if started.is_ok() { "sftp.unsupported" } else { "sftp.openTimeout" }).into())
}

/// An SFTP session's stream, noting when reading from it ends: russh-sftp doesn't tell.
struct Watched {
    inner: Box<dyn super::names::Stream>,
    ended: Arc<AtomicBool>,
}

impl AsyncRead for Watched {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        let eof = matches!(result, Poll::Ready(Ok(()))) && buf.filled().len() == before && buf.remaining() > 0;
        if eof || matches!(result, Poll::Ready(Err(_))) {
            self.ended.store(true, Ordering::Relaxed);
        }
        result
    }
}

impl AsyncWrite for Watched {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::ssh::testing::{connect, Unanswering};

    /// A tab that joins a connection whose file panel has a channel open gets its traffic,
    /// rather than hanging the thread that opens it (and every command waiting on that).
    #[test]
    fn traffic_once_a_channel_is_open() {
        let connection = ConnectionInfo::next(None);
        let channels = Arc::new(Channels::new(connection, Viewers::default()));
        assert!(channels.traffic(connection).is_none());
        *channels.traffic.browse_channel.lock().unwrap() = Some(3);
        let (sender, receiver) = mpsc::channel();
        let asked = channels.clone();
        std::thread::spawn(move || sender.send(asked.traffic(connection).is_some()));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(true));
    }

    /// A server that is up but never answers opening an SFTP channel, or starting the subsystem
    /// on it, fails each operation in time, and the next one tries again rather than waiting
    /// behind it for good. The channels given up on are closed, also those opened too late.
    #[tokio::test]
    async fn unanswered_open_times_out() {
        let late = Some(SFTP_OPEN_TIMEOUT + Duration::from_millis(500));
        tokio::join!(unanswered(None, 0), unanswered(late, 2), unanswered(Some(Duration::ZERO), 2));
    }

    /// Two operations at once on a server opening channels after `open`, which closes `closes`.
    async fn unanswered(open: Option<Duration>, closes: usize) {
        let code = |result: anyhow::Result<Arc<SftpSession>>| result.err().and_then(|e| e.downcast_ref::<Error>().map(Error::code));
        let (server, closed) = Unanswering::new(open);
        let connection = connect(server, "").await;
        let channels = Channels::new(connection.info(), Viewers::default());
        let both = async { tokio::join!(channels.browse(&connection), channels.browse(&connection)) };
        let (first, second) = tokio::time::timeout(SFTP_OPEN_TIMEOUT * 3, both).await.expect("both gave up in time");
        assert_eq!((code(first), code(second)), (Some("sftp.openTimeout"), Some("sftp.openTimeout")), "{open:?}");
        assert!(channels.browse.try_lock().unwrap().is_none());
        let all_closed = async {
            while closed.load(Ordering::Relaxed) < closes {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::time::timeout(SFTP_OPEN_TIMEOUT, all_closed).await.expect("the channels were closed");
    }
}
