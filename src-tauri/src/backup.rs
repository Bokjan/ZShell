//! Exporting sessions (with their folders) to a JSON file and importing them back, for
//! backups and for moving to another computer. Passwords stay in the system keychain and are
//! never exported.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{write_json_atomic, Folder, Profile};
use crate::error::{Error, Result};

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
}

/// A session in a file, as it would be imported.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// The names of the folders it is in, outermost first.
    pub folder: Vec<String>,
    /// The names of its jump hosts.
    pub jump_hosts: Vec<String>,
    /// The name of an existing session with the same name or address; such sessions are not
    /// imported again.
    pub existing: Option<String>,
}

pub fn export(path: &Path, folders: Vec<Folder>, profiles: Vec<Profile>) -> Result<()> {
    let file = SessionsFile { format: FORMAT.to_owned(), version: VERSION, folders, profiles };
    write_json_atomic(path, &file).map_err(|e| Error::new("export.writeFailed").param("path", path.display()).detail(e))
}

pub fn scan(path: &Path, existing: &[Profile]) -> Result<Vec<Candidate>> {
    let file = read(path)?;
    let names: HashMap<&str, &str> = file.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    Ok(file
        .profiles
        .iter()
        .map(|p| Candidate {
            id: p.id.clone(),
            name: p.name.clone(),
            host: p.host.clone(),
            port: p.port,
            username: p.username.clone(),
            folder: folder_path(&file.folders, p.folder.as_deref()),
            jump_hosts: p.jump_hosts.iter().filter_map(|j| names.get(j.as_str()).map(|n| n.to_string())).collect(),
            existing: duplicate_of(p, existing).map(|e| e.name.clone()),
        })
        .collect())
}

/// What to add for importing the sessions `selected` (ids in the file): new folders and
/// profiles, with new ids. Jump hosts come along unless they already exist here, in which
/// case the existing session is used; folders are matched by name and merged.
pub fn plan(path: &Path, selected: &[String], profiles: &[Profile], folders: &[Folder]) -> Result<(Vec<Folder>, Vec<Profile>)> {
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

    let mut merger = FolderMerger { file: &file.folders, existing: folders, added: Vec::new() };
    let new_profiles = file
        .profiles
        .iter()
        .filter(|p| imported.contains(&p.id))
        .map(|p| Profile {
            id: ids[&p.id].clone(),
            jump_hosts: p.jump_hosts.iter().filter_map(|j| ids.get(j).cloned()).collect(),
            folder: p.folder.as_deref().and_then(|f| merger.resolve(f)),
            ..p.clone()
        })
        .collect();
    Ok((merger.added, new_profiles))
}

fn read(path: &Path) -> Result<SessionsFile> {
    let bytes = std::fs::read(path).map_err(|e| Error::new("import.readFailed").param("path", path.display()).detail(e))?;
    let file: SessionsFile =
        serde_json::from_slice(&bytes).map_err(|e| Error::new("import.parseFailed").param("path", path.display()).detail(e))?;
    if file.format != FORMAT {
        return Err(Error::new("import.notSessionsFile").param("path", path.display()));
    }
    Ok(file)
}

/// An existing session with the same name, else one with the same user, host and port.
fn duplicate_of<'a>(profile: &Profile, existing: &'a [Profile]) -> Option<&'a Profile> {
    existing.iter().find(|e| e.name == profile.name).or_else(|| {
        existing.iter().find(|e| e.host == profile.host && e.port == profile.port && e.username == profile.username)
    })
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
        export(&path, folders, profiles).unwrap();

        // Here: a "Work" folder and the bastion already exist (same address, other name).
        let here_folders = vec![folder("w", "Work", None)];
        let here_profiles = vec![profile("b2", "jump", "bastion.example.com", None, &[])];
        let candidates = scan(&path, &here_profiles).unwrap();
        assert_eq!(candidates[0].existing.as_deref(), Some("jump"));
        assert_eq!(candidates[1].folder, ["Work", "DB"]);
        assert_eq!(candidates[1].jump_hosts, ["bastion"]);

        let (new_folders, new_profiles) = plan(&path, &["db".into()], &here_profiles, &here_folders).unwrap();
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

    #[test]
    fn rejects_other_files() {
        let path = std::env::temp_dir().join(format!("zshell-not-sessions-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"format":"something","version":1}"#).unwrap();
        let error = scan(&path, &[]).unwrap_err();
        assert_eq!(error.code(), "import.notSessionsFile");
        std::fs::remove_file(&path).unwrap();
    }
}
