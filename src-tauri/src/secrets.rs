//! The system's credential store (macOS Keychain, Windows Credential Manager, the Secret
//! Service on Linux), which keeps the vault's key when the user chose so (see `vault/`).
//!
//! The calls block, sometimes for long: after an update the app's signature changes and macOS
//! asks the user again, and on Linux the store may first ask to be unlocked. Commands run them
//! on the blocking pool.

use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

/// Under which name the entries are kept: the app's bundle identifier. Debug builds keep theirs
/// apart, so that one run for testing never touches the key of the user's own vault.
const SERVICE: &str = if cfg!(debug_assertions) { "org.boyin.zshell.dev" } else { "org.boyin.zshell" };

/// Which system store the platform has, for the frontend to name it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StoreKind {
    Keychain,
    CredentialManager,
    SecretService,
}

impl StoreKind {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Keychain
        } else if cfg!(windows) {
            Self::CredentialManager
        } else {
            Self::SecretService
        }
    }
}

/// A store of secrets by account name: the system's, or one in memory for tests.
pub trait CredentialStore: Send + Sync {
    fn get(&self, account: &str) -> keyring::Result<String>;
    fn set(&self, account: &str, secret: &str) -> keyring::Result<()>;
    /// Deleting an entry that doesn't exist succeeds.
    fn delete(&self, account: &str) -> keyring::Result<()>;

    /// Whether the store can be used at all. On Linux there may be no Secret Service (no
    /// desktop environment, no keyring daemon), or no session bus to reach it; on macOS no
    /// keychain (a `HOME` without one, as in end-to-end tests).
    fn available(&self) -> bool {
        true
    }
}

/// The system's credential store.
pub struct SystemStore;

/// How long [`SystemStore::available`] waits: a Secret Service that D-Bus has to start, or
/// that doesn't answer, can take until D-Bus gives up (25 seconds).
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

impl CredentialStore for SystemStore {
    fn get(&self, account: &str) -> keyring::Result<String> {
        keyring::Entry::new(SERVICE, account)?.get_password()
    }

    fn set(&self, account: &str, secret: &str) -> keyring::Result<()> {
        keyring::Entry::new(SERVICE, account)?.set_password(secret)
    }

    fn delete(&self, account: &str) -> keyring::Result<()> {
        match keyring::Entry::new(SERVICE, account)?.delete_credential() {
            Err(keyring::Error::NoEntry) => Ok(()),
            result => result,
        }
    }

    fn available(&self) -> bool {
        // Looking up an entry that doesn't exist, which asks the user nothing: "no entry" means
        // the store answered. On a thread of its own, which is left behind if the store never
        // answers.
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(matches!(Self.get("availability-check"), Ok(_) | Err(keyring::Error::NoEntry)));
        });
        receiver.recv_timeout(PROBE_TIMEOUT).unwrap_or(false)
    }
}

#[cfg(test)]
pub use memory::MemoryStore;

#[cfg(test)]
mod memory {
    use std::collections::HashMap;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::CredentialStore;

    /// How long a held call waits for its test at most, so that a vault waiting on it by
    /// mistake fails the test rather than hanging it.
    const HOLD_LIMIT: Duration = Duration::from_secs(10);

    /// A credential store in memory; `failing` makes every call fail, as an unavailable store,
    /// and [`MemoryStore::hold_get`] and [`MemoryStore::hold_set`] keep the next read or save
    /// waiting, as when the system asks the user.
    #[derive(Default)]
    pub struct MemoryStore {
        pub entries: Mutex<HashMap<String, String>>,
        pub failing: std::sync::atomic::AtomicBool,
        get_gate: Mutex<Option<Gate>>,
        set_gate: Mutex<Option<Gate>>,
    }

    /// The store's side of a [`Hold`].
    struct Gate {
        started: Sender<()>,
        release: Receiver<()>,
    }

    impl Gate {
        fn pass(gate: &Mutex<Option<Gate>>) {
            let Some(gate) = gate.lock().unwrap().take() else { return };
            let _ = gate.started.send(());
            // Released when the `Hold` is dropped.
            let _ = gate.release.recv_timeout(HOLD_LIMIT);
        }
    }

    /// A read or save of the store held at its start: [`Hold::started`] waits until a call
    /// has got there, and dropping the hold lets it go on.
    pub struct Hold {
        started: Receiver<()>,
        _release: Sender<()>,
    }

    impl Hold {
        fn new(gate: &Mutex<Option<Gate>>) -> Self {
            let (started, started_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            *gate.lock().unwrap() = Some(Gate { started, release: release_rx });
            Self { started: started_rx, _release: release }
        }

        pub fn started(&self) {
            self.started.recv_timeout(HOLD_LIMIT).expect("the held call never started");
        }
    }

    impl MemoryStore {
        fn check(&self) -> keyring::Result<()> {
            match self.failing.load(std::sync::atomic::Ordering::Relaxed) {
                true => Err(keyring::Error::NoStorageAccess("unavailable".into())),
                false => Ok(()),
            }
        }

        /// Holds the next read, which answers with the entry as it was when it started.
        pub fn hold_get(&self) -> Hold {
            Hold::new(&self.get_gate)
        }

        /// Holds the next save, which stores the entry once it is let go on.
        pub fn hold_set(&self) -> Hold {
            Hold::new(&self.set_gate)
        }
    }

    impl CredentialStore for MemoryStore {
        fn get(&self, account: &str) -> keyring::Result<String> {
            self.check()?;
            let entry = self.entries.lock().unwrap().get(account).cloned();
            Gate::pass(&self.get_gate);
            entry.ok_or(keyring::Error::NoEntry)
        }

        fn set(&self, account: &str, secret: &str) -> keyring::Result<()> {
            self.check()?;
            Gate::pass(&self.set_gate);
            self.entries.lock().unwrap().insert(account.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, account: &str) -> keyring::Result<()> {
            self.check()?;
            self.entries.lock().unwrap().remove(account);
            Ok(())
        }

        fn available(&self) -> bool {
            self.check().is_ok()
        }
    }
}
