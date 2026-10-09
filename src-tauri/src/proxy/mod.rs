//! Proxies for the first connection of a session: SOCKS5 ([`socks`]), HTTP CONNECT
//! ([`http`]), or a local command whose stdin and stdout carry the connection, like
//! OpenSSH's `ProxyCommand` ([`command`]).
//!
//! Proxies are saved by name in `proxies.json` and chosen per session (see
//! [`crate::config::ProfileStore`]); their passwords are in the keychain.

mod command;
mod http;
mod socks;

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::Error;
use crate::forward::host_port;
use crate::net::Stream;
use crate::secrets;
use crate::session::TermIo;

/// Password attempts after the saved one, as for SSH.
const MAX_ATTEMPTS: usize = 3;

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
    /// SOCKS5 and HTTP: empty for a proxy without authentication. The password is in the
    /// keychain.
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
        self.name = self.name.trim().to_owned();
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
                ProxyKind::Command => command::program_name(&self.command),
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

/// Connects to `host:port` through `proxy`. `user` is what a command's `%r` stands for;
/// `keepalive` turns on TCP keepalives on the connection to a SOCKS or HTTP proxy. A
/// password the proxy needs and that isn't saved is asked in the terminal.
pub async fn connect(
    proxy: &Proxy,
    host: &str,
    port: u16,
    user: &str,
    keepalive: Option<Duration>,
    io: &mut TermIo,
) -> Result<Box<dyn Stream>> {
    if proxy.kind == ProxyKind::Command {
        return command::spawn(proxy, host, port, user, io.sink()).await;
    }
    // Sent in the request as it is: a line break would add to the HTTP request.
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!(Error::new("profile.invalidHost"));
    }
    let mut saved = match proxy.uses_password() {
        true => secrets::proxy_password(proxy.id.clone()).await,
        false => None,
    };
    let mut attempts = 0;
    loop {
        let typed = saved.is_none();
        let credentials = match proxy.uses_password() {
            true => Some(Credentials { username: proxy.username.clone(), password: password(proxy, &mut saved, io).await? }),
            false => None,
        };
        let mut stream = crate::net::tcp(&proxy.host, proxy.port)
            .await
            .context(Error::new("proxy.unreachable").param("proxy", &proxy.name))?;
        if let Some(interval) = keepalive {
            crate::net::set_keepalive(&stream, interval);
        }
        let errors = Errors { proxy: &proxy.name, target: host_port(host, port) };
        let handshake = async {
            match proxy.kind {
                ProxyKind::Socks5 => socks::connect(&mut stream, host, port, credentials.as_ref(), &errors).await,
                _ => http::connect(&mut stream, host, port, credentials.as_ref(), &errors).await,
            }
        };
        // The proxy answers once it has reached the target, which has its own time limit.
        let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake)
            .await
            .unwrap_or_else(|_| Err(errors.error("proxy.timeout").into()));
        let Err(e) = handshake else {
            return Ok(Box::new(stream));
        };
        let rejected = e.downcast_ref::<Error>().is_some_and(|e| e.code() == "proxy.authFailed");
        if typed {
            attempts += 1;
        }
        if !rejected || attempts == MAX_ATTEMPTS {
            return Err(e);
        }
        let message = if typed { t!("terminal.proxyPasswordRejected") } else { t!("terminal.savedProxyPasswordRejected") };
        io.print(&format!("{message}\n"));
    }
}

/// How long a SOCKS or HTTP proxy may take to answer, including connecting to the target.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Makes the errors of a handshake with a proxy, which name the proxy and the target.
pub struct Errors<'a> {
    proxy: &'a str,
    target: String,
}

impl Errors<'_> {
    fn error(&self, code: &'static str) -> Error {
        Error::new(code).param("proxy", self.proxy).param("target", &self.target)
    }
}

/// The saved password the first time, then one typed in the terminal.
async fn password(proxy: &Proxy, saved: &mut Option<String>, io: &mut TermIo) -> Result<String> {
    if let Some(password) = saved.take() {
        return Ok(password);
    }
    io.print(&t!("terminal.proxyPassword", user = proxy.username, proxy = proxy.name));
    match io.read_line(false).await {
        Some(password) => Ok(password),
        None => bail!(Error::new("auth.cancelled")),
    }
}

pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy(kind: ProxyKind) -> Proxy {
        Proxy {
            id: String::new(),
            name: String::new(),
            kind,
            host: " proxy.lan ".into(),
            port: 1080,
            username: String::new(),
            command: String::new(),
        }
    }

    #[test]
    fn normalizes_by_kind() {
        let mut socks = proxy(ProxyKind::Socks5);
        socks.normalize().unwrap();
        assert_eq!((socks.name.as_str(), socks.host.as_str()), ("proxy.lan:1080", "proxy.lan"));
        assert!(!socks.uses_password());

        let mut http = Proxy { port: 0, ..proxy(ProxyKind::Http) };
        assert_eq!(http.normalize().unwrap_err().code(), "proxy.missingAddress");

        let mut command = proxy(ProxyKind::Command);
        assert_eq!(command.normalize().unwrap_err().code(), "proxy.missingCommand");
        command.command = "/opt/homebrew/bin/cloudflared access ssh --hostname %h".into();
        command.username = "kept".into();
        command.normalize().unwrap();
        assert_eq!(command.name, "cloudflared");
        assert!(!command.uses_password());
    }

    /// A whole connection through a SOCKS5 proxy (the server side of dynamic forwards) to
    /// an echo server.
    #[tokio::test]
    async fn connects_through_a_socks_proxy() {
        use crate::forward::socks;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_port = echo.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = echo.accept().await.unwrap();
            let (mut reader, mut writer) = stream.split();
            tokio::io::copy(&mut reader, &mut writer).await.unwrap();
        });
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        let requested = tokio::spawn(async move {
            let (mut client, _) = proxy_listener.accept().await.unwrap();
            let request = socks::accept(&mut client).await.unwrap();
            let mut target = tokio::net::TcpStream::connect(("127.0.0.1", request.port)).await.unwrap();
            socks::reply(&mut client, request.version, socks::Reply::Succeeded).await.unwrap();
            tokio::spawn(async move { tokio::io::copy_bidirectional(&mut client, &mut target).await });
            request.host
        });

        let proxy = Proxy { port: proxy_port, host: "127.0.0.1".into(), ..proxy(ProxyKind::Socks5) };
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let mut stream = connect(&proxy, "echo.internal", echo_port, "", None, &mut io).await.unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut reply = [0; 4];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"ping");
        // The name went to the proxy unresolved.
        assert_eq!(requested.await.unwrap(), "echo.internal");

        // Nothing listens on the proxy's port any more.
        let error = connect(&proxy, "echo.internal", echo_port, "", None, &mut io).await.err().unwrap();
        let error = Error::from(error);
        assert_eq!(error.code(), "proxy.unreachable");
        assert!(error.to_string().contains(&format!("127.0.0.1:{proxy_port}")), "{error}");
    }

    #[test]
    fn compares_by_address_or_command() {
        let a = Proxy { name: "a".into(), ..proxy(ProxyKind::Socks5) };
        assert!(a.same_as(&Proxy { name: "b".into(), ..a.clone() }));
        assert!(!a.same_as(&Proxy { kind: ProxyKind::Http, ..a.clone() }));
        assert!(!a.same_as(&Proxy { username: "bob".into(), ..a.clone() }));
        let nc = Proxy { command: "nc %h %p".into(), ..proxy(ProxyKind::Command) };
        assert!(nc.same_as(&Proxy { host: "other".into(), ..nc.clone() }));
    }
}
