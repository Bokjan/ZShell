//! Saved connection profiles and the folders they are organized in, persisted as JSON in the
//! app config directory (`profiles.json`, `folders.json`). Passwords are never stored here;
//! see [`crate::secrets`].
//!
//! The order of each file is the order shown: a folder's subfolders and sessions appear in
//! file order (subfolders first). `profiles.json` stays a plain array, so a profile's folder
//! is one more field and older versions still read the file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::forward::ForwardRule;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Empty when creating a new profile; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// Profiles to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
    /// own profile's address and authentication, but not that profile's jump hosts.
    #[serde(default)]
    pub jump_hosts: Vec<String>,
    /// Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
    /// drop the connection.
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval: u32,
    /// Reconnect automatically when an established connection is lost.
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    /// Edited separately through [`ProfileStore::set_forwards`]; `save` keeps them.
    #[serde(default)]
    pub forwards: Vec<ForwardRule>,
    /// The folder the session is in; `None` for the top level. Changed with
    /// [`ProfileStore::move_item`]; `save` keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Record a session log from the start of each connection.
    #[serde(default)]
    pub auto_log: bool,
    /// The quick command group its tabs show first; `None` for the default group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Empty when creating a new folder; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The folder this one is in; `None` for the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// A session or a folder, as dragged in the sidebar.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Profile { id: String },
    Folder { id: String },
}

fn default_keepalive_interval() -> u32 {
    30
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthMethod {
    /// Agent keys, default key files, then keyboard-interactive / password, like OpenSSH.
    Auto,
    Password,
    PublicKey { key_path: String },
    Agent,
}

pub struct ProfileStore {
    path: PathBuf,
    folders_path: PathBuf,
    state: Mutex<State>,
}

#[derive(Clone, Default)]
struct State {
    profiles: Vec<Profile>,
    folders: Vec<Folder>,
}

impl State {
    fn folder_exists(&self, id: &str) -> bool {
        self.folders.iter().any(|f| f.id == id)
    }

    /// Whether `folder` is `ancestor` or inside it.
    fn is_within(&self, folder: &str, ancestor: &str) -> bool {
        let mut current = Some(folder.to_owned());
        // Bounded, in case of a cycle in a hand-edited file.
        for _ in 0..=self.folders.len() {
            match current {
                Some(id) if id == ancestor => return true,
                Some(id) => current = self.folders.iter().find(|f| f.id == id).and_then(|f| f.parent.clone()),
                None => return false,
            }
        }
        false
    }

    /// Drops references to folders that don't exist, and breaks cycles (hand-edited files).
    fn repair(&mut self) {
        let ids: std::collections::HashSet<String> = self.folders.iter().map(|f| f.id.clone()).collect();
        for profile in &mut self.profiles {
            if profile.folder.as_ref().is_some_and(|f| !ids.contains(f)) {
                profile.folder = None;
            }
        }
        for i in 0..self.folders.len() {
            let parent = self.folders[i].parent.clone();
            let broken = match &parent {
                Some(p) => !ids.contains(p) || self.is_within(p, &self.folders[i].id),
                None => false,
            };
            if broken {
                self.folders[i].parent = None;
            }
        }
    }
}

impl ProfileStore {
    /// `path` is `profiles.json`; the folders are kept next to it.
    pub fn load(path: PathBuf) -> Result<Self> {
        let folders_path = path.with_file_name("folders.json");
        let mut state = State { profiles: read_json(&path)?, folders: read_json(&folders_path)? };
        state.repair();
        Ok(Self { path, folders_path, state: Mutex::new(state) })
    }

    pub fn list(&self) -> Vec<Profile> {
        self.state.lock().unwrap().profiles.clone()
    }

    pub fn folders(&self) -> Vec<Folder> {
        self.state.lock().unwrap().folders.clone()
    }

    pub fn get(&self, id: &str) -> Result<Profile> {
        self.state
            .lock()
            .unwrap()
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| Error::new("profile.notFound"))
    }

    /// Inserts or updates a profile and returns it with its id filled in. A new profile goes
    /// into `profile.folder`; an existing one stays where it is.
    pub fn save(&self, mut profile: Profile) -> Result<Profile> {
        profile.name = profile.name.trim().to_owned();
        profile.host = profile.host.trim().to_owned();
        profile.username = profile.username.trim().to_owned();
        if profile.host.is_empty() || profile.username.is_empty() || profile.port == 0 {
            return Err(Error::new("profile.missingFields"));
        }
        if profile.name.is_empty() {
            profile.name = format!("{}@{}", profile.username, profile.host);
        }

        self.update(|state| {
            let mut seen = std::collections::HashSet::new();
            for jump in &profile.jump_hosts {
                let known = state.profiles.iter().any(|p| p.id == *jump);
                if !known || *jump == profile.id || !seen.insert(jump) {
                    return Err(Error::new("profile.invalidJumpHost"));
                }
            }
            let folder_exists = profile.folder.as_ref().is_some_and(|f| state.folder_exists(f));
            match state.profiles.iter_mut().find(|p| !profile.id.is_empty() && p.id == profile.id) {
                Some(existing) => {
                    profile.forwards = std::mem::take(&mut existing.forwards);
                    profile.folder = existing.folder.take();
                    *existing = profile.clone();
                }
                None => {
                    profile.id = uuid::Uuid::new_v4().to_string();
                    profile.forwards.clear();
                    if !folder_exists {
                        profile.folder = None;
                    }
                    state.profiles.push(profile.clone());
                }
            }
            Ok(profile)
        })
    }

    /// Replaces a profile's port forwarding rules, assigning ids to new ones.
    pub fn set_forwards(&self, id: &str, forwards: Vec<ForwardRule>) -> Result<Profile> {
        let forwards = forwards
            .into_iter()
            .map(|rule| {
                let mut rule = rule.normalize()?;
                if rule.id.is_empty() {
                    rule.id = uuid::Uuid::new_v4().to_string();
                }
                Ok(rule)
            })
            .collect::<Result<Vec<_>>>()?;
        self.update(|state| {
            let profile = state.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::new("profile.notFound"))?;
            profile.forwards = forwards;
            Ok(profile.clone())
        })
    }

    /// A copy of a profile, named `name`, right after it in the same folder.
    pub fn duplicate(&self, id: &str, name: &str) -> Result<Profile> {
        self.update(|state| {
            let index = state.profiles.iter().position(|p| p.id == id).ok_or_else(|| Error::new("profile.notFound"))?;
            let mut copy = state.profiles[index].clone();
            copy.id = uuid::Uuid::new_v4().to_string();
            copy.name = name.trim().to_owned();
            state.profiles.insert(index + 1, copy.clone());
            Ok(copy)
        })
    }

    /// The profiles to connect through to reach `profile`.
    pub fn jump_hosts(&self, profile: &Profile) -> Result<Vec<Profile>> {
        profile.jump_hosts.iter().map(|id| self.get(id).map_err(|_| Error::new("profile.invalidJumpHost"))).collect()
    }

    /// Adds profiles and folders that already have ids (from an import) in one write.
    pub fn add_all(&self, folders: Vec<Folder>, profiles: Vec<Profile>) -> Result<()> {
        self.update(|state| {
            state.folders.extend(folders);
            state.profiles.extend(profiles);
            state.repair();
            Ok(())
        })
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.update(|state| {
            let users: Vec<&str> =
                state.profiles.iter().filter(|p| p.jump_hosts.iter().any(|j| j == id)).map(|p| p.name.as_str()).collect();
            if !users.is_empty() {
                return Err(Error::new("profile.usedAsJumpHost").param("names", users.join(", ")));
            }
            state.profiles.retain(|p| p.id != id);
            Ok(())
        })
    }

    /// Creates a folder (empty id, in `folder.parent`) or renames one.
    pub fn save_folder(&self, mut folder: Folder) -> Result<Folder> {
        folder.name = folder.name.trim().to_owned();
        if folder.name.is_empty() {
            return Err(Error::new("folder.emptyName"));
        }
        self.update(|state| {
            if folder.id.is_empty() {
                folder.id = uuid::Uuid::new_v4().to_string();
                if folder.parent.as_ref().is_some_and(|p| !state.folder_exists(p)) {
                    folder.parent = None;
                }
                state.folders.push(folder.clone());
                return Ok(folder);
            }
            let existing = state.folders.iter_mut().find(|f| f.id == folder.id).ok_or_else(|| Error::new("folder.notFound"))?;
            existing.name = folder.name;
            Ok(existing.clone())
        })
    }

    /// Deletes a folder; what it contains moves up into its parent, in its place.
    pub fn delete_folder(&self, id: &str) -> Result<()> {
        self.update(|state| {
            let index = state.folders.iter().position(|f| f.id == id).ok_or_else(|| Error::new("folder.notFound"))?;
            let folder = state.folders.remove(index);
            let children: Vec<Folder> = state
                .folders
                .iter()
                .filter(|f| f.parent.as_deref() == Some(id))
                .map(|f| Folder { parent: folder.parent.clone(), ..f.clone() })
                .collect();
            state.folders.retain(|f| f.parent.as_deref() != Some(id));
            let at = index.min(state.folders.len());
            state.folders.splice(at..at, children);
            for profile in state.profiles.iter_mut().filter(|p| p.folder.as_deref() == Some(id)) {
                profile.folder = folder.parent.clone();
            }
            Ok(())
        })
    }

    /// Moves a session or folder into `parent` (`None`: the top level), before the session
    /// or folder `before` (of the same kind), or after the others.
    pub fn move_item(&self, item: Item, parent: Option<String>, before: Option<String>) -> Result<()> {
        self.update(|state| {
            if parent.as_ref().is_some_and(|p| !state.folder_exists(p)) {
                return Err(Error::new("folder.notFound"));
            }
            match item {
                Item::Profile { id } => {
                    let index = state.profiles.iter().position(|p| p.id == id).ok_or_else(|| Error::new("profile.notFound"))?;
                    let mut profile = state.profiles.remove(index);
                    profile.folder = parent;
                    let at = before.and_then(|b| state.profiles.iter().position(|p| p.id == b)).unwrap_or(state.profiles.len());
                    state.profiles.insert(at, profile);
                }
                Item::Folder { id } => {
                    if parent.as_ref().is_some_and(|p| state.is_within(p, &id)) {
                        return Err(Error::new("folder.invalidMove"));
                    }
                    let index = state.folders.iter().position(|f| f.id == id).ok_or_else(|| Error::new("folder.notFound"))?;
                    let mut folder = state.folders.remove(index);
                    folder.parent = parent;
                    let at = before.and_then(|b| state.folders.iter().position(|f| f.id == b)).unwrap_or(state.folders.len());
                    state.folders.insert(at, folder);
                }
            }
            Ok(())
        })
    }

    /// Applies `change` to a copy of the state and, if it succeeds, saves the files that
    /// changed and keeps the copy.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        let mut updated = state.clone();
        let value = change(&mut updated)?;
        if updated.folders != state.folders {
            write_json_atomic(&self.folders_path, &updated.folders)?;
        }
        write_json_atomic(&self.path, &updated.profiles)?;
        *state = updated;
        Ok(value)
    }
}

/// A JSON file's contents, or the default if it doesn't exist yet.
fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

/// Writes `value` as pretty JSON, via a temporary file and a rename so that a crash never
/// leaves a truncated file behind.
pub fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (ProfileStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("zshell-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (ProfileStore::load(dir.join("profiles.json")).unwrap(), dir)
    }

    fn new_profile(name: &str, folder: Option<&str>) -> Profile {
        Profile {
            id: String::new(),
            name: name.into(),
            host: format!("{name}.example.com"),
            port: 22,
            username: "alice".into(),
            auth: AuthMethod::Auto,
            jump_hosts: Vec::new(),
            keepalive_interval: 30,
            auto_reconnect: true,
            forwards: Vec::new(),
            folder: folder.map(Into::into),
            command_group: None,
            auto_log: false,
        }
    }

    fn folder(name: &str, parent: Option<&str>) -> Folder {
        Folder { id: String::new(), name: name.into(), parent: parent.map(Into::into) }
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

        // Everything is on disk.
        let reloaded = ProfileStore::load(dir.join("profiles.json")).unwrap();
        assert_eq!(reloaded.list().len(), 2);
        assert!(reloaded.folders().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repairs_dangling_folder_references() {
        let (_, dir) = store("repair");
        fs::create_dir_all(&dir).unwrap();
        let profile = Profile { id: "p".into(), ..new_profile("p", Some("missing")) };
        fs::write(dir.join("profiles.json"), serde_json::to_vec(&[profile]).unwrap()).unwrap();
        let loops = [Folder { id: "x".into(), name: "x".into(), parent: Some("y".into()) }, Folder {
            id: "y".into(),
            name: "y".into(),
            parent: Some("x".into()),
        }];
        fs::write(dir.join("folders.json"), serde_json::to_vec(&loops).unwrap()).unwrap();
        let store = ProfileStore::load(dir.join("profiles.json")).unwrap();
        assert_eq!(store.list()[0].folder, None);
        assert!(store.folders().iter().any(|f| f.parent.is_none()));
        fs::remove_dir_all(&dir).unwrap();
    }
}
