//! Where the sessions, folders, proxies, quick commands, saved passwords and the app lock's
//! settings are kept, and how they are protected: all of them in `vault.json` (see
//! `envelope.rs`), whose header says how. See "加密保险库" in `docs/ARCHITECTURE.md`.
//!
//! - None: the data in clear.
//! - System: encrypted with a random data key that the credential store keeps.
//! - Password: encrypted with a random data key, itself encrypted with a key from the master
//!   password, and also kept in the credential store if the user asked to be remembered.
//!
//! The credential store keeps a key under an entry named after it (`vault-key:<id>`), so that
//! the file always names the entry that opens it: a change of protection saves the new key,
//! puts the new file in place, then deletes the old key, and stopping anywhere in between
//! leaves at most an entry that opens nothing. Each change uses a new key.
//!
//! The stores (`ProfileStore`, `QuickCommandStore`) keep their own copy to answer queries
//! without waiting, and write through [`Vault::update`], which writes the whole file.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::crypto::{self, Kdf, Key, Sealed};
use crate::envelope::{self, Envelope, Invalid, Unopened};
use crate::error::{Error, Result};
use crate::model::AppLockSettings;
use crate::model::Proxy;
use crate::model::QuickCommands;
use crate::model::{Folder, Item, Profile};
use crate::persist;
use crate::secrets::{CredentialStore, StoreKind};

pub const VAULT_FILE: &str = "vault.json";
const FORMAT: &str = "zshell-vault";
const VERSION: u32 = 2;
/// Begins the name of the credential store's entry for a key, which ends with the key's id.
const KEY_ACCOUNT: &str = "vault-key:";
/// Associated data of the data key encrypted with the master password's key.
const WRAP_AAD: &[u8] = b"zshell-vault-key";

/// Everything the vault keeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Data {
    pub folders: Vec<Folder>,
    pub profiles: Vec<Profile>,
    /// The order the sessions and folders are shown in (see `config`). Without one (a vault
    /// of an older version), folders come first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<Item>,
    pub proxies: Vec<Proxy>,
    pub commands: QuickCommands,
    pub passwords: Passwords,
    /// Kept with the sessions rather than in `settings.json`, so that it is as protected as
    /// they are: in the encrypted vault, it can't be turned off by editing a file.
    pub app_lock: AppLockSettings,
}

/// The saved passwords of the sessions and proxies, by id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Passwords {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub proxies: BTreeMap<String, String>,
}

/// Whose password: a session's or a proxy's, by id.
#[derive(Clone, Copy, Debug)]
pub enum Account<'a> {
    Profile(&'a str),
    Proxy(&'a str),
}

impl Passwords {
    fn of(&self, account: Account) -> (&BTreeMap<String, String>, String) {
        match account {
            Account::Profile(id) => (&self.profiles, id.to_owned()),
            Account::Proxy(id) => (&self.proxies, id.to_owned()),
        }
    }

    pub fn get(&self, account: Account) -> Option<&str> {
        let (map, id) = self.of(account);
        map.get(&id).map(String::as_str)
    }

    /// Saves the password of `account`, or deletes it (`None`).
    pub fn set(&mut self, account: Account, password: Option<&str>) {
        let (map, id) = match account {
            Account::Profile(id) => (&mut self.profiles, id),
            Account::Proxy(id) => (&mut self.proxies, id),
        };
        match password {
            Some(password) => map.insert(id.to_owned(), password.to_owned()),
            None => map.remove(id),
        };
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty() && self.proxies.is_empty()
    }

    /// Keeps only the passwords of the sessions and proxies that exist, and of the proxies
    /// that take one.
    pub fn retain_owners(&mut self, profiles: &[Profile], proxies: &[Proxy]) {
        self.profiles.retain(|id, _| profiles.iter().any(|p| p.id == *id));
        self.proxies.retain(|id, _| proxies.iter().any(|p| p.id == *id && p.uses_password()));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Protection {
    None,
    System,
    Password,
}

/// What the protection changes to (see `Vault::set_protection`).
#[derive(Debug, Deserialize, TS)]
#[ts(rename = "ProtectionChange")]
#[serde(tag = "protection", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Target {
    None,
    System,
    /// `password`: the new master password; `None` keeps the current one.
    Password {
        password: Option<String>,
        remember: bool,
    },
}

/// Why the vault is locked, for the unlock screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LockReason {
    /// There is no vault yet: [`Vault::try_unlock`] makes one.
    New,
    /// The key is in the credential store and hasn't been read yet ([`Vault::try_unlock`]).
    Saved,
    /// The master password is needed.
    Password,
    /// The credential store couldn't give the key (system protection): it isn't available,
    /// or has no entry for it. Only a reset helps.
    KeyUnavailable,
    /// `vault.json` couldn't be read (an antivirus or a sync client holding it): trying again
    /// may work.
    Unreadable,
    /// `vault.json` isn't a vault this version can read (hand-edited, or of an older
    /// version), or its key doesn't open it.
    Damaged,
    /// Written by a newer version of ZShell.
    Newer,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "VaultLock")]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub reason: LockReason,
    /// What went wrong, if something did (reading the key, the file).
    pub error: Option<Error>,
}

impl Lock {
    fn new(reason: LockReason) -> Self {
        Self { reason, error: None }
    }
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// `None` when locked with no vault, or one whose header can't be read.
    pub protection: Option<Protection>,
    /// With password protection: the data key is also in the credential store.
    pub remember: bool,
    pub locked: Option<Lock>,
    pub store: StoreKind,
    /// Set by the [`Vault::try_unlock`] that made the vault, which chose its protection
    /// without asking: the frontend says how it is protected, and offers to change it, once.
    pub created: bool,
}

/// How the data in `vault.json` is protected, as its header says. In clear in the file, but
/// authenticated by the encrypted data (see `envelope.rs`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "protection", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Sealing {
    None,
    System {
        key_id: String,
    },
    /// The data key encrypted with a key from the master password (`kdf`).
    Password {
        key_id: String,
        kdf: Kdf,
        wrapped_key: Sealed,
        remember: bool,
    },
}

impl Sealing {
    fn protection(&self) -> Protection {
        match self {
            Sealing::None => Protection::None,
            Sealing::System { .. } => Protection::System,
            Sealing::Password { .. } => Protection::Password,
        }
    }

    fn remember(&self) -> bool {
        matches!(self, Sealing::Password { remember: true, .. })
    }

    fn key_id(&self) -> Option<&str> {
        match self {
            Sealing::None => None,
            Sealing::System { key_id } | Sealing::Password { key_id, .. } => Some(key_id),
        }
    }

    /// The credential store's entry for the data key, if it is kept there.
    fn store_account(&self) -> Option<String> {
        match self {
            Sealing::System { key_id } | Sealing::Password { key_id, remember: true, .. } => Some(format!("{KEY_ACCOUNT}{key_id}")),
            _ => None,
        }
    }

    /// The data key, from the master password; `None` if it is the wrong one.
    fn unwrap_key(&self, password: &str) -> Option<Key> {
        let Sealing::Password { kdf, wrapped_key, .. } = self else { return None };
        let wrapping = kdf.derive(password)?;
        let bytes = crypto::open(&wrapping, WRAP_AAD, wrapped_key)?;
        let mut key = zeroize::Zeroizing::new([0; crypto::KEY_LEN]);
        (bytes.len() == crypto::KEY_LEN).then(|| key.copy_from_slice(&bytes))?;
        Some(key)
    }
}

/// What is at `vault.json`.
enum Found {
    Missing,
    File(Envelope<Sealing>),
    Unusable(Lock),
}

/// An unlocked vault. `key` is the data key, `None` with no protection.
struct Open {
    sealing: Sealing,
    key: Option<Key>,
    data: Data,
}

enum State {
    Open(Open),
    Locked { sealing: Option<Sealing>, lock: Lock },
}

impl State {
    fn locked(sealing: Option<Sealing>, reason: LockReason, error: Option<Error>) -> Self {
        State::Locked { sealing, lock: Lock { reason, error } }
    }
}

pub struct Vault {
    dir: PathBuf,
    store: Arc<dyn CredentialStore>,
    /// Held while writing, and while deriving a key from the master password: nothing else
    /// may change what is stored meanwhile.
    state: Mutex<State>,
    /// Held by [`Vault::try_unlock`], so that two windows starting together don't both make
    /// a vault.
    trying: Mutex<()>,
    /// Counts [`Vault::reset`]s: what [`Vault::try_unlock`] read before one is the deleted
    /// vault's, and must not come back.
    resets: std::sync::atomic::AtomicU64,
}

/// The saved passwords a session may use (its own, its jump hosts' and its proxy's), read
/// from the vault when a server or a proxy asks for one. Whoever opens the session hands it
/// down with the session's route (`net::Route`), so that what a session reads shows in what
/// it is given. Quick connections have none.
#[derive(Clone, Default)]
pub struct SavedPasswords(Option<Arc<Vault>>);

impl SavedPasswords {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self(Some(vault))
    }

    /// The password of `account`; `None` when there is none or the vault is locked. Blocks
    /// while the vault is being written.
    pub fn get(&self, account: Account) -> Option<String> {
        self.0.as_ref()?.password(account).ok().flatten()
    }

    /// A session's saved password (also Telnet's), from async code.
    pub async fn profile(&self, id: &str) -> Option<String> {
        let (this, id) = (self.clone(), id.to_owned());
        tauri::async_runtime::spawn_blocking(move || this.get(Account::Profile(&id))).await.ok().flatten()
    }

    /// A proxy's saved password, from async code.
    pub async fn proxy(&self, id: &str) -> Option<String> {
        let (this, id) = (self.clone(), id.to_owned());
        tauri::async_runtime::spawn_blocking(move || this.get(Account::Proxy(&id))).await.ok().flatten()
    }
}

fn locked() -> Error {
    Error::new("vault.locked")
}

fn parse_failed(detail: Option<String>) -> Option<Error> {
    detail.map(|detail| Error::new("vault.parseFailed").detail(detail))
}

impl Vault {
    /// The vault in `dir`: unlocked if it isn't encrypted, else read only as far as its
    /// header. An encrypted one is unlocked later, and a missing one made, with
    /// [`Vault::try_unlock`] or [`Vault::unlock`], since the credential store may ask the user
    /// or take long to answer.
    pub fn open(dir: PathBuf, store: Arc<dyn CredentialStore>) -> Self {
        let _ = persist::secure_dir(&dir);
        let vault = Self {
            dir,
            store,
            state: Mutex::new(State::locked(None, LockReason::New, None)),
            trying: Mutex::new(()),
            resets: Default::default(),
        };
        *vault.state.lock().unwrap() = vault.load(false).0;
        vault
    }

    fn path(&self) -> PathBuf {
        self.dir.join(VAULT_FILE)
    }

    pub fn status(&self) -> VaultStatus {
        let store = StoreKind::current();
        match &*self.state.lock().unwrap() {
            State::Open(open) => VaultStatus {
                protection: Some(open.sealing.protection()),
                remember: open.sealing.remember(),
                locked: None,
                store,
                created: false,
            },
            State::Locked { sealing, lock } => VaultStatus {
                protection: sealing.as_ref().map(Sealing::protection),
                remember: sealing.as_ref().is_some_and(Sealing::remember),
                locked: Some(lock.clone()),
                store,
                created: false,
            },
        }
    }

    /// Whether the credential store can be used (see [`CredentialStore::available`]); blocks
    /// for up to seconds on Linux.
    pub fn store_available(&self) -> bool {
        self.store.available()
    }

    /// A copy of everything, `vault.locked` while locked.
    pub fn data(&self) -> Result<Data> {
        match &*self.state.lock().unwrap() {
            State::Open(open) => Ok(open.data.clone()),
            State::Locked { .. } => Err(locked()),
        }
    }

    /// The app lock's settings; `None` while locked.
    pub fn app_lock(&self) -> Option<AppLockSettings> {
        match &*self.state.lock().unwrap() {
            State::Open(open) => Some(open.data.app_lock.clone()),
            State::Locked { .. } => None,
        }
    }

    /// The saved password of `account`, if there is one.
    pub fn password(&self, account: Account) -> Result<Option<String>> {
        match &*self.state.lock().unwrap() {
            State::Open(open) => Ok(open.data.passwords.get(account).map(str::to_owned)),
            State::Locked { .. } => Err(locked()),
        }
    }

    /// Changes the data, and writes it if `change` succeeds and changed it.
    pub fn update<T>(&self, change: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        let State::Open(open) = &mut *state else { return Err(locked()) };
        let mut data = open.data.clone();
        let value = change(&mut data)?;
        if data != open.data {
            self.write(&open.sealing, open.key.as_ref(), &data)?;
            open.data = data;
        }
        Ok(value)
    }

    /// Writes the vault, via a temporary file and a rename (see `persist::stage_bytes`).
    fn write(&self, sealing: &Sealing, key: Option<&Key>, data: &Data) -> Result<()> {
        let path = self.path();
        let bytes = envelope::write(FORMAT, VERSION, sealing, key, data)
            .ok_or_else(|| Error::new("vault.verifyFailed").param("path", path.display()))?;
        let staged =
            persist::stage_bytes(&path, &bytes).map_err(|e| Error::new("config.writeFailed").param("path", path.display()).detail(e))?;
        staged.commit()
    }

    fn read(&self) -> Found {
        let bytes = match fs::read(self.path()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Found::Missing,
            Err(e) => {
                return Found::Unusable(Lock { reason: LockReason::Unreadable, error: Some(Error::new("vault.readFailed").detail(e)) })
            }
        };
        match envelope::parse(&bytes, FORMAT, VERSION) {
            Ok(file) => Found::File(file),
            Err(Invalid::Version(version)) if version > VERSION => Found::Unusable(Lock::new(LockReason::Newer)),
            Err(Invalid::Damaged(detail)) => Found::Unusable(Lock { reason: LockReason::Damaged, error: parse_failed(detail) }),
            Err(_) => Found::Unusable(Lock::new(LockReason::Damaged)),
        }
    }

    /// What is on disk, as far as it opens: with the key in the credential store if
    /// `with_store` (making a vault if there is none, and saying so), else locked waiting for
    /// it.
    fn load(&self, with_store: bool) -> (State, bool) {
        match self.read() {
            Found::Missing if with_store => {
                let state = self.create();
                let created = matches!(state, State::Open(_));
                (state, created)
            }
            Found::Missing => (State::locked(None, LockReason::New, None), false),
            Found::Unusable(lock) => (State::Locked { sealing: None, lock }, false),
            Found::File(file) => (self.open_file(file, with_store), false),
        }
    }

    /// What `file` opens to: with the key in the credential store if `with_store`, else
    /// locked waiting for it.
    fn open_file(&self, file: Envelope<Sealing>, with_store: bool) -> State {
        let key = match file.header.store_account() {
            Some(_) if !with_store => return State::locked(Some(file.header), LockReason::Saved, None),
            Some(account) => match self.saved_key(&account, &file.header) {
                Ok(key) => Some(key),
                Err(error) => {
                    let reason = match file.header {
                        Sealing::Password { .. } => LockReason::Password,
                        _ => LockReason::KeyUnavailable,
                    };
                    return State::locked(Some(file.header), reason, Some(error));
                }
            },
            None if matches!(file.header, Sealing::Password { .. }) => return State::locked(Some(file.header), LockReason::Password, None),
            None => None,
        };
        let opened = file.open(key.as_ref());
        match opened {
            Ok(data) => State::Open(Open { sealing: file.header, key, data }),
            Err(Unopened::WrongKey) => State::locked(Some(file.header), LockReason::Damaged, None),
            Err(Unopened::Damaged(detail)) => State::locked(Some(file.header), LockReason::Damaged, parse_failed(detail)),
        }
    }

    /// The data key in the credential store's entry `account`, if it is `sealing`'s.
    fn saved_key(&self, account: &str, sealing: &Sealing) -> Result<Key> {
        let key = match self.store.get(account) {
            Ok(text) => crypto::decode_key(&text).ok_or_else(|| Error::new("vault.keyMismatch"))?,
            Err(keyring::Error::NoEntry) => return Err(Error::new("vault.keyMissing")),
            Err(e) => return Err(e.into()),
        };
        match sealing.key_id() == Some(crypto::key_id(&key).as_str()) {
            true => Ok(key),
            false => Err(Error::new("vault.keyMismatch")),
        }
    }

    /// A new, empty vault: encrypted with a key in the credential store if it can be used,
    /// else in clear.
    fn create(&self) -> State {
        let data = Data::default();
        if self.store.available() {
            if let Ok(open) = self.reseal(Target::System, None, data.clone()) {
                return State::Open(open);
            }
        }
        match self.reseal(Target::None, None, data) {
            Ok(open) => State::Open(open),
            Err(error) => State::locked(None, LockReason::New, Some(error)),
        }
    }

    /// Writes `data` protected as `target` says, with a new key: saved in the credential
    /// store first if it is kept there, and deleted from it again if the file can't be
    /// written, so that a failure changes nothing. The master password is `target`'s, or
    /// `current` if it keeps it.
    fn reseal(&self, target: Target, current: Option<&str>, data: Data) -> Result<Open> {
        let key = crypto::random_key();
        let key_id = crypto::key_id(&key);
        let (sealing, key) = match target {
            Target::None => (Sealing::None, None),
            Target::System => (Sealing::System { key_id }, Some(key)),
            Target::Password { password, remember } => {
                let password = match password.as_deref() {
                    Some("") => return Err(Error::new("vault.emptyPassword")),
                    Some(password) => password,
                    None => current.ok_or_else(|| Error::new("vault.passwordRequired"))?,
                };
                let kdf = Kdf::new();
                let wrapping = kdf.derive(password).expect("the current parameters");
                let wrapped_key = crypto::seal(&wrapping, WRAP_AAD, &*key);
                (Sealing::Password { key_id, kdf, wrapped_key, remember }, Some(key))
            }
        };
        let account = sealing.store_account();
        if let (Some(account), Some(key)) = (&account, &key) {
            self.store.set(account, &crypto::encode(&**key))?;
        }
        if let Err(e) = self.write(&sealing, key.as_ref(), &data) {
            if let Some(account) = &account {
                let _ = self.store.delete(account);
            }
            return Err(e);
        }
        Ok(Open { sealing, key, data })
    }

    /// Unlocks with the key in the credential store (system protection, or a remembered
    /// master password), or makes a vault if there is none, reading the file again first:
    /// also "try again" after it couldn't be read. Blocks while the credential store answers,
    /// without holding the state: the master password may unlock meanwhile.
    pub fn try_unlock(&self) -> VaultStatus {
        use std::sync::atomic::Ordering;

        let _trying = self.trying.lock().unwrap();
        let mut created = false;
        let resets = self.resets.load(Ordering::SeqCst);
        if matches!(&*self.state.lock().unwrap(), State::Locked { .. }) {
            let (next, made) = self.load(true);
            let mut state = self.state.lock().unwrap();
            if matches!(&*state, State::Locked { .. }) && self.resets.load(Ordering::SeqCst) == resets {
                *state = next;
                created = made;
            }
        }
        VaultStatus { created, ..self.status() }
    }

    /// Unlocks with the master password: `vault.wrongPassword` if it isn't the one. Blocks
    /// while the key is derived. Also says whether this call opened it, which checked the
    /// password: an open vault takes any.
    pub fn unlock(&self, password: &str) -> Result<(VaultStatus, bool)> {
        let mut opened = false;
        {
            let mut state = self.state.lock().unwrap();
            if let State::Locked { .. } = &*state {
                *state = match self.read() {
                    Found::Missing => State::locked(None, LockReason::New, None),
                    // Shown on the unlock screen instead.
                    Found::Unusable(lock) => State::Locked { sealing: None, lock },
                    Found::File(file) => {
                        let key = file.header.unwrap_key(password).ok_or_else(|| Error::new("vault.wrongPassword"))?;
                        let data = file.open(Some(&key));
                        match data {
                            Ok(data) => {
                                opened = true;
                                State::Open(Open { sealing: file.header, key: Some(key), data })
                            }
                            Err(_) => State::locked(Some(file.header), LockReason::Damaged, None),
                        }
                    }
                };
            }
        }
        Ok((self.status(), opened))
    }

    /// Deletes the vault, whose key is lost (a forgotten master password) or which can't be
    /// read, with its key in the credential store: no sessions. [`Vault::try_unlock`] then
    /// makes a new one.
    pub fn reset(&self) -> Result<VaultStatus> {
        {
            let mut state = self.state.lock().unwrap();
            let State::Locked { sealing, .. } = &*state else { return Err(Error::new("vault.notLocked")) };
            let account = sealing.as_ref().and_then(Sealing::store_account);
            let path = self.path();
            persist::remove_file(&path).map_err(|e| Error::new("vault.deleteFailed").param("path", path.display()).detail(e))?;
            if let Some(account) = account {
                let _ = self.store.delete(&account);
            }
            *state = State::locked(None, LockReason::New, None);
            self.resets.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(self.status())
    }

    /// Changes the protection, with a new key. Leaving or changing password protection takes
    /// the current master password (`current`). The file is replaced in one rename; the old
    /// key leaves the credential store afterwards. Blocks: the credential store may ask the
    /// user, and keys are derived from passwords.
    pub fn set_protection(&self, target: Target, current: Option<&str>) -> Result<VaultStatus> {
        {
            let mut state = self.state.lock().unwrap();
            let State::Open(open) = &*state else { return Err(locked()) };
            let current = match &open.sealing {
                Sealing::Password { .. } => {
                    let password = current.filter(|p| !p.is_empty()).ok_or_else(|| Error::new("vault.passwordRequired"))?;
                    open.sealing.unwrap_key(password).ok_or_else(|| Error::new("vault.wrongPassword"))?;
                    Some(password)
                }
                _ => None,
            };
            let old_account = open.sealing.store_account();
            let next = self.reseal(target, current, open.data.clone())?;
            // Another key's: the new key always has an entry of its own.
            if let Some(account) = old_account {
                let _ = self.store.delete(&account);
            }
            *state = State::Open(next);
        }
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests;
