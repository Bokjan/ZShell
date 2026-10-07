//! The server side of SOCKS5 (RFC 1928, no authentication, CONNECT only) and SOCKS4/4a, as
//! far as a dynamic forward needs it: read the requested destination, then send one reply.

use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    V4,
    V5,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub version: Version,
    pub host: String,
    pub port: u16,
}

/// SOCKS5 reply codes; SOCKS4 only distinguishes success from failure.
#[derive(Clone, Copy, Debug)]
pub enum Reply {
    Succeeded = 0,
    GeneralFailure = 1,
    NotAllowed = 2,
    ConnectionRefused = 5,
    CommandNotSupported = 7,
    AddressNotSupported = 8,
}

const NO_AUTH: u8 = 0x00;
const NO_ACCEPTABLE_METHODS: u8 = 0xff;
const CONNECT: u8 = 0x01;

/// Reads a client's greeting and CONNECT request. Requests this proxy cannot serve are
/// answered with the matching failure before the error is returned.
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<Request> {
    match read_u8(stream).await? {
        4 => accept_v4(stream).await,
        5 => accept_v5(stream).await,
        version => Err(anyhow!(Error::new("forward.socksVersion").param("version", version))),
    }
}

pub async fn reply<S: AsyncWrite + Unpin>(stream: &mut S, version: Version, reply: Reply) -> Result<()> {
    // The bound address is not meaningful through SSH; report all zeros, as OpenSSH does.
    let bytes: &[u8] = match (version, reply) {
        (Version::V4, Reply::Succeeded) => &[0, 0x5a, 0, 0, 0, 0, 0, 0],
        (Version::V4, _) => &[0, 0x5b, 0, 0, 0, 0, 0, 0],
        (Version::V5, reply) => &[5, reply as u8, 0, 1, 0, 0, 0, 0, 0, 0],
    };
    stream.write_all(bytes).await.context(Error::new("forward.socksProtocol"))?;
    Ok(())
}

async fn accept_v5<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<Request> {
    let count = read_u8(stream).await?;
    let mut methods = vec![0; count.into()];
    read_exact(stream, &mut methods).await?;
    if !methods.contains(&NO_AUTH) {
        let _ = stream.write_all(&[5, NO_ACCEPTABLE_METHODS]).await;
        return Err(anyhow!(Error::new("forward.socksAuth")));
    }
    stream.write_all(&[5, NO_AUTH]).await.context(Error::new("forward.socksProtocol"))?;

    let mut header = [0; 4];
    read_exact(stream, &mut header).await?;
    let [version, command, _, address_type] = header;
    if version != 5 {
        return Err(anyhow!(Error::new("forward.socksProtocol")));
    }
    if command != CONNECT {
        reply(stream, Version::V5, Reply::CommandNotSupported).await?;
        return Err(anyhow!(Error::new("forward.socksCommand").param("command", command)));
    }
    let host = match address_type {
        1 => {
            let mut ip = [0; 4];
            read_exact(stream, &mut ip).await?;
            Ipv4Addr::from(ip).to_string()
        }
        3 => {
            let mut name = vec![0; read_u8(stream).await?.into()];
            read_exact(stream, &mut name).await?;
            String::from_utf8(name).map_err(|_| anyhow!(Error::new("forward.socksProtocol")))?
        }
        4 => {
            let mut ip = [0; 16];
            read_exact(stream, &mut ip).await?;
            Ipv6Addr::from(ip).to_string()
        }
        other => {
            reply(stream, Version::V5, Reply::AddressNotSupported).await?;
            return Err(anyhow!(Error::new("forward.socksAddressType").param("type", other)));
        }
    };
    let port = read_u16(stream).await?;
    Ok(Request { version: Version::V5, host, port })
}

async fn accept_v4<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<Request> {
    let command = read_u8(stream).await?;
    let port = read_u16(stream).await?;
    let mut ip = [0; 4];
    read_exact(stream, &mut ip).await?;
    let _user = read_cstring(stream).await?;
    // SOCKS4a: an address of 0.0.0.x (x != 0) means a host name follows the user id.
    let host = match ip {
        [0, 0, 0, x] if x != 0 => read_cstring(stream).await?,
        ip => Ipv4Addr::from(ip).to_string(),
    };
    if command != CONNECT {
        reply(stream, Version::V4, Reply::CommandNotSupported).await?;
        return Err(anyhow!(Error::new("forward.socksCommand").param("command", command)));
    }
    Ok(Request { version: Version::V4, host, port })
}

async fn read_exact<S: AsyncRead + Unpin>(stream: &mut S, buf: &mut [u8]) -> Result<()> {
    stream.read_exact(buf).await.context(Error::new("forward.socksProtocol"))?;
    Ok(())
}

async fn read_u8<S: AsyncRead + Unpin>(stream: &mut S) -> Result<u8> {
    let mut byte = [0];
    read_exact(stream, &mut byte).await?;
    Ok(byte[0])
}

async fn read_u16<S: AsyncRead + Unpin>(stream: &mut S) -> Result<u16> {
    let mut bytes = [0; 2];
    read_exact(stream, &mut bytes).await?;
    Ok(u16::from_be_bytes(bytes))
}

/// A NUL-terminated string of at most 255 bytes.
async fn read_cstring<S: AsyncRead + Unpin>(stream: &mut S) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        match read_u8(stream).await? {
            0 => break,
            _ if bytes.len() == 255 => return Err(anyhow!(Error::new("forward.socksProtocol"))),
            byte => bytes.push(byte),
        }
    }
    String::from_utf8(bytes).map_err(|_| anyhow!(Error::new("forward.socksProtocol")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Runs `accept` against a client that sends `input`; returns the result and every byte
    /// the server wrote.
    async fn run(input: &[u8]) -> (Result<Request>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(input).await.unwrap();
        client.shutdown().await.unwrap();
        let result = accept(&mut server).await;
        drop(server);
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        (result, output)
    }

    fn code(result: &Result<Request>) -> &'static str {
        result.as_ref().unwrap_err().downcast_ref::<Error>().unwrap().code()
    }

    #[tokio::test]
    async fn socks5_domain() {
        let mut input = vec![5, 2, 2, 0, 5, 1, 0, 3, 11];
        input.extend_from_slice(b"example.com");
        input.extend_from_slice(&443u16.to_be_bytes());
        let (result, output) = run(&input).await;
        assert_eq!(result.unwrap(), Request { version: Version::V5, host: "example.com".into(), port: 443 });
        assert_eq!(output, [5, 0]);
    }

    #[tokio::test]
    async fn socks5_ipv4_and_ipv6() {
        let (result, _) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 1, 0, 80]).await;
        assert_eq!(result.unwrap().host, "10.0.0.1");

        let mut input = vec![5, 1, 0, 5, 1, 0, 4];
        input.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        input.extend_from_slice(&[0x1f, 0x90]);
        let (result, _) = run(&input).await;
        let request = result.unwrap();
        assert_eq!((request.host.as_str(), request.port), ("::1", 8080));
    }

    #[tokio::test]
    async fn socks5_requires_no_auth_method() {
        let (result, output) = run(&[5, 1, 2]).await;
        assert_eq!(code(&result), "forward.socksAuth");
        assert_eq!(output, [5, 0xff]);
    }

    #[tokio::test]
    async fn socks5_rejects_bind_and_udp() {
        let (result, output) = run(&[5, 1, 0, 5, 3, 0, 1, 0, 0, 0, 0, 0, 0]).await;
        assert_eq!(code(&result), "forward.socksCommand");
        assert_eq!(output, [5, 0, 5, 7, 0, 1, 0, 0, 0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn socks4_and_4a() {
        let (result, _) = run(&[4, 1, 0, 22, 192, 168, 1, 2, b'u', 0]).await;
        assert_eq!(result.unwrap(), Request { version: Version::V4, host: "192.168.1.2".into(), port: 22 });

        let mut input = vec![4, 1, 0, 80, 0, 0, 0, 1, 0];
        input.extend_from_slice(b"intranet\0");
        let (result, _) = run(&input).await;
        assert_eq!(result.unwrap().host, "intranet");
    }

    #[tokio::test]
    async fn unknown_version_and_truncated_input() {
        let (result, _) = run(&[0x16, 3, 1]).await;
        assert_eq!(code(&result), "forward.socksVersion");
        let (result, _) = run(&[5, 1, 0, 5, 1]).await;
        assert_eq!(code(&result), "forward.socksProtocol");
    }

    #[tokio::test]
    async fn replies() {
        let (mut client, mut server) = duplex(64);
        reply(&mut server, Version::V5, Reply::ConnectionRefused).await.unwrap();
        reply(&mut server, Version::V4, Reply::Succeeded).await.unwrap();
        drop(server);
        let mut output = Vec::new();
        client.read_to_end(&mut output).await.unwrap();
        assert_eq!(output, [5, 5, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0x5a, 0, 0, 0, 0, 0, 0]);
    }
}
