//! File names in a session's character encoding over SFTP.
//!
//! russh-sftp reads every name as UTF-8 (replacing what isn't), so a GBK name is already
//! lost by the time it reaches us, and paths can only be sent as UTF-8. For sessions with
//! another encoding, this sits between the SFTP channel and russh-sftp and rewrites the path
//! strings of the packets on the way: requests carry UTF-8 paths that become the session's
//! encoding, and the names in `SSH_FXP_NAME` replies (directory listings, `realpath`,
//! `readlink`) become UTF-8. Everything else (file data, attributes, status messages, which
//! the protocol defines as UTF-8) passes through unchanged, and so does any packet that
//! doesn't parse.
//!
//! Characters the encoding cannot represent are sent as `?` (an upload of `😀.txt` to a GBK
//! server creates `?.txt`).

use encoding_rs::{Encoding, UTF_8};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::encoding;

/// The stream russh-sftp runs on.
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Stream for S {}

const FXP_OPEN: u8 = 3;
const FXP_LSTAT: u8 = 7;
const FXP_SETSTAT: u8 = 9;
const FXP_OPENDIR: u8 = 11;
const FXP_REMOVE: u8 = 13;
const FXP_MKDIR: u8 = 14;
const FXP_RMDIR: u8 = 15;
const FXP_REALPATH: u8 = 16;
const FXP_STAT: u8 = 17;
const FXP_RENAME: u8 = 18;
const FXP_READLINK: u8 = 19;
const FXP_SYMLINK: u8 = 20;
const FXP_NAME: u8 = 104;
const FXP_EXTENDED: u8 = 200;

const ATTR_SIZE: u32 = 0x1;
const ATTR_UIDGID: u32 = 0x2;
const ATTR_PERMISSIONS: u32 = 0x4;
const ATTR_ACMODTIME: u32 = 0x8;
const ATTR_EXTENDED: u32 = 0x8000_0000;

/// Larger than any packet russh-sftp or OpenSSH sends (OpenSSH allows 256 KiB).
const MAX_PACKET: usize = 16 << 20;

/// The stream for russh-sftp over `channel`: `channel` itself for UTF-8, otherwise one end of
/// a pipe whose other end a task connects to `channel`, converting paths.
pub fn convert<S>(channel: S, encoding: &'static Encoding) -> Box<dyn Stream>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    if encoding == UTF_8 {
        return Box::new(channel);
    }
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    tauri::async_runtime::spawn(pump(channel, theirs, encoding));
    Box::new(ours)
}

/// Copies packets both ways until either side closes; dropping both ends then closes the
/// other side.
async fn pump<S, C>(channel: S, client: C, encoding: &'static Encoding)
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: AsyncRead + AsyncWrite + Unpin,
{
    let (mut channel_read, mut channel_write) = tokio::io::split(channel);
    let (mut client_read, mut client_write) = tokio::io::split(client);
    tokio::select! {
        _ = copy(&mut client_read, &mut channel_write, request, encoding) => {}
        _ = copy(&mut channel_read, &mut client_write, reply, encoding) => {}
    }
}

/// Copies packets from `from` to `to`, converted by `convert`, until either side fails.
async fn copy(
    from: &mut (impl AsyncRead + Unpin),
    to: &mut (impl AsyncWrite + Unpin),
    convert: fn(Vec<u8>, &'static Encoding) -> Vec<u8>,
    encoding: &'static Encoding,
) -> std::io::Result<()> {
    loop {
        let packet = read_packet(from).await?;
        write_packet(to, &convert(packet, encoding)).await?;
    }
}

async fn read_packet(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let len = reader.read_u32().await? as usize;
    if len > MAX_PACKET {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await?;
    Ok(body)
}

async fn write_packet(writer: &mut (impl AsyncWrite + Unpin), body: &[u8]) -> std::io::Result<()> {
    let mut packet = Vec::with_capacity(body.len() + 4);
    packet.extend_from_slice(&(body.len() as u32).to_be_bytes());
    packet.extend_from_slice(body);
    writer.write_all(&packet).await?;
    writer.flush().await
}

/// A request from russh-sftp (type, id, ...), with its paths in `encoding`.
fn request(body: Vec<u8>, encoding: &'static Encoding) -> Vec<u8> {
    let to_remote = |path: &[u8]| encoding::encode(encoding, &String::from_utf8_lossy(path));
    let Some(&kind) = body.first() else {
        return body;
    };
    // The paths come right after the type and the request id.
    let (start, paths) = match kind {
        FXP_OPEN | FXP_LSTAT | FXP_SETSTAT | FXP_OPENDIR | FXP_REMOVE | FXP_MKDIR | FXP_RMDIR | FXP_REALPATH
        | FXP_STAT | FXP_READLINK => (5, 1),
        FXP_RENAME | FXP_SYMLINK => (5, 2),
        // After the extension's name.
        FXP_EXTENDED => {
            let mut pos = 5;
            let paths = match string(&body, &mut pos) {
                Some(b"posix-rename@openssh.com" | b"hardlink@openssh.com") => 2,
                Some(b"statvfs@openssh.com" | b"expand-path@openssh.com") => 1,
                _ => 0,
            };
            (pos, paths)
        }
        _ => return body,
    };
    if paths == 0 {
        return body;
    }
    rewrite_strings(&body, start, paths, to_remote).unwrap_or(body)
}

/// A reply from the server: the names of `SSH_FXP_NAME` (type, id, count, then per entry the
/// file name, the `ls -l` style long name and the attributes) in UTF-8.
fn reply(body: Vec<u8>, encoding: &'static Encoding) -> Vec<u8> {
    if body.first() != Some(&FXP_NAME) {
        return body;
    }
    names_to_utf8(&body, encoding).unwrap_or(body)
}

fn names_to_utf8(body: &[u8], encoding: &'static Encoding) -> Option<Vec<u8>> {
    let to_utf8 = |name: &[u8]| encoding::decode(encoding, name).into_bytes();
    let mut pos = 5;
    let count = u32_at(body, &mut pos)?;
    let mut out = body[..pos].to_vec();
    for _ in 0..count {
        for _ in 0..2 {
            put_string(&mut out, &to_utf8(string(body, &mut pos)?));
        }
        let attrs = pos;
        skip_attrs(body, &mut pos)?;
        out.extend_from_slice(&body[attrs..pos]);
    }
    out.extend_from_slice(&body[pos..]);
    Some(out)
}

/// `body` with the `count` strings from `start` converted by `convert`.
fn rewrite_strings(body: &[u8], start: usize, count: usize, convert: impl Fn(&[u8]) -> Vec<u8>) -> Option<Vec<u8>> {
    let mut out = body.get(..start)?.to_vec();
    let mut pos = start;
    for _ in 0..count {
        put_string(&mut out, &convert(string(body, &mut pos)?));
    }
    out.extend_from_slice(&body[pos..]);
    Some(out)
}

fn skip_attrs(body: &[u8], pos: &mut usize) -> Option<()> {
    let flags = u32_at(body, pos)?;
    let fixed = [(ATTR_SIZE, 8), (ATTR_UIDGID, 8), (ATTR_PERMISSIONS, 4), (ATTR_ACMODTIME, 8)];
    for (flag, len) in fixed {
        if flags & flag != 0 {
            *pos += len;
        }
    }
    if flags & ATTR_EXTENDED != 0 {
        for _ in 0..u32_at(body, pos)? {
            string(body, pos)?;
            string(body, pos)?;
        }
    }
    (*pos <= body.len()).then_some(())
}

fn u32_at(body: &[u8], pos: &mut usize) -> Option<u32> {
    let bytes = body.get(*pos..*pos + 4)?;
    *pos += 4;
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

fn string<'a>(body: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len = u32_at(body, pos)? as usize;
    let text = body.get(*pos..pos.checked_add(len)?)?;
    *pos += len;
    Some(text)
}

fn put_string(out: &mut Vec<u8>, text: &[u8]) {
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    const NI_HAO: &[u8] = b"\xc4\xe3\xba\xc3"; // "你好" in GBK

    fn gbk() -> &'static Encoding {
        encoding::for_profile("gbk")
    }

    fn packet(kind: u8, id: u32, fields: &[&[u8]], tail: &[u8]) -> Vec<u8> {
        let mut body = vec![kind];
        body.extend_from_slice(&id.to_be_bytes());
        for field in fields {
            put_string(&mut body, field);
        }
        body.extend_from_slice(tail);
        body
    }

    #[test]
    fn converts_request_paths() {
        let open = packet(FXP_OPEN, 7, &["/srv/你好".as_bytes()], &[0, 0, 0, 1, 0, 0, 0, 0]);
        let expected = packet(FXP_OPEN, 7, &[&[b"/srv/".as_slice(), NI_HAO].concat()], &[0, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(request(open, gbk()), expected);

        let rename = packet(FXP_RENAME, 8, &["a".as_bytes(), "你好".as_bytes()], &[]);
        assert_eq!(request(rename, gbk()), packet(FXP_RENAME, 8, &[b"a", NI_HAO], &[]));

        let ext = packet(FXP_EXTENDED, 9, &[b"posix-rename@openssh.com", "你好".as_bytes(), b"b"], &[]);
        assert_eq!(request(ext, gbk()), packet(FXP_EXTENDED, 9, &[b"posix-rename@openssh.com", NI_HAO, b"b"], &[]));

        // File data is not touched, nor are unknown extensions.
        let write = packet(6, 10, &[b"handle"], &[0xc4, 0xe3]);
        assert_eq!(request(write.clone(), gbk()), write);
        let limits = packet(FXP_EXTENDED, 11, &[b"limits@openssh.com"], &[]);
        assert_eq!(request(limits.clone(), gbk()), limits);
    }

    #[test]
    fn converts_names_in_replies() {
        // Two entries: one with size and permissions, one with an extended attribute.
        let mut body = vec![FXP_NAME];
        body.extend_from_slice(&3u32.to_be_bytes());
        body.extend_from_slice(&2u32.to_be_bytes());
        put_string(&mut body, NI_HAO);
        put_string(&mut body, &[b"-rw-r--r-- 1 u g 5 Jan 1 00:00 ".as_slice(), NI_HAO].concat());
        body.extend_from_slice(&(ATTR_SIZE | ATTR_PERMISSIONS).to_be_bytes());
        body.extend_from_slice(&5u64.to_be_bytes());
        body.extend_from_slice(&0o100644u32.to_be_bytes());
        put_string(&mut body, b"plain");
        put_string(&mut body, b"plain");
        body.extend_from_slice(&ATTR_EXTENDED.to_be_bytes());
        body.extend_from_slice(&1u32.to_be_bytes());
        put_string(&mut body, b"key");
        put_string(&mut body, b"value");

        let converted = reply(body, gbk());
        let mut pos = 9;
        assert_eq!(string(&converted, &mut pos), Some("你好".as_bytes()));
        assert!(string(&converted, &mut pos).unwrap().ends_with("你好".as_bytes()));
        skip_attrs(&converted, &mut pos).unwrap();
        assert_eq!(string(&converted, &mut pos), Some(b"plain".as_slice()));
        string(&converted, &mut pos).unwrap();
        skip_attrs(&converted, &mut pos).unwrap();
        assert_eq!(pos, converted.len());
    }

    #[test]
    fn passes_malformed_packets_through() {
        let truncated = vec![FXP_NAME, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0];
        assert_eq!(reply(truncated.clone(), gbk()), truncated);
        let truncated = vec![FXP_OPEN, 0, 0, 0, 1, 0, 0, 0, 9, b'a'];
        assert_eq!(request(truncated.clone(), gbk()), truncated);
    }

    #[tokio::test]
    async fn pumps_both_ways() {
        let (channel, mut server) = tokio::io::duplex(4096);
        let mut client = convert(channel, gbk());

        write_packet(&mut client, &packet(FXP_STAT, 1, &["你好".as_bytes()], &[])).await.unwrap();
        assert_eq!(read_packet(&mut server).await.unwrap(), packet(FXP_STAT, 1, &[NI_HAO], &[]));

        let mut name = vec![FXP_NAME, 0, 0, 0, 2, 0, 0, 0, 1];
        put_string(&mut name, NI_HAO);
        put_string(&mut name, NI_HAO);
        name.extend_from_slice(&0u32.to_be_bytes());
        write_packet(&mut server, &name).await.unwrap();
        let received = read_packet(&mut client).await.unwrap();
        let mut pos = 9;
        assert_eq!(string(&received, &mut pos), Some("你好".as_bytes()));
    }
}
