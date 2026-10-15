//! `~/.ssh/known_hosts`: checking a server's key on connection, as OpenSSH does ([`check`],
//! which also reads the other files OpenSSH reads by default), and for the settings' Known
//! Hosts listing entries, finding hashed ones by host name (as
//! `ssh-keygen -F` does) and removing them (as `ssh-keygen -R` does, keeping the previous
//! file as `known_hosts.old`), and adding a key the user accepted ([`learn`]).
//!
//! russh's own check differs from OpenSSH: it calls a key changed as soon as one line for the
//! host has another key of the type, even if a later line has the server's key; it ignores
//! `@revoked`, wildcards and negations, and it doesn't count `#` lines in the line it reports.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context;
use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{Algorithm, HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;
use ts_rs::TS;

use crate::error::{Error, Result};

/// A line of the file with a host key.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(rename = "KnownHost")]
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

/// The other files OpenSSH checks keys against by default: the rest of `UserKnownHostsFile`
/// (`~/.ssh/known_hosts2`) and `GlobalKnownHostsFile` (keys an administrator installed for
/// every user, in `/etc/ssh`, or `%ProgramData%\ssh` for Windows' OpenSSH).
fn other_paths(user: &Path) -> Vec<PathBuf> {
    let global = if cfg!(windows) {
        std::env::var_os("ProgramData").map(|data| PathBuf::from(data).join("ssh"))
    } else {
        Some(PathBuf::from("/etc/ssh"))
    };
    let mut paths = vec![user.with_file_name("known_hosts2")];
    if let Some(global) = global {
        let files = [global.join("ssh_known_hosts"), global.join("ssh_known_hosts2")];
        // Any user may create folders in `%ProgramData%`: where OpenSSH's `ssh` folder
        // doesn't exist, another user could make it and trust a key of theirs for every host.
        #[cfg(windows)]
        let files = files.into_iter().filter(|file| owned_by_the_system(&global) && owned_by_the_system(file));
        paths.extend(files);
    }
    paths
}

/// Whether `path` belongs to the Administrators group or the system, as the files Windows'
/// OpenSSH installs do; not when it can't be told (a file that isn't there).
#[cfg(windows)]
fn owned_by_the_system(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut owner: PSID = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; `owner` points into `descriptor`, freed after use.
    unsafe {
        let status = GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != ERROR_SUCCESS {
            return false;
        }
        let owned = IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0 || IsWellKnownSid(owner, WinLocalSystemSid) != 0;
        LocalFree(descriptor);
        owned
    }
}

/// Whether `host` is a plain host name or address, which known_hosts can't read as a pattern
/// or a list: a key recorded for `*` would be trusted for every host.
pub fn is_plain_host(host: &str) -> bool {
    !host.is_empty() && !host.chars().any(|c| c.is_whitespace() || c.is_control() || ",*?![]".contains(c))
}

/// Held while ZShell writes known_hosts, so that a removal, which replaces the whole file,
/// can't lose a key appended by another connection meanwhile. `ssh` and `ssh-keygen` don't
/// take it: [`remove_at`] checks that the file is unchanged just before replacing it.
static WRITING: Mutex<()> = Mutex::new(());

fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Records `key` for `host:port` at the end of the user's known_hosts, as OpenSSH does when
/// the user accepts a new key: the host name in lower case, on a line of its own.
pub fn learn(host: &str, port: u16, key: &PublicKey) -> Result<()> {
    learn_at(&required_path()?, host, port, key)
}

fn learn_at(path: &Path, host: &str, port: u16, key: &PublicKey) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    if !is_plain_host(host) {
        return Err(Error::new("profile.invalidHost"));
    }
    let key = key.to_openssh().map_err(|e| Error::new("knownHosts.writeFailed").param("path", path.display()).detail(e))?;
    let write_failed = |e: std::io::Error| Error::new("knownHosts.writeFailed").param("path", path.display()).detail(e);
    if let Some(dir) = path.parent() {
        create_ssh_dir(dir).map_err(write_failed)?;
    }
    let _writing = writing();
    // Readable by others, as OpenSSH leaves it.
    let mut file = fs::OpenOptions::new().read(true).append(true).create(true).open(path).map_err(write_failed)?;
    // A file that doesn't end with a line break gets one first; an empty one doesn't.
    let mut last = *b"\n";
    if file.metadata().map_err(write_failed)?.len() > 0 {
        file.seek(SeekFrom::End(-1)).and_then(|_| file.read_exact(&mut last)).map_err(write_failed)?;
    }
    let separator = if last[0] == b'\n' { "" } else { "\n" };
    let line = format!("{separator}{} {key}\n", host_pattern(&host.to_ascii_lowercase(), port));
    file.write_all(line.as_bytes()).map_err(write_failed)
}

/// Creates `~/.ssh` if it isn't there, as OpenSSH does: only its owner may list it.
fn create_ssh_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)
}

/// How a host is recorded: `host`, or `[host]:port` off port 22.
pub fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
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
    match fs::read(path) {
        Ok(content) => Ok(content.split_inclusive(|&b| b == b'\n').map(line_text).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::new("knownHosts.readFailed").param("path", path.display()).detail(e)),
    }
}

/// A line without its ending. Bytes that aren't UTF-8 (a comment saved in another encoding)
/// are replaced rather than making the whole file unreadable: OpenSSH reads bytes.
fn line_text(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8_lossy(line).into_owned()
}

fn list_at(path: &Path) -> Result<Vec<Entry>> {
    Ok(read_lines(path)?.iter().enumerate().filter_map(|(i, text)| parse_line(i + 1, text)).collect())
}

pub fn list() -> Result<Vec<Entry>> {
    list_at(&required_path()?)
}

/// OpenSSH's `match_pattern`: `*` matches any run of characters and `?` any one, ignoring
/// case (host names are compared in lower case). Only the last `*` is ever gone back to:
/// whatever an earlier one could match, the later one can too. So a line with many stars
/// takes time in proportion to the pattern's length times the host's, not exponential time
/// in the number of stars, while checking a host key.
fn glob_matches(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    // The last `*` seen, and where in the text what it matches ends so far.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == b'?' || c.eq_ignore_ascii_case(&text[t]) => {
                p += 1;
                t += 1;
            }
            // Let the last `*` take one more character, and try the rest again from there.
            _ => match star {
                Some((star_p, star_t)) => {
                    star = Some((star_p, star_t + 1));
                    p = star_p + 1;
                    t = star_t + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

/// Whether a host pattern is `host_port` (see [`host_pattern`]): hashed, or written out,
/// perhaps with wildcards. A negated pattern (`!host`) is matched without its `!`.
fn host_matches(pattern: &str, host_port: &str) -> bool {
    let Some(hashed) = pattern.strip_prefix("|1|") else {
        let pattern = pattern.strip_prefix('!').unwrap_or(pattern);
        return glob_matches(pattern.as_bytes(), host_port.as_bytes());
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

impl Entry {
    /// Whether the line is for `host_port`, as OpenSSH's `match_hostname`: one of its
    /// patterns matches, and none of its negated ones (`!host`) does.
    fn applies_to(&self, host_port: &str) -> bool {
        let matching = |negated: bool| self.hosts.iter().any(|host| host.starts_with('!') == negated && host_matches(host, host_port));
        matching(false) && !matching(true)
    }
}

/// The lines of the entries for the host `query` names (see [`query_host_port`]), hashed or not.
pub fn find(query: &str) -> Result<Vec<usize>> {
    let Some(host_port) = query_host_port(&query.to_ascii_lowercase()) else {
        return Ok(Vec::new());
    };
    Ok(list()?.into_iter().filter(|entry| entry.applies_to(&host_port)).map(|entry| entry.line).collect())
}

/// What known_hosts says about a server's key.
#[derive(Debug, PartialEq)]
pub enum Check {
    /// A line for the host has this key.
    Known,
    /// No line for the host has a key of this type, nor of another.
    Unknown,
    /// No line for the host has a key of this type, but some have keys of other types: a
    /// server the user already trusts, or one pretending to be it (see `prefer_known`).
    OtherTypesKnown(Vec<OtherKey>),
    /// The host's key of this type is another one, on this line of this file.
    Changed { path: PathBuf, line: usize },
    /// The key is marked `@revoked` on this line of this file (for any host).
    Revoked { path: PathBuf, line: usize },
}

/// A key of another type that known_hosts has for a host, as OpenSSH shows it.
#[derive(Debug, PartialEq)]
pub struct OtherKey {
    pub path: PathBuf,
    pub line: usize,
    /// As OpenSSH names the type, e.g. `ECDSA`.
    pub kind: String,
    pub fingerprint: String,
}

/// The entries of the files in `paths`, as one list, the way OpenSSH reads them. The first
/// file (the user's own) must be readable; the others are skipped if they can't be read.
fn entries_at(paths: &[PathBuf]) -> Result<Vec<(&Path, Entry)>> {
    let mut entries = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let listed = match list_at(path) {
            Ok(listed) => listed,
            Err(_) if index > 0 => continue,
            Err(e) => return Err(e),
        };
        entries.extend(listed.into_iter().map(|entry| (path.as_path(), entry)));
    }
    Ok(entries)
}

/// The keys of the plain lines (not `@revoked` or `@cert-authority`) for `host_port`.
fn host_keys<'a>(entries: &'a [(&'a Path, Entry)], host_port: &'a str) -> impl Iterator<Item = (&'a Path, &'a Entry, &'a PublicKey)> {
    entries
        .iter()
        .filter(move |(_, entry)| entry.marker.is_none() && entry.applies_to(host_port))
        .filter_map(|(path, entry)| entry.key.as_ref().map(|key| (*path, entry, key)))
}

/// Checks `key` against the files in `paths` (see [`entries_at`]), the way OpenSSH does.
fn check_at(paths: &[PathBuf], host: &str, port: u16, key: &PublicKey) -> Result<Check> {
    let host_port = host_pattern(host, port).to_ascii_lowercase();
    let entries = entries_at(paths)?;
    let has_key = |(_, entry): &&(&Path, Entry)| entry.key.as_ref() == Some(key);
    // As OpenSSH: a revoked key is refused whatever else is known, and the host's key on any
    // line is accepted whatever other keys of the type it has. CA lines are for certificates.
    if let Some((path, entry)) = entries.iter().filter(has_key).find(|(_, entry)| entry.marker.as_deref() == Some("revoked")) {
        return Ok(Check::Revoked { path: path.to_path_buf(), line: entry.line });
    }
    let for_host: Vec<_> = host_keys(&entries, &host_port).collect();
    if for_host.iter().any(|(_, _, recorded)| *recorded == key) {
        return Ok(Check::Known);
    }
    if let Some((path, entry, _)) = for_host.iter().find(|(_, _, recorded)| same_kind(&recorded.algorithm(), &key.algorithm())) {
        return Ok(Check::Changed { path: path.to_path_buf(), line: entry.line });
    }
    // The first line of each other type, as OpenSSH's `show_other_keys`.
    let mut others: Vec<OtherKey> = Vec::new();
    for (path, entry, recorded) in for_host {
        let kind = kind_name(&recorded.algorithm());
        if !others.iter().any(|other| other.kind == kind) {
            let fingerprint = recorded.fingerprint(HashAlg::Sha256).to_string();
            others.push(OtherKey { path: path.to_path_buf(), line: entry.line, kind, fingerprint });
        }
    }
    Ok(if others.is_empty() { Check::Unknown } else { Check::OtherTypesKnown(others) })
}

/// Whether two algorithms are for the same type of key: RSA keys sign with SHA-1 or SHA-2.
fn same_kind(a: &Algorithm, b: &Algorithm) -> bool {
    matches!((a, b), (Algorithm::Rsa { .. }, Algorithm::Rsa { .. })) || a == b
}

/// A key type as OpenSSH names it in messages (`sshkey_type`): `ED25519`, `ECDSA`, `RSA`.
fn kind_name(algorithm: &Algorithm) -> String {
    match algorithm {
        Algorithm::Ed25519 => "ED25519".to_owned(),
        Algorithm::Ecdsa { .. } => "ECDSA".to_owned(),
        Algorithm::Rsa { .. } => "RSA".to_owned(),
        Algorithm::Dsa => "DSA".to_owned(),
        other => other.as_str().to_owned(),
    }
}

/// `preferred` host key algorithms with those for the types of key known_hosts has for
/// `host:port` first, keeping their order, as OpenSSH orders its `HostKeyAlgorithms`: a server
/// with several keys then shows one that can be checked, rather than one of a type not yet
/// recorded, which would look like a first connection.
pub fn prefer_known(preferred: &[Algorithm], host: &str, port: u16) -> Vec<Algorithm> {
    let user = path();
    let mut paths: Vec<PathBuf> = user.iter().cloned().collect();
    paths.extend(user.as_deref().map(other_paths).unwrap_or_default());
    let host_port = host_pattern(host, port).to_ascii_lowercase();
    let known: Vec<Algorithm> =
        entries_at(&paths).map(|entries| host_keys(&entries, &host_port).map(|(_, _, key)| key.algorithm()).collect()).unwrap_or_default();
    order_known(preferred, &known)
}

fn order_known(preferred: &[Algorithm], known: &[Algorithm]) -> Vec<Algorithm> {
    let (mut first, rest): (Vec<Algorithm>, Vec<Algorithm>) =
        preferred.iter().cloned().partition(|algorithm| known.iter().any(|k| same_kind(k, algorithm)));
    first.extend(rest);
    first
}

/// Checks the key a server at `host:port` presented, as OpenSSH does: against
/// `~/.ssh/known_hosts` and the [other files](other_paths) OpenSSH reads by default.
pub fn check(host: &str, port: u16, key: &PublicKey) -> Result<Check> {
    let user = required_path()?;
    let mut paths = vec![user.clone()];
    paths.extend(other_paths(&user));
    check_at(&paths, host, port, key)
}

/// Writes `content` to `path`, replacing what is there with a file no more readable than the
/// one `like` describes: a copy of a private known_hosts names the hosts the user connects to.
/// Created so on Unix rather than changed afterwards, so it is never readable meanwhile.
/// Elsewhere a plain write: a read-only copy of a read-only file couldn't be replaced later.
fn write_new_like(path: &Path, content: &[u8], like: &fs::Metadata) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        match fs::remove_file(path) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let mode = like.permissions().mode() & 0o777;
        fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(path)?.write_all(content)
    }
    #[cfg(not(unix))]
    {
        let _ = like;
        fs::write(path, content)
    }
}

fn remove_at(path: &Path, line: usize, text: &str) -> Result<()> {
    remove_with(path, line, text, || {})
}

/// [`remove_at`], calling `before_replacing` (for the tests) when about to check that the file
/// is unchanged.
fn remove_with(path: &Path, line: usize, text: &str, before_replacing: impl FnOnce()) -> Result<()> {
    let _writing = writing();
    let content = fs::read(path).map_err(|e| Error::new("knownHosts.readFailed").param("path", path.display()).detail(e))?;
    // Keep each line's own ending, so the rest of the file stays byte for byte.
    let lines: Vec<&[u8]> = content.split_inclusive(|&b| b == b'\n').collect();
    let current = lines.get(line.wrapping_sub(1)).map(|l| line_text(l));
    if current.as_deref() != Some(text) {
        return Err(Error::new("knownHosts.changed"));
    }
    let rest: Vec<u8> = lines.iter().enumerate().filter(|(i, _)| *i != line - 1).flat_map(|(_, l)| l.iter().copied()).collect();
    // Whether the file was replaced: not when it changed since it was read (`ssh` added a
    // key), which would be lost.
    let write = || -> anyhow::Result<bool> {
        let metadata = fs::metadata(path)?;
        // As `ssh-keygen -R` does.
        write_new_like(&path.with_file_name("known_hosts.old"), &content, &metadata).context("known_hosts.old")?;
        // Written beside it and renamed over it, so the file is never half written.
        let temp = path.with_file_name("known_hosts.zshell-tmp");
        write_new_like(&temp, &rest, &metadata)?;
        fs::set_permissions(&temp, metadata.permissions())?;
        before_replacing();
        if fs::read(path)? != content {
            let _ = fs::remove_file(&temp);
            return Ok(false);
        }
        fs::rename(&temp, path)?;
        Ok(true)
    };
    match write().context(Error::new("knownHosts.writeFailed").param("path", path.display())) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::new("knownHosts.changed")),
        Err(e) => Err(Error::from(e)),
    }
}

/// Removes the entry on `line` if the line still reads `text` (otherwise the file has
/// changed since it was listed: `knownHosts.changed`). A line naming several hosts goes as
/// a whole, with the other hosts on it.
pub fn remove(line: usize, text: &str) -> Result<()> {
    remove_at(&required_path()?, line, text)
}

#[cfg(test)]
mod tests;
