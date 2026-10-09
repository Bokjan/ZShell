//! `~/.ssh/known_hosts` for the settings' Known Hosts: listing entries, finding hashed ones by
//! host name (as `ssh-keygen -F` does) and removing them (as `ssh-keygen -R` does, keeping the
//! previous file as `known_hosts.old`).
//!
//! Checking and adding keys on connection is russh's (`check_known_hosts`, `learn_known_hosts`).
//! russh doesn't count `#` comment lines, so the line it reports for a changed key can be off;
//! [`changed_line`] finds the real one. Like russh, host patterns are matched exactly or by
//! their hash; OpenSSH's wildcards and negations are not interpreted.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::Context;
use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;

use crate::error::{Error, Result};

/// A line of the file with a host key.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// 1-based, counting every line of the file.
    pub line: usize,
    /// The line as written (without its line ending), which identifies it for removal.
    pub text: String,
    /// `cert-authority` or `revoked`, from `@cert-authority` / `@revoked`.
    pub marker: Option<String>,
    /// Host names or patterns; hashed ones as written (`|1|salt|hash`).
    pub hosts: Vec<String>,
    /// The key type as written, e.g. `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:…`, as OpenSSH shows it; none when the key can't be read.
    pub fingerprint: Option<String>,
    pub comment: Option<String>,
    /// The key, if it can be read.
    #[serde(skip)]
    key: Option<PublicKey>,
}

/// `~/.ssh/known_hosts`, the file OpenSSH and russh use.
pub fn path() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

fn required_path() -> Result<PathBuf> {
    path().ok_or_else(|| Error::new("knownHosts.noHome"))
}

/// How a host is recorded: `host`, or `[host]:port` off port 22.
pub fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 { host.to_owned() } else { format!("[{host}]:{port}") }
}

fn parse_line(line: usize, text: &str) -> Option<Entry> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let mut fields = trimmed.split_whitespace().peekable();
    let marker = fields.next_if(|field| field.starts_with('@')).map(|field| field[1..].to_owned());
    let hosts = fields.next()?.split(',').map(str::to_owned).collect();
    let algorithm = fields.next().unwrap_or_default().to_owned();
    let key = fields.next();
    let comment: Vec<&str> = fields.collect();
    let key = key.and_then(|key| PublicKey::from_openssh(&format!("{algorithm} {key}")).ok());
    let fingerprint = key.as_ref().map(|key| key.fingerprint(HashAlg::Sha256).to_string());
    Some(Entry {
        line,
        text: text.to_owned(),
        marker,
        hosts,
        algorithm,
        fingerprint,
        comment: (!comment.is_empty()).then(|| comment.join(" ")),
        key,
    })
}

/// The file's lines, each without its line ending; none if there is no file.
fn read_lines(path: &Path) -> Result<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content.lines().map(str::to_owned).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::new("knownHosts.readFailed").param("path", path.display()).detail(e)),
    }
}

fn list_at(path: &Path) -> Result<Vec<Entry>> {
    Ok(read_lines(path)?.iter().enumerate().filter_map(|(i, text)| parse_line(i + 1, text)).collect())
}

pub fn list() -> Result<Vec<Entry>> {
    list_at(&required_path()?)
}

/// Whether a host pattern is `host_port` (see [`host_pattern`]), written out or hashed.
fn host_matches(pattern: &str, host_port: &str) -> bool {
    let Some(hashed) = pattern.strip_prefix("|1|") else {
        return pattern.eq_ignore_ascii_case(host_port);
    };
    let Some((salt, hash)) = hashed.split_once('|') else {
        return false;
    };
    let (Ok(salt), Ok(hash)) = (BASE64.decode(salt.as_bytes()), BASE64.decode(hash.as_bytes())) else {
        return false;
    };
    let Ok(mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.chain_update(host_port.as_bytes()).verify_slice(&hash).is_ok()
}

/// What a search for `query` means as a host: `host`, `host:port` or `[host]:port`.
fn query_host_port(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() || query.contains(char::is_whitespace) {
        return None;
    }
    if let Some(rest) = query.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        return Some(host_pattern(host, port.parse().ok()?));
    }
    // A single colon separates a port; IPv6 addresses have several.
    match query.split_once(':') {
        Some((host, port)) if !port.contains(':') => Some(host_pattern(host, port.parse().ok()?)),
        _ => Some(query.to_owned()),
    }
}

/// The lines of the entries for the host `query` names (see [`query_host_port`]), hashed or not.
pub fn find(query: &str) -> Result<Vec<usize>> {
    let Some(host_port) = query_host_port(query) else {
        return Ok(Vec::new());
    };
    Ok(list()?
        .into_iter()
        .filter(|entry| entry.hosts.iter().any(|host| host_matches(host, &host_port)))
        .map(|entry| entry.line)
        .collect())
}

fn changed_line_at(path: &Path, host: &str, port: u16, key: &PublicKey) -> Option<usize> {
    let host_port = host_pattern(host, port);
    list_at(path).ok()?.into_iter().find_map(|entry| {
        let applies = entry.marker.is_none() && entry.hosts.iter().any(|h| host_matches(h, &host_port));
        let conflicts = entry.key.is_some_and(|recorded| recorded.algorithm() == key.algorithm() && recorded != *key);
        (applies && conflicts).then_some(entry.line)
    })
}

/// The line recording another key of `key`'s type for the host, which russh reported as
/// changed.
pub fn changed_line(host: &str, port: u16, key: &PublicKey) -> Option<usize> {
    changed_line_at(&path()?, host, port, key)
}

fn remove_at(path: &Path, line: usize, text: &str) -> Result<()> {
    let content = fs::read_to_string(path)
        .map_err(|e| Error::new("knownHosts.readFailed").param("path", path.display()).detail(e))?;
    // Keep each line's own ending, so the rest of the file stays byte for byte.
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let current = lines.get(line.wrapping_sub(1)).map(|l| l.trim_end_matches(['\n', '\r']));
    if current != Some(text) {
        return Err(Error::new("knownHosts.changed"));
    }
    let rest: String = lines.iter().enumerate().filter(|(i, _)| *i != line - 1).map(|(_, l)| *l).collect();
    let write = || -> anyhow::Result<()> {
        // As `ssh-keygen -R` does.
        fs::write(path.with_file_name("known_hosts.old"), &content).context("known_hosts.old")?;
        // Written beside it and renamed over it, so the file is never half written.
        let temp = path.with_file_name("known_hosts.zshell-tmp");
        fs::write(&temp, rest)?;
        fs::set_permissions(&temp, fs::metadata(path)?.permissions())?;
        fs::rename(&temp, path)?;
        Ok(())
    };
    write()
        .context(Error::new("knownHosts.writeFailed").param("path", path.display()))
        .map_err(Error::from)
}

/// Removes the entry on `line` if the line still reads `text` (otherwise the file has
/// changed since it was listed: `knownHosts.changed`).
pub fn remove(line: usize, text: &str) -> Result<()> {
    remove_at(&required_path()?, line, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIGTF00LNp4OdvFjVtxiUae5Ybe6KH/QRshtgH9+knbzN";
    const OTHER: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    /// `[127.0.0.1]:2222`, hashed by `ssh-keygen -H`.
    const HASHED: &str = "|1|sXYIBsr10rhd0b/NMnyf/gj1YxQ=|/yzMUScN42HQtI9V9p2rFEu00bU=";

    fn temp_file(content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zshell-known-hosts-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known_hosts");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parses_entries() {
        let entry = parse_line(3, &format!("a.example,10.0.0.1 ssh-ed25519 {KEY} me@laptop")).unwrap();
        assert_eq!(entry.hosts, ["a.example", "10.0.0.1"]);
        assert_eq!(entry.algorithm, "ssh-ed25519");
        assert_eq!(entry.fingerprint.as_deref(), Some("SHA256:Qiqj/En3FON0kY6HRHfPth2y4UraxseLIxWx97Vibfs"));
        assert_eq!(entry.comment.as_deref(), Some("me@laptop"));
        assert_eq!(entry.marker, None);

        let marked = parse_line(1, &format!("@revoked\t*.example ssh-ed25519 {KEY}")).unwrap();
        assert_eq!(marked.marker.as_deref(), Some("revoked"));
        assert_eq!(marked.hosts, ["*.example"]);

        let broken = parse_line(1, "host ssh-ed25519 notbase64").unwrap();
        assert_eq!(broken.fingerprint, None);

        assert_eq!(parse_line(1, "  # comment"), None);
        assert_eq!(parse_line(1, "   "), None);
    }

    #[test]
    fn matches_hashed_hosts() {
        assert!(host_matches(HASHED, "[127.0.0.1]:2222"));
        assert!(!host_matches(HASHED, "127.0.0.1"));
        assert!(host_matches("Server.Example", "server.example"));
        assert!(!host_matches("|1|garbage", "x"));
    }

    #[test]
    fn reads_queries_as_hosts() {
        assert_eq!(query_host_port("127.0.0.1:2222").as_deref(), Some("[127.0.0.1]:2222"));
        assert_eq!(query_host_port("[::1]:2222").as_deref(), Some("[::1]:2222"));
        assert_eq!(query_host_port("host:22").as_deref(), Some("host"));
        assert_eq!(query_host_port("::1").as_deref(), Some("::1"));
        assert_eq!(query_host_port("two words"), None);
    }

    #[test]
    fn counts_comment_lines() {
        let path = temp_file(&format!("# comment\n\n{HASHED} ssh-ed25519 {OTHER}\n"));
        let entries = list_at(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].line, 3);
        let key = PublicKey::from_openssh(&format!("ssh-ed25519 {KEY}")).unwrap();
        assert_eq!(changed_line_at(&path, "127.0.0.1", 2222, &key), Some(3));
        assert_eq!(changed_line_at(&path, "127.0.0.1", 22, &key), None);
    }

    #[test]
    fn removes_a_line_and_keeps_the_rest() {
        let content = format!("# keep\r\na ssh-ed25519 {KEY}\r\nb ssh-ed25519 {OTHER}");
        let path = temp_file(&content);
        let entries = list_at(&path).unwrap();
        assert_eq!(entries[0].text, format!("a ssh-ed25519 {KEY}"));

        // The file changed since it was listed.
        assert_eq!(remove_at(&path, 2, "b").unwrap_err().code(), "knownHosts.changed");
        assert_eq!(remove_at(&path, 9, "b").unwrap_err().code(), "knownHosts.changed");

        remove_at(&path, 2, &entries[0].text).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("# keep\r\nb ssh-ed25519 {OTHER}"));
        assert_eq!(fs::read_to_string(path.with_file_name("known_hosts.old")).unwrap(), content);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
