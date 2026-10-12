use std::sync::atomic::Ordering;

use super::*;
use crate::config::{Connection, Remote, SshOptions};
use crate::secrets::MemoryStore;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zshell-vault-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(dir: &Path, store: &Arc<MemoryStore>) -> Vault {
    Vault::open(dir.to_owned(), store.clone(), &SetAside::default())
}

fn profile(id: &str, name: &str) -> Profile {
    let ssh = SshOptions::new(Remote::new(format!("{name}.example.com"), 22, "alice".into()));
    Profile { id: id.into(), ..Profile::new(name.into(), Connection::Ssh(ssh)) }
}

fn names(vault: &Vault) -> Vec<String> {
    vault.data().unwrap().profiles.iter().map(|p| p.name.clone()).collect()
}

fn password(protection: Option<&str>, remember: bool) -> Target {
    Target::Password { password: protection.map(Into::into), remember }
}

#[test]
fn unset_keeps_plain_files_and_the_credential_store() {
    let dir = dir("unset");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    assert_eq!(vault.status().protection, Some(Protection::Unset));
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_password("a", Some("secret")).unwrap();
    assert!(dir.join("profiles.json").exists() && !dir.join(PASSWORDS_FILE).exists());
    assert_eq!(store.entries.lock().unwrap()["a"], "secret");
    assert_eq!(vault.password("a").unwrap().as_deref(), Some("secret"));
    vault.set_password("a", None).unwrap();
    assert_eq!(vault.password("a").unwrap(), None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[cfg(unix)]
fn plain_files_are_readable_by_the_user_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = dir("modes");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.set_protection(Target::None, None, false).unwrap();
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    for name in ["profiles.json", PASSWORDS_FILE] {
        assert_eq!(fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o777, 0o600, "{name}");
    }
    assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn moves_from_unset_to_a_master_password_and_back_to_plain() {
    let dir = dir("password");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_password("a", Some("secret")).unwrap();

    vault.set_protection(password(Some("master"), false), None, false).unwrap();
    // The plain files and the entry are gone; nothing in the credential store.
    assert!(dir.join(VAULT_FILE).exists());
    for name in PLAIN_FILES {
        assert!(!dir.join(name).exists(), "{name}");
    }
    assert!(store.entries.lock().unwrap().is_empty());
    let text = fs::read_to_string(dir.join(VAULT_FILE)).unwrap();
    assert!(!text.contains("alpha") && !text.contains("secret"));
    assert_eq!(vault.password("a").unwrap().as_deref(), Some("secret"));

    // Starting again: locked until the master password is given.
    let vault = open(&dir, &store);
    let status = vault.status();
    assert_eq!(status.locked.as_ref().map(|l| l.reason), Some(LockReason::Password));
    assert_eq!(vault.data().unwrap_err().code(), "vault.locked");
    assert_eq!(vault.update(|_| ()).unwrap_err().code(), "vault.locked");
    assert_eq!(vault.unlock("wrong").unwrap_err().code(), "vault.wrongPassword");
    let (status, opened) = vault.unlock("master").unwrap();
    assert!(status.locked.is_none() && opened);
    // Open already: any password passes, and says it wasn't checked.
    assert!(!vault.unlock("wrong").unwrap().1);
    assert_eq!(names(&vault), ["alpha"]);

    // Leaving password protection takes the master password.
    assert_eq!(vault.set_protection(Target::None, None, false).unwrap_err().code(), "vault.passwordRequired");
    assert_eq!(vault.set_protection(Target::None, Some("wrong"), false).unwrap_err().code(), "vault.wrongPassword");
    vault.set_protection(Target::None, Some("master"), false).unwrap();
    assert!(!dir.join(VAULT_FILE).exists());
    let vault = open(&dir, &store);
    assert_eq!(vault.status().protection, Some(Protection::None));
    assert_eq!(names(&vault), ["alpha"]);
    assert_eq!(vault.password("a").unwrap().as_deref(), Some("secret"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn system_protection_and_remembered_passwords_unlock_from_the_credential_store() {
    let dir = dir("system");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_protection(Target::System, None, false).unwrap();
    assert!(store.entries.lock().unwrap().contains_key(KEY_ACCOUNT));

    let vault = open(&dir, &store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::Saved));
    assert!(vault.try_unlock().locked.is_none());
    assert_eq!(names(&vault), ["alpha"]);

    // Remembered master password: unlocks the same way, and falls back to asking for it.
    vault.set_protection(password(Some("master"), true), None, false).unwrap();
    let vault = open(&dir, &store);
    assert!(vault.try_unlock().locked.is_none());
    store.failing.store(true, Ordering::Relaxed);
    let vault = open(&dir, &store);
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::Password, Some("credentialStore")));
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    store.failing.store(false, Ordering::Relaxed);

    // Not remembered any more: the key leaves the credential store.
    vault.set_protection(password(None, false), Some("master"), false).unwrap();
    assert!(!store.entries.lock().unwrap().contains_key(KEY_ACCOUNT));
    let vault = open(&dir, &store);
    assert_eq!(vault.try_unlock().locked.map(|l| l.reason), Some(LockReason::Password));
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_key_left_behind_or_missing_does_not_unlock() {
    let dir = dir("stale");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.set_protection(Target::System, None, false).unwrap();
    let first = store.entries.lock().unwrap()[KEY_ACCOUNT].clone();
    // Each change makes a new key: the old one opens nothing.
    vault.set_protection(Target::System, None, false).unwrap();
    store.entries.lock().unwrap().insert(KEY_ACCOUNT.into(), first);
    let vault = open(&dir, &store);
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::KeyUnavailable, Some("vault.keyMismatch")));

    store.entries.lock().unwrap().clear();
    let lock = vault.try_unlock().locked.unwrap();
    assert_eq!((lock.reason, lock.error.map(|e| e.code())), (LockReason::KeyUnavailable, Some("vault.keyMissing")));

    // Only a reset helps, and leaves nothing.
    assert!(vault.reset(&SetAside::default()).unwrap().locked.is_none());
    assert_eq!(vault.status().protection, Some(Protection::Unset));
    assert!(vault.data().unwrap().profiles.is_empty() && !dir.join(VAULT_FILE).exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn damaged_and_newer_vaults_stay_locked_and_untouched() {
    let dir = dir("damaged");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.set_protection(Target::System, None, false).unwrap();
    let text = fs::read_to_string(dir.join(VAULT_FILE)).unwrap();

    // A changed header no longer authenticates.
    fs::write(dir.join(VAULT_FILE), text.replace("\"version\":1", "\"version\":1,\"remember\":true")).unwrap();
    let vault = open(&dir, &store);
    assert_eq!(vault.try_unlock().locked.map(|l| l.reason), Some(LockReason::Damaged));
    fs::write(dir.join(VAULT_FILE), "{").unwrap();
    assert_eq!(open(&dir, &store).status().locked.map(|l| l.reason), Some(LockReason::Damaged));
    fs::write(dir.join(VAULT_FILE), text.replace("\"version\":1", "\"version\":2")).unwrap();
    let vault = open(&dir, &store);
    assert_eq!(vault.status().locked.map(|l| l.reason), Some(LockReason::Newer));
    assert_eq!(vault.update(|_| ()).unwrap_err().code(), "vault.locked");
    assert!(fs::read_to_string(dir.join(VAULT_FILE)).unwrap().contains("\"version\":2"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unreadable_passwords_stop_the_change_unless_skipped() {
    let dir = dir("unreadable");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_password("a", Some("secret")).unwrap();
    store.failing.store(true, Ordering::Relaxed);
    let error = vault.set_protection(Target::None, None, false).unwrap_err();
    assert_eq!(error.code(), "vault.passwordsUnreadable");
    assert_eq!(vault.status().protection, Some(Protection::Unset));
    assert!(!dir.join(PASSWORDS_FILE).exists());

    vault.set_protection(Target::None, None, true).unwrap();
    assert_eq!(vault.status().protection, Some(Protection::None));
    assert_eq!(vault.password("a").unwrap(), None);
    // The entry that couldn't be read stays where it is.
    store.failing.store(false, Ordering::Relaxed);
    assert_eq!(store.entries.lock().unwrap()["a"], "secret");
    fs::remove_dir_all(&dir).unwrap();
}

/// A directory where `name` goes, which a file can't be put in place of.
fn block(dir: &Path, name: &str) {
    let _ = fs::remove_file(dir.join(name));
    fs::create_dir_all(dir.join(name).join("x")).unwrap();
}

fn unblock(dir: &Path, name: &str) {
    fs::remove_dir_all(dir.join(name)).unwrap();
}

#[test]
fn a_new_key_waits_until_its_vault_is_in_place() {
    let dir = dir("next-key");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();

    // The vault can't be put in place: nothing reaches the credential store.
    block(&dir, VAULT_FILE);
    assert!(vault.set_protection(Target::System, None, false).is_err());
    assert!(store.entries.lock().unwrap().is_empty());
    assert_eq!(vault.status().protection, Some(Protection::Unset));
    assert!(dir.join("profiles.json").exists());
    unblock(&dir, VAULT_FILE);

    vault.set_protection(Target::System, None, false).unwrap();
    let first = store.entries.lock().unwrap()[KEY_ACCOUNT].clone();
    assert!(!store.entries.lock().unwrap().contains_key(NEXT_KEY_ACCOUNT));

    // Stopped after saving the new key, before its vault was in place: the old key opens.
    store.entries.lock().unwrap().insert(NEXT_KEY_ACCOUNT.into(), crypto::encode(&*crypto::random_key()));
    assert!(open(&dir, &store).try_unlock().locked.is_none());

    // Stopped after the new vault was in place, before `vault-key` was replaced: the new
    // key opens it, and becomes `vault-key`. From system protection, with no master
    // password to fall back on, this is all there is.
    vault.set_protection(password(Some("master"), true), None, false).unwrap();
    let new = store.entries.lock().unwrap()[KEY_ACCOUNT].clone();
    assert_ne!(new, first);
    store.entries.lock().unwrap().insert(KEY_ACCOUNT.into(), first.clone());
    store.entries.lock().unwrap().insert(NEXT_KEY_ACCOUNT.into(), new.clone());
    let vault = open(&dir, &store);
    assert!(vault.try_unlock().locked.is_none());
    assert_eq!(names(&vault), ["alpha"]);
    assert_eq!(store.entries.lock().unwrap()[KEY_ACCOUNT], new);
    assert!(!store.entries.lock().unwrap().contains_key(NEXT_KEY_ACCOUNT));

    // With a master password, a key that opens nothing still leaves the password.
    store.entries.lock().unwrap().insert(KEY_ACCOUNT.into(), first);
    let vault = open(&dir, &store);
    assert_eq!(vault.try_unlock().locked.map(|l| l.reason), Some(LockReason::Password));
    assert!(vault.unlock("master").unwrap().0.locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn finds_and_deletes_plain_copies_set_aside() {
    let dir = dir("copies");
    let store = Arc::new(MemoryStore::default());
    for name in [
        "passwords.json.bad-20261012-101010",
        "profiles.json.bad-20261012-101010",
        "settings.json.bad-20261012-101010",
        "profiles.json.tmp",
    ] {
        fs::write(dir.join(name), "{").unwrap();
    }
    let vault = open(&dir, &store);
    let names = |vault: &Vault| -> Vec<String> {
        vault.plain_copies().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
    };
    assert_eq!(names(&vault), ["passwords.json.bad-20261012-101010", "profiles.json.bad-20261012-101010"]);
    vault.delete_plain_copies().unwrap();
    assert!(names(&vault).is_empty());
    // Settings aren't the vault's.
    assert!(dir.join("settings.json.bad-20261012-101010").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_change_that_stops_part_way_leaves_nothing_in_the_clear() {
    let dir = dir("clear");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_password("a", Some("secret")).unwrap();
    vault.set_protection(password(Some("master"), false), None, false).unwrap();

    // Turning encryption off fails after some plain files are in place: they go again.
    block(&dir, "proxies.json");
    assert!(vault.set_protection(Target::None, Some("master"), false).is_err());
    unblock(&dir, "proxies.json");
    for name in PLAIN_FILES {
        assert!(!dir.join(name).exists(), "{name}");
    }
    assert_eq!(vault.status().protection, Some(Protection::Password));
    assert_eq!(names(&vault), ["alpha"]);

    // A change that stopped (a crash) left plain files beside the vault: they stay until
    // the vault opens, then go.
    fs::write(dir.join("profiles.json"), "[]").unwrap();
    fs::write(dir.join(PASSWORDS_FILE), r#"{"a":"secret"}"#).unwrap();
    fs::write(dir.join("commands.json.tmp"), "{}").unwrap();
    // One of them can't be deleted: the unlock says so.
    block(&dir, "folders.json");
    let vault = open(&dir, &store);
    assert!(dir.join(PASSWORDS_FILE).exists());
    let (status, _) = vault.unlock("master").unwrap();
    assert!(status.locked.is_none());
    assert_eq!(status.leftover.map(|e| e.code()), Some("vault.plainLeft"));
    assert!(vault.status().leftover.is_none());
    unblock(&dir, "folders.json");
    for name in PLAIN_FILES {
        assert!(!dir.join(name).exists(), "{name}");
    }
    assert!(!dir.join("commands.json.tmp").exists());
    assert_eq!(vault.password("a").unwrap().as_deref(), Some("secret"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_reset_does_not_bring_back_what_was_left_in_the_clear() {
    let dir = dir("reset-leftovers");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_protection(Target::None, None, false).unwrap();
    vault.set_password("a", Some("secret")).unwrap();
    // Left behind by a change to encryption that stopped part way.
    let profiles = fs::read(dir.join("profiles.json")).unwrap();
    let passwords = fs::read(dir.join(PASSWORDS_FILE)).unwrap();
    vault.set_protection(password(Some("master"), false), None, false).unwrap();
    fs::write(dir.join("profiles.json"), profiles).unwrap();
    fs::write(dir.join(PASSWORDS_FILE), passwords).unwrap();

    let vault = open(&dir, &store);
    vault.reset(&SetAside::default()).unwrap();
    assert_eq!(vault.status().protection, Some(Protection::Unset));
    assert!(vault.data().unwrap().profiles.is_empty());
    for name in PLAIN_FILES {
        assert!(!dir.join(name).exists(), "{name}");
    }
    assert!(!dir.join(VAULT_FILE).exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn memory_follows_the_disk_when_a_write_stops_part_way() {
    let dir = dir("part-way");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    let folder = Folder { id: "f".into(), name: "Work".into(), parent: None };

    // Folders are put in place first, proxies next; the proxies can't be.
    block(&dir, "proxies.json");
    let changed = vault.update(|data| {
        data.folders.push(folder.clone());
        let kind = crate::proxy::ProxyKind::Socks5;
        data.proxies.push(Proxy {
            id: "x".into(),
            name: "corp".into(),
            kind,
            host: "proxy.lan".into(),
            port: 1080,
            username: String::new(),
            command: String::new(),
        });
        data.profiles.push(profile("b", "beta"));
    });
    assert!(changed.is_err());
    let data = vault.data().unwrap();
    assert_eq!(data.folders.len(), 1);
    assert!(data.proxies.is_empty());
    assert_eq!(names(&vault), ["alpha"]);

    unblock(&dir, "proxies.json");
    let reopened = open(&dir, &store).data().unwrap();
    assert_eq!(serde_json::to_value(&reopened).unwrap(), serde_json::to_value(&data).unwrap());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_password_saved_while_moving_out_of_the_credential_store_is_kept() {
    let dir = dir("overlap");
    let store = Arc::new(MemoryStore::default());
    let vault = Arc::new(open(&dir, &store));
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_password("a", Some("old")).unwrap();

    // Saving takes a while (the system asking); meanwhile the passwords move out.
    store.set_delay_ms.store(300, Ordering::Relaxed);
    let saving = {
        let vault = vault.clone();
        std::thread::spawn(move || vault.set_password("a", Some("new")).unwrap())
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    vault.set_protection(Target::None, None, false).unwrap();
    saving.join().unwrap();
    store.set_delay_ms.store(0, Ordering::Relaxed);

    assert_eq!(vault.password("a").unwrap().as_deref(), Some("new"));
    assert!(!store.entries.lock().unwrap().contains_key("a"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn system_protection_needs_the_credential_store() {
    let dir = dir("no-store");
    let store = Arc::new(MemoryStore::default());
    let vault = open(&dir, &store);
    vault.update(|data| data.profiles.push(profile("a", "alpha"))).unwrap();
    vault.set_protection(Target::None, None, false).unwrap();
    store.failing.store(true, Ordering::Relaxed);
    assert!(!vault.store_available());
    assert_eq!(vault.set_protection(Target::System, None, false).unwrap_err().code(), "credentialStore");
    // Nothing changed.
    assert_eq!(vault.status().protection, Some(Protection::None));
    assert!(!dir.join(VAULT_FILE).exists() && dir.join("profiles.json").exists());
    // A master password works without it.
    vault.set_protection(password(Some("master"), false), None, false).unwrap();
    assert!(open(&dir, &store).unlock("master").unwrap().0.locked.is_none());
    fs::remove_dir_all(&dir).unwrap();
}
