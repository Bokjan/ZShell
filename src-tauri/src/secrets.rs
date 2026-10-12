//! The system's credential store (macOS Keychain, Windows Credential Manager, the Secret
//! Service on Linux), and the saved passwords as sessions read them.
//!
//! Where the passwords are depends on the vault's protection (see `vault.rs`): each in an entry
//! of its own in the credential store (unset), or in the vault or `passwords.json` with the
//! rest. The credential store also keeps the vault's key, when the user chose so.
//!
//! The calls block, sometimes for long: after an update the app's signature changes and macOS
//! asks the user again for every entry, and on Linux the store may first ask to be unlocked.
//! Async code uses [`password`] and [`proxy_password`], which wait on the blocking pool, so
//! that the other sessions keep running meanwhile; commands run the rest on the blocking pool
//! too.

use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

use crate::vault;

/// Under which name the entries are kept: the app's bundle identifier.
const SERVICE: &str = "org.boyin.zshell";

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

    /// Whether the store can be used at all. The Keychain and the Credential Manager always
    /// can; on Linux there may be no Secret Service (no desktop environment, no keyring
    /// daemon), or no session bus to reach it.
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
        if StoreKind::current() != StoreKind::SecretService {
            return true;
        }
        // Looking up an entry that doesn't exist: "no entry" means the store answered. On a
        // thread of its own, which is left behind if the store never answers.
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(matches!(Self.get("availability-check"), Ok(_) | Err(keyring::Error::NoEntry)));
        });
        receiver.recv_timeout(PROBE_TIMEOUT).unwrap_or(false)
    }
}

/// The entry of a proxy's password: proxies are kept apart from sessions, which use the bare
/// profile id.
pub fn proxy_account(proxy_id: &str) -> String {
    format!("proxy:{proxy_id}")
}

/// A session's saved password (also Telnet's), blocking. `None` when there is none or it
/// can't be read.
pub fn get_password(profile_id: &str) -> Option<String> {
    vault::installed()?.password(profile_id).ok().flatten()
}

/// [`get_password`] from async code.
pub async fn password(profile_id: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || get_password(&profile_id)).await.ok().flatten()
}

/// A proxy's saved password, from async code.
pub async fn proxy_password(proxy_id: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || get_password(&proxy_account(&proxy_id))).await.ok().flatten()
}

#[cfg(test)]
pub use memory::MemoryStore;

#[cfg(test)]
mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::CredentialStore;

    /// A credential store in memory; `failing` makes every call fail, as an unavailable store,
    /// and `set_delay_ms` makes saving take that long, as when the system asks the user.
    #[derive(Default)]
    pub struct MemoryStore {
        pub entries: Mutex<HashMap<String, String>>,
        pub failing: std::sync::atomic::AtomicBool,
        pub set_delay_ms: std::sync::atomic::AtomicU64,
    }

    impl MemoryStore {
        fn check(&self) -> keyring::Result<()> {
            match self.failing.load(std::sync::atomic::Ordering::Relaxed) {
                true => Err(keyring::Error::NoStorageAccess("unavailable".into())),
                false => Ok(()),
            }
        }
    }

    impl CredentialStore for MemoryStore {
        fn get(&self, account: &str) -> keyring::Result<String> {
            self.check()?;
            self.entries.lock().unwrap().get(account).cloned().ok_or(keyring::Error::NoEntry)
        }

        fn set(&self, account: &str, secret: &str) -> keyring::Result<()> {
            self.check()?;
            let delay = self.set_delay_ms.load(std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(delay));
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
