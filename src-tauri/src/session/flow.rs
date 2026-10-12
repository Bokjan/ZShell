//! Output flow control (see the module documentation of `session`).

use std::sync::{Condvar, Mutex};

/// Pause reading once this many output bytes are unacknowledged...
const FLOW_HIGH: usize = 2 << 20;
/// ...and resume once the frontend has caught up to this many.
const FLOW_LOW: usize = 512 << 10;

/// Output flow control: counts the bytes sent to the frontend (or to a ZMODEM transfer) and
/// not yet acknowledged. Every backend stops reading while the count is too high, so the
/// output of a fast remote program (`cat` of a large file) waits on the remote side rather
/// than piling up in memory: in Tauri's IPC queue, which has no limit, and in xterm.js, which
/// drops writes past its own.
#[derive(Default)]
pub struct Flow {
    state: Mutex<FlowState>,
    /// Wakes reader threads ([`Flow::wait_ready`])...
    resumed: Condvar,
    /// ...and reading tasks ([`Flow::ready`]).
    resumed_async: tokio::sync::Notify,
}

#[derive(Default)]
struct FlowState {
    unacked: usize,
    /// Over [`FLOW_HIGH`] since it was last at most [`FLOW_LOW`].
    paused: bool,
    closed: bool,
}

impl FlowState {
    fn waits(&self) -> bool {
        self.paused && !self.closed
    }
}

impl Flow {
    pub fn sent(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_add(bytes);
        if state.unacked >= FLOW_HIGH {
            state.paused = true;
        }
    }

    pub fn ack(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_sub(bytes);
        if state.paused && state.unacked <= FLOW_LOW {
            state.paused = false;
            self.resumed.notify_all();
            self.resumed_async.notify_waiters();
        }
    }

    /// Whether a reader should wait before reading more.
    pub fn is_paused(&self) -> bool {
        self.state.lock().unwrap().waits()
    }

    /// Blocks while the frontend is too far behind, for backends reading on their own
    /// thread; returns at once when the session is closing.
    pub fn wait_ready(&self) {
        let _state = self.resumed.wait_while(self.state.lock().unwrap(), |state| state.waits()).unwrap();
    }

    /// [`Flow::wait_ready`] for backends reading in their task.
    pub async fn ready(&self) {
        loop {
            let resumed = self.resumed_async.notified();
            tokio::pin!(resumed);
            // Registered before checking, so that a resume in between is not missed.
            resumed.as_mut().enable();
            if !self.is_paused() {
                return;
            }
            resumed.await;
        }
    }

    /// Forgets what is unacknowledged: the session moved to another tab, which acknowledges
    /// only the output it receives.
    pub(super) fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.unacked = 0;
        if state.paused {
            state.paused = false;
            self.resumed.notify_all();
            self.resumed_async.notify_waiters();
        }
    }

    /// What is unacknowledged.
    #[cfg(test)]
    pub(super) fn unacked(&self) -> usize {
        self.state.lock().unwrap().unacked
    }

    /// Stops all waiting, for good (the session is closing).
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.resumed.notify_all();
        self.resumed_async.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Reading pauses once too much is unacknowledged and resumes only once the frontend has
    /// caught up most of the way, for threads and tasks alike.
    #[tokio::test(flavor = "multi_thread")]
    async fn flow_pauses_and_resumes() {
        let flow = Arc::new(Flow::default());
        flow.sent(FLOW_HIGH - 1);
        assert!(!flow.is_paused());
        flow.sent(1);
        assert!(flow.is_paused());
        let waiting = tokio::spawn({
            let flow = flow.clone();
            async move { flow.ready().await }
        });
        let thread = std::thread::spawn({
            let flow = flow.clone();
            move || flow.wait_ready()
        });
        // Below the high mark, but not yet at the low one.
        flow.ack(FLOW_HIGH - FLOW_LOW - 1);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(flow.is_paused() && !waiting.is_finished() && !thread.is_finished());
        flow.ack(1);
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting).await.unwrap().unwrap();
        thread.join().unwrap();

        // Closing the session wakes waiters too.
        flow.sent(FLOW_HIGH);
        let waiting = tokio::spawn({
            let flow = flow.clone();
            async move { flow.ready().await }
        });
        flow.close();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting).await.unwrap().unwrap();
    }
}
