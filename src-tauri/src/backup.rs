//! Exporting sessions (with their folders and proxies) to a JSON file and importing them
//! back, for backups and for moving to another computer. Passwords stay in the system keychain
//! and are never exported.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{write_json_atomic, Folder, Profile, Protocol, SerialOptions};
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::proxy::{Proxy, ProxyKind};

const FORMAT: &str = "zshell-sessions";
const VERSION: u32 = 1;

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

/// A session in a file, as it would be imported.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub serial: SerialOptions,
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
}

pub fn export(path: &Path, folders: Vec<Folder>, proxies: Vec<Proxy>, profiles: Vec<Profile>) -> Result<()> {
    let file = SessionsFile { format: FORMAT.to_owned(), version: VERSION, folders, profiles, proxies };
    write_json_atomic(path, &file).map_err(|e| Error::new("export.writeFailed").param("path", path.display()).detail(e))
}

pub fn scan(path: &Path, existing: &[Profile]) -> Result<Vec<Candidate>> {
    let file = read(path)?;
    let names: HashMap<&str, &str> = file.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    Ok(file
        .profiles
        .iter()
        .map(|p| {
            let proxy = p.proxy.as_ref().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == *id));
            Candidate {
                id: p.id.clone(),
                name: p.name.clone(),
                protocol: p.protocol,
                host: p.host.clone(),
                port: p.port,
                username: p.username.clone(),
                serial: p.serial.clone(),
                folder: folder_path(&file.folders, p.folder.as_deref()),
                jump_hosts: p.jump_hosts.iter().filter_map(|j| names.get(j.as_str()).map(|n| n.to_string())).collect(),
                proxy: proxy.map(|proxy| proxy.name.clone()),
                proxy_command: proxy.filter(|proxy| proxy.kind == ProxyKind::Command).map(|proxy| proxy.command.clone()),
                existing: duplicate_of(p, existing).map(|e| e.name.clone()),
                auto_forwards: p.forwards.iter().filter(|f| f.auto_start).filter_map(|f| f.clone().normalize().ok()).collect(),
            }
        })
        .collect())
}

/// What to add for importing the sessions `selected` (ids in the file): new folders, proxies
/// and profiles, with new ids. Jump hosts and proxies come along unless they already exist
/// here, in which case the existing one is used; folders are matched by name and merged.
pub fn plan(path: &Path, selected: &[String], here: &Here) -> Result<Plan> {
    let (profiles, folders) = (here.profiles, here.folders);
    let file = read(path)?;
    let by_id: HashMap<&str, &Profile> = file.profiles.iter().map(|p| (p.id.as_str(), p)).collect();

    // File id → the id of the session it becomes (new, or an existing duplicate).
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut imported: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = selected.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        let Some(profile) = by_id.get(id.as_str()) else { continue };
        if ids.contains_key(&id) {
            continue;
        }
        match duplicate_of(profile, profiles) {
            Some(existing) => {
                ids.insert(id, existing.id.clone());
            }
            None => {
                ids.insert(id.clone(), uuid::Uuid::new_v4().to_string());
                imported.insert(id);
                queue.extend(profile.jump_hosts.iter().cloned());
            }
        }
    }

    // File proxy id → the id of the proxy it becomes, for the proxies the imported use.
    let mut proxy_ids: HashMap<&str, String> = HashMap::new();
    let mut proxies = Vec::new();
    for profile in file.profiles.iter().filter(|p| imported.contains(&p.id)) {
        let Some(proxy) = profile.proxy.as_ref().and_then(|id| file.proxies.iter().find(|proxy| proxy.id == *id)) else {
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
            let mut profile = Profile {
                id: ids[&p.id].clone(),
                jump_hosts: p.jump_hosts.iter().filter_map(|j| ids.get(j).cloned()).collect(),
                proxy: p.proxy.as_deref().and_then(|id| proxy_ids.get(id).cloned()),
                folder: p.folder.as_deref().and_then(|f| merger.resolve(f)),
                ..p.clone()
            };
            // As saving does: an empty bind address becomes localhost rather than every
            // interface of the server, for one.
            profile.normalize_imported().map_err(|e| Error::new("import.invalidSession").param("name", &p.name).detail(e))?;
            Ok(profile)
        })
        .collect::<Result<_>>()?;
    Ok(Plan { folders: merger.added, proxies, profiles: new_profiles })
}

/// What already exists, for an import to match against.
pub struct Here<'a> {
    pub profiles: &'a [Profile],
    pub folders: &'a [Folder],
    pub proxies: &'a [Proxy],
}

/// What an import adds.
pub struct Plan {
    pub folders: Vec<Folder>,
    pub proxies: Vec<Proxy>,
    pub profiles: Vec<Profile>,
}

fn read(path: &Path) -> Result<SessionsFile> {
    let bytes = std::fs::read(path).map_err(|e| Error::new("import.readFailed").param("path", path.display()).detail(e))?;
    let file: SessionsFile =
        serde_json::from_slice(&bytes).map_err(|e| Error::new("import.parseFailed").param("path", path.display()).detail(e))?;
    if file.format != FORMAT {
        return Err(Error::new("import.notSessionsFile").param("path", path.display()));
    }
    let mut file = file;
    // A hand-written file may leave ids out or repeat them; references go to the first.
    unique_ids(file.profiles.iter_mut().map(|p| &mut p.id));
    unique_ids(file.proxies.iter_mut().map(|p| &mut p.id));
    unique_ids(file.folders.iter_mut().map(|f| &mut f.id));
    Ok(file)
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
        .find(|e| e.name == profile.name && e.protocol == profile.protocol)
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

    fn profile(id: &str, name: &str, host: &str, folder: Option<&str>, jumps: &[&str]) -> Profile {
        Profile {
            id: id.into(),
            jump_hosts: jumps.iter().map(|j| j.to_string()).collect(),
            folder: folder.map(Into::into),
            ..Profile::new(name.into(), host.into(), 22, "alice".into())
        }
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
        let candidates = scan(&path, &here_profiles).unwrap();
        assert_eq!(candidates[0].existing.as_deref(), Some("jump"));
        assert_eq!(candidates[1].folder, ["Work", "DB"]);
        assert_eq!(candidates[1].jump_hosts, ["bastion"]);

        let here = Here { profiles: &here_profiles, folders: &here_folders, proxies: &[] };
        let Plan { folders: new_folders, profiles: new_profiles, proxies } = plan(&path, &["db".into()], &here).unwrap();
        assert!(proxies.is_empty());
        // Only "DB" is new, inside the existing "Work".
        assert_eq!(new_folders.len(), 1);
        assert_eq!((new_folders[0].name.as_str(), new_folders[0].parent.as_deref()), ("DB", Some("w")));
        // db comes with new ids, through the existing bastion.
        assert_eq!(new_profiles.len(), 1);
        assert_ne!(new_profiles[0].id, "db");
        assert_eq!(new_profiles[0].jump_hosts, ["b2"]);
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
            Profile { forwards: vec![forward], ..profile("db", "db", "db.internal", None, &["router"]) },
            profile("", "a", "a.internal", None, &[]),
            profile("", "b", "b.internal", None, &[]),
        ];
        export(&path, Vec::new(), Vec::new(), profiles).unwrap();

        let telnet = Profile { protocol: Protocol::Telnet, ..profile("t", "router", "10.0.0.1", None, &[]) };
        let candidates = scan(&path, std::slice::from_ref(&telnet)).unwrap();
        assert_eq!(candidates[0].existing, None);
        assert_eq!(candidates[1].auto_forwards[0].bind_host, "localhost");
        assert_ne!(candidates[2].id, candidates[3].id);

        let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let here = Here { profiles: std::slice::from_ref(&telnet), folders: &[], proxies: &[] };
        let Plan { profiles: new, .. } = plan(&path, &ids, &here).unwrap();
        let names: Vec<&str> = new.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["router", "db", "a", "b"]);
        // The file's own router is the jump host, not the Telnet session.
        assert_eq!(new[1].jump_hosts, std::slice::from_ref(&new[0].id));
        // As when saved: not every interface of the server.
        assert_eq!(new[1].forwards[0].bind_host, "localhost");
        assert!(!new[1].forwards[0].id.is_empty());
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
            Profile { proxy: Some("x1".into()), ..profile("a", "a", "a.internal", None, &[]) },
            Profile { proxy: Some("x2".into()), ..profile("b", "b", "b.internal", None, &[]) },
            Profile { proxy: Some("missing".into()), ..profile("c", "c", "c.internal", None, &[]) },
            Profile { proxy: Some("x1".into()), ..profile("d", "d", "d.internal", None, &[]) },
        ];
        export(&path, Vec::new(), proxies, profiles).unwrap();

        let candidates = scan(&path, &[]).unwrap();
        assert_eq!(candidates[0].proxy.as_deref(), Some("corp"));
        assert_eq!(candidates[0].proxy_command, None);
        assert_eq!(candidates[1].proxy_command.as_deref(), Some("cloudflared access ssh --hostname %h"));
        assert_eq!(candidates[2].proxy, None);

        // Here the SOCKS proxy exists under another name.
        let here_proxies = vec![proxy("mine", "office", ProxyKind::Socks5, "")];
        let here = Here { profiles: &[], folders: &[], proxies: &here_proxies };
        let ids: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let plan = plan(&path, &ids, &here).unwrap();
        // Only the command proxy is new; the unused one isn't imported.
        assert_eq!(plan.proxies.len(), 1);
        assert_eq!(plan.proxies[0].name, "cf");
        assert_ne!(plan.proxies[0].id, "x2");
        let used: Vec<_> = plan.profiles.iter().map(|p| p.proxy.clone()).collect();
        assert_eq!(used, [Some("mine".into()), Some(plan.proxies[0].id.clone()), None, Some("mine".into())]);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_other_files() {
        let path = std::env::temp_dir().join(format!("zshell-not-sessions-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"format":"something","version":1}"#).unwrap();
        let error = scan(&path, &[]).unwrap_err();
        assert_eq!(error.code(), "import.notSessionsFile");
        std::fs::remove_file(&path).unwrap();
    }
}
