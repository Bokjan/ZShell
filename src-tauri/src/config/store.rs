//! The store of the sessions, folders and proxies: queries and changes, saved through the vault.

use std::sync::{Arc, Mutex, RwLock};

use super::{Connection, Folder, Item, Profile, Protocol, Remote};
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::net::Route;
use crate::persist::unique_ids;
use crate::proxy::Proxy;
use crate::vault::{Account, Passwords, Vault};

pub struct ProfileStore {
    /// Where they are kept; empty while it is locked, and loaded again once unlocked.
    vault: Arc<Vault>,
    /// Read briefly by every query; replaced once a change is saved, so that queries (some
    /// on the main thread) never wait for the files to be written.
    state: RwLock<State>,
    /// Held by a change from reading the state to replacing it: one change at a time.
    writing: Mutex<()>,
}

/// The sessions, folders and proxies an import is planned against.
pub struct Snapshot<'a> {
    pub profiles: &'a [Profile],
    pub folders: &'a [Folder],
    pub proxies: &'a [Proxy],
}

/// What an import adds.
#[derive(Default)]
pub struct Additions {
    pub folders: Vec<Folder>,
    pub proxies: Vec<Proxy>,
    pub profiles: Vec<Profile>,
    /// The saved passwords of the new sessions and proxies.
    pub passwords: Passwords,
}

#[derive(Clone, Default)]
struct State {
    profiles: Vec<Profile>,
    folders: Vec<Folder>,
    proxies: Vec<Proxy>,
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

    /// Drops jump hosts that aren't other SSH sessions and breaks folder cycles (hand-edited
    /// files, an import).
    ///
    /// References to folders and proxies that don't exist are kept: the file they are in may
    /// have been set aside (see [`JsonFile::load`]), and they work again once it is restored.
    /// Until then a session whose folder is missing shows at the top level, and one whose proxy
    /// is missing fails to connect, rather than connecting without it.
    fn repair(&mut self) {
        let ids: std::collections::HashSet<String> = self.folders.iter().map(|f| f.id.clone()).collect();
        // Jump hosts must be other SSH sessions that exist (a hand-edited file, an import).
        let ssh: std::collections::HashSet<String> =
            self.profiles.iter().filter(|p| p.protocol() == Protocol::Ssh).map(|p| p.id.clone()).collect();
        for profile in &mut self.profiles {
            let id = profile.id.clone();
            if let Some(remote) = profile.connection.remote_mut() {
                let mut seen = std::collections::HashSet::new();
                remote.jump_hosts.retain(|jump| *jump != id && ssh.contains(jump) && seen.insert(jump.clone()));
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
    pub fn load(vault: Arc<Vault>) -> Self {
        let state = Self::read(&vault);
        Self { vault, state: RwLock::new(state), writing: Mutex::new(()) }
    }

    /// Takes what the vault has again, once it is unlocked or reset.
    pub fn reload(&self) {
        let _writing = self.writing.lock().unwrap();
        *self.state.write().unwrap() = Self::read(&self.vault);
    }

    fn read(vault: &Vault) -> State {
        let data = vault.data().unwrap_or_default();
        let mut state = State { profiles: data.profiles, folders: data.folders, proxies: data.proxies };
        unique_ids(state.profiles.iter_mut().map(|p| &mut p.id));
        unique_ids(state.folders.iter_mut().map(|f| &mut f.id));
        unique_ids(state.proxies.iter_mut().map(|p| &mut p.id));
        state.repair();
        state
    }

    pub fn list(&self) -> Vec<Profile> {
        self.state.read().unwrap().profiles.clone()
    }

    /// The sessions, folders and proxies as one consistent copy (for exporting them).
    pub fn snapshot(&self) -> (Vec<Profile>, Vec<Folder>, Vec<Proxy>) {
        let state = self.state.read().unwrap();
        (state.profiles.clone(), state.folders.clone(), state.proxies.clone())
    }

    pub fn folders(&self) -> Vec<Folder> {
        self.state.read().unwrap().folders.clone()
    }

    pub fn get(&self, id: &str) -> Result<Profile> {
        self.state.read().unwrap().profiles.iter().find(|p| p.id == id).cloned().ok_or_else(|| Error::new("profile.notFound"))
    }

    /// Inserts or updates a profile and returns it with its id filled in. A new profile goes
    /// into `profile.folder`; an existing one stays where it is. `password`: `None` keeps the
    /// saved password, `Some("")` deletes it.
    pub fn save(&self, mut profile: Profile, password: Option<&str>) -> Result<Profile> {
        profile.normalize_target()?;
        profile.normalize_session_options()?;

        self.update(|state, passwords| {
            let mut seen = std::collections::HashSet::new();
            for jump in profile.jump_hosts() {
                let ssh = state.profiles.iter().any(|p| p.id == *jump && p.protocol() == Protocol::Ssh);
                if !ssh || *jump == profile.id || !seen.insert(jump) {
                    return Err(Error::new("profile.invalidJumpHost"));
                }
            }
            if profile.proxy().is_some_and(|id| !state.proxies.iter().any(|p| p.id == id)) {
                return Err(Error::new("profile.invalidProxy"));
            }
            if profile.protocol() != Protocol::Ssh && !profile.id.is_empty() {
                let users = jump_host_users(state, &profile.id);
                if !users.is_empty() {
                    return Err(Error::new("profile.jumpHostNotSsh").param("names", users.join(", ")));
                }
            }
            let folder_exists = profile.folder.as_ref().is_some_and(|f| state.folder_exists(f));
            match state.profiles.iter_mut().find(|p| !profile.id.is_empty() && p.id == profile.id) {
                Some(existing) => {
                    // Kept while the session stays SSH.
                    let forwards = existing.forwards().to_vec();
                    if let Connection::Ssh(ssh) = &mut profile.connection {
                        ssh.forwards = forwards;
                    }
                    profile.folder = existing.folder.take();
                    *existing = profile.clone();
                }
                None => {
                    profile.id = uuid::Uuid::new_v4().to_string();
                    if let Connection::Ssh(ssh) = &mut profile.connection {
                        ssh.forwards.clear();
                    }
                    if !folder_exists {
                        profile.folder = None;
                    }
                    state.profiles.push(profile.clone());
                }
            }
            if let Some(password) = password {
                passwords.set(Account::Profile(&profile.id), Some(password).filter(|p| !p.is_empty()));
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
        self.update(|state, _| {
            let profile = state.profiles.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::new("profile.notFound"))?;
            match &mut profile.connection {
                Connection::Ssh(ssh) => ssh.forwards = forwards,
                _ => return Err(Error::new("profile.forwardsNeedSsh")),
            }
            Ok(profile.clone())
        })
    }

    /// A copy of a profile with its saved password, named `name`, right after it in the same
    /// folder.
    pub fn duplicate(&self, id: &str, name: &str) -> Result<Profile> {
        self.update(|state, passwords| {
            let index = state.profiles.iter().position(|p| p.id == id).ok_or_else(|| Error::new("profile.notFound"))?;
            let mut copy = state.profiles[index].clone();
            copy.id = uuid::Uuid::new_v4().to_string();
            copy.name = name.trim().to_owned();
            state.profiles.insert(index + 1, copy.clone());
            let password = passwords.get(Account::Profile(id)).map(str::to_owned);
            passwords.set(Account::Profile(&copy.id), password.as_deref());
            Ok(copy)
        })
    }

    /// How to reach `remote`: its jump hosts, and the proxy of the first connection.
    pub fn route(&self, remote: &Remote) -> Result<Route> {
        let jumps = remote
            .jump_hosts
            .iter()
            .map(|id| self.get(id).ok().and_then(|jump| jump.ssh_profile()).ok_or_else(|| Error::new("profile.invalidJumpHost")))
            .collect::<Result<Vec<_>>>()?;
        let first = jumps.first().map_or(remote, |jump| &jump.ssh.remote);
        let proxy = first.proxy.as_ref().map(|id| self.proxy(id).map_err(|_| Error::new("profile.invalidProxy"))).transpose()?;
        Ok(Route { jumps, proxy })
    }

    /// Adds what `plan` makes of the current sessions, folders and proxies in one write (an
    /// import: new folders, proxies and profiles that already have ids, and their passwords);
    /// returns the new profiles. Planning while the store is held for writing (queries go on) keeps two
    /// imports at once from both adding the same thing.
    pub fn add_all(&self, plan: impl FnOnce(&Snapshot) -> Result<Additions>) -> Result<Vec<Profile>> {
        self.update(|state, passwords| {
            let snapshot = Snapshot { profiles: &state.profiles, folders: &state.folders, proxies: &state.proxies };
            let Additions { folders, proxies, profiles, passwords: added } = plan(&snapshot)?;
            state.folders.extend(folders);
            state.proxies.extend(proxies);
            state.profiles.extend(profiles.iter().cloned());
            passwords.profiles.extend(added.profiles);
            passwords.proxies.extend(added.proxies);
            state.repair();
            Ok(profiles)
        })
    }

    pub fn proxies(&self) -> Vec<Proxy> {
        self.state.read().unwrap().proxies.clone()
    }

    pub fn proxy(&self, id: &str) -> Result<Proxy> {
        self.state.read().unwrap().proxies.iter().find(|p| p.id == id).cloned().ok_or_else(|| Error::new("proxy.notFound"))
    }

    /// Inserts or updates a proxy and returns it with its id filled in. `password`: `None`
    /// keeps the saved password, `Some("")` deletes it; a proxy without a user name keeps none.
    pub fn save_proxy(&self, mut proxy: Proxy, password: Option<&str>) -> Result<Proxy> {
        proxy.normalize()?;
        self.update(|state, passwords| {
            match state.proxies.iter_mut().find(|p| !proxy.id.is_empty() && p.id == proxy.id) {
                Some(existing) => *existing = proxy.clone(),
                None if !proxy.id.is_empty() => return Err(Error::new("proxy.notFound")),
                None => {
                    proxy.id = uuid::Uuid::new_v4().to_string();
                    state.proxies.push(proxy.clone());
                }
            }
            if let Some(password) = password {
                passwords.set(Account::Proxy(&proxy.id), Some(password).filter(|p| !p.is_empty()));
            }
            Ok(proxy)
        })
    }

    /// Deletes a proxy that no session uses, with its password.
    pub fn delete_proxy(&self, id: &str) -> Result<()> {
        self.update(|state, _| {
            let users: Vec<&str> = state.profiles.iter().filter(|p| p.proxy() == Some(id)).map(|p| p.name.as_str()).collect();
            if !users.is_empty() {
                return Err(Error::new("proxy.inUse").param("names", users.join(", ")));
            }
            state.proxies.retain(|p| p.id != id);
            Ok(())
        })
    }

    /// Deletes a session, with its password.
    pub fn delete(&self, id: &str) -> Result<()> {
        self.update(|state, _| {
            let users = jump_host_users(state, id);
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
        self.update(|state, _| {
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
        self.update(|state, _| {
            let index = state.folders.iter().position(|f| f.id == id).ok_or_else(|| Error::new("folder.notFound"))?;
            let folder = state.folders.remove(index);
            let is_child = |f: &Folder| f.parent.as_deref() == Some(id);
            let children: Vec<Folder> =
                state.folders.iter().filter(|f| is_child(f)).map(|f| Folder { parent: folder.parent.clone(), ..f.clone() }).collect();
            // Where the folder was, once the children before it are taken out too.
            let before = state.folders[..index].iter().filter(|f| is_child(f)).count();
            state.folders.retain(|f| !is_child(f));
            let at = index - before;
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
        self.update(|state, _| {
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

    /// Applies `change` to a copy of the state and the saved passwords and, if it succeeds,
    /// saves them in one write and keeps the copy. Passwords whose session or proxy is gone
    /// (or takes none) go with it.
    fn update<T>(&self, change: impl FnOnce(&mut State, &mut Passwords) -> Result<T>) -> Result<T> {
        let _writing = self.writing.lock().unwrap();
        let mut updated = self.state.read().unwrap().clone();
        let value = self.vault.update(|data| {
            let value = change(&mut updated, &mut data.passwords)?;
            data.passwords.retain_owners(&updated.profiles, &updated.proxies);
            data.folders.clone_from(&updated.folders);
            data.proxies.clone_from(&updated.proxies);
            data.profiles.clone_from(&updated.profiles);
            Ok(value)
        })?;
        *self.state.write().unwrap() = updated;
        Ok(value)
    }
}

/// The names of the profiles that use profile `id` as a jump host.
fn jump_host_users<'a>(state: &'a State, id: &str) -> Vec<&'a str> {
    state.profiles.iter().filter(|p| p.jump_hosts().iter().any(|j| j == id)).map(|p| p.name.as_str()).collect()
}
