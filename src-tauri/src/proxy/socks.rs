//! The client side of SOCKS5 (RFC 1928) with user name and password authentication
//! (RFC 1929), CONNECT only.
//!
//! Host names are sent as names, so the proxy resolves them (curl's `socks5h`), which reaches
//! names that only resolve on the proxy's side.

use std::net::IpAddr;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{Credentials, Errors};

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0x00;
const USER_PASSWORD: u8 = 0x02;
const NO_ACCEPTABLE_METHODS: u8 = 0xff;
const CONNECT: u8 = 0x01;
const IPV4: u8 = 1;
const DOMAIN: u8 = 3;
const IPV6: u8 = 4;

/// Asks the proxy on `stream` to connect to `host:port`; on success the stream carries the
/// connection.
pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<&Credentials>,
    errors: &Errors<'_>,
) -> Result<()> {
    let protocol = || errors.error("proxy.protocol");
    let methods: &[u8] = if credentials.is_some() { &[NO_AUTH, USER_PASSWORD] } else { &[NO_AUTH] };
    let mut greeting = vec![VERSION, methods.len() as u8];
    greeting.extend_from_slice(methods);
    stream.write_all(&greeting).await.with_context(protocol)?;

    let mut choice = [0; 2];
    stream.read_exact(&mut choice).await.with_context(protocol)?;
    match (choice, credentials) {
        ([VERSION, NO_AUTH], _) => {}
        ([VERSION, USER_PASSWORD], Some(credentials)) => authenticate(stream, credentials, errors).await?,
        ([VERSION, NO_ACCEPTABLE_METHODS], None) => bail!(errors.error("proxy.authRequired")),
        ([VERSION, NO_ACCEPTABLE_METHODS], Some(_)) => bail!(errors.error("proxy.authUnsupported")),
        ([version, method], _) => bail!(protocol().detail(format!("version {version}, method {method}"))),
    }

    let mut request = vec![VERSION, CONNECT, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(IPV4);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(IPV6);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            let Ok(length) = u8::try_from(host.len()) else {
                bail!(errors.error("proxy.hostTooLong"));
            };
            request.extend_from_slice(&[DOMAIN, length]);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await.with_context(protocol)?;

    let mut reply = [0; 4];
    stream.read_exact(&mut reply).await.with_context(protocol)?;
    let [version, status, _, address_type] = reply;
    if version != VERSION {
        bail!(protocol().detail(format!("version {version}")));
    }
    if status != 0 {
        bail!(errors.error("proxy.refused").detail(reply_text(status)));
    }
    // The address the proxy connected from, which nothing here needs.
    let length = match address_type {
        IPV4 => 4,
        IPV6 => 16,
        DOMAIN => stream.read_u8().await.with_context(protocol)?.into(),
        other => bail!(protocol().detail(format!("address type {other}"))),
    };
    let mut bound = vec![0; length + 2];
    stream.read_exact(&mut bound).await.with_context(protocol)?;
    Ok(())
}

/// RFC 1929: the user name and password, each at most 255 bytes.
async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, credentials: &Credentials, errors: &Errors<'_>) -> Result<()> {
    let (Ok(user_length), Ok(password_length)) = (u8::try_from(credentials.username.len()), u8::try_from(credentials.password.len())) else {
        bail!(errors.error("proxy.authFailed"));
    };
    let mut request = vec![1, user_length];
    request.extend_from_slice(credentials.username.as_bytes());
    request.push(password_length);
    request.extend_from_slice(credentials.password.as_bytes());
    stream.write_all(&request).await.with_context(|| errors.error("proxy.protocol"))?;
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).await.with_context(|| errors.error("proxy.protocol"))?;
    if reply[1] != 0 {
        bail!(errors.error("proxy.authFailed"));
    }
    Ok(())
}

/// The meaning of a SOCKS5 reply code, as named in RFC 1928 (technical detail).
fn reply_text(status: u8) -> String {
    let text = match status {
        1 => "general SOCKS server failure",
        2 => "connection not allowed by ruleset",
        3 => "network unreachable",
        4 => "host unreachable",
        5 => "connection refused",
        6 => "TTL expired",
        7 => "command not supported",
        8 => "address type not supported",
        _ => "unknown error",
    };
    format!("{text} (SOCKS reply {status})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use tokio::io::duplex;

    fn errors() -> Errors<'static> {
        Errors { proxy: "corp", target: "db.internal:22".into() }
    }

    /// Runs `connect` against a proxy that answers `replies` (all at once); returns the
    /// result and every byte the client sent.
    async fn run(host: &str, credentials: Option<Credentials>, replies: &[u8]) -> (Result<()>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        server.write_all(replies).await.unwrap();
        server.shutdown().await.unwrap();
        let result = connect(&mut client, host, 22, credentials.as_ref(), &errors()).await;
        drop(client);
        let mut sent = Vec::new();
        server.read_to_end(&mut sent).await.unwrap();
        (result, sent)
    }

    fn code(result: &Result<()>) -> &'static str {
        result.as_ref().unwrap_err().downcast_ref::<Error>().unwrap().code()
    }

    const SUCCESS: [u8; 10] = [5, 0, 0, 1, 10, 0, 0, 1, 0x1f, 0x90];

    #[tokio::test]
    async fn connects_by_name_without_auth() {
        let mut replies = vec![5, 0];
        replies.extend_from_slice(&SUCCESS);
        // Data after the reply belongs to the connection.
        replies.extend_from_slice(b"SSH-2.0");
        let (mut client, mut server) = duplex(1024);
        server.write_all(&replies).await.unwrap();
        connect(&mut client, "db.internal", 22, None, &errors()).await.unwrap();
        let mut banner = [0; 7];
        client.read_exact(&mut banner).await.unwrap();
        assert_eq!(&banner, b"SSH-2.0");
        drop(client);
        let mut sent = Vec::new();
        server.read_to_end(&mut sent).await.unwrap();
        let mut expected = vec![5, 1, 0, 5, 1, 0, 3, 11];
        expected.extend_from_slice(b"db.internal");
        expected.extend_from_slice(&[0, 22]);
        assert_eq!(sent, expected);
    }

    #[tokio::test]
    async fn sends_ip_addresses_as_addresses() {
        let mut replies = vec![5, 0];
        replies.extend_from_slice(&SUCCESS);
        let (result, sent) = run("10.0.0.9", None, &replies).await;
        result.unwrap();
        assert_eq!(&sent[3..], [5, 1, 0, 1, 10, 0, 0, 9, 0, 22]);
        let (result, sent) = run("::1", None, &replies).await;
        result.unwrap();
        assert_eq!((sent[6], sent.len()), (IPV6, 3 + 4 + 16 + 2));
    }

    #[tokio::test]
    async fn authenticates_with_user_and_password() {
        let credentials = || Some(Credentials { username: "bob".into(), password: "pw".into() });
        let mut replies = vec![5, 2, 1, 0];
        replies.extend_from_slice(&SUCCESS);
        let (result, sent) = run("db", credentials(), &replies).await;
        result.unwrap();
        assert_eq!(&sent[..11], [5, 2, 0, 2, 1, 3, b'b', b'o', b'b', 2, b'p']);

        let (result, _) = run("db", credentials(), &[5, 2, 1, 1]).await;
        assert_eq!(code(&result), "proxy.authFailed");
        let (result, _) = run("db", credentials(), &[5, 0xff]).await;
        assert_eq!(code(&result), "proxy.authUnsupported");
        let (result, _) = run("db", None, &[5, 0xff]).await;
        assert_eq!(code(&result), "proxy.authRequired");
    }

    #[tokio::test]
    async fn reports_refusals_and_bad_replies() {
        let (result, _) = run("db", None, &[5, 0, 5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await;
        let error = Error::from(result.unwrap_err());
        assert_eq!(error.code(), "proxy.refused");
        assert!(error.to_string().contains("connection refused (SOCKS reply 5)"));
        assert!(error.to_string().contains("db.internal:22"));

        let (result, _) = run("db", None, &[4, 0x5a]).await;
        assert_eq!(code(&result), "proxy.protocol");
        // The proxy closed the connection.
        let (result, _) = run("db", None, &[5]).await;
        assert_eq!(code(&result), "proxy.protocol");
        let (result, _) = run(&"x".repeat(256), None, &[5, 0]).await;
        assert_eq!(code(&result), "proxy.hostTooLong");
    }
}
