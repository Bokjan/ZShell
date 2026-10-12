//! Releasing what a session holds beyond its task.

use std::sync::{Arc, Mutex};

type Release = Box<dyn FnOnce() + Send>;

/// What a session holds beyond its task (its SSH connection's registration, a waiting ZMODEM
/// transfer, the flow control its reader may wait on), as the releases to run when it closes.
///
/// A task or thread a backend starts besides its own either stops here (registered with
/// [`TermIo::on_close`](super::TermIo::on_close): a Telnet connection's writer) or ends by itself once the session's
/// side of it is dropped (a PTY's or serial port's reader and writer, whose input channel
/// closes and whose reads time out); otherwise it could outlive the session, holding its
/// connection open.
#[derive(Clone)]
pub struct Lifetime(Arc<Mutex<Option<Vec<Release>>>>);

impl Lifetime {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(Some(Vec::new()))))
    }

    /// Runs `release` when the session closes; right away if it has already. Something the
    /// task registers just as the session closes is thereby released all the same.
    pub fn on_close(&self, release: impl FnOnce() + Send + 'static) {
        let mut releases = self.0.lock().unwrap();
        match releases.as_mut() {
            Some(releases) => releases.push(Box::new(release)),
            None => {
                drop(releases);
                release();
            }
        }
    }

    /// Runs the releases, once.
    pub(super) fn close(&self) {
        let releases = self.0.lock().unwrap().take();
        for release in releases.into_iter().flatten() {
            release();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Something registered once the session has closed (a task that connected just as its
    /// tab closed) is released at once.
    #[test]
    fn a_closed_lifetime_releases_at_once() {
        let lifetime = Lifetime::new();
        let released = Arc::new(AtomicUsize::new(0));
        let count = {
            let released = released.clone();
            move || released.load(Ordering::SeqCst)
        };
        let early = released.clone();
        lifetime.on_close(move || {
            early.fetch_add(1, Ordering::SeqCst);
        });
        lifetime.close();
        lifetime.close();
        assert_eq!(count(), 1);
        lifetime.on_close(move || {
            released.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(count(), 2);
    }
}
