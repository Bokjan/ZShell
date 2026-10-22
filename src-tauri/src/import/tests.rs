use super::*;

fn write_config(text: &str) -> (tempdir::Dir, PathBuf) {
    let dir = tempdir::Dir::new();
    let path = dir.0.join("config");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

mod tempdir {
    /// A uniquely named directory under the system temp dir, removed on drop.
    pub struct Dir(pub std::path::PathBuf);

    impl Dir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!("zshell-import-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

// `Host *` goes last: as in OpenSSH, the first value found for an option wins.
const CONFIG: &str = "
Host bastion
    HostName bastion.example.com
    Port 2200

Host db web
    HostName %h.internal
    ProxyJump bastion
    LocalForward 15432 localhost:5432

Host legacy
    HostName 10.0.0.9
    User root
    IdentityFile /keys/legacy
    ProxyCommand nc -X 5 %h %p
    ForwardAgent yes
    SetEnv LANG=zh_CN.GBK GREETING=\"hello world\"
    SendEnv LC_*
    DynamicForward 127.0.0.1:1080
    RemoteForward 9000 localhost:3000

Host old
    HostName old.internal
    ProxyCommand ssh -q -W %h:%p bastion

Host both
    ProxyJump bastion
    ProxyCommand nc %h %p

Host tunneled
    ProxyCommand nc -X 5 %h %p

Host *.corp !skip
    User bob

Host *
    User alice
    ServerAliveInterval 15
";

/// `Match` blocks, also in included files, are left out instead of being applied to the
/// `Host` block before them.
#[test]
fn leaves_out_match_blocks() {
    let (dir, path) = write_config("");
    let included = dir.0.join("included");
    std::fs::write(&included, "Host db\n    User carol\nMatch exec \"true\"\n    User mallory\n").unwrap();
    let config = format!(
        "Host web\n    User alice\nMatch host *.corp\n    User bob\n    ProxyJump gate\nInclude {}\nHost *\n    Port 2222\n",
        dir.0.join("incl*").display()
    );
    std::fs::write(&path, config).unwrap();
    let candidates = scan(&path, &[]).unwrap();
    let aliases: Vec<_> = candidates.iter().map(|c| c.alias.as_str()).collect();
    assert_eq!(aliases, ["web", "db"]);
    let (web, db) = (&candidates[0], &candidates[1]);
    assert_eq!((web.username.as_str(), web.port), ("alice", 2222));
    assert!(web.jump_hosts.is_empty());
    assert_eq!((db.username.as_str(), db.port), ("carol", 2222));
    assert!(candidates.iter().all(|c| c.skipped.contains(&"Match".to_owned())));
}

/// A file that includes itself is an error, as in OpenSSH, rather than a crash.
#[test]
fn refuses_includes_nested_too_deeply() {
    let (_dir, path) = write_config("");
    std::fs::write(&path, format!("Host web\n    User alice\nInclude {}\n", path.display())).unwrap();
    assert_eq!(scan(&path, &[]).unwrap_err().code(), "import.includeTooDeep");
}

/// Options after an `Include` inside a `Host` block belong to that block, even when the
/// included file has blocks of its own.
#[test]
fn options_after_an_include_stay_in_their_block() {
    let (dir, path) = write_config("");
    let included = dir.0.join("db.conf");
    std::fs::write(&included, "Host db\n    User bob\n").unwrap();
    std::fs::write(&path, format!("Host web\n    Include {}\n    User alice\n", included.display())).unwrap();
    let candidates = scan(&path, &[]).unwrap();
    let users: Vec<_> = candidates.iter().map(|c| (c.alias.as_str(), c.username.as_str())).collect();
    assert!(users.contains(&("web", "alice")) && users.contains(&("db", "bob")), "{users:?}");
}

/// `Include config.d/*` leaves out hidden files and folders, which can't be read as
/// configs (`.DS_Store`).
#[test]
fn includes_neither_hidden_files_nor_folders() {
    let (dir, path) = write_config("");
    let included = dir.0.join("config.d");
    std::fs::create_dir_all(included.join("old")).unwrap();
    std::fs::write(included.join("web.conf"), "Host web\n    User alice\n").unwrap();
    std::fs::write(included.join(".DS_Store"), [0u8, 0xff, 0xfe, 0x80]).unwrap();
    std::fs::write(&path, format!("Include {}\n", included.join("*").display())).unwrap();
    let candidates = scan(&path, &[]).unwrap();
    assert_eq!(candidates.iter().map(|c| c.alias.as_str()).collect::<Vec<_>>(), ["web"]);
}

#[test]
fn stores_keys_under_the_home_folder_with_a_tilde() {
    let home = Path::new("/home/bob");
    assert_eq!(home_relative(Path::new("/home/bob/.ssh/corp"), Some(home)), "~/.ssh/corp");
    assert_eq!(home_relative(Path::new("/home/bobby/.ssh/corp"), Some(home)), "/home/bobby/.ssh/corp");
    assert_eq!(home_relative(Path::new("/keys/corp"), Some(home)), "/keys/corp");
    assert_eq!(home_relative(Path::new("/keys/corp"), None), "/keys/corp");

    let (_dir, path) = write_config("Host corp\n    IdentityFile ~/.ssh/corp\n");
    let candidates = scan(&path, &[]).unwrap();
    assert!(matches!(&candidates[0].auth, AuthMethod::PublicKey { key_path } if key_path == "~/.ssh/corp"), "{:?}", candidates[0].auth);
}

#[test]
fn scans_concrete_aliases_with_inherited_settings() {
    let (_dir, path) = write_config(CONFIG);
    let candidates = scan(&path, &[]).unwrap();
    let aliases: Vec<_> = candidates.iter().map(|c| c.alias.as_str()).collect();
    assert_eq!(aliases, ["bastion", "db", "web", "legacy", "old", "both", "tunneled"]);

    let db = &candidates[1];
    assert_eq!((db.host.as_str(), db.port, db.username.as_str()), ("db.internal", 22, "alice"));
    assert_eq!(db.keepalive_interval, 15);
    assert!(matches!(db.auth, AuthMethod::Auto));
    assert_eq!(db.jump_hosts, ["bastion"]);
    assert_eq!(db.forwards.len(), 1);
    let forward = &db.forwards[0];
    assert_eq!(forward.kind, ForwardKind::Local);
    assert_eq!((forward.bind_port, forward.target_host.as_str(), forward.target_port), (15432, "localhost", 5432));

    let legacy = &candidates[3];
    assert_eq!(legacy.username, "root");
    assert!(matches!(&legacy.auth, AuthMethod::PublicKey { key_path } if key_path == "/keys/legacy"));
    assert_eq!(legacy.skipped, ["SendEnv"]);
    assert_eq!(legacy.proxy_command.as_deref(), Some("nc -X 5 %h %p"));
    // The older way to write ProxyJump becomes a jump host; ProxyJump wins over ProxyCommand.
    assert_eq!((candidates[4].jump_hosts.as_slice(), candidates[4].proxy_command.as_deref()), (["bastion".to_owned()].as_slice(), None));
    assert_eq!(candidates[5].jump_hosts, ["bastion"]);
    assert_eq!((candidates[5].proxy_command.as_deref(), candidates[5].skipped.as_slice()), (None, ["ProxyCommand".to_owned()].as_slice()));
    assert!(legacy.forward_agent && !db.forward_agent);
    let env: Vec<_> = legacy.env.iter().map(|v| (v.name.as_str(), v.value.as_str())).collect();
    assert_eq!(env, [("LANG", "zh_CN.GBK"), ("GREETING", "hello world")]);
    let kinds: Vec<_> = legacy.forwards.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [ForwardKind::Remote, ForwardKind::Dynamic]);
}

#[test]
fn plan_imports_jump_hosts_and_reuses_existing_profiles() {
    let (_dir, path) = write_config(CONFIG);
    let plan_ids = |profiles: &[Profile]| profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>();

    // Selecting "db" pulls in its jump host.
    let (_, profiles) = plan(&path, &["db".to_owned()], &[], &[]).unwrap();
    assert_eq!(plan_ids(&profiles), ["db", "bastion"]);
    assert_eq!(profiles[0].jump_hosts(), [profiles[1].id.clone()]);
    assert!(profiles.iter().all(|p| p.forwards().iter().all(|f| !f.id.is_empty())));
    assert_eq!(profiles[0].forwards()[0].bind_host, "127.0.0.1");

    // An existing profile for the jump host is referenced instead of duplicated.
    let ssh = SshOptions { auth: AuthMethod::Agent, ..SshOptions::new(Remote::new("bastion.example.com".into(), 2200, "alice".into())) };
    let bastion = new_profile("existing".into(), "bastion".into(), ssh);
    let (_, profiles) = plan(&path, &["db".to_owned(), "web".to_owned()], std::slice::from_ref(&bastion), &[]).unwrap();
    assert_eq!(plan_ids(&profiles), ["db", "web"]);
    assert!(profiles.iter().all(|p| p.jump_hosts() == ["existing"]));

    // A Telnet session of the same name, listed first, is not the one used.
    let mut telnet = bastion.clone();
    telnet.id = "telnet".into();
    telnet.connection = Connection::Telnet(Remote::new("bastion.example.com".into(), 23, "alice".into()));
    let (_, profiles) = plan(&path, &["db".to_owned()], &[telnet, bastion], &[]).unwrap();
    assert_eq!(plan_ids(&profiles), ["db"]);
    assert_eq!(profiles[0].jump_hosts(), ["existing"]);
}

/// A jump host written as an alias with another user or port keeps the alias's address
/// and key, and a jump host's own `ProxyJump` comes before it in the list.
#[test]
fn plan_follows_jump_host_aliases() {
    let (_dir, path) = write_config(
        "Host outer\n    HostName outer.example.com\n    User gw\n\
         Host bastion\n    HostName bastion.example.com\n    User ops\n    IdentityFile /keys/bastion\n    ProxyJump outer\n\
         Host app\n    ProxyJump bastion\n\
         Host db\n    ProxyJump admin@bastion:2222\n",
    );
    let (_, profiles) = plan(&path, &["app".to_owned(), "db".to_owned()], &[], &[]).unwrap();
    let by_name = |name: &str| profiles.iter().find(|p| p.name == name).unwrap();
    let (outer, bastion) = (by_name("outer"), by_name("bastion"));
    assert_eq!(by_name("app").jump_hosts(), [outer.id.clone(), bastion.id.clone()]);
    assert_eq!(bastion.jump_hosts(), std::slice::from_ref(&outer.id));

    let db = by_name("db");
    assert_eq!(db.jump_hosts()[0], outer.id);
    let admin = profiles.iter().find(|p| p.id == db.jump_hosts()[1]).unwrap().ssh().unwrap();
    assert_eq!((admin.remote.host.as_str(), admin.remote.port, admin.remote.username.as_str()), ("bastion.example.com", 2222, "admin"));
    assert!(matches!(&admin.auth, AuthMethod::PublicKey { key_path } if key_path == "/keys/bastion"));
}

#[test]
fn plan_shares_one_proxy_per_command() {
    let (_dir, path) = write_config(CONFIG);
    let (proxies, profiles) = plan(&path, &["legacy".to_owned(), "tunneled".to_owned()], &[], &[]).unwrap();
    assert_eq!(proxies.len(), 1);
    assert_eq!((proxies[0].kind, proxies[0].command.as_str()), (ProxyKind::Command, "nc -X 5 %h %p"));
    assert!(profiles.iter().all(|p| p.proxy() == Some(proxies[0].id.as_str())));

    // An existing proxy with the same command is used.
    let existing = Proxy { id: "nc".into(), name: "netcat".into(), ..proxies[0].clone() };
    let (proxies, profiles) = plan(&path, &["tunneled".to_owned()], &[], &[existing]).unwrap();
    assert!(proxies.is_empty());
    assert_eq!(profiles[0].proxy(), Some("nc"));
}

#[test]
fn parses_addresses() {
    assert_eq!(parse_listen("8080"), Some((String::new(), 8080)));
    assert_eq!(parse_listen("*:8080"), Some(("*".into(), 8080)));
    assert_eq!(parse_listen(":8080"), Some(("*".into(), 8080)));
    assert_eq!(parse_host_port("[::1]:22"), Some(("::1".into(), 22)));
    assert_eq!(parse_host_port("db/5432"), Some(("db".into(), 5432)));
    assert_eq!(split_destination("ops@jump:2222"), (Some("ops".into()), "jump".into(), Some(2222)));
    assert_eq!(split_destination("jump"), (None, "jump".into(), None));
    assert_eq!(parse_host_port("2001:db8::1"), None);
    assert_eq!(parse_host_port("2001:db8::1/22"), Some(("2001:db8::1".into(), 22)));
    assert_eq!(split_destination("ops@2001:db8::1"), (Some("ops".into()), "2001:db8::1".into(), None));
    assert_eq!(split_destination("[::1]"), (None, "::1".into(), None));
    assert_eq!(expand_host_tokens("%h.corp%%", "db"), "db.corp%");
    assert_eq!(stdio_forward_host("ssh -W %h:%p jump").as_deref(), Some("jump"));
    assert_eq!(stdio_forward_host("/usr/bin/ssh -l ops -p 2222 jump -W [%h]:%p").as_deref(), Some("ops@jump:2222"));
    assert_eq!(stdio_forward_host("ssh -W %h:%p ops@::1 -p 2").as_deref(), Some("ops@[::1]:2"));
    assert_eq!(stdio_forward_host("ssh -W %h:%p ops@::1").as_deref(), Some("ops@[::1]"));
    assert_eq!(stdio_forward_host("ssh -i key -W %h:%p jump"), None);
    assert_eq!(stdio_forward_host("ssh jump nc %h %p"), None);
    assert_eq!(stdio_forward_host("ssh -W db:22 jump"), None);
}

#[test]
fn checks_sessions_as_saving_does() {
    let (_dir, path) = write_config("Host v6\n  HostName [::1]\n  User me\n\nHost bad\n  HostName a?b\n  User me\n");
    let (_, profiles) = plan(&path, &["v6".to_owned()], &[], &[]).unwrap();
    assert_eq!(profiles[0].ssh().unwrap().remote.host, "::1");
    let error = plan(&path, &["bad".to_owned()], &[], &[]).unwrap_err();
    assert_eq!(error.code(), "import.invalidSession");
}
