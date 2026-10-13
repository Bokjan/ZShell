//! The app lock (M26): covers the window until the user authenticates with the system (Touch
//! ID or the login password on macOS, Windows Hello, polkit on Linux), at startup, after the system has been
//! idle for a while, when the system locks or sleeps, and on demand. Only the window is
//! locked: the sessions keep running, and the data stays decrypted in memory.
//!
//! Whether it is locked is kept here rather than in the frontend, so that a page that loads
//! again (reloaded, or after its web content process crashed) is still locked, and only an
//! authentication here unlocks it. The settings are in the vault (`Data::app_lock`), as
//! protected as the sessions: in an encrypted vault they can't be turned off by editing a
//! file, which would otherwise get past the lock at the next start.

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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::vault::{Protection, Vault};

/// The idle times that can be chosen, in minutes; 0 never locks.
const IDLE_MINUTES: [u32; 6] = [0, 1, 5, 15, 30, 60];
const DEFAULT_IDLE_MINUTES: u32 = 15;
/// How often the system's idle time is checked: the lock comes at most this late.
const IDLE_CHECK: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AppLockSettings {
    pub enabled: bool,
    /// Locks after the system (not only ZShell) has had no input for this long; 0 never.
    pub idle_minutes: u32,
    /// Locks when the system locks its screen, sleeps or switches to another user.
    pub on_system_lock: bool,
}

impl Default for AppLockSettings {
    fn default() -> Self {
        Self { enabled: false, idle_minutes: DEFAULT_IDLE_MINUTES, on_system_lock: true }
    }
}

impl AppLockSettings {
    fn normalize(mut self) -> Self {
        if !IDLE_MINUTES.contains(&self.idle_minutes) {
            self.idle_minutes = DEFAULT_IDLE_MINUTES;
        }
        self
    }
}

/// How the user unlocks, for the frontend to name it.
// Each platform makes its own.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[ts(rename = "AppLockMethod")]
#[serde(rename_all = "camelCase")]
pub enum Method {
    TouchId,
    /// macOS without Touch ID (or with it locked out): the login password.
    Password,
    WindowsHello,
    /// Linux: the desktop's authentication dialog (the account's password, or a fingerprint).
    Polkit,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppLockStatus {
    pub locked: bool,
    /// `None` while the vault is locked.
    pub settings: Option<AppLockSettings>,
    /// `None` when the system offers no way to authenticate, which `unavailable` explains.
    pub method: Option<Method>,
    pub unavailable: Option<Error>,
    /// Whether the system's idle time can be known, and its locking and sleeping.
    pub idle: bool,
    pub system_lock: bool,
}

/// What the system offers: the platform's, or a fake one in tests.
pub trait System: Send + Sync {
    /// How the user can authenticate, or why they can't.
    fn method(&self) -> Result<Method>;
    /// Asks the user to authenticate, over `window` where the platform can; blocks until they
    /// have. `appLock.cancelled` when they cancelled.
    fn authenticate(&self, window: Option<&WebviewWindow>) -> Result<()>;
    /// Seconds since the last input anywhere in the system; `None` if it can't be known.
    fn idle_seconds(&self) -> Option<u64>;
    /// Whether [`watch_system`] reports the system locking and sleeping: started, and
    /// working.
    fn watches_system_lock(&self) -> bool;
}

/// What locks the app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trigger {
    Manual,
    Idle,
    System,
}

pub struct AppLock {
    system: Box<dyn System>,
    vault: Arc<Vault>,
    state: Mutex<State>,
    /// Held while the system asks the user, so that it asks once at a time.
    authenticating: Mutex<()>,
    /// Tells the frontend that the app locked by itself (idle, the system).
    notify: Box<dyn Fn() + Send + Sync>,
}

struct State {
    /// Locked until the user authenticates, from the start. Only counts while the app lock is
    /// on (see [`AppLock::is_locked`]).
    locked: bool,
    /// When the user last authenticated: Touch ID or a face isn't input, so the system's idle
    /// time goes on through it (see [`AppLock::check_idle`]).
    unlocked_at: Option<Instant>,
}

/// Whether `error` from [`System::method`] says that there is no way to authenticate (none set
/// up, turned off, unsupported), rather than that asking failed, which may pass. Windows Hello
/// turned off by policy counts: an administrator did it, and anyone who can could read the
/// sessions from the app's memory anyway; keeping the lock would shut the user out for good.
fn gone(error: &Error) -> bool {
    error.code() != "appLock.failed"
}

impl AppLock {
    pub fn new(system: Box<dyn System>, vault: Arc<Vault>, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let state = State { locked: true, unlocked_at: None };
        Self { system, vault, state: Mutex::new(state), authenticating: Mutex::new(()), notify: Box::new(notify) }
    }

    /// The app lock for this platform.
    pub fn for_platform(vault: Arc<Vault>, notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self::new(Box::new(platform::Platform), vault, notify)
    }

    fn is_locked(&self) -> bool {
        self.vault.app_lock().is_some_and(|settings| settings.enabled) && self.state.lock().unwrap().locked
    }

    fn unlocked(&self) {
        *self.state.lock().unwrap() = State { locked: false, unlocked_at: Some(Instant::now()) };
    }

    /// Blocks while the system is asked what it offers (Windows Hello may take a moment).
    pub fn status(&self) -> AppLockStatus {
        let method = self.system.method();
        AppLockStatus {
            locked: self.is_locked(),
            settings: self.vault.app_lock(),
            method: method.as_ref().ok().copied(),
            unavailable: method.err(),
            idle: self.system.idle_seconds().is_some(),
            system_lock: self.system.watches_system_lock(),
        }
    }

    /// The master password unlocked the vault: that counts as authenticating, so the app
    /// doesn't lock again right after.
    pub fn unlocked_by_password(&self) {
        self.unlocked();
    }

    /// Locks if the settings ask for it on `trigger`; tells the frontend unless it asked.
    /// Without asking the system what it offers: the system's locking and sleeping happen once,
    /// and Windows Hello may fail to answer, or answer late, while the system suspends.
    /// [`AppLock::unlock`] deals with a way to authenticate that went away.
    pub fn lock(&self, trigger: Trigger) {
        let Some(settings) = self.vault.app_lock() else { return };
        let wanted = match trigger {
            Trigger::Manual => true,
            Trigger::Idle => settings.idle_minutes > 0,
            Trigger::System => settings.on_system_lock,
        };
        if !settings.enabled || !wanted {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if state.locked {
            return;
        }
        state.locked = true;
        drop(state);
        if trigger != Trigger::Manual {
            (self.notify)();
        }
    }

    /// Locks once the system has been idle for longer than the settings allow, counting from
    /// the last unlock at most.
    fn check_idle(&self) {
        let Some(settings) = self.vault.app_lock() else { return };
        let since_unlock = {
            let state = self.state.lock().unwrap();
            if !settings.enabled || settings.idle_minutes == 0 || state.locked {
                return;
            }
            state.unlocked_at.map(|at| at.elapsed().as_secs())
        };
        let idle = self.system.idle_seconds().map(|idle| since_unlock.map_or(idle, |since| idle.min(since)));
        if idle.is_some_and(|idle| idle >= u64::from(settings.idle_minutes) * 60) {
            self.lock(Trigger::Idle);
        }
    }

    /// Asks the user to authenticate, and unlocks if they do. Blocks until they have.
    ///
    /// If the way to authenticate went away (a Windows Hello PIN removed), it fails with
    /// `appLock.gone`, which says why: the user may then unlock without authenticating
    /// ([`Self::unlock_anyway`]) rather than be locked out. Removing it took the account's
    /// password, which unlocks everything anyway. Asking the system failing is not that: it
    /// asks all the same.
    pub fn unlock(&self, window: Option<&WebviewWindow>) -> Result<AppLockStatus> {
        self.unlock_with(window, false)
    }

    /// Unlocks, without authenticating if there is no way to (see [`Self::unlock`]).
    pub fn unlock_anyway(&self, window: Option<&WebviewWindow>) -> Result<AppLockStatus> {
        self.unlock_with(window, true)
    }

    fn unlock_with(&self, window: Option<&WebviewWindow>, anyway: bool) -> Result<AppLockStatus> {
        let _asking = self.authenticating.try_lock().map_err(|_| Error::new("appLock.busy"))?;
        if self.is_locked() {
            match self.system.method() {
                Err(error) if gone(&error) && anyway => {}
                Err(error) if gone(&error) => return Err(Error::new("appLock.gone").detail(error)),
                _ => self.system.authenticate(window)?,
            }
        }
        self.unlocked();
        Ok(self.status())
    }

    /// Saves the settings. Turning the lock on takes an authentication, which shows that it
    /// works; unset protection can't have it, having no file to keep its settings in.
    pub fn configure(&self, settings: AppLockSettings, window: Option<&WebviewWindow>) -> Result<AppLockStatus> {
        let settings = settings.normalize();
        let current = self.vault.app_lock().ok_or_else(|| Error::new("vault.locked"))?;
        if self.is_locked() {
            return Err(Error::new("appLock.locked"));
        }
        if settings.enabled && !current.enabled {
            if self.vault.status().protection == Some(Protection::Unset) {
                return Err(Error::new("appLock.protectionUnset"));
            }
            self.system.method()?;
            let _asking = self.authenticating.try_lock().map_err(|_| Error::new("appLock.busy"))?;
            self.system.authenticate(window)?;
            self.unlocked();
        }
        self.vault.update(|data| data.app_lock = settings)?;
        Ok(self.status())
    }
}

/// Checks the system's idle time every few seconds, for as long as the app runs.
pub fn watch_idle(lock: Arc<AppLock>) {
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(IDLE_CHECK);
        loop {
            timer.tick().await;
            // Off the async threads: the vault's state may be held while the credential store
            // asks the user.
            let lock = lock.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || lock.check_idle()).await;
        }
    });
}

/// Locks when the system locks its screen, sleeps or switches user, for as long as the app
/// runs. On the main thread.
pub fn watch_system(lock: Arc<AppLock>) {
    platform::watch_system(Box::new(move || {
        lock.lock(Trigger::System);
    }));
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use super::*;
    use crate::persist::SetAside;
    use crate::secrets::MemoryStore;
    use crate::vault::Target;

    #[derive(Default)]
    struct Fake {
        unavailable: AtomicBool,
        /// Asking what the system offers fails, as Windows Hello may.
        query_fails: AtomicBool,
        refuses: AtomicBool,
        asked: AtomicUsize,
        idle: AtomicU64,
    }

    impl System for Arc<Fake> {
        fn method(&self) -> Result<Method> {
            if self.query_fails.load(Ordering::Relaxed) {
                return Err(Error::new("appLock.failed"));
            }
            match self.unavailable.load(Ordering::Relaxed) {
                true => Err(Error::new("appLock.unsupported")),
                false => Ok(Method::TouchId),
            }
        }

        fn authenticate(&self, _window: Option<&WebviewWindow>) -> Result<()> {
            self.asked.fetch_add(1, Ordering::Relaxed);
            match self.refuses.load(Ordering::Relaxed) {
                true => Err(Error::new("appLock.cancelled")),
                false => Ok(()),
            }
        }

        fn idle_seconds(&self) -> Option<u64> {
            Some(self.idle.load(Ordering::Relaxed))
        }

        fn watches_system_lock(&self) -> bool {
            true
        }
    }

    struct Setup {
        fake: Arc<Fake>,
        vault: Arc<Vault>,
        notified: Arc<AtomicUsize>,
        dir: PathBuf,
    }

    impl Setup {
        /// A vault in `name`'s directory, with the given protection (`None` leaves it unset).
        fn new(name: &str, protection: Option<Target>) -> Self {
            let dir = std::env::temp_dir().join(format!("zshell-applock-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let vault = Arc::new(Vault::open(dir.clone(), Arc::new(MemoryStore::default()), &SetAside::default()));
            if let Some(target) = protection {
                vault.set_protection(target, None, false).unwrap();
            }
            Self { fake: Arc::default(), vault, notified: Arc::default(), dir }
        }

        /// The app lock as it is at startup.
        fn start(&self) -> AppLock {
            let notified = self.notified.clone();
            AppLock::new(Box::new(self.fake.clone()), self.vault.clone(), move || {
                notified.fetch_add(1, Ordering::Relaxed);
            })
        }

        fn enable(&self, settings: AppLockSettings) {
            self.vault.update(|data| data.app_lock = AppLockSettings { enabled: true, ..settings }).unwrap();
        }

        fn asked(&self) -> usize {
            self.fake.asked.load(Ordering::Relaxed)
        }
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn starts_locked_when_on_and_unlocks_only_by_authenticating() {
        let setup = Setup::new("start", Some(Target::None));
        assert!(!setup.start().status().locked, "off: not locked");
        setup.enable(AppLockSettings::default());

        let lock = setup.start();
        assert!(lock.status().locked);
        setup.fake.refuses.store(true, Ordering::Relaxed);
        assert_eq!(lock.unlock(None).unwrap_err().code(), "appLock.cancelled");
        assert!(lock.status().locked);
        setup.fake.refuses.store(false, Ordering::Relaxed);
        assert!(!lock.unlock(None).unwrap().locked);
        assert_eq!(setup.asked(), 2);

        // Unlocked: nothing to ask.
        lock.unlock(None).unwrap();
        assert_eq!(setup.asked(), 2);
    }

    #[test]
    fn the_master_password_counts_as_authenticating() {
        let setup = Setup::new("password", Some(Target::None));
        setup.enable(AppLockSettings::default());
        let lock = setup.start();
        lock.unlocked_by_password();
        assert!(!lock.status().locked);
    }

    #[test]
    fn a_way_to_authenticate_that_went_away_doesnt_lock_the_user_out() {
        let setup = Setup::new("unavailable", Some(Target::None));
        setup.enable(AppLockSettings::default());
        setup.fake.unavailable.store(true, Ordering::Relaxed);
        let lock = setup.start();
        let status = lock.status();
        assert!(status.locked && status.method.is_none());
        assert_eq!(status.unavailable.unwrap().code(), "appLock.unsupported");
        // Says so first, rather than unlock without a word.
        assert_eq!(lock.unlock(None).unwrap_err().code(), "appLock.gone");
        assert!(lock.status().locked);
        assert!(!lock.unlock_anyway(None).unwrap().locked);
        assert_eq!(setup.asked(), 0);

        // Locking doesn't ask the system, which may not answer while it suspends.
        setup.fake.unavailable.store(false, Ordering::Relaxed);
        setup.fake.query_fails.store(true, Ordering::Relaxed);
        lock.lock(Trigger::System);
        assert!(lock.status().locked);
        // Asking failing is no reason to let anyone in.
        setup.fake.refuses.store(true, Ordering::Relaxed);
        assert_eq!(lock.unlock(None).unwrap_err().code(), "appLock.cancelled");
        assert_eq!(lock.unlock_anyway(None).unwrap_err().code(), "appLock.cancelled");
        assert!(lock.status().locked);
        assert_eq!(setup.asked(), 2);
    }

    /// Makes the last unlock `seconds` ago.
    fn unlocked_ago(lock: &AppLock, seconds: u64) {
        lock.state.lock().unwrap().unlocked_at = Some(Instant::now() - Duration::from_secs(seconds));
    }

    #[test]
    fn locks_after_the_idle_time_and_on_system_events_as_set() {
        let setup = Setup::new("triggers", Some(Target::None));
        setup.enable(AppLockSettings { idle_minutes: 5, on_system_lock: false, ..Default::default() });
        let lock = setup.start();

        // Touch ID isn't input: right after unlocking, the system has been idle all along.
        setup.fake.idle.store(3600, Ordering::Relaxed);
        lock.unlock(None).unwrap();
        lock.check_idle();
        assert!(!lock.status().locked, "idle since the unlock only");
        unlocked_ago(&lock, 4 * 60);
        lock.check_idle();
        assert!(!lock.status().locked);

        unlocked_ago(&lock, 3600);
        setup.fake.idle.store(4 * 60, Ordering::Relaxed);
        lock.check_idle();
        assert!(!lock.status().locked);
        lock.lock(Trigger::System);
        assert!(!lock.status().locked, "not on system events");
        setup.fake.idle.store(5 * 60, Ordering::Relaxed);
        lock.check_idle();
        assert!(lock.status().locked);
        assert_eq!(setup.notified.load(Ordering::Relaxed), 1);
        lock.check_idle();
        assert_eq!(setup.notified.load(Ordering::Relaxed), 1, "already locked");

        lock.unlock(None).unwrap();
        unlocked_ago(&lock, 24 * 3600);
        setup.enable(AppLockSettings { idle_minutes: 0, on_system_lock: true, ..Default::default() });
        setup.fake.idle.store(24 * 3600, Ordering::Relaxed);
        lock.check_idle();
        assert!(!lock.status().locked, "never on idle");
        lock.lock(Trigger::System);
        assert!(lock.status().locked);
        assert_eq!(setup.notified.load(Ordering::Relaxed), 2);

        // Asked for by the frontend, which needs no telling.
        lock.unlock(None).unwrap();
        lock.lock(Trigger::Manual);
        assert!(lock.status().locked);
        assert_eq!(setup.notified.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn turning_on_takes_an_authentication_and_a_protection() {
        let on = AppLockSettings { enabled: true, ..Default::default() };

        let unset = Setup::new("unset", None);
        let lock = unset.start();
        assert_eq!(lock.configure(on.clone(), None).unwrap_err().code(), "appLock.protectionUnset");

        let setup = Setup::new("configure", Some(Target::System));
        let lock = setup.start();
        setup.fake.refuses.store(true, Ordering::Relaxed);
        assert_eq!(lock.configure(on.clone(), None).unwrap_err().code(), "appLock.cancelled");
        assert!(!setup.vault.app_lock().unwrap().enabled);

        setup.fake.refuses.store(false, Ordering::Relaxed);
        let status = lock.configure(on.clone(), None).unwrap();
        assert!(status.settings.unwrap().enabled && !status.locked);
        assert_eq!(setup.asked(), 2);

        // Changing it while on, or turning it off, asks nothing; idle times are those offered.
        let status = lock.configure(AppLockSettings { idle_minutes: 7, ..on }, None).unwrap();
        assert_eq!(status.settings.unwrap().idle_minutes, DEFAULT_IDLE_MINUTES);
        lock.configure(AppLockSettings::default(), None).unwrap();
        assert_eq!(setup.asked(), 2);

        // Locked, the settings can't change.
        lock.configure(AppLockSettings { enabled: true, ..Default::default() }, None).unwrap();
        lock.lock(Trigger::Manual);
        assert_eq!(lock.configure(AppLockSettings::default(), None).unwrap_err().code(), "appLock.locked");
    }

    #[test]
    fn the_settings_are_kept_with_the_sessions() {
        let setup = Setup::new("kept", Some(Target::None));
        setup.enable(AppLockSettings::default());
        assert!(setup.dir.join("applock.json").exists());
        let reopened = Vault::open(setup.dir.clone(), Arc::new(MemoryStore::default()), &SetAside::default());
        assert!(reopened.app_lock().unwrap().enabled);

        // Moving to the encrypted vault takes them along, and leaves no plain copy.
        setup.vault.set_protection(Target::System, None, false).unwrap();
        assert!(!setup.dir.join("applock.json").exists());
        assert!(setup.vault.app_lock().unwrap().enabled);
    }
}
