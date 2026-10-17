//! The sessions and folders as they are stored (see `crate::config` for their store).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::settings::{FONT_SIZE_MAX, FONT_SIZE_MIN};
use super::ForwardRule;
#[cfg(doc)]
use crate::config::ProfileStore;
use crate::error::{Error, Result};
#[cfg(doc)]
use crate::net::Route;

/// A saved session: what it connects to (by protocol), and how its tabs behave.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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
    /// What the user notes about the session (what the machine is for, how to get root);
    /// shown with it in the sidebar and found by the search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notes: Option<String>,
}

/// What a session connects to, with the settings of its protocol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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
    /// drop an SSH connection; Telnet uses TCP keepalives, as does SSH until authenticated.
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval: u32,
    /// The terminal type the remote side is told (`TERM`, Telnet's terminal type option).
    #[serde(default = "default_term_type")]
    pub term_type: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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
    /// Edited separately, one at a time (`ProfileStore::save_forward`); `save` keeps them.
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

    /// The keepalive interval, if keepalives are on.
    pub fn keepalive(&self) -> Option<std::time::Duration> {
        (self.keepalive_interval > 0).then(|| std::time::Duration::from_secs(self.keepalive_interval.into()))
    }
}

impl Remote {
    /// Checks and tidies the address; SSH also needs a user name.
    fn normalize(&mut self, ssh: bool) -> Result<()> {
        let host = self.host.trim();
        // An IPv6 address written in brackets, as in a URL or `ssh user@[::1]`; the port has
        // a field of its own.
        let unbracketed = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
        let is_ipv6 = |h: &str| h.split('%').next().is_some_and(|address| address.parse::<std::net::Ipv6Addr>().is_ok());
        self.host = unbracketed.filter(|h| is_ipv6(h)).unwrap_or(host).to_owned();
        self.username = self.username.trim().to_owned();
        if !self.host.is_empty() && !is_plain_host(&self.host) {
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
        self.font_size = self.font_size.filter(|size| (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(size));
        self
    }
}

/// Whether `host` is a plain host name or address, which known_hosts can't read as a pattern
/// or a list: a key recorded for `*` would be trusted for every host.
pub fn is_plain_host(host: &str) -> bool {
    !host.is_empty() && !host.chars().any(|c| c.is_whitespace() || c.is_control() || ",*?![]".contains(c))
}

/// Whether a path names a file on another computer (`\\server\share`, `//server/share`) or
/// a Windows device (`\\?\`, `\\.\`), where a file from elsewhere must not lead.
fn is_network_path(path: &str) -> bool {
    let path = path.trim_start();
    path.starts_with("\\\\") || path.starts_with("//") || path.starts_with("\\/") || path.starts_with("/\\")
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
            notes: None,
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
    pub(crate) fn normalize_target(&mut self) -> Result<()> {
        self.name = crate::i18n::printable(self.name.trim()).trim().to_owned();
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
            // Reading a key from another computer's share would sign in there as the user
            // (Windows sends the account's NTLM response), before the session even connects:
            // the edit dialog checks that the file exists.
            if let AuthMethod::PublicKey { key_path } = &ssh.auth {
                if is_network_path(key_path) {
                    return Err(Error::new("profile.networkKeyPath").param("path", key_path));
                }
            }
            ssh.forwards = std::mem::take(&mut ssh.forwards)
                .into_iter()
                .map(|rule| ForwardRule { id: uuid::Uuid::new_v4().to_string(), ..rule }.normalize())
                .collect::<Result<_>>()?;
        }
        Ok(())
    }

    /// Checks and tidies what the user edits beyond the address (encoding, terminal type,
    /// environment, login commands, appearance, notes).
    pub(crate) fn normalize_session_options(&mut self) -> Result<()> {
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
        self.notes = self.notes.take().map(|notes| notes.trim_end().to_owned()).filter(|notes| !notes.trim().is_empty());
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

/// A session or a folder: as dragged in the sidebar, and in the order they are shown.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(rename = "TreeItem")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Profile { id: String },
    Folder { id: String },
}

/// The user name `ssh` uses when none is given: the local one.
pub fn local_username() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
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

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthMethod {
    /// Agent keys, default key files, then keyboard-interactive / password, like OpenSSH.
    #[default]
    Auto,
    Password,
    PublicKey {
        key_path: String,
    },
    Agent,
}
