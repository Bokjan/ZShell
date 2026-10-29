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
//! Queries never wait for a change: what changes the vault (writing the file, deriving a key
//! from the master password, asking the credential store, which may wait for the user) holds
//! its state, and publishes what queries read ([`View`]) once done. The stores
//! (`ProfileStore`, `QuickCommandStore`) keep a copy of their part ([`Part`]), read again by
//! itself once the vault is unlocked or reset, and change it through [`Part::update`], which
//! writes the whole file.

mod part;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

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
pub use part::Part;

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

/// The saved passwords of the sessions and proxies, by id. Shown by `{:?}` without them.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Passwords {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub proxies: BTreeMap<String, String>,
}

impl std::fmt::Debug for Passwords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Passwords")
            .field("profiles", &self.profiles.keys().collect::<Vec<_>>())
            .field("proxies", &self.proxies.keys().collect::<Vec<_>>())
            .finish()
    }
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

/// What the protection changes to (see `Vault::set_protection`). Shown by `{:?}` without the
/// master password.
#[derive(Deserialize, TS)]
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

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::System => f.write_str("System"),
            Self::Password { password, remember } => {
                let password = password.as_ref().map(|_| "…");
                f.debug_struct("Password").field("password", &password).field("remember", remember).finish()
            }
        }
    }
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
    data: Arc<Data>,
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

/// What queries read: the state as of the last change, published by it before letting go of
/// the state ([`Vault::publish`]).
#[derive(Default)]
struct View {
    /// `None` while locked.
    data: Option<Arc<Data>>,
    protection: Option<Protection>,
    remember: bool,
    lock: Option<Lock>,
}

pub struct Vault {
    dir: PathBuf,
    store: Arc<dyn CredentialStore>,
    /// Held by a change for as long as it takes (writing, deriving a key from the master
    /// password, the credential store): one change at a time. Queries don't take it.
    state: Mutex<State>,
    /// What queries read, held only to read or replace it.
    view: RwLock<View>,
    /// Held by [`Vault::try_unlock`], so that two windows starting together don't both make
    /// a vault.
    trying: Mutex<()>,
    /// Counts [`Vault::reset`]s: what [`Vault::try_unlock`] read before one is the deleted
    /// vault's, and must not come back.
    resets: std::sync::atomic::AtomicU64,
    /// Counts the times the data was replaced as a whole (see [`Vault::generation`]).
    generation: std::sync::atomic::AtomicU64,
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

    /// The password of `account`; `None` when there is none or the vault is locked. Doesn't
    /// wait for a change of the vault (see [`Vault::data`]), so async code calls it as it is.
    pub fn get(&self, account: Account) -> Option<String> {
        self.0.as_ref()?.password(account).ok().flatten()
    }

    /// A session's saved password (SSH, Telnet).
    pub fn profile(&self, id: &str) -> Option<String> {
        self.get(Account::Profile(id))
    }

    /// A proxy's saved password.
    pub fn proxy(&self, id: &str) -> Option<String> {
        self.get(Account::Proxy(id))
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
            view: RwLock::default(),
            trying: Mutex::new(()),
            resets: Default::default(),
            generation: Default::default(),
        };
        let mut state = vault.state.lock().unwrap();
        *state = vault.load(false).0;
        vault.publish(&state);
        drop(state);
        vault
    }

    fn path(&self) -> PathBuf {
        self.dir.join(VAULT_FILE)
    }

    pub fn status(&self) -> VaultStatus {
        let view = self.view.read().unwrap();
        VaultStatus {
            protection: view.protection,
            remember: view.remember,
            locked: view.lock.clone(),
            store: StoreKind::current(),
            created: false,
        }
    }

    /// Makes `state` what queries read; by every change, before it lets go of the state, so
    /// that what they read changes in the same order.
    fn publish(&self, state: &State) {
        let view = match state {
            State::Open(open) => View {
                data: Some(open.data.clone()),
                protection: Some(open.sealing.protection()),
                remember: open.sealing.remember(),
                lock: None,
            },
            State::Locked { sealing, lock } => View {
                data: None,
                protection: sealing.as_ref().map(Sealing::protection),
                remember: sealing.as_ref().is_some_and(Sealing::remember),
                lock: Some(lock.clone()),
            },
        };
        *self.view.write().unwrap() = view;
    }

    /// Whether the credential store can be used (see [`CredentialStore::available`]); blocks
    /// for up to seconds on Linux.
    pub fn store_available(&self) -> bool {
        self.store.available()
    }

    /// Counts the times the data has been replaced as a whole: unlocked (or made) and reset.
    /// The stores' copies ([`Part`]) are read again once it has moved; changes written through
    /// [`Vault::update`] don't move it.
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Puts `next` in place of the state, as a new generation of the data. Published before
    /// the generation moves: a [`Part`] that sees the new generation reads the new data.
    fn replace(&self, state: &mut State, next: State) {
        *state = next;
        self.publish(state);
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Everything, as of the last change; `vault.locked` while locked.
    pub fn data(&self) -> Result<Arc<Data>> {
        self.view.read().unwrap().data.clone().ok_or_else(locked)
    }

    /// The app lock's settings; `None` while locked.
    pub fn app_lock(&self) -> Option<AppLockSettings> {
        self.view.read().unwrap().data.as_ref().map(|data| data.app_lock.clone())
    }

    /// The saved password of `account`, if there is one.
    pub fn password(&self, account: Account) -> Result<Option<String>> {
        Ok(self.data()?.passwords.get(account).map(str::to_owned))
    }

    /// Changes the data, and writes it if `change` succeeds and changed it. `change` gets the
    /// data as it is now: changes go one at a time.
    pub fn update<T>(&self, change: impl FnOnce(&mut Data) -> Result<T>) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        let State::Open(open) = &mut *state else { return Err(locked()) };
        let mut data = Data::clone(&open.data);
        let value = change(&mut data)?;
        if data != *open.data {
            self.write(&open.sealing, open.key.as_ref(), &data)?;
            open.data = Arc::new(data);
            self.publish(&state);
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
            Ok(data) => State::Open(Open { sealing: file.header, key, data: Arc::new(data) }),
            Err(Unopened::WrongKey) => State::locked(Some(file.header), LockReason::Damaged, None),
            Err(Unopened::Damaged(detail)) => State::locked(Some(file.header), LockReason::Damaged, parse_failed(detail)),
        }
    }

    /// The data key in the credential store's entry `account`, if it is `sealing`'s.
    fn saved_key(&self, account: &str, sealing: &Sealing) -> Result<Key> {
        let key = match self.store.get(account) {
            Ok(text) => crypto::decode_key(&zeroize::Zeroizing::new(text)).ok_or_else(|| Error::new("vault.keyMismatch"))?,
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
            self.store.set(account, &zeroize::Zeroizing::new(crypto::encode(&**key)))?;
        }
        if let Err(e) = self.write(&sealing, key.as_ref(), &data) {
            if let Some(account) = &account {
                let _ = self.store.delete(account);
            }
            return Err(e);
        }
        Ok(Open { sealing, key, data: Arc::new(data) })
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
        if self.view.read().unwrap().data.is_none() {
            let (next, made) = self.load(true);
            let mut state = self.state.lock().unwrap();
            if matches!(&*state, State::Locked { .. }) && self.resets.load(Ordering::SeqCst) == resets {
                self.replace(&mut state, next);
                created = made;
            }
        }
        let status = VaultStatus { created, ..self.status() };
        log::info!(
            "vault: {}",
            if status.locked.is_some() {
                "locked, waiting for the master password"
            } else if created {
                "made"
            } else {
                "open"
            }
        );
        status
    }

    /// Unlocks with the master password: `vault.wrongPassword` if it isn't the one. Blocks
    /// while the key is derived. Also says whether this call opened it, which checked the
    /// password: an open vault takes any.
    pub fn unlock(&self, password: &str) -> Result<(VaultStatus, bool)> {
        let mut opened = false;
        {
            let mut state = self.state.lock().unwrap();
            if let State::Locked { .. } = &*state {
                let next = match self.read() {
                    Found::Missing => State::locked(None, LockReason::New, None),
                    // Shown on the unlock screen instead.
                    Found::Unusable(lock) => State::Locked { sealing: None, lock },
                    Found::File(file) => {
                        let key = file.header.unwrap_key(password).ok_or_else(|| {
                            log::info!("vault: a wrong master password");
                            Error::new("vault.wrongPassword")
                        })?;
                        let data = file.open(Some(&key));
                        match data {
                            Ok(data) => {
                                log::info!("vault: opened with the master password");
                                opened = true;
                                State::Open(Open { sealing: file.header, key: Some(key), data: Arc::new(data) })
                            }
                            Err(_) => State::locked(Some(file.header), LockReason::Damaged, None),
                        }
                    }
                };
                self.replace(&mut state, next);
            }
        }
        Ok((self.status(), opened))
    }

    /// Starts again without sessions when the vault's key is lost (a forgotten master password)
    /// or it can't be read: the file is moved aside (`vault.json.reset-<time>`), and its key, if
    /// the credential store has it, stays there under its own name, so that the file can still
    /// be opened if the password comes back to mind or the key is restored (a keychain backup):
    /// renamed back to `vault.json`. [`Vault::try_unlock`] then makes a new one.
    pub fn reset(&self) -> Result<VaultStatus> {
        {
            let mut state = self.state.lock().unwrap();
            let State::Locked { .. } = &*state else { return Err(Error::new("vault.notLocked")) };
            let path = self.path();
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let aside = (0..)
                .map(|n| {
                    path.with_file_name(if n == 0 {
                        format!("{VAULT_FILE}.reset-{stamp}")
                    } else {
                        format!("{VAULT_FILE}.reset-{stamp}-{n}")
                    })
                })
                .find(|aside| !aside.exists())
                .expect("a free name");
            match std::fs::rename(&path, &aside) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(Error::new("vault.moveAsideFailed").param("path", path.display()).detail(e));
                }
                _ => {}
            }
            log::warn!("vault: started again, the old file is now {}", aside.display());
            self.replace(&mut state, State::locked(None, LockReason::New, None));
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
            let next = self.reseal(target, current, Data::clone(&open.data))?;
            // Another key's: the new key always has an entry of its own.
            if let Some(account) = old_account {
                let _ = self.store.delete(&account);
            }
            *state = State::Open(next);
            self.publish(&state);
        }
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests;
