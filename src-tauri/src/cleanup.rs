//! Work that tidies up after itself and should get to the end before the process does: a
//! transfer cancelled as the app quits removes the files it left incomplete (a download's
//! temporary file, a ZMODEM file cut short, the name it took), which the process ending
//! first would leave behind. Quitting waits for it a little (see `quitting` in `lib.rs`).

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// How much such work is going on.
pub struct Cleanup {
    count: Mutex<usize>,
    done: Condvar,
}

/// The app's.
static CLEANUP: Cleanup = Cleanup::new();

/// Counts as going on until dropped.
#[must_use = "held while the work goes on"]
pub struct Holding(&'static Cleanup);

impl Cleanup {
    const fn new() -> Self {
        Self { count: Mutex::new(0), done: Condvar::new() }
    }

    fn hold(&'static self) -> Holding {
        *self.count.lock().unwrap() += 1;
        Holding(self)
    }

    /// Waits up to `timeout` for all of it to end; whether it did.
    fn wait(&self, timeout: Duration) -> bool {
        let count = self.count.lock().unwrap();
        let (count, _) = self.done.wait_timeout_while(count, timeout, |count| *count > 0).unwrap();
        *count == 0
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        let mut count = self.0.count.lock().unwrap();
        *count -= 1;
        if *count == 0 {
            self.0.done.notify_all();
        }
    }
}

/// Counts work that tidies up after itself as going on, while the result is held.
pub fn hold() -> Holding {
    CLEANUP.hold()
}

/// Waits up to `timeout` for such work to end; whether it did.
pub fn wait(timeout: Duration) -> bool {
    CLEANUP.wait(timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_until_all_of_it_ends_or_the_time_is_up() {
        static OURS: Cleanup = Cleanup::new();
        assert!(OURS.wait(Duration::ZERO));
        let (first, second) = (OURS.hold(), OURS.hold());
        assert!(!OURS.wait(Duration::from_millis(20)));
        drop(first);
        let ending = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(second);
        });
        assert!(OURS.wait(Duration::from_secs(5)));
        ending.join().unwrap();
    }
}
