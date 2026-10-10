//! Saved connection profiles, the folders they are organized in and the proxies they use,
//! persisted as JSON in the app config directory (`profiles.json`, `folders.json`,
//! `proxies.json`). Passwords are never stored here; see [`crate::secrets`].
//!
//! The order of each file is the order shown: a folder's subfolders and sessions appear in
//! file order (subfolders first).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::net::Route;
use crate::persist::{unique_ids, JsonFile, SetAside};
use crate::proxy::Proxy;

/// A saved session: what it connects to (by protocol), and how its tabs behave.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Empty when creating a new profile; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The folder the session is in; `None` for the top level. Changed with
    /// [`ProfileStore::move_item`]; `save` keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<String>,
    pub connection: Connection,
    /// Reconnect automatically when an established connection is lost, or when a serial
    /// device is plugged in again.
    #[serde(default)]
    pub auto_reconnect: bool,
    /// The remote side's character encoding, one of [`crate::encoding::SUPPORTED`].
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// Commands typed into each new shell, in order, each once the shell shows a prompt.
    /// Sent by the frontend.
    #[serde(default)]
    pub login_commands: Vec<String>,
    /// Record a session log from the start of each connection.
    #[serde(default)]
    pub auto_log: bool,
    /// The quick command group its tabs show first; `None` for the default group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command_group: Option<String>,
    /// Terminal appearance for this session; unset values follow the settings.
    #[serde(default, skip_serializing_if = "ProfileAppearance::is_empty")]
    pub appearance: ProfileAppearance,
}

/// What a session connects to, with the settings of its protocol.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum Connection {
    Ssh(SshOptions),
    Telnet(Remote),
    Serial(SerialOptions),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Ssh,
    Telnet,
    Serial,
}

/// What SSH and Telnet sessions have in common: where they connect, through what, and the
/// terminal type the remote side is told.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub host: String,
    pub port: u16,
    /// Required for SSH; for Telnet, optional and typed at the login prompt.
    #[serde(default)]
    pub username: String,
    /// Sessions to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
    /// own session's address and authentication, but not that session's jump hosts. Only
    /// SSH sessions can be jump hosts.
    #[serde(default)]
    pub jump_hosts: Vec<String>,
    /// The id of the proxy to connect through. Not kept with jump hosts, where the first jump
    /// host's own proxy is used (see [`Route`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub proxy: Option<String>,
    /// Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
    /// drop an SSH connection; Telnet uses TCP keepalives.
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval: u32,
    /// The terminal type the remote side is told (`TERM`, Telnet's terminal type option).
    #[serde(default = "default_term_type")]
    pub term_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SshOptions {
    #[serde(flatten)]
    pub remote: Remote,
    #[serde(default)]
    pub auth: AuthMethod,
    /// Let the remote shell use the local SSH agent (OpenSSH's `ForwardAgent`).
    #[serde(default)]
    pub forward_agent: bool,
    /// Environment variables for the remote shell (OpenSSH's `SetEnv`); the server only
    /// accepts those its `AcceptEnv` allows.
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Edited separately through [`ProfileStore::set_forwards`]; `save` keeps them.
    #[serde(default)]
    pub forwards: Vec<ForwardRule>,
}

/// What the SSH backend connects with: an SSH session's options, with the session's id (for
/// its saved password and forwarding rules; empty for a quick connection), name (of a jump
/// host, in messages) and character encoding (of SFTP file names).
#[derive(Clone, Debug)]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub encoding: String,
    pub ssh: SshOptions,
}

impl SshProfile {
    /// A quick connection to `user@host:port`, with automatic authentication.
    pub fn quick(name: String, remote: Remote) -> Self {
        Self { id: String::new(), name, encoding: default_encoding(), ssh: SshOptions::new(remote) }
    }
}

impl Remote {
    pub fn new(host: String, port: u16, username: String) -> Self {
        Self {
            host,
            port,
            username,
            jump_hosts: Vec::new(),
            proxy: None,
            keepalive_interval: default_keepalive_interval(),
            term_type: default_term_type(),
        }
    }
}

impl Remote {
    /// Checks and tidies the address; SSH also needs a user name.
    fn normalize(&mut self, ssh: bool) -> Result<()> {
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        if self.host.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(Error::new("profile.invalidHost"));
        }
        if self.username.chars().any(char::is_control) {
            return Err(Error::new("profile.invalidUser"));
        }
        if ssh && (self.host.is_empty() || self.username.is_empty() || self.port == 0) {
            return Err(Error::new("profile.missingFields"));
        }
        if self.host.is_empty() || self.port == 0 {
            return Err(Error::new("profile.missingHost"));
        }
        // The first jump host's proxy is used instead.
        if !self.jump_hosts.is_empty() {
            self.proxy = None;
        }
        Ok(())
    }
}

impl SshOptions {
    /// Automatic authentication, nothing forwarded.
    pub fn new(remote: Remote) -> Self {
        Self { remote, auth: AuthMethod::Auto, forward_agent: false, env: Vec::new(), forwards: Vec::new() }
    }
}

impl Connection {
    pub fn protocol(&self) -> Protocol {
        match self {
            Connection::Ssh(_) => Protocol::Ssh,
            Connection::Telnet(_) => Protocol::Telnet,
            Connection::Serial(_) => Protocol::Serial,
        }
    }

    /// The address and route of an SSH or Telnet session.
    pub fn remote(&self) -> Option<&Remote> {
        match self {
            Connection::Ssh(ssh) => Some(&ssh.remote),
            Connection::Telnet(remote) => Some(remote),
            Connection::Serial(_) => None,
        }
    }

    pub fn remote_mut(&mut self) -> Option<&mut Remote> {
        match self {
            Connection::Ssh(ssh) => Some(&mut ssh.remote),
            Connection::Telnet(remote) => Some(remote),
            Connection::Serial(_) => None,
        }
    }
}

/// A serial line's settings: 115200 8N1 without flow control unless set otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FlowControl {
    #[default]
    None,
    /// XON / XOFF.
    Software,
    /// RTS / CTS.
    Hardware,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Overrides of the terminal settings for one session. Only the frontend interprets them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAppearance {
    /// A built-in color scheme id, or "auto", as in the settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color_scheme: Option<String>,
    /// Replaces the color scheme's background, as `#rrggbb` (a red one for production).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
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
    pub fn new(name: String, connection: Connection) -> Self {
        Self {
            id: String::new(),
            name,
            folder: None,
            connection,
            auto_reconnect: true,
            encoding: default_encoding(),
            login_commands: Vec::new(),
            auto_log: false,
            command_group: None,
            appearance: ProfileAppearance::default(),
        }
    }

    pub fn protocol(&self) -> Protocol {
        self.connection.protocol()
    }

    /// The address and route of an SSH or Telnet session.
    pub fn remote(&self) -> Option<&Remote> {
        self.connection.remote()
    }

    pub fn ssh(&self) -> Option<&SshOptions> {
        match &self.connection {
            Connection::Ssh(ssh) => Some(ssh),
            _ => None,
        }
    }

    /// What the SSH backend connects with; `None` for other protocols.
    pub fn ssh_profile(&self) -> Option<SshProfile> {
        let ssh = self.ssh()?.clone();
        Some(SshProfile { id: self.id.clone(), name: self.name.clone(), encoding: self.encoding.clone(), ssh })
    }

    pub fn jump_hosts(&self) -> &[String] {
        self.remote().map_or(&[], |remote| &remote.jump_hosts)
    }

    pub fn proxy(&self) -> Option<&str> {
        self.remote().and_then(|remote| remote.proxy.as_deref())
    }

    pub fn forwards(&self) -> &[ForwardRule] {
        self.ssh().map_or(&[], |ssh| &ssh.forwards)
    }

    /// Whether `other` connects to the same place: the same user, host and port (and
    /// protocol), or the same serial device.
    pub fn same_target(&self, other: &Profile) -> bool {
        match (&self.connection, &other.connection) {
            (Connection::Serial(a), Connection::Serial(b)) => a.device == b.device,
            (a, b) if a.protocol() == b.protocol() => match (a.remote(), b.remote()) {
                (Some(a), Some(b)) => a.host == b.host && a.port == b.port && a.username == b.username,
                _ => false,
            },
            _ => false,
        }
    }

    /// Checks and tidies the address, by protocol, and names an unnamed profile after it.
    fn normalize_target(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        match &mut self.connection {
            Connection::Serial(serial) => {
                serial.device = serial.device.trim().to_owned();
                if serial.device.is_empty() {
                    return Err(Error::new("profile.missingDevice"));
                }
                if serial.baud_rate == 0 || !(5..=8).contains(&serial.data_bits) || !(1..=2).contains(&serial.stop_bits) {
                    return Err(Error::new("profile.invalidSerialSettings"));
                }
            }
            Connection::Ssh(ssh) => ssh.remote.normalize(true)?,
            Connection::Telnet(remote) => remote.normalize(false)?,
        }
        if self.name.is_empty() {
            self.name = match &self.connection {
                Connection::Ssh(ssh) => format!("{}@{}", ssh.remote.username, ssh.remote.host),
                Connection::Telnet(remote) => remote.host.clone(),
                Connection::Serial(serial) => serial.device.trim_start_matches("/dev/").to_owned(),
            };
        }
        Ok(())
    }

    /// Checks and tidies a session from an import file as saving does, with its forwarding
    /// rules (which get ids of their own): the file may have been edited, or written by
    /// someone else.
    pub fn normalize_imported(&mut self) -> Result<()> {
        self.normalize_target()?;
        self.normalize_session_options()?;
        if let Connection::Ssh(ssh) = &mut self.connection {
            ssh.forwards = std::mem::take(&mut ssh.forwards)
                .into_iter()
                .map(|rule| ForwardRule { id: uuid::Uuid::new_v4().to_string(), ..rule }.normalize())
                .collect::<Result<_>>()?;
        }
        Ok(())
    }

    /// Checks and tidies what the user edits beyond the address (encoding, terminal type,
    /// environment, login commands, appearance).
    fn normalize_session_options(&mut self) -> Result<()> {
        if crate::encoding::lookup(&self.encoding).is_none() {
            return Err(Error::new("profile.invalidEncoding").param("encoding", &self.encoding));
        }
        if let Some(remote) = self.connection.remote_mut() {
            remote.term_type = remote.term_type.trim().to_owned();
            if remote.term_type.is_empty() {
                remote.term_type = default_term_type();
            }
            if !remote.term_type.chars().all(|c| c.is_ascii_graphic()) {
                return Err(Error::new("profile.invalidTermType"));
            }
        }
        if let Connection::Ssh(ssh) = &mut self.connection {
            for var in &mut ssh.env {
                var.name = var.name.trim().to_owned();
                if var.name.is_empty() || var.name.contains(['=', '\0']) || var.name.chars().any(char::is_whitespace) {
                    return Err(Error::new("profile.invalidEnvName").param("name", &var.name));
                }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Empty when creating a new folder; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The folder this one is in; `None` for the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent: Option<String>,
}

/// A session or a folder, as dragged in the sidebar.
#[derive(Clone, Debug, Deserialize, TS)]
#[ts(rename = "TreeItem")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Profile { id: String },
    Folder { id: String },
}

fn default_keepalive_interval() -> u32 {
    30
}

fn default_encoding() -> String {
    crate::encoding::DEFAULT.to_owned()
}

pub fn default_term_type() -> String {
    "xterm-256color".to_owned()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthMethod {
    /// Agent keys, default key files, then keyboard-interactive / password, like OpenSSH.
    #[default]
    Auto,
    Password,
    PublicKey { key_path: String },
    Agent,
}

pub struct ProfileStore {
    profiles_file: JsonFile,
    folders_file: JsonFile,
    proxies_file: JsonFile,
    state: Mutex<State>,
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
    /// `path` is `profiles.json`; the folders and proxies are kept next to it.
    pub fn load(path: PathBuf, set_aside: &SetAside) -> Self {
        let (profiles_file, profiles) = JsonFile::load(path.clone(), set_aside);
        let (folders_file, folders) = JsonFile::load(path.with_file_name("folders.json"), set_aside);
        let (proxies_file, proxies) = JsonFile::load(path.with_file_name("proxies.json"), set_aside);
        let mut state = State { profiles, folders, proxies };
        unique_ids(state.profiles.iter_mut().map(|p| &mut p.id));
        unique_ids(state.folders.iter_mut().map(|f| &mut f.id));
        unique_ids(state.proxies.iter_mut().map(|p| &mut p.id));
        state.repair();
        Self { profiles_file, folders_file, proxies_file, state: Mutex::new(state) }
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
            match &mut profile.connection {
                Connection::Ssh(ssh) => ssh.forwards = forwards,
                _ => return Err(Error::new("profile.forwardsNeedSsh")),
            }
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
    /// import: new folders, proxies and profiles that already have ids); returns the new
    /// profiles. Planning under the store's lock keeps two imports at once from both adding
    /// the same thing.
    pub fn add_all(&self, plan: impl FnOnce(&Snapshot) -> Result<Additions>) -> Result<Vec<Profile>> {
        self.update(|state| {
            let snapshot = Snapshot { profiles: &state.profiles, folders: &state.folders, proxies: &state.proxies };
            let Additions { folders, proxies, profiles } = plan(&snapshot)?;
            state.folders.extend(folders);
            state.proxies.extend(proxies);
            state.profiles.extend(profiles.iter().cloned());
            state.repair();
            Ok(profiles)
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
            let users: Vec<&str> = state.profiles.iter().filter(|p| p.proxy() == Some(id)).map(|p| p.name.as_str()).collect();
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
    ///
    /// Every file is written before any is put in place, so a failed write (a full disk)
    /// leaves all of them as they were, matching the state kept. Only a rename failing
    /// part way through can still leave them apart; loading repairs references across them.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let mut state = self.state.lock().unwrap();
        let mut updated = state.clone();
        let value = change(&mut updated)?;
        let mut staged = Vec::new();
        if updated.folders != state.folders {
            staged.push(self.folders_file.stage(&updated.folders)?);
        }
        if updated.proxies != state.proxies {
            staged.push(self.proxies_file.stage(&updated.proxies)?);
        }
        if serde_json::to_value(&updated.profiles)? != serde_json::to_value(&state.profiles)? {
            staged.push(self.profiles_file.stage(&updated.profiles)?);
        }
        for file in staged {
            file.commit()?;
        }
        *state = updated;
        Ok(value)
    }
}

/// The names of the profiles that use profile `id` as a jump host.
fn jump_host_users<'a>(state: &'a State, id: &str) -> Vec<&'a str> {
    state.profiles.iter().filter(|p| p.jump_hosts().iter().any(|j| j == id)).map(|p| p.name.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn store(name: &str) -> (ProfileStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("zshell-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (ProfileStore::load(dir.join("profiles.json"), &SetAside::default()), dir)
    }

    fn new_profile(name: &str, folder: Option<&str>) -> Profile {
        let ssh = SshOptions::new(Remote::new(format!("{name}.example.com"), 22, "alice".into()));
        Profile { folder: folder.map(Into::into), ..Profile::new(name.into(), Connection::Ssh(ssh)) }
    }

    /// `profile` with its address or route changed.
    fn with_remote(mut profile: Profile, change: impl FnOnce(&mut Remote)) -> Profile {
        change(profile.connection.remote_mut().unwrap());
        profile
    }

    fn folder(name: &str, parent: Option<&str>) -> Folder {
        Folder { id: String::new(), name: name.into(), parent: parent.map(Into::into) }
    }

    /// An unreadable file is moved aside instead of stopping the app or being overwritten.
    #[test]
    fn sets_aside_unreadable_files() {
        let (_, dir) = store("set-aside");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profiles.json"), b"").unwrap();
        fs::write(dir.join("proxies.json"), br#"[{"id": "x", "type": "fromTheFuture"}]"#).unwrap();
        let set_aside = SetAside::default();
        let store = ProfileStore::load(dir.join("profiles.json"), &set_aside);
        assert!(store.list().is_empty());
        let files = set_aside.take();
        assert_eq!(files.len(), 2, "{files:?}");
        for file in &files {
            assert!(!file.path.exists() && file.moved_to.as_ref().is_some_and(|p| p.exists()), "{file:?}");
        }
        assert!(set_aside.take().is_empty());
        store.save(new_profile("a", None)).unwrap();
        assert_eq!(fs::read(files[0].moved_to.as_ref().unwrap()).unwrap(), b"");
        fs::remove_dir_all(&dir).unwrap();
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
        let reloaded = ProfileStore::load(dir.join("profiles.json"), &SetAside::default());
        assert_eq!(reloaded.list().len(), 2);
        assert!(reloaded.folders().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A change that has to write two files writes neither if the second can't be written.
    #[test]
    fn a_failed_write_leaves_every_file_as_it_was() {
        let (store, dir) = store("all-or-none");
        let work = store.save_folder(folder("Work", None)).unwrap();
        store.save(new_profile("a", Some(&work.id))).unwrap();
        let folders = fs::read(dir.join("folders.json")).unwrap();
        let profiles = fs::read(dir.join("profiles.json")).unwrap();

        // Deleting the folder rewrites folders.json and then profiles.json, which can't be.
        fs::create_dir(dir.join("profiles.json.tmp")).unwrap();
        assert_eq!(store.delete_folder(&work.id).unwrap_err().code(), "config.writeFailed");
        assert_eq!(fs::read(dir.join("folders.json")).unwrap(), folders);
        assert_eq!(fs::read(dir.join("profiles.json")).unwrap(), profiles);
        assert!(!dir.join("folders.json.tmp").exists());
        assert_eq!(store.folders().len(), 1);

        fs::remove_dir(dir.join("profiles.json.tmp")).unwrap();
        store.delete_folder(&work.id).unwrap();
        let reloaded = ProfileStore::load(dir.join("profiles.json"), &SetAside::default());
        assert!(reloaded.folders().is_empty() && reloaded.list()[0].folder.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Folder cycles are broken; a session's missing folder is kept (see `State::repair`).
    #[test]
    fn repairs_folder_cycles() {
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
        let store = ProfileStore::load(dir.join("profiles.json"), &SetAside::default());
        assert_eq!(store.list()[0].folder.as_deref(), Some("missing"));
        assert!(store.folders().iter().any(|f| f.parent.is_none()));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A session keeps the settings of its protocol under `connection`. Files from before
    /// 2.0, with every setting at the top level, are set aside rather than read.
    #[test]
    fn keeps_protocol_settings_under_connection() {
        let written = serde_json::to_value(new_profile("web", None)).unwrap();
        assert_eq!(written["connection"]["protocol"], "ssh");
        assert_eq!(written["connection"]["host"], "web.example.com");
        assert_eq!(written["connection"]["auth"]["type"], "auto");
        assert!(written.get("host").is_none() && written.get("protocol").is_none());

        let (_, dir) = store("before-2.0");
        fs::create_dir_all(&dir).unwrap();
        let old = r#"[{"id":"p","name":"web","host":"web.example.com","port":22,"username":"alice","auth":{"type":"auto"}}]"#;
        fs::write(dir.join("profiles.json"), old).unwrap();
        let set_aside = SetAside::default();
        assert!(ProfileStore::load(dir.join("profiles.json"), &set_aside).list().is_empty());
        assert_eq!(set_aside.take().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checks_each_protocol_s_fields() {
        let (store, dir) = store("protocols");
        let telnet = |host: &str| Profile::new(String::new(), Connection::Telnet(Remote::new(host.into(), 23, String::new())));
        let switch = store.save(telnet("switch.lan")).unwrap();
        assert_eq!(switch.name, "switch.lan");
        assert_eq!(store.save(telnet("")).unwrap_err().code(), "profile.missingHost");

        let serial = |device: &str| {
            Profile::new(String::new(), Connection::Serial(SerialOptions { device: device.into(), ..SerialOptions::default() }))
        };
        let console = store.save(serial(" /dev/cu.usbserial-1410 ")).unwrap();
        let Connection::Serial(line) = &console.connection else { panic!("{console:?}") };
        assert_eq!((console.name.as_str(), line.device.as_str()), ("cu.usbserial-1410", "/dev/cu.usbserial-1410"));
        assert_eq!(line.summary(), "115200 8N1");
        assert_eq!(store.save(serial("")).unwrap_err().code(), "profile.missingDevice");
        let odd = Profile { connection: Connection::Serial(SerialOptions { data_bits: 9, ..line.clone() }), ..console.clone() };
        assert_eq!(store.save(odd).unwrap_err().code(), "profile.invalidSerialSettings");

        // Only SSH sessions can be jump hosts, and a jump host stays SSH.
        let bastion = store.save(new_profile("bastion", None)).unwrap();
        let via_switch = with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![switch.id.clone()]);
        assert_eq!(store.save(via_switch).unwrap_err().code(), "profile.invalidJumpHost");
        let router = store.save(with_remote(telnet("router.lan"), |remote| remote.jump_hosts = vec![bastion.id.clone()])).unwrap();
        let bastion_telnet = Profile { connection: Connection::Telnet(bastion.remote().unwrap().clone()), ..bastion.clone() };
        assert_eq!(store.save(bastion_telnet).unwrap_err().code(), "profile.jumpHostNotSsh");
        assert!(router.same_target(&Profile { name: "other".into(), ..router.clone() }));
        let router_ssh = Profile { connection: Connection::Ssh(SshOptions::new(router.remote().unwrap().clone())), ..router.clone() };
        assert!(!router.same_target(&router_ssh));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Forwarding rules are edited on their own, kept when the session is saved, and go when
    /// it stops being an SSH session.
    #[test]
    fn keeps_forwarding_rules_of_ssh_sessions() {
        let (store, dir) = store("forwards");
        let web = store.save(new_profile("web", None)).unwrap();
        let rule = ForwardRule {
            id: String::new(),
            kind: crate::forward::ForwardKind::Local,
            bind_host: String::new(),
            bind_port: 8080,
            target_host: "localhost".into(),
            target_port: 80,
            description: String::new(),
            auto_start: false,
        };
        let web = store.set_forwards(&web.id, vec![rule]).unwrap();
        assert!(!web.forwards()[0].id.is_empty());
        // The dialog sends no rules.
        let renamed = store.save(Profile { id: web.id.clone(), name: "renamed".into(), ..new_profile("web", None) }).unwrap();
        assert_eq!(renamed.forwards().len(), 1);

        let telnet = Profile { connection: Connection::Telnet(web.remote().unwrap().clone()), ..web.clone() };
        assert!(store.save(telnet).unwrap().forwards().is_empty());
        assert_eq!(store.set_forwards(&web.id, Vec::new()).unwrap_err().code(), "profile.forwardsNeedSsh");
        let ssh_again = store.save(Profile { connection: web.connection.clone(), ..web.clone() }).unwrap();
        assert!(ssh_again.forwards().is_empty());
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
        let missing = with_remote(new_profile("x", None), |remote| remote.proxy = Some("missing".into()));
        assert_eq!(store.save(missing).unwrap_err().code(), "profile.invalidProxy");

        let bastion = store.save(with_remote(new_profile("bastion", None), |remote| remote.proxy = Some(socks.id.clone()))).unwrap();
        // With jump hosts, the session's own proxy is dropped; the first jump host's is used.
        let db = with_remote(new_profile("db", None), |remote| {
            remote.jump_hosts = vec![bastion.id.clone()];
            remote.proxy = Some(socks.id.clone());
        });
        let db = store.save(db).unwrap();
        assert_eq!(db.proxy(), None);
        let route = store.route(db.remote().unwrap()).unwrap();
        assert_eq!((route.jumps.len(), route.proxy.as_ref().map(|p| p.id.as_str())), (1, Some(socks.id.as_str())));
        assert_eq!(store.route(bastion.remote().unwrap()).unwrap().proxy, Some(socks.clone()));
        assert!(store.route(new_profile("direct", None).remote().unwrap()).unwrap().proxy.is_none());

        // A proxy in use cannot be deleted.
        let error = store.delete_proxy(&socks.id).unwrap_err();
        assert_eq!((error.code(), error.to_string().contains("bastion")), ("proxy.inUse", true));
        store.save(with_remote(bastion, |remote| remote.proxy = None)).unwrap();
        store.delete_proxy(&socks.id).unwrap();
        assert!(store.proxies().is_empty());
        assert_eq!(store.save_proxy(socks).unwrap_err().code(), "proxy.notFound");

        // Dangling references (hand-edited files) are kept, and fail to connect.
        let kept = store.save_proxy(Proxy { id: String::new(), kind: crate::proxy::ProxyKind::Command, command: "nc %h %p".into(), ..store_proxy() }).unwrap();
        assert_eq!(kept.name, "nc");
        let dangling = with_remote(Profile { id: "p".into(), ..new_profile("p", None) }, |remote| remote.proxy = Some("gone".into()));
        fs::write(dir.join("profiles.json"), serde_json::to_vec(&[dangling]).unwrap()).unwrap();
        let reloaded = ProfileStore::load(dir.join("profiles.json"), &SetAside::default());
        assert_eq!(reloaded.list()[0].proxy(), Some("gone"));
        assert_eq!(reloaded.route(reloaded.list()[0].remote().unwrap()).err().map(|e| e.code()), Some("profile.invalidProxy"));
        assert_eq!(reloaded.proxies(), [kept]);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// What a hand-written file may leave out or repeat: ids (references go to the first) and
    /// the settings that have defaults.
    #[test]
    fn reads_untidy_files() {
        let (_, dir) = store("untidy");
        fs::create_dir_all(&dir).unwrap();
        let connection = r#"{"protocol": "telnet", "host": "router.lan", "port": 23, "username": ""}"#;
        let profiles = format!(r#"[{{"id": "a", "name": "one", "connection": {connection}}}, {{"id": "a", "name": "two", "connection": {connection}}}, {{"name": "three", "connection": {connection}}}]"#);
        fs::write(dir.join("profiles.json"), profiles).unwrap();
        let set_aside = SetAside::default();
        let store = ProfileStore::load(dir.join("profiles.json"), &set_aside);
        assert!(set_aside.take().is_empty());
        let list = store.list();
        let ids: std::collections::HashSet<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!((list.len(), ids.len(), list[0].id.as_str()), (3, 3, "a"));
        assert_eq!((list[1].encoding.as_str(), list[1].auto_reconnect), (crate::encoding::DEFAULT, false));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder or proxy file that is set aside doesn't take the sessions' references to its
    /// folders and proxies with it: they work again once the file is restored, and a session
    /// whose proxy is missing fails to connect rather than connecting directly.
    #[test]
    fn keeps_references_into_files_set_aside() {
        let (_, dir) = store("keep-references");
        fs::create_dir_all(&dir).unwrap();
        let profile = with_remote(Profile { id: "p".into(), ..new_profile("p", Some("work")) }, |remote| remote.proxy = Some("corp".into()));
        fs::write(dir.join("profiles.json"), serde_json::to_vec(&[profile]).unwrap()).unwrap();
        fs::write(dir.join("folders.json"), b"[{").unwrap();
        fs::write(dir.join("proxies.json"), br#"[{"id": "corp", "type": "fromTheFuture"}]"#).unwrap();
        let set_aside = SetAside::default();
        let store = ProfileStore::load(dir.join("profiles.json"), &set_aside);
        assert_eq!(set_aside.take().len(), 2);
        let p = store.get("p").unwrap();
        assert_eq!((p.folder.as_deref(), p.proxy()), (Some("work"), Some("corp")));
        assert_eq!(store.route(p.remote().unwrap()).err().map(|e| e.code()), Some("profile.invalidProxy"));

        // Saving something else, and importing, keep them on disk too.
        store.save(new_profile("q", None)).unwrap();
        store.add_all(|_| Ok(Additions { profiles: vec![Profile { id: "r".into(), ..new_profile("r", None) }], ..Additions::default() })).unwrap();
        let reloaded = ProfileStore::load(dir.join("profiles.json"), &SetAside::default());
        let p = reloaded.get("p").unwrap();
        assert_eq!((p.folder.as_deref(), p.proxy()), (Some("work"), Some("corp")));
        assert_eq!(reloaded.list().len(), 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that exists but can't be read (open in another program) is left where it is
    /// and never written over; the other files are saved as usual.
    #[cfg(unix)]
    #[test]
    fn never_writes_over_a_file_it_could_not_read() {
        use std::os::unix::fs::PermissionsExt;

        let (_, dir) = store("unreadable");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.json");
        fs::write(&path, serde_json::to_vec(&[new_profile("a", None)]).unwrap()).unwrap();
        let saved = fs::read(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let set_aside = SetAside::default();
        let store = ProfileStore::load(path.clone(), &set_aside);
        let files = set_aside.take();
        assert_eq!((files.len(), files[0].moved_to.is_none()), (1, true), "{files:?}");
        assert!(store.list().is_empty());

        let error = store.save(new_profile("b", None)).unwrap_err();
        assert_eq!(error.code(), "config.unreadable");
        assert!(error.to_string().contains("profiles.json"), "{error}");
        store.save_folder(folder("Work", None)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(ProfileStore::load(path, &SetAside::default()).folders().len(), 1);
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
