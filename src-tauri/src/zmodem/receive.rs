//! Receiving files (the remote runs `sz`).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncWriteExt, BufWriter};

use super::frame::{Encoding, Header, Kind, CANFC32, CANFDX, CANOVIO, ESCCTL, ZCRCE, ZCRCG, ZCRCQ, ZCRCW};
use super::link::{is_timeout, Link, TIMEOUT};
use super::Report;
use crate::error::Error;
use crate::sftp::transfer::unique_path;

/// Timeouts in a row before giving up.
const MAX_RETRIES: u32 = 5;
const WRITE_BUFFER: usize = 256 * 1024;

/// Full duplex, overlapped I/O and 32-bit CRCs; and please escape control characters, which
/// bastion hosts and some terminals don't pass through.
const OUR_FLAGS: u8 = CANFDX | CANOVIO | CANFC32 | ESCCTL;

/// The file being received.
struct Incoming {
    name: String,
    path: PathBuf,
    file: BufWriter<tokio::fs::File>,
    offset: u64,
    modified: Option<SystemTime>,
}

/// Receives files into `dir` until the sender ends the session. A file left incomplete (by
/// an error or a cancel) is deleted.
pub async fn receive(link: &mut Link, dir: &Path, report: &mut impl Report) -> Result<()> {
    let mut current: Option<Incoming> = None;
    let result = run(link, dir, report, &mut current).await;
    if let Some(incoming) = current {
        drop(incoming.file);
        let _ = tokio::fs::remove_file(&incoming.path).await;
    }
    result
}

async fn run(link: &mut Link, dir: &Path, report: &mut impl Report, current: &mut Option<Incoming>) -> Result<()> {
    let rinit = Header::with_flags(Kind::Rinit, OUR_FLAGS);
    link.send_header(rinit, Encoding::Hex).await?;
    let mut retries = 0;
    let mut data = Vec::with_capacity(8192);
    loop {
        let header = match link.header(TIMEOUT).await {
            Ok(header) => header,
            Err(e) if is_timeout(&e) && retries < MAX_RETRIES => {
                retries += 1;
                // Ask again for what we are missing.
                match current {
                    Some(incoming) => link.send_header(Header::with_pos(Kind::Rpos, incoming.offset), Encoding::Hex).await?,
                    None => link.send_header(rinit, Encoding::Hex).await?,
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        retries = 0;
        match header.kind {
            Kind::Rqinit => link.send_header(rinit, Encoding::Hex).await?,
            Kind::Sinit => {
                // Options and an attention string, neither of which matters here.
                if link.subpacket(&mut data).await?.is_ok() {
                    link.send_header(Header::new(Kind::Ack), Encoding::Hex).await?;
                }
            }
            Kind::File => {
                if link.subpacket(&mut data).await?.is_err() {
                    link.send_header(Header::new(Kind::Nak), Encoding::Hex).await?;
                    continue;
                }
                let info = FileInfo::parse(&data);
                if let Some(incoming) = current.as_ref().filter(|incoming| incoming.name == info.name) {
                    // Our ZRPOS was lost and the sender is offering the same file again.
                    link.send_header(Header::with_pos(Kind::Rpos, incoming.offset), Encoding::Hex).await?;
                    continue;
                }
                let Some(name) = safe_name(&info.name) else {
                    report.failed(Error::new("zmodem.invalidName").param("name", &info.name).into());
                    link.send_header(Header::new(Kind::Skip), Encoding::Hex).await?;
                    continue;
                };
                let path = unique_path(dir.join(&name));
                let file = match tokio::fs::File::create(&path).await {
                    Ok(file) => file,
                    Err(e) => {
                        report.failed(anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path.display())));
                        link.send_header(Header::new(Kind::Skip), Encoding::Hex).await?;
                        continue;
                    }
                };
                report.start(&name, info.size);
                *current = Some(Incoming {
                    name: info.name,
                    path,
                    file: BufWriter::with_capacity(WRITE_BUFFER, file),
                    offset: 0,
                    modified: info.modified,
                });
                link.send_header(Header::with_pos(Kind::Rpos, 0), Encoding::Hex).await?;
            }
            Kind::Data => {
                let Some(incoming) = current.as_mut() else {
                    link.send_header(rinit, Encoding::Hex).await?;
                    continue;
                };
                if u64::from(header.pos()) != incoming.offset & 0xffff_ffff {
                    link.send_header(Header::with_pos(Kind::Rpos, incoming.offset), Encoding::Hex).await?;
                    continue;
                }
                loop {
                    let Ok(end) = link.subpacket(&mut data).await? else {
                        // Resume from what we have; the junk until the next header is skipped.
                        link.send_header(Header::with_pos(Kind::Rpos, incoming.offset), Encoding::Hex).await?;
                        break;
                    };
                    incoming
                        .file
                        .write_all(&data)
                        .await
                        .with_context(|| Error::new("transfer.downloadFailed").param("path", incoming.path.display()))?;
                    incoming.offset += data.len() as u64;
                    report.progress(incoming.offset);
                    match end {
                        ZCRCW => {
                            link.send_header(Header::with_pos(Kind::Ack, incoming.offset), Encoding::Hex).await?;
                            break;
                        }
                        ZCRCQ => link.send_header(Header::with_pos(Kind::Ack, incoming.offset), Encoding::Hex).await?,
                        ZCRCG => {}
                        ZCRCE => break,
                        _ => unreachable!(),
                    }
                }
            }
            Kind::Eof => {
                let Some(incoming) = current.as_ref() else {
                    link.send_header(rinit, Encoding::Hex).await?;
                    continue;
                };
                // An EOF for a position we haven't reached yet is stale; the data will be
                // resent from our ZRPOS.
                if u64::from(header.pos()) != incoming.offset & 0xffff_ffff {
                    continue;
                }
                let mut incoming = current.take().unwrap();
                finish(&mut incoming).await?;
                report.received(&incoming.path, incoming.offset);
                link.send_header(rinit, Encoding::Hex).await?;
            }
            Kind::Fin => {
                link.send_header(Header::new(Kind::Fin), Encoding::Hex).await?;
                link.over_and_out().await;
                return Ok(());
            }
            Kind::Can | Kind::Abort => bail!(Error::new("zmodem.remoteCancelled")),
            // ZFREECNT, ZCOMMAND and the rest: not supported, and not sent by sz.
            _ => {}
        }
    }
}

async fn finish(incoming: &mut Incoming) -> Result<()> {
    let context = || Error::new("transfer.downloadFailed").param("path", incoming.path.display());
    incoming.file.flush().await.with_context(context)?;
    if let Some(modified) = incoming.modified {
        let file = incoming.file.get_ref().try_clone().await.with_context(context)?.into_std().await;
        let _ = tokio::task::spawn_blocking(move || file.set_modified(modified)).await;
    }
    Ok(())
}

/// The `ZFILE` subpacket: the name, then (optionally) "length mtime mode ..." with the
/// time and mode in octal.
struct FileInfo {
    name: String,
    size: Option<u64>,
    modified: Option<SystemTime>,
}

impl FileInfo {
    fn parse(data: &[u8]) -> Self {
        let mut parts = data.splitn(2, |&b| b == 0);
        let name = String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned();
        let rest = parts.next().unwrap_or_default();
        let rest = String::from_utf8_lossy(rest.split(|&b| b == 0).next().unwrap_or_default()).into_owned();
        let mut fields = rest.split_ascii_whitespace();
        let size = fields.next().and_then(|s| s.parse().ok());
        let modified = fields
            .next()
            .and_then(|s| u64::from_str_radix(s, 8).ok())
            .filter(|&secs| secs > 0)
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
        Self { name, size, modified }
    }
}

/// The last component of the sender's path, if it is a usable file name.
fn safe_name(name: &str) -> Option<String> {
    let name = name.rsplit(['/', '\\']).next()?.trim();
    let invalid = name.is_empty() || name == "." || name == ".." || name.chars().any(|c| c.is_control());
    (!invalid).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_info() {
        let info = FileInfo::parse(b"report.pdf\x0012345 14712345671 100644 0 1 12345\x00");
        assert_eq!(info.name, "report.pdf");
        assert_eq!(info.size, Some(12345));
        assert_eq!(info.modified, Some(SystemTime::UNIX_EPOCH + Duration::from_secs(0o14712345671)));
        let bare = FileInfo::parse(b"a.txt\x00\x00");
        assert_eq!((bare.name.as_str(), bare.size, bare.modified), ("a.txt", None, None));
    }

    #[test]
    fn keeps_only_safe_names() {
        assert_eq!(safe_name("dir/sub/a.txt").as_deref(), Some("a.txt"));
        assert_eq!(safe_name("..\\evil.exe").as_deref(), Some("evil.exe"));
        assert_eq!(safe_name("dir/.."), None);
        assert_eq!(safe_name("dir/"), None);
        assert_eq!(safe_name("bad\x01name"), None);
    }
}
