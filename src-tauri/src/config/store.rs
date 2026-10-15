//! The store of the sessions, folders and proxies: queries and changes, saved through the vault.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};
use crate::model::ForwardRule;
use crate::model::Proxy;
use crate::model::{Connection, Folder, Item, Profile, Protocol, Remote};
use crate::net::Route;
use crate::persist::unique_ids;
use crate::vault::{Account, Data, Part, Passwords, SavedPasswords, Vault};

pub struct ProfileStore {
    /// Read briefly by every query; replaced once a change is saved, so that queries (some
    /// on the main thread) never wait for the files to be written. Empty while the vault is
    /// locked, and read again once it is unlocked.
    state: Part<State>,
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
    /// The new folders and sessions in the order they are shown, after the others; those left
    /// out follow.
    pub order: Vec<Item>,
    /// The saved passwords of the new sessions and proxies.
    pub passwords: Passwords,
}

#[derive(Clone, Default)]
struct State {
    profiles: Vec<Profile>,
    folders: Vec<Folder>,
    proxies: Vec<Proxy>,
    /// The order the sessions and folders are shown in, each once (see [`State::arrange`]).
    order: Vec<Item>,
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
        let ids: HashSet<String> = self.folders.iter().map(|f| f.id.clone()).collect();
        // Jump hosts must be other SSH sessions that exist (a hand-edited file, an import).
        let ssh: HashSet<String> = self.profiles.iter().filter(|p| p.protocol() == Protocol::Ssh).map(|p| p.id.clone()).collect();
        for profile in &mut self.profiles {
            let id = profile.id.clone();
            if let Some(remote) = profile.connection.remote_mut() {
                let mut seen = HashSet::new();
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

    /// Makes the order list each session and folder once: those it lacks (new, imported, or
    /// all of them in an older version's file) go last, folders first, and those that are
    /// gone leave it. Then sorts the lists of sessions and folders in that order, for older
    /// versions, which show them in file order.
    fn arrange(&mut self) {
        let exists: HashSet<Item> = self.items().collect();
        let mut seen = HashSet::new();
        self.order.retain(|item| exists.contains(item) && seen.insert(item.clone()));
        let missing: Vec<Item> = self.items().filter(|item| !seen.contains(item)).collect();
        self.order.extend(missing);
        let rank: HashMap<&Item, usize> = self.order.iter().enumerate().map(|(i, item)| (item, i)).collect();
        let folder_rank: Vec<usize> = self.folders.iter().map(|f| rank[&Item::Folder { id: f.id.clone() }]).collect();
        let profile_rank: Vec<usize> = self.profiles.iter().map(|p| rank[&Item::Profile { id: p.id.clone() }]).collect();
        sort_by_rank(&mut self.folders, folder_rank);
        sort_by_rank(&mut self.profiles, profile_rank);
    }

    /// The folders, then the sessions.
    fn items(&self) -> impl Iterator<Item = Item> + '_ {
        let folders = self.folders.iter().map(|f| Item::Folder { id: f.id.clone() });
        folders.chain(self.profiles.iter().map(|p| Item::Profile { id: p.id.clone() }))
    }

    /// Where `item` is in the order.
    fn position(&self, item: &Item) -> Option<usize> {
        self.order.iter().position(|i| i == item)
    }
}

/// Sorts `list` by the `rank` of each element.
fn sort_by_rank<T>(list: &mut Vec<T>, rank: Vec<usize>) {
    let mut ranked: Vec<(usize, T)> = rank.into_iter().zip(std::mem::take(list)).collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    list.extend(ranked.into_iter().map(|(_, value)| value));
}

impl ProfileStore {
    pub fn load(vault: Arc<Vault>) -> Self {
        Self { state: Part::new(vault, Self::read), writing: Mutex::new(()) }
    }

    fn read(data: Data) -> State {
        let mut state = State { profiles: data.profiles, folders: data.folders, proxies: data.proxies, order: data.order };
        unique_ids(state.profiles.iter_mut().map(|p| &mut p.id));
        unique_ids(state.folders.iter_mut().map(|f| &mut f.id));
        unique_ids(state.proxies.iter_mut().map(|p| &mut p.id));
        state.repair();
        state.arrange();
        state
    }

    pub fn list(&self) -> Vec<Profile> {
        self.state.read().profiles.clone()
    }

    /// The sessions, folders and proxies, and the order of the first two, as one consistent
    /// copy (for exporting them).
    pub fn snapshot(&self) -> (Vec<Profile>, Vec<Folder>, Vec<Proxy>, Vec<Item>) {
        let state = self.state.read();
        (state.profiles.clone(), state.folders.clone(), state.proxies.clone(), state.order.clone())
    }

    pub fn folders(&self) -> Vec<Folder> {
        self.state.read().folders.clone()
    }

    /// The order the sessions and folders are shown in: each folder's contents, sessions and
    /// subfolders mixed, appear in this order.
    pub fn order(&self) -> Vec<Item> {
        self.state.read().order.clone()
    }

    pub fn get(&self, id: &str) -> Result<Profile> {
        self.state.read().profiles.iter().find(|p| p.id == id).cloned().ok_or_else(|| Error::new("profile.notFound"))
    }

    /// Inserts or updates a profile and returns it with its id filled in. A new profile goes
    /// into `profile.folder`; an existing one stays where it is. `password`: `None` keeps the
    /// saved password, `Some("")` deletes it. A new profile has an empty id: an unknown one is
    /// a session deleted since it was opened for editing (in another window), which is not
    /// brought back.
    pub fn save(&self, mut profile: Profile, password: Option<&str>) -> Result<Profile> {
        profile.normalize_target()?;
        profile.normalize_session_options()?;

        self.update(|state, passwords| {
            let mut seen = HashSet::new();
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
                None if !profile.id.is_empty() => return Err(Error::new("profile.notFound")),
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
            let at = state.position(&Item::Profile { id: id.to_owned() }).map_or(state.order.len(), |i| i + 1);
            state.order.insert(at, Item::Profile { id: copy.id.clone() });
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
        Ok(Route { jumps, proxy, passwords: SavedPasswords::new(self.state.vault().clone()) })
    }

    /// Adds what `plan` makes of the current sessions, folders and proxies in one write (an
    /// import: new folders, proxies and profiles that already have ids, and their passwords);
    /// returns the new profiles. Planning while the store is held for writing (queries go on) keeps two
    /// imports at once from both adding the same thing.
    pub fn add_all(&self, plan: impl FnOnce(&Snapshot) -> Result<Additions>) -> Result<Vec<Profile>> {
        self.update(|state, passwords| {
            let snapshot = Snapshot { profiles: &state.profiles, folders: &state.folders, proxies: &state.proxies };
            let Additions { folders, proxies, profiles, order, passwords: added } = plan(&snapshot)?;
            state.folders.extend(folders);
            state.proxies.extend(proxies);
            state.profiles.extend(profiles.iter().cloned());
            state.order.extend(order);
            passwords.profiles.extend(added.profiles);
            passwords.proxies.extend(added.proxies);
            state.repair();
            Ok(profiles)
        })
    }

    pub fn proxies(&self) -> Vec<Proxy> {
        self.state.read().proxies.clone()
    }

    pub fn proxy(&self, id: &str) -> Result<Proxy> {
        self.state.read().proxies.iter().find(|p| p.id == id).cloned().ok_or_else(|| Error::new("proxy.notFound"))
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
            let mut children = HashSet::new();
            for child in state.folders.iter_mut().filter(|f| f.parent.as_deref() == Some(id)) {
                child.parent = folder.parent.clone();
                children.insert(Item::Folder { id: child.id.clone() });
            }
            for profile in state.profiles.iter_mut().filter(|p| p.folder.as_deref() == Some(id)) {
                profile.folder = folder.parent.clone();
                children.insert(Item::Profile { id: profile.id.clone() });
            }
            // In the order, the contents take the folder's place, wherever they were.
            let at = state.position(&Item::Folder { id: id.to_owned() }).unwrap_or(state.order.len());
            let before = state.order[..at].iter().filter(|item| children.contains(*item)).count();
            let moved: Vec<Item> = state.order.iter().filter(|item| children.contains(*item)).cloned().collect();
            state.order.retain(|item| !children.contains(item));
            let at = at - before;
            state.order.splice(at..at, moved);
            Ok(())
        })
    }

    /// Moves a session or folder into `parent` (`None`: the top level), before the session
    /// or folder `before`, or after the others.
    pub fn move_item(&self, item: Item, parent: Option<String>, before: Option<Item>) -> Result<()> {
        self.update(|state, _| {
            if parent.as_ref().is_some_and(|p| !state.folder_exists(p)) {
                return Err(Error::new("folder.notFound"));
            }
            match &item {
                Item::Profile { id } => {
                    let profile = state.profiles.iter_mut().find(|p| p.id == *id).ok_or_else(|| Error::new("profile.notFound"))?;
                    profile.folder = parent;
                }
                Item::Folder { id } => {
                    if parent.as_ref().is_some_and(|p| state.is_within(p, id)) {
                        return Err(Error::new("folder.invalidMove"));
                    }
                    let folder = state.folders.iter_mut().find(|f| f.id == *id).ok_or_else(|| Error::new("folder.notFound"))?;
                    folder.parent = parent;
                }
            }
            state.order.retain(|i| *i != item);
            let at = before.and_then(|b| state.position(&b)).unwrap_or(state.order.len());
            state.order.insert(at, item);
            Ok(())
        })
    }

    /// Applies `change` to a copy of the state and the saved passwords and, if it succeeds,
    /// saves them in one write and keeps the copy. Passwords whose session or proxy is gone
    /// (or takes none) go with it.
    fn update<T>(&self, change: impl FnOnce(&mut State, &mut Passwords) -> Result<T>) -> Result<T> {
        let _writing = self.writing.lock().unwrap();
        let mut updated = self.state.read().clone();
        let value = self.state.vault().update(|data| {
            let value = change(&mut updated, &mut data.passwords)?;
            updated.arrange();
            data.passwords.retain_owners(&updated.profiles, &updated.proxies);
            data.folders.clone_from(&updated.folders);
            data.proxies.clone_from(&updated.proxies);
            data.profiles.clone_from(&updated.profiles);
            data.order.clone_from(&updated.order);
            Ok(value)
        })?;
        self.state.replace(updated);
        Ok(value)
    }
}

/// The names of the profiles that use profile `id` as a jump host.
fn jump_host_users<'a>(state: &'a State, id: &str) -> Vec<&'a str> {
    state.profiles.iter().filter(|p| p.jump_hosts().iter().any(|j| j == id)).map(|p| p.name.as_str()).collect()
}
