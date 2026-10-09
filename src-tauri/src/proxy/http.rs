//! The client side of an HTTP proxy's CONNECT method (RFC 9110), with Basic authentication.
//! The connection to the proxy itself is plain HTTP.

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{Credentials, Errors};
use crate::forward::host_port;

/// The most a response head may take before the proxy is considered broken.
const MAX_HEAD: usize = 16 << 10;

/// Asks the proxy on `stream` to connect to `host:port`; on success the stream carries the
/// connection. The credentials are sent with the request rather than after a 407.
pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    port: u16,
    credentials: Option<&Credentials>,
    errors: &Errors<'_>,
) -> Result<()> {
    let authority = host_port(host, port);
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(Credentials { username, password }) = credentials {
        let token = base64(format!("{username}:{password}").as_bytes());
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.with_context(|| errors.error("proxy.protocol"))?;

    let head = read_head(stream).await.with_context(|| errors.error("proxy.protocol"))?;
    let status_line = head.lines().next().unwrap_or_default().trim().to_owned();
    let status = status_line
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok());
    match status {
        Some(200..=299) => Ok(()),
        Some(407) if credentials.is_some() => bail!(errors.error("proxy.authFailed")),
        Some(407) => bail!(errors.error("proxy.authRequired")),
        Some(_) => bail!(errors.error("proxy.refused").detail(status_line)),
        None => bail!(errors.error("proxy.protocol").detail(status_line)),
    }
}

/// Reads the response up to the blank line after its headers, one byte at a time: what
/// follows belongs to the tunnel (the server may speak first, as SSH servers do).
async fn read_head<S: AsyncRead + Unpin>(stream: &mut S) -> Result<String> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") && !head.ends_with(b"\n\n") {
        if head.len() == MAX_HEAD {
            bail!("response headers too long");
        }
        head.push(stream.read_u8().await?);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// Standard base64 with padding (RFC 4648).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use tokio::io::duplex;

    fn errors() -> Errors<'static> {
        Errors { proxy: "corp", target: "db.internal:22".into() }
    }

    async fn run(host: &str, credentials: Option<Credentials>, response: &str) -> (Result<()>, String) {
        let (mut client, mut server) = duplex(4096);
        server.write_all(response.as_bytes()).await.unwrap();
        server.shutdown().await.unwrap();
        let result = connect(&mut client, host, 22, credentials.as_ref(), &errors()).await;
        drop(client);
        let mut sent = String::new();
        server.read_to_string(&mut sent).await.unwrap();
        (result, sent)
    }

    fn code(result: &Result<()>) -> &'static str {
        result.as_ref().unwrap_err().downcast_ref::<Error>().unwrap().code()
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"Aladdin:open sesame"), "QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[tokio::test]
    async fn connects_and_leaves_the_tunnel_data_unread() {
        let (mut client, mut server) = duplex(4096);
        server.write_all(b"HTTP/1.1 200 Connection established\r\nVia: squid\r\n\r\nSSH-2.0").await.unwrap();
        connect(&mut client, "::1", 22, None, &errors()).await.unwrap();
        let mut banner = [0; 7];
        client.read_exact(&mut banner).await.unwrap();
        assert_eq!(&banner, b"SSH-2.0");
        drop(client);
        let mut sent = String::new();
        server.read_to_string(&mut sent).await.unwrap();
        assert_eq!(sent, "CONNECT [::1]:22 HTTP/1.1\r\nHost: [::1]:22\r\n\r\n");
    }

    #[tokio::test]
    async fn sends_basic_credentials() {
        let credentials = Some(Credentials { username: "Aladdin".into(), password: "open sesame".into() });
        let (result, sent) = run("db", credentials, "HTTP/1.0 200 OK\r\n\r\n").await;
        result.unwrap();
        assert!(sent.contains("\r\nProxy-Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==\r\n"));

        let credentials = Some(Credentials { username: "a".into(), password: "b".into() });
        let (result, _) = run("db", credentials, "HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await;
        assert_eq!(code(&result), "proxy.authFailed");
        let (result, _) = run("db", None, "HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await;
        assert_eq!(code(&result), "proxy.authRequired");
    }

    #[tokio::test]
    async fn reports_refusals_and_bad_responses() {
        let (result, _) = run("db", None, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n").await;
        let error = Error::from(result.unwrap_err());
        assert_eq!(error.code(), "proxy.refused");
        assert!(error.to_string().ends_with(": HTTP/1.1 403 Forbidden"));

        let (result, _) = run("db", None, "SSH-2.0-OpenSSH_9.9\r\n\r\n").await;
        assert_eq!(code(&result), "proxy.protocol");
        let (result, _) = run("db", None, "HTTP/1.1 200 OK\r\n").await;
        assert_eq!(code(&result), "proxy.protocol");
    }
}
