use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::forward::ForwardRule;
use crate::proxy::Proxy;
use crate::vault::{Account, Vault};

/// The vault in `dir`, in clear (made if there is none), unlocked.
fn vault(dir: &Path) -> Arc<Vault> {
    let store = Arc::new(crate::secrets::MemoryStore::default());
    store.failing.store(true, std::sync::atomic::Ordering::Relaxed);
    let vault = Vault::open(dir.to_owned(), store);
    vault.try_unlock();
    Arc::new(vault)
}

fn load(dir: &Path) -> ProfileStore {
    ProfileStore::load(vault(dir))
}

/// A vault in clear in `dir` holding `data`, as a hand-edited file may.
fn write_vault(dir: &Path, data: serde_json::Value) {
    fs::create_dir_all(dir).unwrap();
    let file = serde_json::json!({ "header": { "format": "zshell-vault", "version": 2, "protection": "none" }, "data": data });
    fs::write(dir.join(crate::vault::VAULT_FILE), file.to_string()).unwrap();
}

fn store(name: &str) -> (ProfileStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!("zshell-store-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    (load(&dir), dir)
}

fn new_profile(name: &str, folder: Option<&str>) -> Profile {
    let ssh = SshOptions::new(Remote::new(format!("{name}.example.com"), 22, "alice".into()));
    Profile { folder: folder.map(Into::into), ..Profile::new(name.into(), Connection::Ssh(ssh)) }
}

/// `profile` with its address or route changed.
fn with_remote(mut profile: Profile, change: impl FnOnce(&mut Remote)) -> Profile {
    change(profile.connection.remote_mut().unwrap());
    profile
}

fn folder(name: &str, parent: Option<&str>) -> Folder {
    Folder { id: String::new(), name: name.into(), parent: parent.map(Into::into) }
}

#[test]
fn moves_and_deletes_folders_and_sessions() {
    let (store, dir) = store("tree");
    let work = store.save_folder(folder("Work", None)).unwrap();
    let db = store.save_folder(folder("DB", Some(&work.id))).unwrap();
    let a = store.save(new_profile("a", Some(&db.id)), None).unwrap();
    let b = store.save(new_profile("b", None), None).unwrap();
    assert_eq!(a.folder.as_deref(), Some(db.id.as_str()));

    // b moves into DB, before a.
    store.move_item(Item::Profile { id: b.id.clone() }, Some(db.id.clone()), Some(a.id.clone())).unwrap();
    let names: Vec<_> = store.list().iter().map(|p| (p.name.clone(), p.folder.clone())).collect();
    assert_eq!(names, [("b".to_owned(), Some(db.id.clone())), ("a".to_owned(), Some(db.id.clone()))]);

    // Editing a session keeps its folder.
    store.save(Profile { folder: None, ..store.get(&a.id).unwrap() }, None).unwrap();
    assert_eq!(store.get(&a.id).unwrap().folder.as_deref(), Some(db.id.as_str()));

    // A folder cannot go into itself or its subfolders.
    let error = store.move_item(Item::Folder { id: work.id.clone() }, Some(db.id.clone()), None).unwrap_err();
    assert_eq!(error.code(), "folder.invalidMove");

    // Deleting Work moves DB to the top level; deleting DB moves its sessions there too.
    store.delete_folder(&work.id).unwrap();
    assert_eq!(store.folders(), [Folder { parent: None, ..db.clone() }]);
    store.delete_folder(&db.id).unwrap();
    assert!(store.list().iter().all(|p| p.folder.is_none()));

    // Subfolders take the deleted folder's place, wherever they were listed.
    let a = store.save_folder(folder("A", None)).unwrap();
    let x = store.save_folder(folder("X", None)).unwrap();
    store.save_folder(folder("B", None)).unwrap();
    let c = store.save_folder(folder("C", Some(&x.id))).unwrap();
    store.move_item(Item::Folder { id: c.id.clone() }, Some(x.id.clone()), Some(a.id.clone())).unwrap();
    let names = || store.folders().iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(), ["C", "A", "X", "B"]);
    store.delete_folder(&x.id).unwrap();
    assert_eq!(names(), ["A", "C", "B"]);
    for folder in store.folders() {
        store.delete_folder(&folder.id).unwrap();
    }

    // Everything is on disk.
    let reloaded = load(&dir);
    assert_eq!(reloaded.list().len(), 2);
    assert!(reloaded.folders().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

/// A change that can't be written changes nothing, on disk or in the store.
#[test]
fn a_failed_write_changes_nothing() {
    let (store, dir) = store("all-or-none");
    let work = store.save_folder(folder("Work", None)).unwrap();
    store.save(new_profile("a", Some(&work.id)), None).unwrap();
    let saved = fs::read(dir.join(crate::vault::VAULT_FILE)).unwrap();

    fs::create_dir(dir.join("vault.json.tmp")).unwrap();
    assert_eq!(store.delete_folder(&work.id).unwrap_err().code(), "config.writeFailed");
    assert_eq!(fs::read(dir.join(crate::vault::VAULT_FILE)).unwrap(), saved);
    assert_eq!(store.folders().len(), 1);
    assert_eq!(store.list()[0].folder.as_deref(), Some(work.id.as_str()));

    fs::remove_dir(dir.join("vault.json.tmp")).unwrap();
    store.delete_folder(&work.id).unwrap();
    let reloaded = load(&dir);
    assert!(reloaded.folders().is_empty() && reloaded.list()[0].folder.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

/// Passwords are saved, copied and deleted with their sessions and proxies, in the same write.
#[test]
fn keeps_passwords_with_their_sessions_and_proxies() {
    let dir = std::env::temp_dir().join(format!("zshell-store-passwords-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let vault = vault(&dir);
    let store = ProfileStore::load(vault.clone());
    let password = |account: Account<'_>| vault.password(account).unwrap();

    let a = store.save(new_profile("a", None), Some("secret")).unwrap();
    assert_eq!(password(Account::Profile(&a.id)).as_deref(), Some("secret"));
    // Kept unless given; an empty one deletes it.
    store.save(Profile { name: "renamed".into(), ..a.clone() }, None).unwrap();
    assert_eq!(password(Account::Profile(&a.id)).as_deref(), Some("secret"));
    let copy = store.duplicate(&a.id, "copy").unwrap();
    assert_eq!(password(Account::Profile(&copy.id)).as_deref(), Some("secret"));
    store.save(a.clone(), Some("")).unwrap();
    assert_eq!(password(Account::Profile(&a.id)), None);
    store.delete(&copy.id).unwrap();
    assert_eq!(password(Account::Profile(&copy.id)), None);
    // Saving it again (from an editor opened before it was deleted) doesn't bring it back.
    assert_eq!(store.save(copy.clone(), None).unwrap_err().code(), "profile.notFound");
    assert!(store.list().iter().all(|p| p.name != "copy"));

    let corp = Proxy {
        name: "corp".into(),
        kind: crate::proxy::ProxyKind::Socks5,
        host: "proxy.lan".into(),
        port: 1080,
        username: "u".into(),
        ..store_proxy()
    };
    let corp = store.save_proxy(corp, Some("proxy-secret")).unwrap();
    assert_eq!(password(Account::Proxy(&corp.id)).as_deref(), Some("proxy-secret"));
    // Without a user name, a proxy takes no password.
    let corp = store.save_proxy(Proxy { username: String::new(), ..corp }, None).unwrap();
    assert_eq!(password(Account::Proxy(&corp.id)), None);
    let corp = store.save_proxy(Proxy { username: "u".into(), ..corp }, Some("again")).unwrap();
    store.delete_proxy(&corp.id).unwrap();
    assert_eq!(password(Account::Proxy(&corp.id)), None);

    // An import brings its passwords in the same write.
    let mut passwords = crate::vault::Passwords::default();
    passwords.set(Account::Profile("r"), Some("imported"));
    let profiles = vec![Profile { id: "r".into(), ..new_profile("r", None) }];
    store.add_all(|_| Ok(Additions { profiles, passwords, ..Additions::default() })).unwrap();
    assert_eq!(password(Account::Profile("r")).as_deref(), Some("imported"));
    fs::remove_dir_all(&dir).unwrap();
}

/// Folder cycles are broken; a session's missing folder is kept (see `State::repair`).
#[test]
fn repairs_folder_cycles() {
    let (_, dir) = store("repair");
    let profile = Profile { id: "p".into(), ..new_profile("p", Some("missing")) };
    let loops = [
        Folder { id: "x".into(), name: "x".into(), parent: Some("y".into()) },
        Folder { id: "y".into(), name: "y".into(), parent: Some("x".into()) },
    ];
    write_vault(&dir, serde_json::json!({ "profiles": [profile], "folders": loops }));
    let store = load(&dir);
    assert_eq!(store.list()[0].folder.as_deref(), Some("missing"));
    assert!(store.folders().iter().any(|f| f.parent.is_none()));
    fs::remove_dir_all(&dir).unwrap();
}

/// Names are shown in terminals ("Connecting to …"): one from an import file can't carry
/// control sequences there.
#[test]
fn names_are_printable() {
    let (store, dir) = store("names");
    let web = store.save(Profile { name: " web\x1b[2J\tprod\n".into(), ..new_profile("web", None) }, None).unwrap();
    assert_eq!(web.name, "web\u{FFFD}[2J prod");
    let proxy = store.save_proxy(Proxy { name: "corp\x07".into(), host: "proxy.lan".into(), port: 8080, ..store_proxy() }, None).unwrap();
    assert_eq!(proxy.name, "corp\u{FFFD}");
    fs::remove_dir_all(&dir).unwrap();
}

/// A session keeps the settings of its protocol under `connection`.
#[test]
fn keeps_protocol_settings_under_connection() {
    let written = serde_json::to_value(new_profile("web", None)).unwrap();
    assert_eq!(written["connection"]["protocol"], "ssh");
    assert_eq!(written["connection"]["host"], "web.example.com");
    assert_eq!(written["connection"]["auth"]["type"], "auto");
    assert!(written.get("host").is_none() && written.get("protocol").is_none());
}

#[test]
fn checks_each_protocol_s_fields() {
    let (store, dir) = store("protocols");
    let telnet = |host: &str| Profile::new(String::new(), Connection::Telnet(Remote::new(host.into(), 23, String::new())));
    let switch = store.save(telnet("switch.lan"), None).unwrap();
    assert_eq!(switch.name, "switch.lan");
    assert_eq!(store.save(telnet(""), None).unwrap_err().code(), "profile.missingHost");
    // An IPv6 address loses its brackets; other bracketed hosts are still refused.
    for (host, saved) in [(" [2001:db8::1] ", "2001:db8::1"), ("[fe80::1%en0]", "fe80::1%en0"), ("::1", "::1")] {
        let Connection::Telnet(remote) = store.save(telnet(host), None).unwrap().connection else { unreachable!() };
        assert_eq!(remote.host, saved);
    }
    for host in ["[switch.lan]", "[::1]:23", "[::1"] {
        assert_eq!(store.save(telnet(host), None).unwrap_err().code(), "profile.invalidHost", "{host}");
    }

    let serial =
        |device: &str| Profile::new(String::new(), Connection::Serial(SerialOptions { device: device.into(), ..SerialOptions::default() }));
    let console = store.save(serial(" /dev/cu.usbserial-1410 "), None).unwrap();
    let Connection::Serial(line) = &console.connection else { panic!("{console:?}") };
    assert_eq!((console.name.as_str(), line.device.as_str()), ("cu.usbserial-1410", "/dev/cu.usbserial-1410"));
    assert_eq!(line.summary(), "115200 8N1");
    assert_eq!(store.save(serial(""), None).unwrap_err().code(), "profile.missingDevice");
    let odd = Profile { connection: Connection::Serial(SerialOptions { data_bits: 9, ..line.clone() }), ..console.clone() };
    assert_eq!(store.save(odd, None).unwrap_err().code(), "profile.invalidSerialSettings");

    // Only SSH sessions can be jump hosts, and a jump host stays SSH.
    let bastion = store.save(new_profile("bastion", None), None).unwrap();
    let via_switch = with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![switch.id.clone()]);
    assert_eq!(store.save(via_switch, None).unwrap_err().code(), "profile.invalidJumpHost");
    let router = store.save(with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![bastion.id.clone()]), None).unwrap();
    let bastion_telnet = Profile { connection: Connection::Telnet(bastion.remote().unwrap().clone()), ..bastion.clone() };
    assert_eq!(store.save(bastion_telnet, None).unwrap_err().code(), "profile.jumpHostNotSsh");
    assert!(router.same_target(&Profile { name: "other".into(), ..router.clone() }));
    let router_ssh = Profile { connection: Connection::Ssh(SshOptions::new(router.remote().unwrap().clone())), ..router.clone() };
    assert!(!router.same_target(&router_ssh));
    fs::remove_dir_all(&dir).unwrap();
}

/// Forwarding rules are edited on their own, kept when the session is saved, and go when
/// it stops being an SSH session.
#[test]
fn keeps_forwarding_rules_of_ssh_sessions() {
    let (store, dir) = store("forwards");
    let web = store.save(new_profile("web", None), None).unwrap();
    let rule = ForwardRule {
        id: String::new(),
        kind: crate::forward::ForwardKind::Local,
        bind_host: String::new(),
        bind_port: 8080,
        target_host: "localhost".into(),
        target_port: 80,
        description: String::new(),
        auto_start: false,
    };
    let web = store.set_forwards(&web.id, vec![rule]).unwrap();
    assert!(!web.forwards()[0].id.is_empty());
    // The dialog sends no rules.
    let renamed = store.save(Profile { id: web.id.clone(), name: "renamed".into(), ..new_profile("web", None) }, None).unwrap();
    assert_eq!(renamed.forwards().len(), 1);

    let telnet = Profile { connection: Connection::Telnet(web.remote().unwrap().clone()), ..web.clone() };
    assert!(store.save(telnet, None).unwrap().forwards().is_empty());
    assert_eq!(store.set_forwards(&web.id, Vec::new()).unwrap_err().code(), "profile.forwardsNeedSsh");
    let ssh_again = store.save(Profile { connection: web.connection.clone(), ..web.clone() }, None).unwrap();
    assert!(ssh_again.forwards().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keeps_proxies_and_routes_through_them() {
    let (store, dir) = store("proxies");
    let socks = Proxy {
        id: String::new(),
        name: "corp".into(),
        kind: crate::proxy::ProxyKind::Socks5,
        host: "proxy.lan".into(),
        port: 1080,
        username: String::new(),
        command: String::new(),
    };
    let socks = store.save_proxy(socks, None).unwrap();
    let missing = with_remote(new_profile("x", None), |remote| remote.proxy = Some("missing".into()));
    assert_eq!(store.save(missing, None).unwrap_err().code(), "profile.invalidProxy");

    let bastion = store.save(with_remote(new_profile("bastion", None), |remote| remote.proxy = Some(socks.id.clone())), None).unwrap();
    // With jump hosts, the session's own proxy is dropped; the first jump host's is used.
    let db = with_remote(new_profile("db", None), |remote| {
        remote.jump_hosts = vec![bastion.id.clone()];
        remote.proxy = Some(socks.id.clone());
    });
    let db = store.save(db, None).unwrap();
    assert_eq!(db.proxy(), None);
    let route = store.route(db.remote().unwrap()).unwrap();
    assert_eq!((route.jumps.len(), route.proxy.as_ref().map(|p| p.id.as_str())), (1, Some(socks.id.as_str())));
    assert_eq!(store.route(bastion.remote().unwrap()).unwrap().proxy, Some(socks.clone()));
    assert!(store.route(new_profile("direct", None).remote().unwrap()).unwrap().proxy.is_none());

    // A proxy in use cannot be deleted.
    let error = store.delete_proxy(&socks.id).unwrap_err();
    assert_eq!((error.code(), error.to_string().contains("bastion")), ("proxy.inUse", true));
    store.save(with_remote(bastion, |remote| remote.proxy = None), None).unwrap();
    store.delete_proxy(&socks.id).unwrap();
    assert!(store.proxies().is_empty());
    assert_eq!(store.save_proxy(socks, None).unwrap_err().code(), "proxy.notFound");

    // Dangling references (hand-edited files) are kept, and fail to connect.
    let kept = store
        .save_proxy(Proxy { id: String::new(), kind: crate::proxy::ProxyKind::Command, command: "nc %h %p".into(), ..store_proxy() }, None)
        .unwrap();
    assert_eq!(kept.name, "nc");
    let dangling = with_remote(Profile { id: "p".into(), ..new_profile("p", None) }, |remote| remote.proxy = Some("gone".into()));
    write_vault(&dir, serde_json::json!({ "profiles": [dangling], "proxies": [kept] }));
    let reloaded = load(&dir);
    assert_eq!(reloaded.list()[0].proxy(), Some("gone"));
    assert_eq!(reloaded.route(reloaded.list()[0].remote().unwrap()).err().map(|e| e.code()), Some("profile.invalidProxy"));
    assert_eq!(reloaded.proxies(), [kept]);
    fs::remove_dir_all(&dir).unwrap();
}

/// What a hand-written file may leave out or repeat: ids (references go to the first) and
/// the settings that have defaults.
#[test]
fn reads_untidy_files() {
    let (_, dir) = store("untidy");
    let connection = r#"{"protocol": "telnet", "host": "router.lan", "port": 23, "username": ""}"#;
    let profiles = format!(
        r#"[{{"id": "a", "name": "one", "connection": {connection}}}, {{"id": "a", "name": "two", "connection": {connection}}}, {{"name": "three", "connection": {connection}}}]"#
    );
    let profiles: serde_json::Value = serde_json::from_str(&profiles).unwrap();
    write_vault(&dir, serde_json::json!({ "profiles": profiles }));
    let store = load(&dir);
    let list = store.list();
    let ids: std::collections::HashSet<&str> = list.iter().map(|p| p.id.as_str()).collect();
    assert_eq!((list.len(), ids.len(), list[0].id.as_str()), (3, 3, "a"));
    assert_eq!((list[1].encoding.as_str(), list[1].auto_reconnect), (crate::encoding::DEFAULT, false));
    fs::remove_dir_all(&dir).unwrap();
}

fn store_proxy() -> Proxy {
    Proxy {
        id: String::new(),
        name: String::new(),
        kind: crate::proxy::ProxyKind::Http,
        host: String::new(),
        port: 0,
        username: String::new(),
        command: String::new(),
    }
}
