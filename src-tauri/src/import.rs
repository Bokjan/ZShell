//! Importing hosts from an OpenSSH client config (`~/.ssh/config`) as session profiles.
//!
//! Import is a one-time copy: each concrete `Host` alias (no wildcards) becomes a profile
//! with the settings `ssh` would use for it, including those inherited from `Host *`.
//! `ProxyJump` hosts are imported too, as profiles of their own, and each `ProxyCommand` as a
//! command proxy (one per distinct command).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Serialize;
use ssh2_config::{HostParams, ParseRule, RemoteForwardDestination, RemoteForwardListen, SshConfig};

use crate::config::{AuthMethod, EnvVar, Profile};
use crate::error::{Error, Result};
use crate::forward::{ForwardKind, ForwardRule};
use crate::proxy::{Proxy, ProxyKind};

/// Options that change how `ssh` connects but are not imported, reported per host.
const NOTABLE_UNSUPPORTED: &[&str] = &[
    "forwardx11",
    "remotecommand",
    "localcommand",
    "hostkeyalias",
    "identityagent",
    "pkcs11provider",
];

/// A host found in the config, as it would be imported.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub alias: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// `ProxyJump` entries as written: aliases or `[user@]host[:port]`. Also the host of a
    /// `ProxyCommand ssh -W %h:%p host`, the older way to write it.
    pub jump_hosts: Vec<String>,
    /// Any other `ProxyCommand`, as written.
    pub proxy_command: Option<String>,
    pub keepalive_interval: u32,
    pub forwards: Vec<ForwardRule>,
    pub forward_agent: bool,
    /// From `SetEnv`.
    pub env: Vec<EnvVar>,
    /// The name of an existing profile for the same alias or address; such hosts are not
    /// imported again.
    pub existing: Option<String>,
    /// Options in the config that are not imported.
    pub skipped: Vec<String>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".ssh").join("config"))
}

pub fn scan(path: &Path, existing: &[Profile]) -> Result<Vec<Candidate>> {
    let config = parse(path)?;
    Ok(aliases(&config).into_iter().map(|alias| candidate(&config, alias, existing)).collect())
}

/// The proxies and profiles to add for importing `selected`, with ids assigned and jump hosts
/// and proxies resolved. Jump hosts not yet in `existing`, and proxy commands not yet in
/// `proxies`, are added as well.
pub fn plan(path: &Path, selected: &[String], existing: &[Profile], proxies: &[Proxy]) -> Result<(Vec<Proxy>, Vec<Profile>)> {
    let config = parse(path)?;
    let known: HashSet<String> = aliases(&config).into_iter().collect();

    // Profile id for every alias involved, following ProxyJump chains.
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut pending = Vec::new();
    let mut queue: VecDeque<String> = selected.iter().cloned().collect();
    while let Some(alias) = queue.pop_front() {
        if ids.contains_key(&alias) || !known.contains(&alias) {
            continue;
        }
        let candidate = candidate(&config, alias.clone(), existing);
        if let Some(name) = &candidate.existing {
            let profile = existing.iter().find(|p| &p.name == name).expect("existing profile");
            ids.insert(alias, profile.id.clone());
            continue;
        }
        queue.extend(candidate.jump_hosts.iter().filter(|jump| known.contains(*jump)).cloned());
        ids.insert(alias, new_id());
        pending.push(candidate);
    }

    let mut profiles = Vec::new();
    let mut new_proxies: Vec<Proxy> = Vec::new();
    // Jump hosts written as addresses, by address, to share one profile between hosts.
    let mut by_address: HashMap<(String, u16, String), String> = HashMap::new();
    for candidate in pending {
        let mut jump_hosts = Vec::new();
        for jump in &candidate.jump_hosts {
            if let Some(id) = ids.get(jump) {
                jump_hosts.push(id.clone());
                continue;
            }
            let (username, host, port) = parse_destination(jump);
            let key = (host.clone(), port, username.clone());
            let found = existing.iter().find(|p| (&p.host, p.port, &p.username) == (&key.0, key.1, &key.2));
            let id = match (found, by_address.get(&key)) {
                (Some(profile), _) => profile.id.clone(),
                (None, Some(id)) => id.clone(),
                (None, None) => {
                    let id = new_id();
                    by_address.insert(key, id.clone());
                    profiles.push(new_profile(id.clone(), jump.clone(), host, port, username));
                    id
                }
            };
            jump_hosts.push(id);
        }
        let mut profile = new_profile(ids[&candidate.alias].clone(), candidate.alias, candidate.host, candidate.port, candidate.username);
        profile.auth = candidate.auth;
        profile.keepalive_interval = candidate.keepalive_interval;
        profile.forward_agent = candidate.forward_agent;
        profile.env = candidate.env;
        profile.jump_hosts = jump_hosts;
        if let Some(command) = candidate.proxy_command {
            let same = |p: &&Proxy| p.kind == ProxyKind::Command && p.command == command;
            profile.proxy = Some(match proxies.iter().chain(&new_proxies).find(same) {
                Some(proxy) => proxy.id.clone(),
                None => {
                    let proxy = Proxy {
                        id: new_id(),
                        name: command.clone(),
                        kind: ProxyKind::Command,
                        host: String::new(),
                        port: 0,
                        username: String::new(),
                        command,
                    };
                    new_proxies.push(proxy.clone());
                    proxy.id
                }
            });
        }
        // Normalizing fills in the default bind address, as saving from the panel does.
        profile.forwards = candidate
            .forwards
            .into_iter()
            .filter_map(|rule| ForwardRule { id: new_id(), ..rule }.normalize().ok())
            .collect();
        profiles.push(profile);
    }
    Ok((new_proxies, profiles))
}

fn parse(path: &Path) -> Result<SshConfig> {
    let file = File::open(path).map_err(|e| Error::new("import.readFailed").param("path", path.display()).detail(e))?;
    SshConfig::default()
        .parse(&mut BufReader::new(file), ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS)
        .map_err(|e| Error::new("import.parseFailed").param("path", path.display()).detail(e))
}

/// Concrete host aliases in file order: patterns without wildcards or negation.
fn aliases(config: &SshConfig) -> Vec<String> {
    let mut seen = HashSet::new();
    config
        .get_hosts()
        .iter()
        .flat_map(|host| &host.pattern)
        .filter(|clause| !clause.negated && !clause.pattern.contains(['*', '?']))
        .filter(|clause| seen.insert(clause.pattern.clone()))
        .map(|clause| clause.pattern.clone())
        .collect()
}

fn candidate(config: &SshConfig, alias: String, existing: &[Profile]) -> Candidate {
    let params = config.query(&alias);
    let host = params.host_name.as_deref().map(|name| expand_host_tokens(name, &alias)).unwrap_or_else(|| alias.clone());
    let port = params.port.unwrap_or(22);
    let username = params.user.clone().unwrap_or_else(local_username);
    let identity_files = params.identity_file.clone().unwrap_or_default();
    let auth = match identity_files.first() {
        Some(path) => AuthMethod::PublicKey { key_path: path.display().to_string() },
        None => AuthMethod::Auto,
    };
    let mut jump_hosts: Vec<String> = params
        .proxy_jump
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|jump| !jump.eq_ignore_ascii_case("none"))
        .collect();
    let keepalive_interval = params.server_alive_interval.map_or(30, |interval| interval.as_secs().try_into().unwrap_or(u32::MAX));

    let mut skipped = Vec::new();
    let forwards = forwards(&params, &mut skipped);
    if identity_files.len() > 1 {
        skipped.push("IdentityFile".to_owned());
    }
    if params.certificate_file.is_some() {
        skipped.push("CertificateFile".to_owned());
    }
    // OpenSSH uses whichever of ProxyJump and ProxyCommand comes first; the parser doesn't
    // keep the order, so ProxyJump wins.
    let command = params.unsupported_fields.get("proxycommand").map(|args| args.join(" "));
    let proxy_command = match command.filter(|command| !command.eq_ignore_ascii_case("none")) {
        Some(_) if !jump_hosts.is_empty() => {
            skipped.push("ProxyCommand".to_owned());
            None
        }
        Some(command) => match stdio_forward_host(&command) {
            Some(jump) => {
                jump_hosts.push(jump);
                None
            }
            None => Some(command),
        },
        None => None,
    };
    let forward_agent = params.forward_agent == Some(true);
    let env = params.unsupported_fields.get("setenv").map(|args| set_env(args, &mut skipped)).unwrap_or_default();
    if params.unsupported_fields.contains_key("sendenv") {
        skipped.push("SendEnv".to_owned());
    }
    skipped.extend(
        NOTABLE_UNSUPPORTED
            .iter()
            .filter(|field| params.unsupported_fields.contains_key(**field))
            .map(|field| field.to_string()),
    );

    let existing = existing
        .iter()
        .find(|p| p.name == alias || (p.host == host && p.port == port && p.username == username))
        .map(|p| p.name.clone());
    Candidate {
        alias,
        host,
        port,
        username,
        auth,
        jump_hosts,
        proxy_command,
        keepalive_interval,
        forwards,
        forward_agent,
        env,
        existing,
        skipped,
    }
}

/// `SetEnv NAME=value ...`, with the quotes of `NAME="a value"` removed. Entries without a
/// name are reported as skipped.
fn set_env(args: &[String], skipped: &mut Vec<String>) -> Vec<EnvVar> {
    let mut vars = Vec::new();
    for arg in args {
        let mut text = String::new();
        let mut chars = arg.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => {}
                '\\' => text.extend(chars.next()),
                c => text.push(c),
            }
        }
        match text.split_once('=') {
            Some((name, value)) if !name.is_empty() => vars.push(EnvVar { name: name.to_owned(), value: value.to_owned() }),
            _ => skipped.push("SetEnv".to_owned()),
        }
    }
    vars
}

/// Forwarding rules from `LocalForward`, `RemoteForward` and `DynamicForward`; Unix socket
/// forwards are reported as skipped. The parser keeps only the last `LocalForward` and
/// `DynamicForward` of each host block.
fn forwards(params: &HostParams, skipped: &mut Vec<String>) -> Vec<ForwardRule> {
    let mut rules = Vec::new();
    let rule = |kind, (bind_host, bind_port): (String, u16), (target_host, target_port): (String, u16)| ForwardRule {
        id: String::new(),
        kind,
        bind_host,
        bind_port,
        target_host,
        target_port,
        description: String::new(),
        auto_start: false,
    };
    if let Some(args) = params.unsupported_fields.get("localforward") {
        match (args.first().and_then(|a| parse_listen(a)), args.get(1).and_then(|a| parse_host_port(a))) {
            (Some(listen), Some(target)) => rules.push(rule(ForwardKind::Local, listen, target)),
            _ => skipped.push("LocalForward".to_owned()),
        }
    }
    for forward in &params.remote_forward {
        let listen = match &forward.listen {
            RemoteForwardListen::Port(port) => Some((String::new(), *port)),
            // OpenSSH's "*" (all interfaces) is the empty address in the protocol; keep it
            // explicit, as an empty bind address here means loopback.
            RemoteForwardListen::Host { host, port } if host == "*" => Some(("0.0.0.0".to_owned(), *port)),
            RemoteForwardListen::Host { host, port } => Some((host.clone(), *port)),
            RemoteForwardListen::UnixSocket(_) => None,
        };
        match (listen, &forward.destination) {
            (Some(listen), Some(RemoteForwardDestination::Host { host, port })) => {
                rules.push(rule(ForwardKind::Remote, listen, (host.clone(), *port)));
            }
            _ => skipped.push("RemoteForward".to_owned()),
        }
    }
    if let Some(args) = params.unsupported_fields.get("dynamicforward") {
        match args.first().and_then(|a| parse_listen(a)) {
            Some(listen) => rules.push(rule(ForwardKind::Dynamic, listen, (String::new(), 0))),
            None => skipped.push("DynamicForward".to_owned()),
        }
    }
    skipped.dedup();
    rules
}

/// `port`, `host:port` or `[v6]:port`; an empty or `*` host means all interfaces.
fn parse_listen(text: &str) -> Option<(String, u16)> {
    match text.parse::<u16>() {
        Ok(port) => Some((String::new(), port)),
        Err(_) => {
            let (host, port) = parse_host_port(text)?;
            Some((if host == "*" { "0.0.0.0".to_owned() } else { host }, port))
        }
    }
}

/// `host:port` or `[v6]:port` (also `host/port`, an OpenSSH alternative).
fn parse_host_port(text: &str) -> Option<(String, u16)> {
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (host, port) = rest.split_once("]:")?;
            (host, port)
        }
        None => text.rsplit_once(':').or_else(|| text.rsplit_once('/'))?,
    };
    Some((host.to_owned(), port.parse().ok()?))
}

/// `[user@]host[:port]`, as used by `ProxyJump`.
fn parse_destination(text: &str) -> (String, String, u16) {
    let (username, address) = match text.rsplit_once('@') {
        Some((user, address)) => (user.to_owned(), address),
        None => (local_username(), text),
    };
    match parse_host_port(address) {
        Some((host, port)) => (username, host, port),
        None => (username, address.trim_start_matches('[').trim_end_matches(']').to_owned(), 22),
    }
}

/// The jump host of `ssh [-q] [-l user] [-p port] -W %h:%p host`, written as a ProxyJump
/// entry (`[user@]host[:port]`); `None` for other commands.
fn stdio_forward_host(command: &str) -> Option<String> {
    let mut words = command.split_whitespace();
    let program = words.next()?;
    if program != "ssh" && !program.ends_with("/ssh") {
        return None;
    }
    let (mut forward, mut host, mut user, mut port) = (false, None, None, None);
    while let Some(word) = words.next() {
        match word {
            "-W" => forward = matches!(words.next()?, "%h:%p" | "[%h]:%p"),
            "-q" => {}
            "-l" => user = Some(words.next()?),
            "-p" => port = Some(words.next()?.parse::<u16>().ok()?),
            word if word.starts_with('-') || host.is_some() => return None,
            word => host = Some(word),
        }
    }
    let host = host.filter(|_| forward)?;
    let (user, host) = match host.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (user, host),
    };
    let address = match port {
        Some(port) => crate::forward::host_port(host, port),
        None => host.to_owned(),
    };
    Some(match user {
        Some(user) => format!("{user}@{address}"),
        None => address,
    })
}

/// `%h` (the alias) and `%%` in `HostName`.
fn expand_host_tokens(name: &str, alias: &str) -> String {
    name.replace("%%", "\0").replace("%h", alias).replace('\0', "%")
}

fn local_username() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_profile(id: String, name: String, host: String, port: u16, username: String) -> Profile {
    Profile { id, ..Profile::new(name, host, port, username) }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(profiles[0].jump_hosts, [profiles[1].id.clone()]);
        assert!(profiles.iter().all(|p| p.forwards.iter().all(|f| !f.id.is_empty())));
        assert_eq!(profiles[0].forwards[0].bind_host, "127.0.0.1");

        // An existing profile for the jump host is referenced instead of duplicated.
        let mut bastion = new_profile("existing".into(), "bastion".into(), "bastion.example.com".into(), 2200, "alice".into());
        bastion.auth = AuthMethod::Agent;
        let (_, profiles) = plan(&path, &["db".to_owned(), "web".to_owned()], &[bastion], &[]).unwrap();
        assert_eq!(plan_ids(&profiles), ["db", "web"]);
        assert!(profiles.iter().all(|p| p.jump_hosts == ["existing"]));
    }

    #[test]
    fn plan_shares_one_proxy_per_command() {
        let (_dir, path) = write_config(CONFIG);
        let (proxies, profiles) = plan(&path, &["legacy".to_owned(), "tunneled".to_owned()], &[], &[]).unwrap();
        assert_eq!(proxies.len(), 1);
        assert_eq!((proxies[0].kind, proxies[0].command.as_str()), (ProxyKind::Command, "nc -X 5 %h %p"));
        assert!(profiles.iter().all(|p| p.proxy.as_ref() == Some(&proxies[0].id)));

        // An existing proxy with the same command is used.
        let existing = Proxy { id: "nc".into(), name: "netcat".into(), ..proxies[0].clone() };
        let (proxies, profiles) = plan(&path, &["tunneled".to_owned()], &[], &[existing]).unwrap();
        assert!(proxies.is_empty());
        assert_eq!(profiles[0].proxy.as_deref(), Some("nc"));
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(parse_listen("8080"), Some((String::new(), 8080)));
        assert_eq!(parse_listen("*:8080"), Some(("0.0.0.0".into(), 8080)));
        assert_eq!(parse_host_port("[::1]:22"), Some(("::1".into(), 22)));
        assert_eq!(parse_host_port("db/5432"), Some(("db".into(), 5432)));
        assert_eq!(parse_destination("ops@jump:2222"), ("ops".into(), "jump".into(), 2222));
        assert_eq!(parse_destination("jump").1, "jump");
        assert_eq!(expand_host_tokens("%h.corp%%", "db"), "db.corp%");
        assert_eq!(stdio_forward_host("ssh -W %h:%p jump").as_deref(), Some("jump"));
        assert_eq!(stdio_forward_host("/usr/bin/ssh -l ops -p 2222 jump -W [%h]:%p").as_deref(), Some("ops@jump:2222"));
        assert_eq!(stdio_forward_host("ssh -W %h:%p ops@::1 -p 2").as_deref(), Some("ops@[::1]:2"));
        assert_eq!(stdio_forward_host("ssh -i key -W %h:%p jump"), None);
        assert_eq!(stdio_forward_host("ssh jump nc %h %p"), None);
        assert_eq!(stdio_forward_host("ssh -W db:22 jump"), None);
    }
}
