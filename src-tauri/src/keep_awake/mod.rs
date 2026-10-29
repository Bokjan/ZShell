//! Keeping the system from sleeping while sessions are connected, as the `keepAwake` setting
//! says: never, while the computer runs on external power, or always. Only idle sleep is
//! prevented: the display still turns off and the screen locks, and closing the lid or
//! choosing to sleep still puts the system to sleep, on every platform.
//!
//! Each window tells whether any of its tabs has a remote session connected, connecting or
//! waiting to reconnect (`window_set_connected`); the system is kept awake while one does.
//! Waiting to reconnect counts: the system has long been idle by then, and would sleep at
//! once between two attempts.
//!
//! The system is held and released on a thread of its own, which follows the latest state:
//! the callers, the main thread among them, never wait for the system (or for D-Bus).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use win as platform;

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::model::KeepAwakeMode;

/// Keeps the system awake until dropped.
type Hold = Box<dyn Send>;

/// Called with whether the computer runs on external power: first as watching starts, then on
/// changes.
type PowerChanged = Box<dyn Fn(bool) + Send + Sync>;

pub struct KeepAwake {
    state: Arc<Mutex<State>>,
    /// The thread that holds and releases the system (see `work`).
    worker: Sender<Message>,
}

struct State {
    mode: KeepAwakeMode,
    /// True until known otherwise: a computer without a battery, or one whose power source
    /// can't be told, counts as plugged in.
    external_power: bool,
    /// The windows with tabs that are connected (by label).
    connected: HashSet<String>,
}

impl State {
    fn wanted(&self) -> bool {
        !self.connected.is_empty()
            && match self.mode {
                KeepAwakeMode::Off => false,
                KeepAwakeMode::PluggedIn => self.external_power,
                KeepAwakeMode::Always => true,
            }
    }
}

enum Message {
    /// The state has changed, or the language: hold or release the system to match.
    Apply,
    /// Whether the system is held, once the messages before have been handled.
    #[cfg(test)]
    Holding(Sender<bool>),
}

impl KeepAwake {
    /// `reason` is the text the system shows with the hold; `hold` asks the system to stay
    /// awake, saying why, and gives `None` if it can't.
    fn new(
        mode: KeepAwakeMode,
        reason: impl Fn() -> String + Send + 'static,
        hold: impl Fn(&str) -> Option<Hold> + Send + 'static,
    ) -> Self {
        let state = Arc::new(Mutex::new(State { mode, external_power: true, connected: HashSet::new() }));
        let (worker, messages) = mpsc::channel();
        let watched = state.clone();
        std::thread::spawn(move || work(&watched, reason, hold, messages));
        Self { state, worker }
    }

    pub fn for_platform(mode: KeepAwakeMode) -> Self {
        Self::new(mode, || t!("keepAwake.reason"), platform::hold)
    }

    /// Also once the language has changed: a hold made since gives its reason in the new one.
    pub fn set_mode(&self, mode: KeepAwakeMode) {
        self.update(|state| state.mode = mode);
    }

    /// Window `label` has tabs that are connected, or none (also once it is gone).
    pub fn set_connected(&self, label: &str, connected: bool) {
        self.update(|state| match connected {
            true => state.connected.insert(label.to_owned()),
            false => state.connected.remove(label),
        });
    }

    fn set_external_power(&self, external_power: bool) {
        self.update(|state| state.external_power = external_power);
    }

    /// Changes the state, then has the worker hold or release the system to match it.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> T) {
        change(&mut self.state.lock().unwrap());
        let _ = self.worker.send(Message::Apply);
    }

    #[cfg(test)]
    fn holding(&self) -> bool {
        let (sender, receiver) = mpsc::channel();
        self.worker.send(Message::Holding(sender)).unwrap();
        receiver.recv().unwrap()
    }
}

/// Holds the system while the state wants it, asking again on each change if the system
/// refused; a hold whose reason is no longer in the app's language is replaced, the new one
/// made before the old one goes, so that the system is never let go in between.
fn work(state: &Mutex<State>, reason: impl Fn() -> String, hold: impl Fn(&str) -> Option<Hold>, messages: Receiver<Message>) {
    // With the reason it was given.
    let mut held: Option<(Hold, String)> = None;
    for message in messages {
        match message {
            Message::Apply => {
                // The latest state, whichever change sent this.
                if !state.lock().unwrap().wanted() {
                    if held.take().is_some() {
                        log::debug!("the system may sleep again");
                    }
                    continue;
                }
                let reason = reason();
                if held.as_ref().is_some_and(|(_, given)| *given == reason) {
                    continue;
                }
                if let Some(hold) = hold(&reason) {
                    if held.is_none() {
                        log::debug!("keeping the system awake");
                    }
                    held = Some((hold, reason));
                }
            }
            #[cfg(test)]
            Message::Holding(reply) => {
                let _ = reply.send(held.is_some());
            }
        }
    }
}

/// Follows the power source, for `KeepAwakeMode::PluggedIn`.
pub fn watch_power(keep_awake: Arc<KeepAwake>) {
    platform::watch_power(Box::new(move |external_power| keep_awake.set_external_power(external_power)));
}

/// Whether the system ends the hold soon after its sleep time while on battery, as Windows does
/// on computers with Modern Standby: the settings say so.
pub fn limited_on_battery() -> bool {
    platform::limited_on_battery()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A system that counts the holds it gives and those still held.
    #[derive(Default)]
    struct Counts {
        given: AtomicUsize,
        held: AtomicUsize,
    }

    struct FakeHold(Arc<Counts>);

    impl Drop for FakeHold {
        fn drop(&mut self) {
            self.0.held.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn keep_awake_saying(mode: KeepAwakeMode, reason: Arc<Mutex<String>>) -> (KeepAwake, Arc<Counts>) {
        let counts = Arc::new(Counts::default());
        let system = counts.clone();
        let keep_awake = KeepAwake::new(
            mode,
            move || reason.lock().unwrap().clone(),
            move |reason| {
                assert!(!reason.is_empty());
                system.given.fetch_add(1, Ordering::SeqCst);
                system.held.fetch_add(1, Ordering::SeqCst);
                Some(Box::new(FakeHold(system.clone())) as Hold)
            },
        );
        (keep_awake, counts)
    }

    fn keep_awake(mode: KeepAwakeMode) -> (KeepAwake, Arc<Counts>) {
        keep_awake_saying(mode, Arc::new(Mutex::new("Connected".to_owned())))
    }

    #[test]
    fn holds_while_a_window_is_connected() {
        let (keep_awake, counts) = keep_awake(KeepAwakeMode::Always);
        assert!(!keep_awake.holding());
        keep_awake.set_connected("main", true);
        keep_awake.set_connected("window-1", true);
        // One hold for all the windows, kept as long as one is connected.
        assert!(keep_awake.holding());
        assert_eq!(counts.given.load(Ordering::SeqCst), 1);
        keep_awake.set_connected("main", false);
        assert!(keep_awake.holding());
        keep_awake.set_connected("window-1", false);
        assert!(!keep_awake.holding());
        assert_eq!(counts.held.load(Ordering::SeqCst), 0);
        // A window that was never connected changes nothing.
        keep_awake.set_connected("window-2", false);
        assert!(!keep_awake.holding());
        assert_eq!(counts.given.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn follows_the_setting() {
        let (keep_awake, counts) = keep_awake(KeepAwakeMode::Off);
        keep_awake.set_connected("main", true);
        assert!(!keep_awake.holding());
        keep_awake.set_mode(KeepAwakeMode::Always);
        assert!(keep_awake.holding());
        keep_awake.set_mode(KeepAwakeMode::PluggedIn);
        assert!(keep_awake.holding());
        keep_awake.set_mode(KeepAwakeMode::Off);
        assert!(!keep_awake.holding());
        assert_eq!(counts.given.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn plugged_in_follows_the_power_source() {
        let (keep_awake, counts) = keep_awake(KeepAwakeMode::PluggedIn);
        keep_awake.set_connected("main", true);
        assert!(keep_awake.holding());
        keep_awake.set_external_power(false);
        assert!(!keep_awake.holding());
        keep_awake.set_external_power(true);
        assert!(keep_awake.holding());
        assert_eq!(counts.given.load(Ordering::SeqCst), 2);
        // Always holds on battery too.
        keep_awake.set_external_power(false);
        keep_awake.set_mode(KeepAwakeMode::Always);
        assert!(keep_awake.holding());
    }

    #[test]
    fn a_system_that_refuses_is_asked_again_on_the_next_change() {
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        let keep_awake = KeepAwake::new(
            KeepAwakeMode::Always,
            || "Connected".to_owned(),
            move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                None
            },
        );
        keep_awake.set_connected("main", true);
        keep_awake.set_connected("window-1", true);
        assert!(!keep_awake.holding());
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_reason_follows_the_language() {
        let reason = Arc::new(Mutex::new("Connected".to_owned()));
        let (keep_awake, counts) = keep_awake_saying(KeepAwakeMode::Always, reason.clone());
        keep_awake.set_connected("main", true);
        assert!(keep_awake.holding());
        // As the settings apply the language, then the mode.
        *reason.lock().unwrap() = "已连接".to_owned();
        keep_awake.set_mode(KeepAwakeMode::Always);
        assert!(keep_awake.holding());
        // The new hold replaces the old one.
        assert_eq!(counts.given.load(Ordering::SeqCst), 2);
        assert_eq!(counts.held.load(Ordering::SeqCst), 1);
    }
}
