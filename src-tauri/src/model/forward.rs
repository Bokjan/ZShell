//! The forwarding rules saved with an SSH session (`-L`, `-R`, `-D`); `crate::forward` runs them.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ForwardKind {
    /// `-L`: listen here, connect from the server.
    Local,
    /// `-R`: listen on the server, connect from here.
    Remote,
    /// `-D`: SOCKS proxy here, connect from the server.
    Dynamic,
}

/// A saved forwarding rule. `target_*` is unused for dynamic rules.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ForwardRule {
    /// Empty for a new rule; assigned on save.
    #[serde(default)]
    pub id: String,
    pub kind: ForwardKind,
    /// Loopback when empty (filled in on save); `*` ([`ALL_ADDRESSES`]) is every address.
    #[serde(default)]
    pub bind_host: String,
    /// 0 lets the OS (or the server, for remote rules) pick a port.
    pub bind_port: u16,
    #[serde(default)]
    pub target_host: String,
    #[serde(default)]
    pub target_port: u16,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub auto_start: bool,
}

/// The bind address that listens on every address, IPv4 and IPv6, as in OpenSSH.
pub const ALL_ADDRESSES: &str = "*";

impl ForwardRule {
    /// Trims fields, fills in the default bind address and checks the target.
    pub fn normalize(mut self) -> Result<Self> {
        self.bind_host = self.bind_host.trim().to_owned();
        self.target_host = self.target_host.trim().to_owned();
        self.description = self.description.trim().to_owned();
        if self.bind_host.is_empty() {
            // Loopback by default, as OpenSSH does; the server resolves "localhost" itself.
            self.bind_host = if self.kind == ForwardKind::Remote { "localhost" } else { "127.0.0.1" }.to_owned();
        }
        if self.kind == ForwardKind::Dynamic {
            self.target_host.clear();
            self.target_port = 0;
        } else if self.target_host.is_empty() || self.target_port == 0 {
            return Err(Error::new("forward.missingTarget"));
        }
        Ok(self)
    }
}

/// Formats `host:port`, bracketing IPv6 literals.
pub fn host_port(host: &str, port: impl fmt::Display) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
