//! Saved connection profiles, the folders they are organized in and the proxies they use,
//! persisted as JSON in the app config directory (`profiles.json`, `folders.json`,
//! `proxies.json`). Passwords are never stored here; see [`crate::secrets`].
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
use crate::net::Route;
use crate::proxy::Proxy;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Empty when creating a new profile; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Absent in files from before Telnet and serial sessions, which are all SSH.
    #[serde(default)]
    pub protocol: Protocol,
    /// SSH and Telnet; unused by serial sessions.
    pub host: String,
    pub port: u16,
    /// Required for SSH; for Telnet, optional and typed at the login prompt.
    pub username: String,
    /// SSH only. Telnet uses the saved password, if any, at the password prompt.
    pub auth: AuthMethod,
    /// The device and line settings of a serial session.
    #[serde(default, skip_serializing_if = "SerialOptions::is_default")]
    pub serial: SerialOptions,
    /// Profiles to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
    /// own profile's address and authentication, but not that profile's jump hosts. SSH
    /// profiles only; SSH and Telnet sessions can use them.
    #[serde(default)]
    pub jump_hosts: Vec<String>,
    /// The id of the proxy to connect through; SSH and Telnet. Not kept with jump hosts,
    /// where the first jump host's own proxy is used (see [`Route`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
    /// drop the connection. Telnet uses TCP keepalives.
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
    /// Let the remote shell use the local SSH agent (OpenSSH's `ForwardAgent`).
    #[serde(default)]
    pub forward_agent: bool,
    /// The remote side's character encoding, one of [`crate::encoding::SUPPORTED`].
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// The terminal type the remote shell is told (`TERM`).
    #[serde(default = "default_term_type")]
    pub term_type: String,
    /// Environment variables for the remote shell (OpenSSH's `SetEnv`); the server only
    /// accepts those its `AcceptEnv` allows.
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Commands typed into each new shell, in order, each once the shell shows a prompt.
    /// Sent by the frontend.
    #[serde(default)]
    pub login_commands: Vec<String>,
    /// Terminal appearance for this session; unset values follow the settings.
    #[serde(default, skip_serializing_if = "ProfileAppearance::is_empty")]
    pub appearance: ProfileAppearance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    #[default]
    Ssh,
    Telnet,
    Serial,
}

/// A serial line's settings: 115200 8N1 without flow control unless set otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SerialOptions {
    /// `/dev/cu.*` on macOS, `COM3` on Windows.
    pub device: String,
    pub baud_rate: u32,
    /// 5 to 8.
    pub data_bits: u8,
    pub parity: Parity,
    /// 1 or 2.
    pub stop_bits: u8,
    pub flow_control: FlowControl,
}

impl Default for SerialOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            baud_rate: 115_200,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow_control: FlowControl::None,
        }
    }
}

impl SerialOptions {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The line settings in the usual short form, `115200 8N1`.
    pub fn summary(&self) -> String {
        let parity = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        format!("{} {}{}{}", self.baud_rate, self.data_bits, parity, self.stop_bits)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlowControl {
    #[default]
    None,
    /// XON / XOFF.
    Software,
    /// RTS / CTS.
    Hardware,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Overrides of the terminal settings for one session. Only the frontend interprets them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAppearance {
    /// A built-in color scheme id, or "auto", as in the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
    /// Replaces the color scheme's background, as `#rrggbb` (a red one for production).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u16>,
}

impl ProfileAppearance {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Drops values that are blank or out of range.
    fn normalize(mut self) -> Self {
        let trimmed = |value: Option<String>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        self.color_scheme = trimmed(self.color_scheme);
        self.font_family = trimmed(self.font_family);
        self.background = trimmed(self.background).filter(|color| is_hex_color(color)).map(|c| c.to_ascii_lowercase());
        self.font_size = self.font_size.filter(|size| (crate::settings::FONT_SIZE_MIN..=crate::settings::FONT_SIZE_MAX).contains(size));
        self
    }
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Profile {
    /// A profile with the default settings, before it is saved (no id).
    pub fn new(name: String, host: String, port: u16, username: String) -> Self {
        Self {
            id: String::new(),
            name,
            protocol: Protocol::Ssh,
            host,
            port,
            username,
            auth: AuthMethod::Auto,
            serial: SerialOptions::default(),
            jump_hosts: Vec::new(),
            proxy: None,
            keepalive_interval: default_keepalive_interval(),
            auto_reconnect: true,
            forwards: Vec::new(),
            folder: None,
            auto_log: false,
            command_group: None,
            forward_agent: false,
            encoding: default_encoding(),
            term_type: default_term_type(),
            env: Vec::new(),
            login_commands: Vec::new(),
            appearance: ProfileAppearance::default(),
        }
    }

    /// Whether `other` connects to the same place: the same user, host and port (and
    /// protocol), or the same serial device.
    pub fn same_target(&self, other: &Profile) -> bool {
        self.protocol == other.protocol
            && match self.protocol {
                Protocol::Serial => self.serial.device == other.serial.device,
                _ => self.host == other.host && self.port == other.port && self.username == other.username,
            }
    }

    /// Checks and tidies the address, by protocol, and names an unnamed profile after it.
    fn normalize_target(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.serial.device = self.serial.device.trim().to_owned();
        match self.protocol {
            Protocol::Ssh if self.host.is_empty() || self.username.is_empty() || self.port == 0 => {
                return Err(Error::new("profile.missingFields"))
            }
            Protocol::Telnet if self.host.is_empty() || self.port == 0 => return Err(Error::new("profile.missingHost")),
            Protocol::Serial => {
                let serial = &self.serial;
                if serial.device.is_empty() {
                    return Err(Error::new("profile.missingDevice"));
                }
                if serial.baud_rate == 0 || !(5..=8).contains(&serial.data_bits) || !(1..=2).contains(&serial.stop_bits) {
                    return Err(Error::new("profile.invalidSerialSettings"));
                }
                // Jump hosts and proxies don't apply; keeping them would also keep those
                // from being deleted.
                self.jump_hosts.clear();
                self.proxy = None;
            }
            _ => {}
        }
        // The first jump host's proxy is used instead.
        if !self.jump_hosts.is_empty() {
            self.proxy = None;
        }
        if self.name.is_empty() {
            self.name = match self.protocol {
                Protocol::Ssh => format!("{}@{}", self.username, self.host),
                Protocol::Telnet => self.host.clone(),
                Protocol::Serial => self.serial.device.trim_start_matches("/dev/").to_owned(),
            };
        }
        Ok(())
    }

    /// Checks and tidies what the user edits beyond the address (encoding, terminal type,
    /// environment, login commands, appearance).
    fn normalize_session_options(&mut self) -> Result<()> {
        if crate::encoding::lookup(&self.encoding).is_none() {
            return Err(Error::new("profile.invalidEncoding").param("encoding", &self.encoding));
        }
        self.term_type = self.term_type.trim().to_owned();
        if self.term_type.is_empty() {
            self.term_type = default_term_type();
        }
        if !self.term_type.chars().all(|c| c.is_ascii_graphic()) {
            return Err(Error::new("profile.invalidTermType"));
        }
        for var in &mut self.env {
            var.name = var.name.trim().to_owned();
            if var.name.is_empty() || var.name.contains(['=', '\0']) || var.name.chars().any(char::is_whitespace) {
                return Err(Error::new("profile.invalidEnvName").param("name", &var.name));
            }
        }
        self.login_commands = std::mem::take(&mut self.login_commands)
            .into_iter()
            .map(|command| command.trim_end().to_owned())
            .filter(|command| !command.trim().is_empty())
            .collect();
        self.appearance = std::mem::take(&mut self.appearance).normalize();
        Ok(())
    }
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

fn default_encoding() -> String {
    crate::encoding::DEFAULT.to_owned()
}

pub fn default_term_type() -> String {
    "xterm-256color".to_owned()
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
    proxies_path: PathBuf,
    state: Mutex<State>,
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

    /// Drops references to folders and proxies that don't exist, and breaks cycles
    /// (hand-edited files).
    fn repair(&mut self) {
        let ids: std::collections::HashSet<String> = self.folders.iter().map(|f| f.id.clone()).collect();
        let proxies: std::collections::HashSet<&str> = self.proxies.iter().map(|p| p.id.as_str()).collect();
        for profile in &mut self.profiles {
            if profile.folder.as_ref().is_some_and(|f| !ids.contains(f)) {
                profile.folder = None;
            }
            if profile.proxy.as_ref().is_some_and(|p| !proxies.contains(p.as_str())) {
                profile.proxy = None;
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
    /// `path` is `profiles.json`; the folders and proxies are kept next to it.
    pub fn load(path: PathBuf) -> Result<Self> {
        let folders_path = path.with_file_name("folders.json");
        let proxies_path = path.with_file_name("proxies.json");
        let mut state = State { profiles: read_json(&path)?, folders: read_json(&folders_path)?, proxies: read_json(&proxies_path)? };
        state.repair();
        Ok(Self { path, folders_path, proxies_path, state: Mutex::new(state) })
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
        profile.normalize_target()?;
        profile.normalize_session_options()?;

        self.update(|state| {
            let mut seen = std::collections::HashSet::new();
            for jump in &profile.jump_hosts {
                let ssh = state.profiles.iter().any(|p| p.id == *jump && p.protocol == Protocol::Ssh);
                if !ssh || *jump == profile.id || !seen.insert(jump) {
                    return Err(Error::new("profile.invalidJumpHost"));
                }
            }
            if profile.proxy.as_ref().is_some_and(|id| !state.proxies.iter().any(|p| p.id == *id)) {
                return Err(Error::new("profile.invalidProxy"));
            }
            if profile.protocol != Protocol::Ssh && !profile.id.is_empty() {
                let users = jump_host_users(state, &profile.id);
                if !users.is_empty() {
                    return Err(Error::new("profile.jumpHostNotSsh").param("names", users.join(", ")));
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

    /// How to reach `profile`: its jump hosts, and the proxy of the first connection.
    pub fn route(&self, profile: &Profile) -> Result<Route> {
        let jumps = profile
            .jump_hosts
            .iter()
            .map(|id| self.get(id).map_err(|_| Error::new("profile.invalidJumpHost")))
            .collect::<Result<Vec<_>>>()?;
        let first = jumps.first().unwrap_or(profile);
        let proxy = first.proxy.as_ref().map(|id| self.proxy(id).map_err(|_| Error::new("profile.invalidProxy"))).transpose()?;
        Ok(Route { jumps, proxy })
    }

    /// Adds folders, proxies and profiles that already have ids (from an import) in one write.
    pub fn add_all(&self, folders: Vec<Folder>, proxies: Vec<Proxy>, profiles: Vec<Profile>) -> Result<()> {
        self.update(|state| {
            state.folders.extend(folders);
            state.proxies.extend(proxies);
            state.profiles.extend(profiles);
            state.repair();
            Ok(())
        })
    }

    pub fn proxies(&self) -> Vec<Proxy> {
        self.state.lock().unwrap().proxies.clone()
    }

    pub fn proxy(&self, id: &str) -> Result<Proxy> {
        self.state.lock().unwrap().proxies.iter().find(|p| p.id == id).cloned().ok_or_else(|| Error::new("proxy.notFound"))
    }

    /// Inserts or updates a proxy and returns it with its id filled in.
    pub fn save_proxy(&self, mut proxy: Proxy) -> Result<Proxy> {
        proxy.normalize()?;
        self.update(|state| {
            match state.proxies.iter_mut().find(|p| !proxy.id.is_empty() && p.id == proxy.id) {
                Some(existing) => *existing = proxy.clone(),
                None if !proxy.id.is_empty() => return Err(Error::new("proxy.notFound")),
                None => {
                    proxy.id = uuid::Uuid::new_v4().to_string();
                    state.proxies.push(proxy.clone());
                }
            }
            Ok(proxy)
        })
    }

    /// Deletes a proxy that no session uses.
    pub fn delete_proxy(&self, id: &str) -> Result<()> {
        self.update(|state| {
            let users: Vec<&str> = state.profiles.iter().filter(|p| p.proxy.as_deref() == Some(id)).map(|p| p.name.as_str()).collect();
            if !users.is_empty() {
                return Err(Error::new("proxy.inUse").param("names", users.join(", ")));
            }
            state.proxies.retain(|p| p.id != id);
            Ok(())
        })
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.update(|state| {
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
        if updated.proxies != state.proxies {
            write_json_atomic(&self.proxies_path, &updated.proxies)?;
        }
        write_json_atomic(&self.path, &updated.profiles)?;
        *state = updated;
        Ok(value)
    }
}

/// The names of the profiles that use profile `id` as a jump host.
fn jump_host_users<'a>(state: &'a State, id: &str) -> Vec<&'a str> {
    state.profiles.iter().filter(|p| p.jump_hosts.iter().any(|j| j == id)).map(|p| p.name.as_str()).collect()
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
            folder: folder.map(Into::into),
            ..Profile::new(name.into(), format!("{name}.example.com"), 22, "alice".into())
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

    #[test]
    fn reads_profiles_from_before_protocols() {
        let json = r#"[{"id":"p","name":"web","host":"web.example.com","port":22,"username":"alice","auth":{"type":"auto"}}]"#;
        let profiles: Vec<Profile> = serde_json::from_str(json).unwrap();
        assert_eq!(profiles[0].protocol, Protocol::Ssh);
        assert_eq!(profiles[0].serial, SerialOptions::default());
        // Default serial settings are not written for every profile.
        let written = serde_json::to_string(&profiles).unwrap();
        assert!(written.contains(r#""protocol":"ssh""#) && !written.contains("serial"));
    }

    #[test]
    fn checks_each_protocol_s_fields() {
        let (store, dir) = store("protocols");
        let telnet = |host: &str| Profile { protocol: Protocol::Telnet, ..Profile::new(String::new(), host.into(), 23, String::new()) };
        let switch = store.save(telnet("switch.lan")).unwrap();
        assert_eq!(switch.name, "switch.lan");
        assert_eq!(store.save(telnet("")).unwrap_err().code(), "profile.missingHost");

        let serial = |device: &str| Profile {
            protocol: Protocol::Serial,
            serial: SerialOptions { device: device.into(), ..SerialOptions::default() },
            ..Profile::new(String::new(), String::new(), 22, String::new())
        };
        let console = store.save(serial(" /dev/cu.usbserial-1410 ")).unwrap();
        assert_eq!((console.name.as_str(), console.serial.device.as_str()), ("cu.usbserial-1410", "/dev/cu.usbserial-1410"));
        assert_eq!(console.serial.summary(), "115200 8N1");
        assert_eq!(store.save(serial("")).unwrap_err().code(), "profile.missingDevice");
        let odd = Profile { serial: SerialOptions { data_bits: 9, ..console.serial.clone() }, ..console.clone() };
        assert_eq!(store.save(odd).unwrap_err().code(), "profile.invalidSerialSettings");

        // Only SSH sessions can be jump hosts, and a jump host stays SSH.
        let bastion = store.save(new_profile("bastion", None)).unwrap();
        let via_switch = Profile { jump_hosts: vec![switch.id.clone()], ..telnet("router.lan") };
        assert_eq!(store.save(via_switch).unwrap_err().code(), "profile.invalidJumpHost");
        let router = store.save(Profile { jump_hosts: vec![bastion.id.clone()], ..telnet("router.lan") }).unwrap();
        let bastion_telnet = Profile { protocol: Protocol::Telnet, ..bastion.clone() };
        assert_eq!(store.save(bastion_telnet).unwrap_err().code(), "profile.jumpHostNotSsh");
        // Serial sessions drop jump hosts.
        let serial_router = store.save(Profile { protocol: Protocol::Serial, ..serial("COM3") }).unwrap();
        assert!(serial_router.jump_hosts.is_empty());
        assert!(router.same_target(&Profile { name: "other".into(), ..router.clone() }));
        assert!(!router.same_target(&Profile { protocol: Protocol::Ssh, ..router.clone() }));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_proxies_and_routes_through_them() {
        let (store, dir) = store("proxies");
        let socks = Proxy {
            id: String::new(),
            name: "corp".into(),
            kind: crate::proxy::ProxyKind::Socks5,
            host: "proxy.lan".into(),
            port: 1080,
            username: String::new(),
            command: String::new(),
        };
        let socks = store.save_proxy(socks).unwrap();
        let missing = Profile { proxy: Some("missing".into()), ..new_profile("x", None) };
        assert_eq!(store.save(missing).unwrap_err().code(), "profile.invalidProxy");

        let bastion = store.save(Profile { proxy: Some(socks.id.clone()), ..new_profile("bastion", None) }).unwrap();
        // With jump hosts, the session's own proxy is dropped; the first jump host's is used.
        let db = Profile { jump_hosts: vec![bastion.id.clone()], proxy: Some(socks.id.clone()), ..new_profile("db", None) };
        let db = store.save(db).unwrap();
        assert_eq!(db.proxy, None);
        let route = store.route(&db).unwrap();
        assert_eq!((route.jumps.len(), route.proxy.as_ref().map(|p| p.id.as_str())), (1, Some(socks.id.as_str())));
        assert_eq!(store.route(&bastion).unwrap().proxy, Some(socks.clone()));
        assert!(store.route(&new_profile("direct", None)).unwrap().proxy.is_none());

        // A proxy in use cannot be deleted.
        let error = store.delete_proxy(&socks.id).unwrap_err();
        assert_eq!((error.code(), error.to_string().contains("bastion")), ("proxy.inUse", true));
        store.save(Profile { proxy: None, ..bastion }).unwrap();
        store.delete_proxy(&socks.id).unwrap();
        assert!(store.proxies().is_empty());
        assert_eq!(store.save_proxy(socks).unwrap_err().code(), "proxy.notFound");

        // Dangling references (hand-edited files) are dropped on load.
        let kept = store.save_proxy(Proxy { id: String::new(), kind: crate::proxy::ProxyKind::Command, command: "nc %h %p".into(), ..store_proxy() }).unwrap();
        assert_eq!(kept.name, "nc");
        fs::write(dir.join("profiles.json"), serde_json::to_vec(&[Profile { id: "p".into(), proxy: Some("gone".into()), ..new_profile("p", None) }]).unwrap()).unwrap();
        let reloaded = ProfileStore::load(dir.join("profiles.json")).unwrap();
        assert_eq!(reloaded.list()[0].proxy, None);
        assert_eq!(reloaded.proxies(), [kept]);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn store_proxy() -> Proxy {
        Proxy {
            id: String::new(),
            name: String::new(),
            kind: crate::proxy::ProxyKind::Http,
            host: String::new(),
            port: 0,
            username: String::new(),
            command: String::new(),
        }
    }
}
