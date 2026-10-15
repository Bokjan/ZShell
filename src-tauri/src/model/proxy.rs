//! The proxies sessions connect through, saved by name; `crate::proxy` connects through them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::host_port;
use crate::error::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Proxy {
    /// Empty when creating a new proxy; assigned on save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: ProxyKind,
    /// SOCKS5 and HTTP: the proxy server. Kept, unused, by command proxies.
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    /// SOCKS5 and HTTP: empty for a proxy without authentication. The password is kept with
    /// the others (see `vault/`).
    #[serde(default)]
    pub username: String,
    /// Command proxies: run by the user's shell, with `%h`, `%p`, `%r` and `%%` replaced.
    #[serde(default)]
    pub command: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProxyKind {
    Socks5,
    Http,
    Command,
}

impl Proxy {
    /// Checks and tidies the fields of its kind, and names an unnamed proxy after its
    /// address or program.
    pub fn normalize(&mut self) -> crate::error::Result<()> {
        self.name = crate::i18n::printable(self.name.trim()).trim().to_owned();
        self.host = self.host.trim().to_owned();
        self.username = self.username.trim().to_owned();
        self.command = self.command.trim().to_owned();
        match self.kind {
            ProxyKind::Command if self.command.is_empty() => return Err(Error::new("proxy.missingCommand")),
            ProxyKind::Socks5 | ProxyKind::Http if self.host.is_empty() || self.port == 0 => {
                return Err(Error::new("proxy.missingAddress"))
            }
            _ => {}
        }
        if self.name.is_empty() {
            self.name = match self.kind {
                ProxyKind::Command => program_name(&self.command),
                _ => host_port(&self.host, self.port),
            };
        }
        Ok(())
    }

    /// Whether `other` is the same proxy under any name: the same server and user, or the
    /// same command.
    pub fn same_as(&self, other: &Proxy) -> bool {
        self.kind == other.kind
            && match self.kind {
                ProxyKind::Command => self.command == other.command,
                _ => self.host == other.host && self.port == other.port && self.username == other.username,
            }
    }

    /// Whether it needs a password from the keychain (or the user).
    pub fn uses_password(&self) -> bool {
        self.kind != ProxyKind::Command && !self.username.is_empty()
    }
}

/// The name of the program a command runs, without its directory and extension.
fn program_name(command: &str) -> String {
    let program = split_program(command).0;
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    name.strip_suffix(".exe").unwrap_or(name).to_owned()
}

/// The program and the rest of a command line; the program may be in double quotes.
pub(crate) fn split_program(command: &str) -> (&str, &str) {
    let command = command.trim_start();
    match command.strip_prefix('"') {
        Some(rest) => rest.split_once('"').unwrap_or((rest, "")),
        None => command.split_once(char::is_whitespace).unwrap_or((command, "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_programs() {
        assert_eq!(program_name("/opt/homebrew/bin/cloudflared access ssh --hostname %h"), "cloudflared");
        assert_eq!(program_name(r#""C:\Program Files\Git\usr\bin\connect.exe" -S proxy:1080 %h %p"#), "connect");
        assert_eq!(split_program(r#""C:\a b\nc.exe" -x p %h"#), (r"C:\a b\nc.exe", " -x p %h"));
        assert_eq!(split_program("nc"), ("nc", ""));
    }
}
