//! Importing hosts from an OpenSSH client config (`~/.ssh/config`) as session profiles.
//!
//! Import is a one-time copy: each concrete `Host` alias (no wildcards) becomes a profile
//! with the settings `ssh` would use for it, including those inherited from `Host *`.
//! `ProxyJump` hosts are imported too, as profiles of their own, and each `ProxyCommand` as a
//! command proxy (one per distinct command).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Serialize;
use ssh2_config::{HostParams, ParseRule, RemoteForwardDestination, RemoteForwardListen, SshConfig};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::model::{local_username, AuthMethod, Connection, EnvVar, Profile, Remote, SshOptions};
use crate::model::{ForwardKind, ForwardRule, ALL_ADDRESSES};
use crate::model::{Proxy, ProxyKind};

/// Options that change how `ssh` connects but are not imported, reported per host.
const NOTABLE_UNSUPPORTED: &[&str] = &["forwardx11", "remotecommand", "localcommand", "hostkeyalias", "identityagent", "pkcs11provider"];

/// A host found in the config, as it would be imported.
#[derive(Debug, Serialize, TS)]
#[ts(rename = "ImportCandidate")]
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
    /// That profile's id: the name may also be another session's (a Telnet one).
    #[serde(skip)]
    #[ts(skip)]
    pub existing_id: Option<String>,
    /// Options in the config that are not imported.
    pub skipped: Vec<String>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".ssh").join("config"))
}

pub fn scan(path: &Path, existing: &[Profile]) -> Result<Vec<Candidate>> {
    let (config, has_match) = parse(path)?;
    let mut candidates: Vec<Candidate> = aliases(&config).into_iter().map(|alias| candidate(&config, alias, existing)).collect();
    // A `Match` block may apply to any host.
    if has_match {
        candidates.iter_mut().for_each(|candidate| candidate.skipped.push("Match".to_owned()));
    }
    Ok(candidates)
}

/// The proxies and profiles to add for importing `selected`, with ids assigned and jump hosts
/// and proxies resolved. Jump hosts not yet in `existing`, and proxy commands not yet in
/// `proxies`, are added as well.
pub fn plan(path: &Path, selected: &[String], existing: &[Profile], proxies: &[Proxy]) -> Result<(Vec<Proxy>, Vec<Profile>)> {
    let (config, _) = parse(path)?;
    let known: HashSet<String> = aliases(&config).into_iter().collect();

    // Profile id for every alias involved, following ProxyJump chains.
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut pending = Vec::new();
    let mut queue: VecDeque<String> = selected.iter().cloned().collect();
    while let Some(alias) = queue.pop_front() {
        if ids.contains_key(&alias) || !known.contains(&alias) {
            continue;
        }
        let mut candidate = candidate(&config, alias.clone(), existing);
        if let Some(id) = candidate.existing_id {
            ids.insert(alias, id);
            continue;
        }
        candidate.jump_hosts = jump_chain(&config, &candidate.jump_hosts, &known, existing, 0);
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
            // An alias with another user or port (`admin@bastion`, `bastion:2222`) still has the
            // alias's address and key, as in `ssh`.
            let (user, host, port) = split_destination(jump);
            let alias = known.contains(&host).then(|| self::candidate(&config, host.clone(), existing));
            let (host, port, username) = match &alias {
                Some(alias) => (alias.host.clone(), port.unwrap_or(alias.port), user.unwrap_or_else(|| alias.username.clone())),
                None => (host, port.unwrap_or(22), user.unwrap_or_else(local_username)),
            };
            let key = (host.clone(), port, username.clone());
            let found = existing
                .iter()
                .find(|p| p.ssh().is_some_and(|ssh| (&ssh.remote.host, ssh.remote.port, &ssh.remote.username) == (&key.0, key.1, &key.2)));
            let id = match (found, by_address.get(&key)) {
                (Some(profile), _) => profile.id.clone(),
                (None, Some(id)) => id.clone(),
                (None, None) => {
                    let id = new_id();
                    by_address.insert(key, id.clone());
                    let mut ssh = SshOptions::new(Remote::new(host, port, username));
                    if let Some(alias) = alias {
                        ssh.auth = alias.auth;
                        ssh.remote.keepalive_interval = alias.keepalive_interval;
                    }
                    profiles.push(new_profile(id.clone(), jump.clone(), ssh));
                    id
                }
            };
            jump_hosts.push(id);
        }
        let remote = Remote {
            jump_hosts,
            keepalive_interval: candidate.keepalive_interval,
            ..Remote::new(candidate.host, candidate.port, candidate.username)
        };
        let mut ssh =
            SshOptions { auth: candidate.auth, forward_agent: candidate.forward_agent, env: candidate.env, ..SshOptions::new(remote) };
        if let Some(command) = candidate.proxy_command {
            let same = |p: &&Proxy| p.kind == ProxyKind::Command && p.command == command;
            ssh.remote.proxy = Some(match proxies.iter().chain(&new_proxies).find(same) {
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
        ssh.forwards = candidate.forwards.into_iter().filter_map(|rule| ForwardRule { id: new_id(), ..rule }.normalize().ok()).collect();
        profiles.push(new_profile(ids[&candidate.alias].clone(), candidate.alias, ssh));
    }
    // Checked and tidied as saving does, as an import from a sessions file is.
    for proxy in &mut new_proxies {
        proxy.normalize().map_err(|e| Error::new("import.invalidProxy").param("name", &proxy.name).detail(e))?;
    }
    for profile in &mut profiles {
        profile.normalize_imported().map_err(|e| Error::new("import.invalidSession").param("name", &profile.name).detail(e))?;
    }
    Ok((new_proxies, profiles))
}

/// The parsed config, and whether it has `Match` blocks (which are not imported).
fn parse(path: &Path) -> Result<(SshConfig, bool)> {
    let mut text = String::new();
    let has_match = preprocess(path, 0, &mut text)?;
    let config = SshConfig::default()
        .parse(&mut text.as_bytes(), ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS)
        .map_err(|e| Error::new("import.parseFailed").param("path", path.display()).detail(e))?;
    Ok((config, has_match))
}

/// How deeply `Include`s may nest, as in OpenSSH, which also stops with an error (a file that
/// includes itself).
const MAX_INCLUDE_DEPTH: usize = 16;
/// Stands in for a `Match` line: a block that applies to no host and is not one to import.
const SKIPPED_MATCH: &str = "Host *.zshell-skipped-match.invalid";

/// Appends the config at `path` to `out` as ssh2-config is to read it, and returns whether it
/// has a `Match` block. ssh2-config doesn't know `Match` and would apply the options under it
/// to the `Host` block before (importing a host with another user or jump host), so each
/// `Match` line becomes a `Host` line matching nothing. `Include`d files are inlined here,
/// which is how OpenSSH reads them, so that theirs are seen too; none is left for
/// ssh2-config, which follows them with no limit (a file that includes itself would overflow
/// the stack).
fn preprocess(path: &Path, depth: usize, out: &mut String) -> Result<bool> {
    let read_failed = |e: std::io::Error| Error::new("import.readFailed").param("path", path.display()).detail(e);
    let text = std::fs::read_to_string(path).map_err(read_failed)?;
    let mut has_match = false;
    // The block the lines are in: options after an `Include` inside it still belong to it,
    // even if the included file starts blocks of its own (as in OpenSSH).
    let mut block = None;
    for line in text.lines() {
        let key = keyword(line);
        if key.eq_ignore_ascii_case("match") {
            has_match = true;
            out.push_str(SKIPPED_MATCH);
            block = Some(SKIPPED_MATCH);
        } else if key.eq_ignore_ascii_case("include") {
            if depth >= MAX_INCLUDE_DEPTH {
                return Err(Error::new("import.includeTooDeep").param("path", path.display()));
            }
            let trimmed = line.trim_start();
            let args = trimmed[key.len()..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
            let start = out.len();
            for pattern in args.split_whitespace().map(|arg| arg.trim_matches('"')) {
                for included in include_paths(pattern) {
                    has_match |= preprocess(&included, depth + 1, out)?;
                }
            }
            let starts_blocks = out[start..].lines().any(|line| keyword(line).eq_ignore_ascii_case("host"));
            if starts_blocks {
                // Before any block, options apply to every host.
                out.push_str(block.unwrap_or("Host *"));
            }
        } else {
            if key.eq_ignore_ascii_case("host") {
                block = Some(line);
            }
            out.push_str(line);
        }
        out.push('\n');
    }
    Ok(has_match)
}

/// The keyword a config line starts with.
fn keyword(line: &str) -> &str {
    line.trim_start().split(|c: char| c.is_whitespace() || c == '=').next().unwrap_or_default()
}

/// The files an `Include` pattern names: relative to `~/.ssh` unless absolute, with `~`
/// expanded and wildcards matched (no match is no file, as in OpenSSH). As with OpenSSH's
/// glob(3), a wildcard doesn't match a leading dot (`.DS_Store`, an editor's swap file);
/// folders are left out too.
fn include_paths(pattern: &str) -> Vec<PathBuf> {
    let home = std::env::home_dir().unwrap_or_default();
    let path = match pattern.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if Path::new(pattern).is_absolute() => PathBuf::from(pattern),
        None => home.join(".ssh").join(pattern),
    };
    let options = glob::MatchOptions { require_literal_leading_dot: true, ..glob::MatchOptions::new() };
    let mut paths: Vec<PathBuf> = glob::glob_with(&path.to_string_lossy(), options)
        .map(|paths| paths.flatten().filter(|path| !path.is_dir()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
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

/// `path` with the home folder written as `~/`, as key paths are stored, so that the session
/// still works under another user name or on another computer. ssh2-config expands the `~`
/// of `IdentityFile ~/.ssh/key`.
fn home_relative(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        _ => path.display().to_string(),
    }
}

fn candidate(config: &SshConfig, alias: String, existing: &[Profile]) -> Candidate {
    let params = config.query(&alias);
    let host = params.host_name.as_deref().map(|name| expand_host_tokens(name, &alias)).unwrap_or_else(|| alias.clone());
    let port = params.port.unwrap_or(22);
    let username = params.user.clone().unwrap_or_else(local_username);
    let identity_files = params.identity_file.clone().unwrap_or_default();
    let auth = match identity_files.first() {
        Some(path) => AuthMethod::PublicKey { key_path: home_relative(path, std::env::home_dir().as_deref()) },
        None => AuthMethod::Auto,
    };
    let mut jump_hosts: Vec<String> =
        params.proxy_jump.clone().unwrap_or_default().into_iter().filter(|jump| !jump.eq_ignore_ascii_case("none")).collect();
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
    skipped
        .extend(NOTABLE_UNSUPPORTED.iter().filter(|field| params.unsupported_fields.contains_key(**field)).map(|field| field.to_string()));

    // Only SSH sessions: a Telnet session with the same name must not become a jump host.
    let existing = existing
        .iter()
        .filter_map(|p| Some((p, &p.ssh()?.remote)))
        .find(|(p, remote)| p.name == alias || (remote.host == host && remote.port == port && remote.username == username))
        .map(|(p, _)| p);
    let (existing, existing_id) = (existing.map(|p| p.name.clone()), existing.map(|p| p.id.clone()));
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
        existing_id,
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
            // An empty bind address here means loopback; every address is `*`, as in OpenSSH.
            RemoteForwardListen::Host { host, port } if host.is_empty() => Some((ALL_ADDRESSES.to_owned(), *port)),
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

/// `port` (loopback), `host:port` or `[v6]:port`; an empty or `*` host means every address
/// (`*`, as the rule keeps it).
fn parse_listen(text: &str) -> Option<(String, u16)> {
    match text.parse::<u16>() {
        Ok(port) => Some((String::new(), port)),
        Err(_) => {
            let (host, port) = parse_host_port(text)?;
            Some((if host.is_empty() { ALL_ADDRESSES.to_owned() } else { host }, port))
        }
    }
}

/// `host:port` or `[v6]:port` (also `host/port`, an OpenSSH alternative). A single colon
/// separates a port: an IPv6 address without brackets, which has several, has no port.
fn parse_host_port(text: &str) -> Option<(String, u16)> {
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (host, port) = rest.split_once("]:")?;
            (host, port)
        }
        None => match text.split_once(':') {
            Some((host, port)) if !port.contains(':') => (host, port),
            _ => text.rsplit_once('/')?,
        },
    };
    Some((host.to_owned(), port.parse().ok()?))
}

/// `[user@]host[:port]`, as used by `ProxyJump`, with only what is written.
fn split_destination(text: &str) -> (Option<String>, String, Option<u16>) {
    let (user, address) = match text.rsplit_once('@') {
        Some((user, address)) => (Some(user.to_owned()), address),
        None => (None, text),
    };
    match parse_host_port(address) {
        Some((host, port)) => (user, host, Some(port)),
        None => (user, address.trim_start_matches('[').trim_end_matches(']').to_owned(), None),
    }
}

/// How deeply jump hosts' own `ProxyJump`s are followed (a loop in the config ends here).
const MAX_JUMP_DEPTH: usize = 8;

/// The jump hosts to list for a host with these `ProxyJump` entries. `ssh` reaches a jump
/// host that has a `ProxyJump` of its own through those first, but ZShell doesn't follow a
/// jump host's jump hosts, so they go before it in the list.
fn jump_chain(config: &SshConfig, jumps: &[String], known: &HashSet<String>, existing: &[Profile], depth: usize) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    for jump in jumps {
        let host = split_destination(jump).1;
        if depth < MAX_JUMP_DEPTH && known.contains(&host) {
            let own = candidate(config, host, existing).jump_hosts;
            chain.extend(jump_chain(config, &own, known, existing, depth + 1));
        }
        chain.push(jump.clone());
    }
    let mut seen = HashSet::new();
    chain.retain(|jump| seen.insert(jump.clone()));
    chain
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
        Some(port) => crate::model::host_port(host, port),
        // An IPv6 address in brackets, as a ProxyJump entry writes it.
        None if host.contains(':') => format!("[{host}]"),
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

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_profile(id: String, name: String, ssh: SshOptions) -> Profile {
    Profile { id, ..Profile::new(name, Connection::Ssh(ssh)) }
}

#[cfg(test)]
mod tests;
