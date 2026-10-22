use std::path::Path;
use std::sync::atomic::Ordering;

use super::*;
use crate::model::{Connection, Remote, SshOptions};
use crate::secrets::MemoryStore;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zshell-vault-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(dir: &Path, store: &Arc<MemoryStore>) -> Vault {
    Vault::open(dir.to_owned(), store.clone())
}

/// A new vault in `dir`, unlocked: encrypted with a key in `store` if it can be used.
fn create(dir: &Path, store: &Arc<MemoryStore>) -> Vault {
    let vault = open(dir, store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::New));
    let status = vault.try_unlock();
    assert!(status.locked.is_none() && status.created);
    // Said once.
    assert!(!vault.try_unlock().created && !vault.status().created);
    vault
}

fn profile(id: &str, name: &str) -> Profile {
    let ssh = SshOptions::new(Remote::new(format!("{name}.example.com"), 22, "alice".into()));
    Profile { id: id.into(), ..Profile::new(name.into(), Connection::Ssh(ssh)) }
}

fn add(vault: &Vault, id: &str, name: &str, password: Option<&str>) {
    vault
        .update(|data| {
            data.profiles.push(profile(id, name));
            data.passwords.set(Account::Profile(id), password);
            Ok(())
        })
        .unwrap();
}

fn names(vault: &Vault) -> Vec<String> {
    vault.data().unwrap().profiles.iter().map(|p| p.name.clone()).collect()
}

fn password(protection: Option<&str>, remember: bool) -> Target {
    Target::Password { password: protection.map(Into::into), remember }
}

/// The credential store's entries for keys.
fn keys(store: &MemoryStore) -> Vec<String> {
    let mut keys: Vec<String> = store.entries.lock().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

fn text(dir: &Path) -> String {
    fs::read_to_string(dir.join(VAULT_FILE)).unwrap()
}

#[test]
fn a_new_vault_uses_the_credential_store_if_it_can() {
    let dir = dir("new");
    let store = Arc::new(MemoryStore::default());
    let vault = create(&dir, &store);
    assert_eq!(vault.status().protection, Some(Protection::System));
    let id = crypto::key_id(&crypto::decode_key(&store.entries.lock().unwrap().values().next().unwrap().clone()).unwrap());
    assert_eq!(keys(&store), [format!("{KEY_ACCOUNT}{id}")]);
    assert!(text(&dir).contains(&format!(r#""keyId":"{id}""#)));
    fs::remove_dir_all(&dir).unwrap();

    // Without it, in clear.
    let dir = self::dir("new-plain");
    let store = Arc::new(MemoryStore::default());
    store.failing.store(true, Ordering::Relaxed);
    let vault = create(&dir, &store);
    assert_eq!(vault.status().protection, Some(Protection::None));
    add(&vault, "a", "alpha", Some("secret"));
    assert!(text(&dir).contains("alpha") && text(&dir).contains("secret"));
    // Read without the credential store, at once.
    let vault = open(&dir, &store);
    assert!(vault.status().locked.is_none());
    assert_eq!(vault.password(Account::Profile("a")).unwrap().as_deref(), Some("secret"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[cfg(unix)]
fn the_file_is_readable_by_the_user_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = dir("modes");
    let store = Arc::new(MemoryStore::default());
    create(&dir, &store);
    assert_eq!(fs::metadata(dir.join(VAULT_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn changes_protection_with_a_new_key_each_time() {
    let dir = dir("change");
    let store = Arc::new(MemoryStore::default());
    let vault = create(&dir, &store);
    add(&vault, "a", "alpha", Some("secret"));
    let system = keys(&store);

    vault.set_protection(password(Some("master"), false), None).unwrap();
    // The old key is gone, and the new one isn't kept.
    assert!(keys(&store).is_empty());
    assert!(!text(&dir).contains("alpha") && !text(&dir).contains("secret"));
    let vault = open(&dir, &store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::Password));
    assert_eq!(vault.unlock("wrong").unwrap_err().code(), "vault.wrongPassword");
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    assert_eq!(vault.password(Account::Profile("a")).unwrap().as_deref(), Some("secret"));

    // Leaving it takes the master password.
    assert_eq!(vault.set_protection(Target::None, None).unwrap_err().code(), "vault.passwordRequired");
    assert_eq!(vault.set_protection(Target::None, Some("wrong")).unwrap_err().code(), "vault.wrongPassword");
    vault.set_protection(Target::None, Some("master")).unwrap();
    assert!(text(&dir).contains("secret"));
    assert_eq!(names(&open(&dir, &store)), ["alpha"]);

    vault.set_protection(Target::System, None).unwrap();
    assert_eq!(keys(&store).len(), 1);
    assert_ne!(keys(&store), system);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn system_protection_and_remembered_passwords_unlock_from_the_credential_store() {
    let dir = dir("system");
    let store = Arc::new(MemoryStore::default());
    let vault = create(&dir, &store);
    add(&vault, "a", "alpha", None);

    let vault = open(&dir, &store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::Saved));
    assert_eq!(vault.data().unwrap_err().code(), "vault.locked");
    let status = vault.try_unlock();
    assert!(status.locked.is_none() && !status.created);
    assert_eq!(names(&vault), ["alpha"]);

    // Remembered master password: unlocks the same way, and falls back to asking for it.
    vault.set_protection(password(Some("master"), true), None).unwrap();
    assert_eq!(keys(&store).len(), 1);
    let vault = open(&dir, &store);
    assert!(vault.status().remember);
    assert!(vault.try_unlock().locked.is_none());
    store.failing.store(true, Ordering::Relaxed);
    let vault = open(&dir, &store);
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::Password, Some("credentialStore")));
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    store.failing.store(false, Ordering::Relaxed);

    // Not remembered any more: the key leaves the credential store.
    vault.set_protection(password(None, false), Some("master")).unwrap();
    assert!(keys(&store).is_empty());
    let vault = open(&dir, &store);
    assert_eq!(vault.try_unlock().locked.map(|l| l.reason), Some(LockReason::Password));
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_or_wrong_key_does_not_unlock() {
    let dir = dir("stale");
    let store = Arc::new(MemoryStore::default());
    create(&dir, &store);
    let account = keys(&store).remove(0);

    store.entries.lock().unwrap().insert(account.clone(), crypto::encode(&*crypto::random_key()));
    let vault = open(&dir, &store);
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::KeyUnavailable, Some("vault.keyMismatch")));

    store.entries.lock().unwrap().clear();
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::KeyUnavailable, Some("vault.keyMissing")));

    // Only a reset helps; a new vault follows. The old one is moved aside, and its key stays
    // in the credential store, to be opened again if the key comes back.
    store.entries.lock().unwrap().insert(account.clone(), "left".into());
    let old = fs::read(dir.join(VAULT_FILE)).unwrap();
    assert_eq!(vault.reset().unwrap().locked.map(|l| l.reason), Some(LockReason::New));
    assert!(!dir.join(VAULT_FILE).exists());
    let aside: Vec<_> =
        fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().contains(".reset-")).collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(fs::read(&aside[0]).unwrap(), old);
    assert_eq!(keys(&store), std::slice::from_ref(&account));
    assert!(vault.try_unlock().locked.is_none());
    assert_eq!(keys(&store).len(), 2);
    assert!(vault.data().unwrap().profiles.is_empty());
    assert_eq!(vault.reset().unwrap_err().code(), "vault.notLocked");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn damaged_and_newer_vaults_stay_locked_and_untouched() {
    let dir = dir("damaged");
    let store = Arc::new(MemoryStore::default());
    create(&dir, &store);
    let text = text(&dir);

    // A changed header no longer authenticates.
    fs::write(dir.join(VAULT_FILE), text.replace(r#""version":2"#, r#""version":2,"x":1"#)).unwrap();
    let vault = open(&dir, &store);
    assert_eq!(vault.try_unlock().locked.map(|l| l.reason), Some(LockReason::Damaged));
    fs::write(dir.join(VAULT_FILE), "{").unwrap();
    let lock = open(&dir, &store).status().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::Damaged, Some("vault.parseFailed")));
    fs::write(dir.join(VAULT_FILE), text.replace(r#""version":2"#, r#""version":1"#)).unwrap();
    assert_eq!(open(&dir, &store).status().locked.map(|l| l.reason), Some(LockReason::Damaged));
    fs::write(dir.join(VAULT_FILE), text.replace(r#""version":2"#, r#""version":3"#)).unwrap();
    let vault = open(&dir, &store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::Newer));
    assert_eq!(vault.update(|_| Ok(())).unwrap_err().code(), "vault.locked");
    assert!(fs::read_to_string(dir.join(VAULT_FILE)).unwrap().contains(r#""version":3"#));

    // In clear, data that doesn't parse says where; fixed by hand, trying again opens it.
    let plain = r#"{ "header": { "format": "zshell-vault", "version": 2, "protection": "none" }, "data": { "profiles": 1 } }"#;
    fs::write(dir.join(VAULT_FILE), plain).unwrap();
    let vault = open(&dir, &store);
    let lock = vault.status().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::Damaged, Some("vault.parseFailed")));
    fs::write(dir.join(VAULT_FILE), plain.replace(r#""profiles": 1"#, r#""profiles": []"#)).unwrap();
    assert!(vault.try_unlock().locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

/// A directory where `name` goes, which a file can't be put in place of.
fn block(dir: &Path, name: &str) {
    let _ = fs::remove_file(dir.join(name));
    fs::create_dir_all(dir.join(name).join("x")).unwrap();
}

#[test]
fn a_change_that_fails_changes_nothing() {
    let dir = dir("fails");
    let store = Arc::new(MemoryStore::default());
    let vault = create(&dir, &store);
    add(&vault, "a", "alpha", None);
    let before = keys(&store);
    let saved = text(&dir);

    // The new file can't be put in place: its key leaves the credential store again.
    fs::rename(dir.join(VAULT_FILE), dir.join("saved")).unwrap();
    block(&dir, VAULT_FILE);
    assert!(vault.set_protection(password(Some("master"), true), None).is_err());
    assert_eq!(keys(&store), before);
    assert_eq!(vault.status().protection, Some(Protection::System));
    assert!(vault
        .update(|data| {
            data.profiles.clear();
            Ok(())
        })
        .is_err());
    assert_eq!(names(&vault), ["alpha"]);
    fs::remove_dir_all(dir.join(VAULT_FILE)).unwrap();
    fs::rename(dir.join("saved"), dir.join(VAULT_FILE)).unwrap();
    assert_eq!(text(&dir), saved);

    // A failed change of the data writes nothing.
    let error = vault.update(|data| -> Result<()> {
        data.profiles.clear();
        Err(Error::new("profile.notFound"))
    });
    assert_eq!(error.unwrap_err().code(), "profile.notFound");
    assert_eq!(text(&dir), saved);
    assert_eq!(names(&vault), ["alpha"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn system_protection_needs_the_credential_store() {
    let dir = dir("no-store");
    let store = Arc::new(MemoryStore::default());
    store.failing.store(true, Ordering::Relaxed);
    let vault = create(&dir, &store);
    add(&vault, "a", "alpha", None);
    assert!(!vault.store_available());
    assert_eq!(vault.set_protection(Target::System, None).unwrap_err().code(), "credentialStore");
    // Nothing changed.
    assert_eq!(vault.status().protection, Some(Protection::None));
    assert!(text(&dir).contains("alpha"));
    // A master password works without it.
    vault.set_protection(password(Some("master"), false), None).unwrap();
    assert!(open(&dir, &store).unlock("master").unwrap().0.locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keeps_only_the_passwords_of_what_exists() {
    let mut passwords = Passwords::default();
    passwords.set(Account::Profile("a"), Some("1"));
    passwords.set(Account::Profile("gone"), Some("2"));
    passwords.set(Account::Proxy("x"), Some("3"));
    passwords.set(Account::Proxy("open"), Some("4"));
    let proxy = |id: &str, username: &str| Proxy {
        id: id.into(),
        name: id.into(),
        kind: crate::model::ProxyKind::Socks5,
        host: "proxy.lan".into(),
        port: 1080,
        username: username.into(),
        command: String::new(),
    };
    passwords.retain_owners(&[profile("a", "alpha")], &[proxy("x", "u"), proxy("open", "")]);
    assert_eq!(passwords.get(Account::Profile("a")), Some("1"));
    assert_eq!(passwords.get(Account::Profile("gone")), None);
    assert_eq!(passwords.get(Account::Proxy("x")), Some("3"));
    // A proxy without a user name takes no password.
    assert_eq!(passwords.get(Account::Proxy("open")), None);
    passwords.set(Account::Profile("a"), None);
    passwords.set(Account::Proxy("x"), None);
    assert!(passwords.is_empty());
}

/// A reset while the credential store is still answering an unlock (the system asking the
/// user) stays: the old vault, read before, doesn't come back.
#[test]
fn a_reset_during_an_unlock_stays() {
    let dir = dir("reset-while-unlocking");
    let store = Arc::new(MemoryStore::default());
    add(&create(&dir, &store), "a", "web", None);

    let vault = Arc::new(open(&dir, &store));
    let hold = store.hold_get();
    let unlocking = std::thread::spawn({
        let vault = vault.clone();
        move || vault.try_unlock()
    });
    // The old key is read; reset while the store still answers.
    hold.started();
    assert_eq!(vault.reset().unwrap().locked.map(|l| l.reason), Some(LockReason::New));
    drop(hold);
    assert!(unlocking.join().unwrap().locked.is_some());
    assert!(vault.data().is_err());

    assert!(vault.try_unlock().locked.is_none());
    assert!(names(&vault).is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn copies_follow_the_vault_being_unlocked_and_reset() {
    let dir = dir("part");
    let store = Arc::new(MemoryStore::default());
    let vault = create(&dir, &store);
    add(&vault, "a", "alpha", None);
    vault.set_protection(password(Some("master"), false), None).unwrap();

    // Made while the vault is locked, as at startup: empty.
    let vault = Arc::new(open(&dir, &store));
    let part = Part::new(vault.clone(), |data| data.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>());
    assert!(part.read().is_empty());
    let generation = vault.generation();
    vault.unlock("master").unwrap();
    assert_eq!(*part.read(), ["alpha"]);
    // Changes written by others don't make it read again: the store replaces its copy.
    add(&vault, "b", "beta", None);
    assert_eq!(vault.generation(), generation + 1);
    assert_eq!(*part.read(), ["alpha"]);

    let vault = Arc::new(open(&dir, &store));
    let part = Part::new(vault.clone(), |data| data.profiles.len());
    vault.reset().unwrap();
    assert_eq!(*part.read(), 0);
    vault.try_unlock();
    assert_eq!(*part.read(), 0);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn queries_answer_while_a_change_waits_for_the_credential_store() {
    let dir = dir("view");
    let store = Arc::new(MemoryStore::default());
    store.failing.store(true, Ordering::Relaxed);
    let vault = Arc::new(create(&dir, &store));
    add(&vault, "a", "alpha", Some("secret"));
    store.failing.store(false, Ordering::Relaxed);
    let hold = store.hold_set();

    let changing = {
        let vault = vault.clone();
        std::thread::spawn(move || vault.set_protection(Target::System, None).unwrap())
    };
    hold.started();
    assert_eq!(names(&vault), ["alpha"]);
    assert_eq!(vault.password(Account::Profile("a")).unwrap().as_deref(), Some("secret"));
    assert!(vault.app_lock().is_some());
    // As it was until the change is done.
    assert_eq!(vault.status().protection, Some(Protection::None));
    // Answered without waiting for it.
    assert!(!changing.is_finished());
    drop(hold);
    changing.join().unwrap();
    assert_eq!(vault.status().protection, Some(Protection::System));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_part_changes_the_data_as_it_is_not_its_copy() {
    let dir = dir("part-update");
    let store = Arc::new(MemoryStore::default());
    let vault = Arc::new(create(&dir, &store));
    let part = Part::new(vault.clone(), |data| data.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>());
    // Written by someone else: the copy doesn't know.
    add(&vault, "a", "alpha", None);
    assert!(part.read().is_empty());

    part.update(|names, data| {
        assert_eq!(*names, ["alpha"]);
        names.push("beta".into());
        data.profiles.push(profile("b", "beta"));
        Ok(())
    })
    .unwrap();
    assert_eq!(*part.read(), ["alpha", "beta"]);
    assert_eq!(names(&vault), ["alpha", "beta"]);

    // A failed change keeps both.
    let failed = part.update(|names, data| {
        names.clear();
        data.profiles.clear();
        Err::<(), _>(Error::new("unexpected"))
    });
    assert!(failed.is_err());
    assert_eq!(*part.read(), ["alpha", "beta"]);
    assert_eq!(names(&vault), ["alpha", "beta"]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn debug_output_leaves_out_passwords() {
    let mut passwords = Passwords::default();
    passwords.profiles.insert("p1".into(), "hunter2".into());
    passwords.proxies.insert("x1".into(), "swordfish".into());
    let data = Data { passwords, ..Default::default() };
    let target = Target::Password { password: Some("master".into()), remember: true };
    let shown = format!("{data:?} {target:?}");
    assert!(shown.contains("p1") && shown.contains("x1"), "{shown}");
    for secret in ["hunter2", "swordfish", "master"] {
        assert!(!shown.contains(secret), "{shown}");
    }
}
