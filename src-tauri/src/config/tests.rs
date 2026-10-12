use std::fs;

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::forward::ForwardRule;
use crate::persist::SetAside;
use crate::proxy::Proxy;
use crate::vault::Vault;

/// A store over plain files next to `path` (`profiles.json`).
fn load_at(path: PathBuf, set_aside: &SetAside) -> ProfileStore {
    let store = Arc::new(crate::secrets::MemoryStore::default());
    ProfileStore::load(Arc::new(Vault::open(path.parent().unwrap().to_owned(), store, set_aside)))
}

fn store(name: &str) -> (ProfileStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!("zshell-store-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    (load_at(dir.join("profiles.json"), &SetAside::default()), dir)
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

/// An unreadable file is moved aside instead of stopping the app or being overwritten.
#[test]
fn sets_aside_unreadable_files() {
    let (_, dir) = store("set-aside");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("profiles.json"), b"").unwrap();
    fs::write(dir.join("proxies.json"), br#"[{"id": "x", "type": "fromTheFuture"}]"#).unwrap();
    let set_aside = SetAside::default();
    let store = load_at(dir.join("profiles.json"), &set_aside);
    assert!(store.list().is_empty());
    let files = set_aside.take();
    assert_eq!(files.len(), 2, "{files:?}");
    for file in &files {
        assert!(!file.path.exists() && file.moved_to.as_ref().is_some_and(|p| p.exists()), "{file:?}");
    }
    assert!(set_aside.take().is_empty());
    store.save(new_profile("a", None)).unwrap();
    assert_eq!(fs::read(files[0].moved_to.as_ref().unwrap()).unwrap(), b"");
    fs::remove_dir_all(&dir).unwrap();
}

/// Some files of a change in place and others not: the store shows what is on disk.
#[test]
fn follows_the_disk_when_a_change_is_saved_part_way() {
    let (store, dir) = store("part-way");
    store.save(new_profile("a", None)).unwrap();
    // Folders are put in place before sessions, which can't be.
    fs::remove_file(dir.join("profiles.json")).unwrap();
    fs::create_dir_all(dir.join("profiles.json").join("x")).unwrap();
    let work = Folder { id: "w".into(), name: "Work".into(), parent: None };
    let added = store.add_all(|_| Ok(Additions { folders: vec![work], profiles: vec![new_profile("b", None)], ..Default::default() }));
    assert!(added.is_err());
    assert_eq!(store.folders().len(), 1);
    assert_eq!(store.list().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn moves_and_deletes_folders_and_sessions() {
    let (store, dir) = store("tree");
    let work = store.save_folder(folder("Work", None)).unwrap();
    let db = store.save_folder(folder("DB", Some(&work.id))).unwrap();
    let a = store.save(new_profile("a", Some(&db.id))).unwrap();
    let b = store.save(new_profile("b", None)).unwrap();
    assert_eq!(a.folder.as_deref(), Some(db.id.as_str()));

    // b moves into DB, before a.
    store.move_item(Item::Profile { id: b.id.clone() }, Some(db.id.clone()), Some(a.id.clone())).unwrap();
    let names: Vec<_> = store.list().iter().map(|p| (p.name.clone(), p.folder.clone())).collect();
    assert_eq!(names, [("b".to_owned(), Some(db.id.clone())), ("a".to_owned(), Some(db.id.clone()))]);

    // Editing a session keeps its folder.
    store.save(Profile { folder: None, ..store.get(&a.id).unwrap() }).unwrap();
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
    let reloaded = load_at(dir.join("profiles.json"), &SetAside::default());
    assert_eq!(reloaded.list().len(), 2);
    assert!(reloaded.folders().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

/// A change that has to write two files writes neither if the second can't be written.
#[test]
fn a_failed_write_leaves_every_file_as_it_was() {
    let (store, dir) = store("all-or-none");
    let work = store.save_folder(folder("Work", None)).unwrap();
    store.save(new_profile("a", Some(&work.id))).unwrap();
    let folders = fs::read(dir.join("folders.json")).unwrap();
    let profiles = fs::read(dir.join("profiles.json")).unwrap();

    // Deleting the folder rewrites folders.json and then profiles.json, which can't be.
    fs::create_dir(dir.join("profiles.json.tmp")).unwrap();
    assert_eq!(store.delete_folder(&work.id).unwrap_err().code(), "config.writeFailed");
    assert_eq!(fs::read(dir.join("folders.json")).unwrap(), folders);
    assert_eq!(fs::read(dir.join("profiles.json")).unwrap(), profiles);
    assert!(!dir.join("folders.json.tmp").exists());
    assert_eq!(store.folders().len(), 1);

    fs::remove_dir(dir.join("profiles.json.tmp")).unwrap();
    store.delete_folder(&work.id).unwrap();
    let reloaded = load_at(dir.join("profiles.json"), &SetAside::default());
    assert!(reloaded.folders().is_empty() && reloaded.list()[0].folder.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

/// Folder cycles are broken; a session's missing folder is kept (see `State::repair`).
#[test]
fn repairs_folder_cycles() {
    let (_, dir) = store("repair");
    fs::create_dir_all(&dir).unwrap();
    let profile = Profile { id: "p".into(), ..new_profile("p", Some("missing")) };
    fs::write(dir.join("profiles.json"), serde_json::to_vec(&[profile]).unwrap()).unwrap();
    let loops = [
        Folder { id: "x".into(), name: "x".into(), parent: Some("y".into()) },
        Folder { id: "y".into(), name: "y".into(), parent: Some("x".into()) },
    ];
    fs::write(dir.join("folders.json"), serde_json::to_vec(&loops).unwrap()).unwrap();
    let store = load_at(dir.join("profiles.json"), &SetAside::default());
    assert_eq!(store.list()[0].folder.as_deref(), Some("missing"));
    assert!(store.folders().iter().any(|f| f.parent.is_none()));
    fs::remove_dir_all(&dir).unwrap();
}

/// A session keeps the settings of its protocol under `connection`. Files from before
/// 2.0, with every setting at the top level, are set aside rather than read.
#[test]
fn keeps_protocol_settings_under_connection() {
    let written = serde_json::to_value(new_profile("web", None)).unwrap();
    assert_eq!(written["connection"]["protocol"], "ssh");
    assert_eq!(written["connection"]["host"], "web.example.com");
    assert_eq!(written["connection"]["auth"]["type"], "auto");
    assert!(written.get("host").is_none() && written.get("protocol").is_none());

    let (_, dir) = store("before-2.0");
    fs::create_dir_all(&dir).unwrap();
    let old = r#"[{"id":"p","name":"web","host":"web.example.com","port":22,"username":"alice","auth":{"type":"auto"}}]"#;
    fs::write(dir.join("profiles.json"), old).unwrap();
    let set_aside = SetAside::default();
    assert!(load_at(dir.join("profiles.json"), &set_aside).list().is_empty());
    assert_eq!(set_aside.take().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn checks_each_protocol_s_fields() {
    let (store, dir) = store("protocols");
    let telnet = |host: &str| Profile::new(String::new(), Connection::Telnet(Remote::new(host.into(), 23, String::new())));
    let switch = store.save(telnet("switch.lan")).unwrap();
    assert_eq!(switch.name, "switch.lan");
    assert_eq!(store.save(telnet("")).unwrap_err().code(), "profile.missingHost");
    // An IPv6 address loses its brackets; other bracketed hosts are still refused.
    for (host, saved) in [(" [2001:db8::1] ", "2001:db8::1"), ("[fe80::1%en0]", "fe80::1%en0"), ("::1", "::1")] {
        let Connection::Telnet(remote) = store.save(telnet(host)).unwrap().connection else { unreachable!() };
        assert_eq!(remote.host, saved);
    }
    for host in ["[switch.lan]", "[::1]:23", "[::1"] {
        assert_eq!(store.save(telnet(host)).unwrap_err().code(), "profile.invalidHost", "{host}");
    }

    let serial =
        |device: &str| Profile::new(String::new(), Connection::Serial(SerialOptions { device: device.into(), ..SerialOptions::default() }));
    let console = store.save(serial(" /dev/cu.usbserial-1410 ")).unwrap();
    let Connection::Serial(line) = &console.connection else { panic!("{console:?}") };
    assert_eq!((console.name.as_str(), line.device.as_str()), ("cu.usbserial-1410", "/dev/cu.usbserial-1410"));
    assert_eq!(line.summary(), "115200 8N1");
    assert_eq!(store.save(serial("")).unwrap_err().code(), "profile.missingDevice");
    let odd = Profile { connection: Connection::Serial(SerialOptions { data_bits: 9, ..line.clone() }), ..console.clone() };
    assert_eq!(store.save(odd).unwrap_err().code(), "profile.invalidSerialSettings");

    // Only SSH sessions can be jump hosts, and a jump host stays SSH.
    let bastion = store.save(new_profile("bastion", None)).unwrap();
    let via_switch = with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![switch.id.clone()]);
    assert_eq!(store.save(via_switch).unwrap_err().code(), "profile.invalidJumpHost");
    let router = store.save(with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![bastion.id.clone()])).unwrap();
    let bastion_telnet = Profile { connection: Connection::Telnet(bastion.remote().unwrap().clone()), ..bastion.clone() };
    assert_eq!(store.save(bastion_telnet).unwrap_err().code(), "profile.jumpHostNotSsh");
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
    let web = store.save(new_profile("web", None)).unwrap();
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
    let renamed = store.save(Profile { id: web.id.clone(), name: "renamed".into(), ..new_profile("web", None) }).unwrap();
    assert_eq!(renamed.forwards().len(), 1);

    let telnet = Profile { connection: Connection::Telnet(web.remote().unwrap().clone()), ..web.clone() };
    assert!(store.save(telnet).unwrap().forwards().is_empty());
    assert_eq!(store.set_forwards(&web.id, Vec::new()).unwrap_err().code(), "profile.forwardsNeedSsh");
    let ssh_again = store.save(Profile { connection: web.connection.clone(), ..web.clone() }).unwrap();
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
    let socks = store.save_proxy(socks).unwrap();
    let missing = with_remote(new_profile("x", None), |remote| remote.proxy = Some("missing".into()));
    assert_eq!(store.save(missing).unwrap_err().code(), "profile.invalidProxy");

    let bastion = store.save(with_remote(new_profile("bastion", None), |remote| remote.proxy = Some(socks.id.clone()))).unwrap();
    // With jump hosts, the session's own proxy is dropped; the first jump host's is used.
    let db = with_remote(new_profile("db", None), |remote| {
        remote.jump_hosts = vec![bastion.id.clone()];
        remote.proxy = Some(socks.id.clone());
    });
    let db = store.save(db).unwrap();
    assert_eq!(db.proxy(), None);
    let route = store.route(db.remote().unwrap()).unwrap();
    assert_eq!((route.jumps.len(), route.proxy.as_ref().map(|p| p.id.as_str())), (1, Some(socks.id.as_str())));
    assert_eq!(store.route(bastion.remote().unwrap()).unwrap().proxy, Some(socks.clone()));
    assert!(store.route(new_profile("direct", None).remote().unwrap()).unwrap().proxy.is_none());

    // A proxy in use cannot be deleted.
    let error = store.delete_proxy(&socks.id).unwrap_err();
    assert_eq!((error.code(), error.to_string().contains("bastion")), ("proxy.inUse", true));
    store.save(with_remote(bastion, |remote| remote.proxy = None)).unwrap();
    store.delete_proxy(&socks.id).unwrap();
    assert!(store.proxies().is_empty());
    assert_eq!(store.save_proxy(socks).unwrap_err().code(), "proxy.notFound");

    // Dangling references (hand-edited files) are kept, and fail to connect.
    let kept = store
        .save_proxy(Proxy { id: String::new(), kind: crate::proxy::ProxyKind::Command, command: "nc %h %p".into(), ..store_proxy() })
        .unwrap();
    assert_eq!(kept.name, "nc");
    let dangling = with_remote(Profile { id: "p".into(), ..new_profile("p", None) }, |remote| remote.proxy = Some("gone".into()));
    fs::write(dir.join("profiles.json"), serde_json::to_vec(&[dangling]).unwrap()).unwrap();
    let reloaded = load_at(dir.join("profiles.json"), &SetAside::default());
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
    fs::create_dir_all(&dir).unwrap();
    let connection = r#"{"protocol": "telnet", "host": "router.lan", "port": 23, "username": ""}"#;
    let profiles = format!(
        r#"[{{"id": "a", "name": "one", "connection": {connection}}}, {{"id": "a", "name": "two", "connection": {connection}}}, {{"name": "three", "connection": {connection}}}]"#
    );
    fs::write(dir.join("profiles.json"), profiles).unwrap();
    let set_aside = SetAside::default();
    let store = load_at(dir.join("profiles.json"), &set_aside);
    assert!(set_aside.take().is_empty());
    let list = store.list();
    let ids: std::collections::HashSet<&str> = list.iter().map(|p| p.id.as_str()).collect();
    assert_eq!((list.len(), ids.len(), list[0].id.as_str()), (3, 3, "a"));
    assert_eq!((list[1].encoding.as_str(), list[1].auto_reconnect), (crate::encoding::DEFAULT, false));
    fs::remove_dir_all(&dir).unwrap();
}

/// A folder or proxy file that is set aside doesn't take the sessions' references to its
/// folders and proxies with it: they work again once the file is restored, and a session
/// whose proxy is missing fails to connect rather than connecting directly.
#[test]
fn keeps_references_into_files_set_aside() {
    let (_, dir) = store("keep-references");
    fs::create_dir_all(&dir).unwrap();
    let profile = with_remote(Profile { id: "p".into(), ..new_profile("p", Some("work")) }, |remote| remote.proxy = Some("corp".into()));
    fs::write(dir.join("profiles.json"), serde_json::to_vec(&[profile]).unwrap()).unwrap();
    fs::write(dir.join("folders.json"), b"[{").unwrap();
    fs::write(dir.join("proxies.json"), br#"[{"id": "corp", "type": "fromTheFuture"}]"#).unwrap();
    let set_aside = SetAside::default();
    let store = load_at(dir.join("profiles.json"), &set_aside);
    assert_eq!(set_aside.take().len(), 2);
    let p = store.get("p").unwrap();
    assert_eq!((p.folder.as_deref(), p.proxy()), (Some("work"), Some("corp")));
    assert_eq!(store.route(p.remote().unwrap()).err().map(|e| e.code()), Some("profile.invalidProxy"));

    // Saving something else, and importing, keep them on disk too.
    store.save(new_profile("q", None)).unwrap();
    store
        .add_all(|_| Ok(Additions { profiles: vec![Profile { id: "r".into(), ..new_profile("r", None) }], ..Additions::default() }))
        .unwrap();
    let reloaded = load_at(dir.join("profiles.json"), &SetAside::default());
    let p = reloaded.get("p").unwrap();
    assert_eq!((p.folder.as_deref(), p.proxy()), (Some("work"), Some("corp")));
    assert_eq!(reloaded.list().len(), 3);
    fs::remove_dir_all(&dir).unwrap();
}

/// A file that exists but can't be read (open in another program) is left where it is
/// and never written over; the other files are saved as usual.
#[cfg(unix)]
#[test]
fn never_writes_over_a_file_it_could_not_read() {
    use std::os::unix::fs::PermissionsExt;

    let (_, dir) = store("unreadable");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("profiles.json");
    fs::write(&path, serde_json::to_vec(&[new_profile("a", None)]).unwrap()).unwrap();
    let saved = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let set_aside = SetAside::default();
    let store = load_at(path.clone(), &set_aside);
    let files = set_aside.take();
    assert_eq!((files.len(), files[0].moved_to.is_none()), (1, true), "{files:?}");
    assert!(store.list().is_empty());

    let error = store.save(new_profile("b", None)).unwrap_err();
    assert_eq!(error.code(), "config.unreadable");
    assert!(error.to_string().contains("profiles.json"), "{error}");
    store.save_folder(folder("Work", None)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), saved);
    assert_eq!(load_at(path, &SetAside::default()).folders().len(), 1);
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
