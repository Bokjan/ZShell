//! Noticing soon that a connection has gone dead, when the user would: as they type and nothing
//! comes back, as an SFTP command waits, as a forward's client waits for its channel or for an
//! answer to what it sent, and as the system wakes from sleep (when the network may have
//! changed under the connection). Keepalives alone take minutes:
//! they are sent every interval and given up on after several, and not at all while russh's
//! connection task waits on a write the system can't send.
//!
//! A check waits a moment for anything from the server, then asks it for a reply (a keepalive
//! that wants one) and waits for anything again: the reply, or whatever else comes first,
//! output or a transfer's data, or room to write that the server's acknowledgements made (russh
//! reads nothing while it writes, which takes long on a slow link). The server is taken for
//! gone, and the transport ended (see `super::abortable`), which ends the connection as a lost
//! one, when nothing comes while russh waits for it, and either what russh sent after the
//! request went unanswered for the whole wait, or russh waited for room to write all along.
//!
//! Otherwise the connection is left alone: russh may be held up on this side (a terminal behind
//! on its output, see `super::bridge`), when it neither reads nor sends; or it may hold back the
//! request, as it holds back all we send while a channel is out of window (the server is quiet
//! then, but may well be there). Only one check runs at a time.
//!
//! Like keepalives, checks are off for a session with keepalives turned off.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::time::Instant;

use super::abortable::Link;
use super::SshHandle;

/// How long a check waits: first for anything from the server, then for an answer to the
/// keepalive it sends. A tenth of it in tests, which wait for checks.
struct Wait {
    before: Duration,
    answer: Duration,
}

const fn millis(ms: u64) -> Duration {
    Duration::from_millis(if cfg!(test) { ms / 10 } else { ms })
}

/// After sending something the server answers (typing, which it echoes, or opening a channel).
const REPLY: Wait = Wait { before: millis(2000), answer: millis(8000) };

/// After the system wakes, longer: the network may still be joining, and TCP's retransmissions
/// wait longer and longer meanwhile.
const WAKE: Wait = Wait { before: millis(2000), answer: millis(15000) };

/// How often a check looks for something read.
const LOOK: Duration = millis(100);

/// A connection's checks; cheap to clone.
#[derive(Clone)]
pub struct Liveness(Option<Arc<Inner>>);

struct Inner {
    /// The connection's number, for the debug log.
    conn: u32,
    link: Link,
    /// Weak, so that a check doesn't keep a connection that closes meanwhile.
    handle: Weak<SshHandle>,
    checking: AtomicBool,
}

impl Liveness {
    /// Checks over `link`, the transport of `handle`'s connection `conn`, unless `enabled` is
    /// false.
    pub fn new(conn: u32, link: Link, handle: &Arc<SshHandle>, enabled: bool) -> Self {
        Self(enabled.then(|| Arc::new(Inner { conn, link, handle: Arc::downgrade(handle), checking: AtomicBool::new(false) })))
    }

    /// Something the server answers was just sent: checks the connection unless the answer
    /// comes soon.
    pub fn expect_reply(&self) {
        self.check(REPLY);
    }

    /// The system has woken from sleep.
    pub fn woke(&self) {
        self.check(WAKE);
    }

    fn check(&self, wait: Wait) {
        let Some(inner) = &self.0 else { return };
        if inner.checking.swap(true, Ordering::Relaxed) {
            return;
        }
        let inner = inner.clone();
        tauri::async_runtime::spawn(async move {
            inner.check(wait).await;
            inner.checking.store(false, Ordering::Relaxed);
        });
    }
}

impl Inner {
    async fn check(&self, wait: Wait) {
        let heard = self.link.heard();
        if self.heard_within(heard, wait.before).await {
            return;
        }
        let Some(handle) = self.handle.upgrade() else { return };
        log::debug!(conn = self.conn; "nothing from the server for {:?}: asking it for a reply", wait.before);
        let sent = self.link.sent();
        // On a task of its own: it waits for the connection task, which may be stuck. The
        // answer is noticed as something read, as anything else is.
        let ping = tauri::async_runtime::spawn(async move {
            let _ = handle.send_ping().await;
        });
        let gone = self.unanswered(heard, sent, wait.answer).await;
        ping.abort();
        if gone {
            log::warn!(conn = self.conn; "the server didn't answer in {:?}: the connection is taken for lost", wait.answer);
            self.link.end(io::ErrorKind::TimedOut);
        } else {
            log::debug!(conn = self.conn; "the server answered");
        }
    }

    /// Whether anything is heard after the first `heard` signs, within `time`.
    async fn heard_within(&self, heard: u64, time: Duration) -> bool {
        let deadline = Instant::now() + time;
        loop {
            if self.link.heard() != heard {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            tokio::time::sleep(LOOK.min(deadline - now)).await;
        }
    }

    /// Whether the server stays silent, with nothing heard after the first `heard` signs, while
    /// russh waits for it: for `answer` after the first write past the first `sent` (the request
    /// just made, or what went before it), or for `answer` with no room to write meanwhile.
    async fn unanswered(&self, heard: u64, sent: u64, answer: Duration) -> bool {
        let start = Instant::now();
        let mut went_out = None;
        loop {
            if self.link.heard() != heard {
                return false;
            }
            let now = Instant::now();
            if went_out.is_none() && self.link.sent() != sent {
                went_out = Some(now);
            }
            let deadline = went_out.unwrap_or(start) + answer;
            if now >= deadline {
                return self.link.waiting() && (went_out.is_some() || self.link.writing());
            }
            tokio::time::sleep(LOOK.min(deadline - now)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{self, Anyone};
    use super::*;

    /// How long a check takes at most, in tests.
    const CHECK: Duration = millis(2000 + 2 * 15000 + 1000);

    /// A connection whose transport stalls is ended by a check, with keepalives far off.
    #[tokio::test]
    async fn a_check_ends_a_dead_connection() {
        let (connection, stall) = testing::connect_stalling(Anyone, Duration::from_secs(3600)).await;
        let mut reason = connection.disconnect_reason();
        stall.now();
        connection.liveness().expect_reply();
        let ended = tokio::time::timeout(CHECK, reason.wait_for(Option::is_some)).await;
        assert!(ended.is_ok(), "the connection is still open");
    }

    /// A server that answers keeps its connection, though it sends nothing by itself.
    #[tokio::test]
    async fn a_check_leaves_a_quiet_connection_open() {
        let (connection, _stall) = testing::connect_stalling(Anyone, Duration::from_secs(3600)).await;
        connection.liveness().woke();
        tokio::time::sleep(CHECK).await;
        assert!(connection.disconnect_reason().borrow().is_none());
        assert!(!connection.handle().is_closed());
    }

    /// A connection on a slow link, where russh writes for long without reading anything, is
    /// left open: the room the server makes as it takes what was sent tells that it is there.
    #[tokio::test]
    async fn a_check_leaves_a_slow_connection_open() {
        let (connection, stall) = testing::connect_stalling(testing::Accepting::default(), Duration::from_secs(3600)).await;
        let channel = connection.handle().channel_open_direct_tcpip("db", 5432, "127.0.0.1", 50000).await.unwrap();
        stall.slow();
        tokio::spawn(async move {
            let data = vec![0; 8 << 20];
            let _ = channel.data(&data[..]).await;
        });
        // Until russh writes what the channel's window allows, which takes many seconds here.
        tokio::time::sleep(Duration::from_millis(200)).await;
        connection.liveness().expect_reply();
        tokio::time::sleep(CHECK).await;
        assert!(connection.disconnect_reason().borrow().is_none());
    }

    /// What a check makes of a transport: a server silent while what was sent after the request
    /// waits for an answer, or while the transport waits for room to write, is gone; a request
    /// that never went out (held back by russh, or russh held up on this side) tells nothing.
    #[tokio::test]
    async fn a_check_tells_a_silent_server_from_a_request_held_back() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        const ANSWER: Duration = Duration::from_millis(300);
        let (stream, mut peer) = tokio::io::duplex(64);
        let (stream, guard) = super::super::abortable::abortable(stream, None);
        let inner = Inner { conn: 0, link: guard.keep(), handle: Weak::new(), checking: AtomicBool::new(false) };
        let unanswered = || inner.unanswered(inner.link.heard(), inner.link.sent(), ANSWER);
        let (mut reader, mut writer) = tokio::io::split(stream);

        // Neither reading nor writing: held up on this side.
        assert!(!unanswered().await);
        // Waiting for the server, and sending nothing: the request is held back.
        tokio::spawn(async move {
            let mut buf = [0; 64];
            while reader.read(&mut buf).await.is_ok_and(|n| n > 0) {}
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!unanswered().await);

        // What is sent goes unanswered, or is answered.
        let (heard, sent) = (inner.link.heard(), inner.link.sent());
        writer.write_all(b"ping").await.unwrap();
        assert!(inner.unanswered(heard, sent, ANSWER).await);
        let (heard, sent) = (inner.link.heard(), inner.link.sent());
        writer.write_all(b"ping").await.unwrap();
        peer.write_all(b"pong").await.unwrap();
        assert!(!inner.unanswered(heard, sent, ANSWER).await);

        // Waiting for room to write all along, the server taking nothing.
        tokio::spawn(async move { writer.write_all(&[0; 128]).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(inner.link.writing());
        assert!(unanswered().await);
    }
}
