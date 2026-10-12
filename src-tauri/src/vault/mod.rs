//! Where the sessions, folders, proxies, quick commands, saved passwords and the app lock's
//! settings are kept, and how they are protected. See "加密保险库" in `docs/ARCHITECTURE.md`.
//!
//! - Unset (as before 2.7): plain JSON files, each password in an entry of its own in the
//!   system's credential store.
//! - None: plain JSON files, the passwords in `passwords.json`.
//! - System or Password: everything in `vault.json`, encrypted with a random data key that the
//!   credential store keeps (System), or that is encrypted with a key from the master password
//!   (Password), and also kept in the credential store if the user asked to be remembered.
//!
//! Which one is in use follows from the files, so that nothing can say otherwise: `vault.json`
//! (its header says which), else `passwords.json`, else unset.
//!
//! The stores (`ProfileStore`, `QuickCommandStore`) keep their own copy to answer queries
//! without waiting, and write through [`Vault::update`], which writes the plain files that
//! changed or the whole vault.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::applock::AppLockSettings;
use crate::config::{Folder, Profile};
use crate::crypto::{self, Kdf, Key, Sealed};
use crate::error::{Error, Result};
use crate::persist::{self, JsonFile, SetAside};
use crate::proxy::Proxy;
use crate::quick::QuickCommands;
use crate::secrets::{self, CredentialStore, StoreKind};

pub const VAULT_FILE: &str = "vault.json";
const PASSWORDS_FILE: &str = "passwords.json";
const APP_LOCK_FILE: &str = "applock.json";
/// The plain files that the vault replaces, `passwords.json` included.
const PLAIN_FILES: [&str; 6] = ["profiles.json", "folders.json", "proxies.json", "commands.json", APP_LOCK_FILE, PASSWORDS_FILE];
const FORMAT: &str = "zshell-vault";
const VERSION: u32 = 1;
/// The credential store's entry for the data key.
const KEY_ACCOUNT: &str = "vault-key";
/// Where a new data key waits until the vault that needs it is in place, so that `vault-key`
/// opens the vault on disk whenever a change stops part way (see [`Vault::set_protection`]).
const NEXT_KEY_ACCOUNT: &str = "vault-key-next";
/// Associated data of the data key encrypted with the master password's key.
const WRAP_AAD: &[u8] = b"zshell-vault-key";

/// Everything the vault keeps.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Data {
    pub folders: Vec<Folder>,
    pub profiles: Vec<Profile>,
    pub proxies: Vec<Proxy>,
    pub commands: QuickCommands,
    /// By account: a session's id, or `proxy:<id>` (see `secrets::proxy_account`). Empty
    /// while unset, when the credential store has them.
    pub passwords: BTreeMap<String, String>,
    /// Kept with the sessions rather than in `settings.json`, so that it is as protected as
    /// they are: in the encrypted vault, it can't be turned off by editing a file.
    pub app_lock: AppLockSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Protection {
    Unset,
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
    /// The key is in the credential store and hasn't been read yet ([`Vault::try_unlock`]).
    Saved,
    /// The master password is needed.
    Password,
    /// The credential store couldn't give the key (system protection): it isn't available,
    /// has no entry, or one for another vault. Only a reset helps.
    KeyUnavailable,
    /// `vault.json` couldn't be read (an antivirus or a sync client holding it): trying again
    /// may work.
    Unreadable,
    /// `vault.json` isn't a vault this version can read, or its key doesn't open it.
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

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// `None` when locked by a file whose header can't be read.
    pub protection: Option<Protection>,
    /// With password protection: the data key is also in the credential store.
    pub remember: bool,
    pub locked: Option<Lock>,
    pub store: StoreKind,
    /// Set by the unlocking that couldn't delete the plain data files left by a change of
    /// protection that stopped part way (`vault.plainLeft`).
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leftover: Option<Error>,
}

/// The header of `vault.json`, in clear: the encrypted data authenticates it (as associated
/// data, the bytes exactly as in the file).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: String,
    version: u32,
    /// System or Password.
    protection: Protection,
    key_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    remember: bool,
    /// Password: how the master password's key is derived, and the data key encrypted with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kdf: Option<Kdf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wrapped_key: Option<Sealed>,
}

impl Header {
    fn uses_store(&self) -> bool {
        self.protection == Protection::System || self.remember
    }

    /// The data key, from the master password; `None` if it is the wrong one.
    fn unwrap_key(&self, password: &str) -> Option<Key> {
        let wrapping = self.kdf.as_ref()?.derive(password)?;
        let bytes = crypto::open(&wrapping, WRAP_AAD, self.wrapped_key.as_ref()?)?;
        let mut key = Zeroizing::new([0; crypto::KEY_LEN]);
        (bytes.len() == crypto::KEY_LEN).then(|| key.copy_from_slice(&bytes))?;
        Some(key)
    }
}

/// `vault.json` as written.
#[derive(Serialize)]
struct FileOut<'a> {
    header: &'a RawValue,
    nonce: &'a str,
    data: &'a str,
}

/// `vault.json` as read: the header's bytes as they are in the file.
#[derive(Deserialize)]
struct FileIn {
    header: Box<RawValue>,
    nonce: String,
    data: String,
}

/// A `vault.json` read and its header understood.
struct VaultFile {
    header: Header,
    header_raw: String,
    sealed: Sealed,
}

#[derive(Deserialize)]
struct Version {
    format: String,
    version: u32,
}

impl VaultFile {
    fn read(path: &Path) -> std::result::Result<Self, Lock> {
        let lock = |reason, error: Option<Error>| Lock { reason, error };
        let bytes = fs::read(path).map_err(|e| lock(LockReason::Unreadable, Some(Error::new("vault.readFailed").detail(e))))?;
        Self::parse(&bytes).map_err(|reason| lock(reason, None))
    }

    fn parse(bytes: &[u8]) -> std::result::Result<Self, LockReason> {
        let file: FileIn = serde_json::from_slice(bytes).map_err(|_| LockReason::Damaged)?;
        let version: Version = serde_json::from_str(file.header.get()).map_err(|_| LockReason::Damaged)?;
        if version.format != FORMAT {
            return Err(LockReason::Damaged);
        }
        if version.version > VERSION {
            return Err(LockReason::Newer);
        }
        let header: Header = serde_json::from_str(file.header.get()).map_err(|_| LockReason::Damaged)?;
        Ok(Self { header, header_raw: file.header.get().to_owned(), sealed: Sealed { nonce: file.nonce, data: file.data } })
    }

    /// The data, if `key` is this vault's key.
    fn open(&self, key: &Key) -> std::result::Result<Data, LockReason> {
        if crypto::key_id(key) != self.header.key_id {
            return Err(LockReason::KeyUnavailable);
        }
        let plaintext = crypto::open(key, self.header_raw.as_bytes(), &self.sealed).ok_or(LockReason::Damaged)?;
        serde_json::from_slice(&plaintext).map_err(|_| LockReason::Damaged)
    }
}

/// Writes the vault next to `path`, and reads it back to check that it opens with `key` to
/// the same data, before it is put in place.
fn stage_vault(path: &Path, header: &Header, key: &Key, data: &Data) -> Result<persist::Staged> {
    let header_raw = serde_json::to_string(header)?;
    let plaintext = Zeroizing::new(serde_json::to_vec(data)?);
    let sealed = crypto::seal(key, header_raw.as_bytes(), &plaintext);
    let raw = RawValue::from_string(header_raw)?;
    let bytes = serde_json::to_vec_pretty(&FileOut { header: &raw, nonce: &sealed.nonce, data: &sealed.data })?;
    let write_failed = |e: std::io::Error| Error::new("config.writeFailed").param("path", path.display()).detail(e);
    let staged = persist::stage_bytes(path, &bytes).map_err(write_failed)?;
    let read = fs::read(staged.written()).map_err(write_failed)?;
    let same = VaultFile::parse(&read)
        .ok()
        .and_then(|file| crypto::open(key, file.header_raw.as_bytes(), &file.sealed))
        .is_some_and(|read| *read == *plaintext);
    if !same {
        return Err(Error::new("vault.verifyFailed").param("path", path.display()));
    }
    Ok(staged)
}

/// Copies a part of the data (a file's) from the first to the second.
type TakeSection = fn(&Data, &mut Data);

/// The plain files of the unset and none protections.
struct Plain {
    profiles: JsonFile,
    folders: JsonFile,
    proxies: JsonFile,
    commands: JsonFile,
    /// `applock.json`. Only written with none protection: unset, the app lock can't be turned
    /// on (see `AppLock::configure`).
    app_lock: JsonFile,
    /// None protection: `passwords.json`.
    passwords: Option<JsonFile>,
    data: Data,
}

impl Plain {
    fn load(dir: &Path, set_aside: &SetAside) -> Self {
        let (profiles, data_profiles) = JsonFile::load(dir.join("profiles.json"), set_aside);
        let (folders, data_folders) = JsonFile::load(dir.join("folders.json"), set_aside);
        let (proxies, data_proxies) = JsonFile::load(dir.join("proxies.json"), set_aside);
        let (commands, data_commands) = JsonFile::load(dir.join("commands.json"), set_aside);
        let (app_lock, data_app_lock) = JsonFile::load(dir.join(APP_LOCK_FILE), set_aside);
        let passwords_path = dir.join(PASSWORDS_FILE);
        let (passwords, data_passwords) = match passwords_path.exists() {
            true => {
                let (file, passwords) = JsonFile::load(passwords_path, set_aside);
                (Some(file), passwords)
            }
            false => (None, BTreeMap::new()),
        };
        let data = Data {
            folders: data_folders,
            profiles: data_profiles,
            proxies: data_proxies,
            commands: data_commands,
            passwords: data_passwords,
            app_lock: data_app_lock,
        };
        Self { profiles, folders, proxies, commands, app_lock, passwords, data }
    }

    /// Plain files in `dir` to be written (none protection), holding nothing yet.
    fn at(dir: &Path) -> Self {
        let file = |name: &str| JsonFile::at(dir.join(name));
        Self {
            profiles: file("profiles.json"),
            folders: file("folders.json"),
            proxies: file("proxies.json"),
            commands: file("commands.json"),
            app_lock: file(APP_LOCK_FILE),
            passwords: Some(file(PASSWORDS_FILE)),
            data: Data::default(),
        }
    }

    fn protection(&self) -> Protection {
        match self.passwords {
            Some(_) => Protection::None,
            None => Protection::Unset,
        }
    }

    /// Writes the files whose part of the data differs from what they hold (all of them with
    /// `all`), all of them staged before any is put in place.
    ///
    /// Putting several files in place is not atomic: if one fails, those before it hold the
    /// new data. `self.data` follows what is on disk, file by file, so that memory and disk
    /// agree either way.
    fn write(&mut self, new: &Data, all: bool) -> Result<()> {
        fn changed<T: Serialize>(all: bool, old: &T, new: &T) -> Result<bool> {
            Ok(all || serde_json::to_value(old)? != serde_json::to_value(new)?)
        }
        let old = &self.data;
        let mut staged: Vec<(persist::Staged, TakeSection)> = Vec::new();
        if changed(all, &old.folders, &new.folders)? {
            staged.push((self.folders.stage(&new.folders)?, |new, data| data.folders.clone_from(&new.folders)));
        }
        if changed(all, &old.proxies, &new.proxies)? {
            staged.push((self.proxies.stage(&new.proxies)?, |new, data| data.proxies.clone_from(&new.proxies)));
        }
        if changed(all, &old.profiles, &new.profiles)? {
            staged.push((self.profiles.stage(&new.profiles)?, |new, data| data.profiles.clone_from(&new.profiles)));
        }
        if changed(all, &old.commands, &new.commands)? {
            staged.push((self.commands.stage(&new.commands)?, |new, data| data.commands.clone_from(&new.commands)));
        }
        if changed(all, &old.app_lock, &new.app_lock)? {
            staged.push((self.app_lock.stage(&new.app_lock)?, |new, data| data.app_lock.clone_from(&new.app_lock)));
        }
        if let Some(passwords) = &self.passwords {
            if changed(all, &old.passwords, &new.passwords)? {
                staged.push((passwords.stage(&new.passwords)?, |new, data| data.passwords.clone_from(&new.passwords)));
            }
        }
        for (file, take) in staged {
            file.commit()?;
            take(new, &mut self.data);
        }
        Ok(())
    }

    /// A file that couldn't be read at startup and is left alone: replacing the plain files
    /// would lose what it holds.
    fn left_alone(&self) -> Option<&JsonFile> {
        [&self.profiles, &self.folders, &self.proxies, &self.commands, &self.app_lock]
            .into_iter()
            .chain(self.passwords.as_ref())
            .find(|file| file.left_alone())
    }
}

struct Encrypted {
    header: Header,
    key: Key,
    data: Data,
}

enum State {
    Plain(Plain),
    Encrypted(Encrypted),
    Locked { header: Option<Header>, lock: Lock },
}

pub struct Vault {
    dir: PathBuf,
    store: Arc<dyn CredentialStore>,
    /// Held while writing, and while reading the key or deriving it: nothing else may
    /// change what is stored meanwhile.
    state: Mutex<State>,
}

static INSTALLED: OnceLock<Arc<Vault>> = OnceLock::new();

/// Makes `vault` the one sessions read their passwords from (see `secrets::password`).
pub fn install(vault: Arc<Vault>) {
    let _ = INSTALLED.set(vault);
}

pub fn installed() -> Option<&'static Arc<Vault>> {
    INSTALLED.get()
}

fn locked() -> Error {
    Error::new("vault.locked")
}

impl Vault {
    /// The data in `dir`. An encrypted vault is only read as far as its header: it is
    /// unlocked later, with [`Vault::try_unlock`] or [`Vault::unlock`], since the credential
    /// store may ask the user or take long to answer.
    pub fn open(dir: PathBuf, store: Arc<dyn CredentialStore>, set_aside: &SetAside) -> Self {
        let _ = persist::secure_dir(&dir);
        let path = dir.join(VAULT_FILE);
        let state = match path.exists() {
            false => State::Plain(Plain::load(&dir, set_aside)),
            true => match VaultFile::read(&path) {
                Ok(file) => {
                    let reason = if file.header.uses_store() { LockReason::Saved } else { LockReason::Password };
                    State::Locked { header: Some(file.header), lock: Lock { reason, error: None } }
                }
                Err(lock) => State::Locked { header: None, lock },
            },
        };
        Self { dir, store, state: Mutex::new(state) }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(VAULT_FILE)
    }

    pub fn status(&self) -> VaultStatus {
        let store = StoreKind::current();
        match &*self.state.lock().unwrap() {
            State::Plain(plain) => {
                VaultStatus { protection: Some(plain.protection()), remember: false, locked: None, store, leftover: None }
            }
            State::Encrypted(vault) => VaultStatus {
                protection: Some(vault.header.protection),
                remember: vault.header.remember,
                locked: None,
                store,
                leftover: None,
            },
            State::Locked { header, lock } => VaultStatus {
                protection: header.as_ref().map(|h| h.protection),
                remember: header.as_ref().is_some_and(|h| h.remember),
                locked: Some(lock.clone()),
                store,
                leftover: None,
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
            State::Plain(plain) => Ok(plain.data.clone()),
            State::Encrypted(vault) => Ok(vault.data.clone()),
            State::Locked { .. } => Err(locked()),
        }
    }

    /// The app lock's settings; `None` while locked.
    pub fn app_lock(&self) -> Option<AppLockSettings> {
        match &*self.state.lock().unwrap() {
            State::Plain(plain) => Some(plain.data.app_lock.clone()),
            State::Encrypted(vault) => Some(vault.data.app_lock.clone()),
            State::Locked { .. } => None,
        }
    }

    /// Changes the data and writes what changed.
    pub fn update<T>(&self, change: impl FnOnce(&mut Data) -> T) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        match &mut *state {
            State::Plain(plain) => {
                let mut data = plain.data.clone();
                let value = change(&mut data);
                plain.write(&data, false)?;
                Ok(value)
            }
            State::Encrypted(vault) => {
                let mut data = vault.data.clone();
                let value = change(&mut data);
                if serde_json::to_value(&data)? == serde_json::to_value(&vault.data)? {
                    return Ok(value);
                }
                stage_vault(&self.dir.join(VAULT_FILE), &vault.header, &vault.key, &data)?.commit()?;
                vault.data = data;
                Ok(value)
            }
            State::Locked { .. } => Err(locked()),
        }
    }

    /// The saved password of `account`, if there is one. Unset, it is read from the credential
    /// store, which blocks.
    pub fn password(&self, account: &str) -> Result<Option<String>> {
        match &*self.state.lock().unwrap() {
            State::Plain(plain) if plain.passwords.is_none() => {}
            State::Plain(plain) => return Ok(plain.data.passwords.get(account).cloned()),
            State::Encrypted(vault) => return Ok(vault.data.passwords.get(account).cloned()),
            State::Locked { .. } => return Err(locked()),
        }
        // Unset: from the credential store, without holding the state, since the store may
        // wait for the user.
        match self.store.get(account) {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Saves the password of `account`, or deletes it (`None`).
    pub fn set_password(&self, account: &str, password: Option<&str>) -> Result<()> {
        let state = self.state.lock().unwrap();
        if matches!(&*state, State::Plain(plain) if plain.passwords.is_none()) {
            // Unset: into the credential store while holding the state, so that a change of
            // protection moving the passwords out of it waits, rather than taking the old
            // password and deleting the entry this one goes into.
            return Ok(match password {
                Some(password) => self.store.set(account, password),
                None => self.store.delete(account),
            }?);
        }
        // Elsewhere the protection can't go back to unset: wherever it is by then.
        drop(state);
        self.update(|data| match password {
            Some(password) => data.passwords.insert(account.to_owned(), password.to_owned()),
            None => data.passwords.remove(account),
        })
        .map(drop)
    }

    /// Unlocks with the key in the credential store (system protection, or a remembered
    /// master password), reading the file again first: also "try again" after it couldn't be
    /// read. Blocks while the credential store answers, without holding the state: the master
    /// password may unlock meanwhile.
    pub fn try_unlock(&self) -> VaultStatus {
        let mut leftover = None;
        if matches!(&*self.state.lock().unwrap(), State::Locked { .. }) {
            let next = self.unlock_saved();
            let mut state = self.state.lock().unwrap();
            if let State::Locked { .. } = &*state {
                if let State::Encrypted(_) = &next {
                    leftover = self.remove_leftovers();
                }
                *state = next;
            }
        }
        VaultStatus { leftover, ..self.status() }
    }

    /// Deletes plain data files next to an encrypted vault that opened, so that it is known
    /// to be good: left by a change of protection that stopped part way. Until then they may
    /// be all that is left of the sessions, should the vault turn out to be unreadable.
    /// Failures are reported (they hold the sessions in the clear), and tried again at the
    /// next unlock.
    fn remove_leftovers(&self) -> Option<Error> {
        self.remove_plain().err().map(|e| Error::new("vault.plainLeft").detail(e))
    }

    /// Deletes the plain data files (and temporary ones next to them); the first that can't
    /// be deleted fails it.
    fn remove_plain(&self) -> Result<()> {
        let mut result = Ok(());
        for name in PLAIN_FILES {
            for path in [self.dir.join(name), self.dir.join(format!("{name}.tmp"))] {
                if let Err(e) = persist::remove_file(&path) {
                    if result.is_ok() {
                        result = Err(Error::new("vault.deleteFailed").param("path", path.display()).detail(e));
                    }
                }
            }
        }
        result
    }

    /// Plain data files that couldn't be read and were set aside (`<name>.bad-<time>`, see
    /// `JsonFile::load`). They are the user's to repair or restore, so never deleted
    /// unasked; but they may hold sessions and passwords in the clear, which encryption
    /// doesn't cover.
    pub fn plain_copies(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut copies: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                PLAIN_FILES.iter().any(|plain| name.strip_prefix(plain).is_some_and(|rest| rest.starts_with(".bad-")))
            })
            .map(|entry| entry.path())
            .collect();
        copies.sort();
        copies
    }

    /// Deletes the [plain copies](Self::plain_copies); the first that can't be deleted fails
    /// it.
    pub fn delete_plain_copies(&self) -> Result<()> {
        let mut result = Ok(());
        for path in self.plain_copies() {
            if let Err(e) = persist::remove_file(&path) {
                if result.is_ok() {
                    result = Err(Error::new("vault.deleteFailed").param("path", path.display()).detail(e));
                }
            }
        }
        result
    }

    /// The data key in the credential store, if it opens `file`: `vault-key`, else a new key
    /// left in `vault-key-next` by a change that stopped after putting its vault in place,
    /// which then becomes `vault-key`. The error is `vault-key`'s.
    fn saved_key(&self, file: &VaultFile) -> Result<Key> {
        let read = |account| match self.store.get(account) {
            Ok(text) => crypto::decode_key(&text).ok_or_else(|| Error::new("vault.keyMismatch")),
            Err(keyring::Error::NoEntry) => Err(Error::new("vault.keyMissing")),
            Err(e) => Err(Error::from(e)),
        };
        let opens = |key: &Key| crypto::key_id(key) == file.header.key_id;
        let error = match read(KEY_ACCOUNT) {
            Ok(key) if opens(&key) => return Ok(key),
            Ok(_) => Error::new("vault.keyMismatch"),
            Err(e) => e,
        };
        match read(NEXT_KEY_ACCOUNT) {
            Ok(key) if opens(&key) => {
                if self.store.set(KEY_ACCOUNT, &crypto::encode(&*key)).is_ok() {
                    let _ = self.store.delete(NEXT_KEY_ACCOUNT);
                }
                Ok(key)
            }
            _ => Err(error),
        }
    }

    fn unlock_saved(&self) -> State {
        let file = match VaultFile::read(&self.path()) {
            Ok(file) => file,
            Err(lock) => return State::Locked { header: None, lock },
        };
        let fallback = match file.header.protection {
            Protection::Password => LockReason::Password,
            _ => LockReason::KeyUnavailable,
        };
        if !file.header.uses_store() {
            return State::Locked { header: Some(file.header), lock: Lock { reason: fallback, error: None } };
        }
        let key = match self.saved_key(&file) {
            Ok(key) => key,
            Err(error) => return State::Locked { header: Some(file.header), lock: Lock { reason: fallback, error: Some(error) } },
        };
        match file.open(&key) {
            Ok(data) => State::Encrypted(Encrypted { header: file.header, key, data }),
            Err(reason) => State::Locked { header: Some(file.header), lock: Lock { reason, error: None } },
        }
    }

    /// Unlocks with the master password: `vault.wrongPassword` if it isn't the one. Blocks
    /// while the key is derived. Also says whether this call opened it, which checked the
    /// password: an open vault takes any.
    pub fn unlock(&self, password: &str) -> Result<(VaultStatus, bool)> {
        let mut opened = false;
        let mut leftover = None;
        {
            let mut state = self.state.lock().unwrap();
            if let State::Locked { .. } = &*state {
                *state = match VaultFile::read(&self.path()) {
                    // Shown on the unlock screen instead.
                    Err(lock) => State::Locked { header: None, lock },
                    Ok(file) => {
                        if file.header.protection != Protection::Password {
                            return Err(Error::new("vault.wrongPassword"));
                        }
                        let key = file.header.unwrap_key(password).ok_or_else(|| Error::new("vault.wrongPassword"))?;
                        match file.open(&key) {
                            Ok(data) => {
                                leftover = self.remove_leftovers();
                                opened = true;
                                State::Encrypted(Encrypted { header: file.header, key, data })
                            }
                            Err(_) => State::Locked { header: Some(file.header), lock: Lock { reason: LockReason::Damaged, error: None } },
                        }
                    }
                };
            }
        }
        Ok((VaultStatus { leftover, ..self.status() }, opened))
    }

    /// Deletes the vault, whose key is lost (a forgotten master password), with any plain
    /// data files left next to it, which must not come back as the sessions: no sessions,
    /// unset.
    pub fn reset(&self, set_aside: &SetAside) -> Result<VaultStatus> {
        {
            let mut state = self.state.lock().unwrap();
            if !matches!(&*state, State::Locked { .. }) {
                return Err(Error::new("vault.notLocked"));
            }
            self.remove_plain()?;
            let path = self.path();
            persist::remove_file(&path).map_err(|e| Error::new("vault.deleteFailed").param("path", path.display()).detail(e))?;
            let _ = self.store.delete(KEY_ACCOUNT);
            let _ = self.store.delete(NEXT_KEY_ACCOUNT);
            *state = State::Plain(Plain::load(&self.dir, set_aside));
        }
        Ok(self.status())
    }

    /// Changes the protection. Leaving or changing password protection takes the current
    /// master password (`current`). From unset, the passwords move out of their entries in
    /// the credential store: one that can't be read fails the change
    /// (`vault.passwordsUnreadable`, with the sessions and proxies named) unless
    /// `skip_unreadable`, which leaves it there.
    ///
    /// The new files are written and checked before the old ones are deleted, and each change
    /// uses a new data key, so that a key left in the credential store opens nothing. A new
    /// key goes into the credential store under `vault-key-next`, and replaces `vault-key`
    /// only once the vault that needs it is in place: stopping at any point leaves a vault
    /// that `vault-key`, `vault-key-next` or the master password opens. If the old files
    /// can't be deleted afterwards the change is made all the same and `vault.leftover` says
    /// so; they go at the next unlock. Blocks: the credential store may ask the user, and keys
    /// are derived from passwords.
    pub fn set_protection(&self, target: Target, current: Option<&str>, skip_unreadable: bool) -> Result<VaultStatus> {
        let leftover = {
            let mut state = self.state.lock().unwrap();
            let (next, leftover) = self.change(&state, target, current, skip_unreadable)?;
            *state = next;
            leftover
        };
        match leftover {
            Some(error) => Err(error),
            None => Ok(self.status()),
        }
    }

    fn change(&self, state: &State, target: Target, current: Option<&str>, skip_unreadable: bool) -> Result<(State, Option<Error>)> {
        let (from, mut data) = match state {
            State::Locked { .. } => return Err(locked()),
            State::Plain(plain) => {
                if let Some(file) = plain.left_alone() {
                    let name = file.path().file_name().unwrap_or_default().to_string_lossy().into_owned();
                    return Err(Error::new("config.unreadable").param("name", name));
                }
                (plain.protection(), plain.data.clone())
            }
            State::Encrypted(vault) => (vault.header.protection, vault.data.clone()),
        };
        let old = match state {
            State::Encrypted(vault) => Some(vault),
            _ => None,
        };
        let current_password = match old {
            Some(vault) if vault.header.protection == Protection::Password => {
                let password = current.filter(|p| !p.is_empty()).ok_or_else(|| Error::new("vault.passwordRequired"))?;
                vault.header.unwrap_key(password).ok_or_else(|| Error::new("vault.wrongPassword"))?;
                Some(password)
            }
            _ => None,
        };

        // From unset, the passwords come out of the credential store.
        let mut moved = Vec::new();
        if from == Protection::Unset {
            let mut unreadable = Vec::new();
            let accounts = data
                .profiles
                .iter()
                .filter(|p| !p.id.is_empty())
                .map(|p| (p.id.clone(), p.name.clone()))
                .chain(data.proxies.iter().filter(|p| p.uses_password()).map(|p| (secrets::proxy_account(&p.id), p.name.clone())));
            for (account, name) in accounts {
                match self.store.get(&account) {
                    Ok(password) => {
                        data.passwords.insert(account.clone(), password);
                        moved.push(account);
                    }
                    Err(keyring::Error::NoEntry) => {}
                    Err(_) => unreadable.push(name),
                }
            }
            if !unreadable.is_empty() && !skip_unreadable {
                return Err(Error::new("vault.passwordsUnreadable").param("names", unreadable.join(", ")));
            }
        }
        let used_store = old.is_some_and(|vault| vault.header.uses_store());
        let mut leftover = None;

        let next = match target {
            Target::None => {
                let mut plain = Plain::at(&self.dir);
                let written = plain.write(&data, true);
                if old.is_some() {
                    // While the vault is there it is the one read: plain files beside it,
                    // half written or complete, are only a copy in the clear.
                    let path = self.path();
                    let removed = written.and_then(|()| {
                        persist::remove_file(&path).map_err(|e| Error::new("vault.deleteFailed").param("path", path.display()).detail(e))
                    });
                    if let Err(e) = removed {
                        let _ = self.remove_plain();
                        return Err(e);
                    }
                } else {
                    written?;
                }
                State::Plain(plain)
            }
            Target::System | Target::Password { .. } => {
                let key = crypto::random_key();
                let (protection, remember, kdf, wrapped_key) = match target {
                    Target::Password { password, remember } => {
                        let password = match password.as_deref() {
                            Some("") => return Err(Error::new("vault.emptyPassword")),
                            Some(password) => password,
                            None => current_password.ok_or_else(|| Error::new("vault.passwordRequired"))?,
                        };
                        let kdf = Kdf::new();
                        let wrapping = kdf.derive(password).expect("the current parameters");
                        (Protection::Password, remember, Some(kdf), Some(crypto::seal(&wrapping, WRAP_AAD, &*key)))
                    }
                    _ => (Protection::System, false, None, None),
                };
                let header = Header {
                    format: FORMAT.to_owned(),
                    version: VERSION,
                    protection,
                    key_id: crypto::key_id(&key),
                    remember,
                    kdf,
                    wrapped_key,
                };
                let path = self.path();
                let staged = stage_vault(&path, &header, &key, &data)?;
                if header.uses_store() {
                    self.store.set(NEXT_KEY_ACCOUNT, &crypto::encode(&*key))?;
                }
                if let Err(e) = staged.commit() {
                    let _ = self.store.delete(NEXT_KEY_ACCOUNT);
                    return Err(e);
                }
                if header.uses_store() {
                    // Only once the vault in place opens with the new key: until then
                    // `vault-key` (the old key) or `vault-key-next` opens whichever is there.
                    let opens = VaultFile::read(&path).ok().is_some_and(|file| file.open(&key).is_ok());
                    if !opens {
                        // Coming from plain files, which stay the ones written to, a vault
                        // beside them would be read instead of them at the next start.
                        if old.is_none() {
                            let _ = persist::remove_file(&path);
                            let _ = self.store.delete(NEXT_KEY_ACCOUNT);
                        }
                        return Err(Error::new("vault.verifyFailed").param("path", path.display()));
                    }
                    if self.store.set(KEY_ACCOUNT, &crypto::encode(&*key)).is_ok() {
                        let _ = self.store.delete(NEXT_KEY_ACCOUNT);
                    }
                }
                if old.is_none() {
                    leftover = self.remove_plain().err().map(|e| Error::new("vault.leftover").detail(e));
                }
                State::Encrypted(Encrypted { header, key, data })
            }
        };

        // What is left of the old protection; the new one works without it.
        let uses_store = matches!(&next, State::Encrypted(vault) if vault.header.uses_store());
        if used_store && !uses_store {
            let _ = self.store.delete(KEY_ACCOUNT);
            let _ = self.store.delete(NEXT_KEY_ACCOUNT);
        }
        for account in moved {
            let _ = self.store.delete(&account);
        }
        Ok((next, leftover))
    }
}

#[cfg(test)]
mod tests;
