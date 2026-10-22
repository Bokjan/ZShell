//! Noticing soon that a connection has gone dead, when the user would: as they type and nothing
//! comes back, as an SFTP command waits, as a forward's client waits for its channel or for an
//! answer to what it sent, and as the system wakes from sleep (when the network may have
//! changed under the connection). Keepalives alone take minutes:
//! they are sent every interval and given up on after several, and not at all while russh's
//! connection task waits on a write the system can't send.
//!
//! A check waits a moment for anything from the server, then asks it for a reply (a keepalive
//! that wants one) and waits for anything again: the reply, or whatever else comes first,
//! output or a transfer's data. Nothing at all, while russh waits for the server, ends the
//! transport (see `super::abortable`), which ends the connection as a lost one. Nothing while
//! russh is held up on this side (a terminal behind on its output, see `super::bridge`) is not
//! the server's doing, and the connection is left alone. Only one check runs at a time.
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
    link: Link,
    /// Weak, so that a check doesn't keep a connection that closes meanwhile.
    handle: Weak<SshHandle>,
    checking: AtomicBool,
}

impl Liveness {
    /// Checks over `link`, the transport of `handle`'s connection, unless `enabled` is false.
    pub fn new(link: Link, handle: &Arc<SshHandle>, enabled: bool) -> Self {
        Self(enabled.then(|| Arc::new(Inner { link, handle: Arc::downgrade(handle), checking: AtomicBool::new(false) })))
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
        let before = self.link.received();
        if self.heard(before, wait.before).await {
            return;
        }
        let Some(handle) = self.handle.upgrade() else { return };
        // On a task of its own: it waits for the connection task, which may be stuck. The
        // answer is noticed as something read, as anything else is.
        let ping = tauri::async_runtime::spawn(async move {
            let _ = handle.send_ping().await;
        });
        let answered = self.heard(before, wait.answer).await;
        ping.abort();
        if !answered && self.link.waiting() {
            self.link.end(io::ErrorKind::TimedOut);
        }
    }

    /// Whether anything is read after the first `before` reads, within `time`.
    async fn heard(&self, before: u64, time: Duration) -> bool {
        let deadline = Instant::now() + time;
        loop {
            if self.link.received() != before {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
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
    const CHECK: Duration = millis(2000 + 15000 + 1000);

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
}
