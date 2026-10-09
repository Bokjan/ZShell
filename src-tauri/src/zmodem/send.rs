//! Sending files (the remote runs `rz`).

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};

use super::frame::{self, Encoding, Header, Kind, CANFC32, CANFDX, CANOVIO, ZCBIN, ZCRCE, ZCRCG, ZCRCW};
use super::link::{is_timeout, Link};
use super::Report;
use crate::error::Error;

/// Timeouts in a row before giving up.
const MAX_RETRIES: u32 = 5;
/// Data subpacket size: the classic ZMODEM block, which every receiver accepts.
const BLOCK: usize = 1024;
const READ_BUFFER: usize = 256 * 1024;

/// A file to send, with what the receiver is told about it.
struct Outgoing<'a> {
    path: &'a Path,
    name: String,
    size: u64,
    modified: u64,
    mode: u32,
}

/// Sends `paths` to a receiver that has announced itself with `rinit`, then ends the
/// session. Files that cannot be read are reported and skipped.
pub async fn send(link: &mut Link, rinit: Header, paths: &[PathBuf], report: &mut impl Report) -> Result<()> {
    let encoding = if rinit.zf0() & CANFC32 != 0 { Encoding::Bin32 } else { Encoding::Bin16 };
    // A receiver that can't take data while writing, or has a limited buffer (ZP0/ZP1),
    // acknowledges each subpacket.
    let streaming = rinit.zf0() & (CANFDX | CANOVIO) == CANFDX | CANOVIO && rinit.data[0] == 0 && rinit.data[1] == 0;

    let mut files = Vec::new();
    for path in paths {
        match describe(path).await {
            Ok(file) => files.push(file),
            Err(e) => report.failed(e),
        }
    }
    let mut bytes_left: u64 = files.iter().map(|f| f.size).sum();
    for (index, file) in files.iter().enumerate() {
        let files_left = files.len() - index;
        let mut info = crate::encoding::encode(link.charset, &file.name);
        info.extend(format!("\0{} {:o} {:o} 0 {files_left} {bytes_left}\0", file.size, file.modified, file.mode).into_bytes());
        bytes_left -= file.size;
        match send_file(link, file, &info, encoding, streaming, report).await {
            Ok(()) => {}
            // A file that went away or became unreadable midway; the receiver is told it ended.
            Err(e) if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "transfer.readFailed") => report.failed(e),
            Err(e) => return Err(e),
        }
    }
    finish(link).await
}

async fn describe(path: &Path) -> Result<Outgoing<'_>> {
    let context = || Error::new("transfer.readFailed").param("path", path.display());
    let meta = tokio::fs::metadata(path).await.with_context(context)?;
    if !meta.is_file() {
        bail!(Error::new("zmodem.notFile").param("path", path.display()));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    // No Unix permissions to pass on; the receiver uses its default.
    #[cfg(not(unix))]
    let mode = 0;
    Ok(Outgoing { path, name, size: meta.len(), modified, mode })
}

async fn send_file(
    link: &mut Link,
    file: &Outgoing<'_>,
    info: &[u8],
    encoding: Encoding,
    streaming: bool,
    report: &mut impl Report,
) -> Result<()> {
    let crc32 = encoding == Encoding::Bin32;
    let read_error = || Error::new("transfer.readFailed").param("path", file.path.display());
    let mut reader = BufReader::with_capacity(READ_BUFFER, tokio::fs::File::open(file.path).await.with_context(read_error)?);

    let mut offer = frame::encode_header(&Header::with_flags(Kind::File, ZCBIN), encoding);
    frame::encode_subpacket(&mut offer, info, ZCRCW, crc32);
    let mut pos = match ask(link, &offer, 0, Offer::File).await? {
        Answer::Pos(pos) => pos,
        Answer::Skip | Answer::Done => {
            report.skipped(&file.name);
            return Ok(());
        }
    };
    report.start(&file.name, Some(file.size));

    let mut block = vec![0u8; BLOCK];
    let mut packet = Vec::with_capacity(BLOCK * 2 + 16);
    // Whether the reader has to move to `pos`: at the start (the receiver may resume a file it
    // has part of) and when the receiver asks for other data.
    let mut reposition = true;
    let mut retries = 0;
    'frame: loop {
        if reposition {
            reader.seek(SeekFrom::Start(pos)).await.with_context(read_error)?;
            reposition = false;
        }
        link.send(&frame::encode_header(&Header::with_pos(Kind::Data, pos), encoding)).await?;
        loop {
            let n = read_block(&mut reader, &mut block).await.with_context(read_error)?;
            if n == 0 {
                break;
            }
            packet.clear();
            frame::encode_subpacket(&mut packet, &block[..n], if streaming { ZCRCG } else { ZCRCW }, crc32);
            link.send(&packet).await?;
            pos += n as u64;
            report.progress(pos);
            let reply = if streaming {
                match link.header_if_any().await? {
                    Some(header) => Reply::Header(header),
                    None => continue,
                }
            } else {
                link.flush().await?;
                acknowledgement(link, pos).await?
            };
            match reply {
                // ZCRCW ended the frame: the next subpacket needs a ZDATA header of its own.
                Reply::Header(header) if header.kind == Kind::Ack => {
                    retries = 0;
                    continue 'frame;
                }
                Reply::Header(header) if header.kind == Kind::Rpos => {
                    pos = unwrap_pos(header.pos(), pos);
                    reposition = true;
                    continue 'frame;
                }
                Reply::Header(header) if header.kind == Kind::Skip => {
                    report.skipped(&file.name);
                    return Ok(());
                }
                Reply::Header(header) if matches!(header.kind, Kind::Can | Kind::Abort) => bail!(Error::new("zmodem.remoteCancelled")),
                Reply::Header(_) => {}
                // No acknowledgement: send the subpacket again, as lrzsz does.
                Reply::Timeout(e) => {
                    retries += 1;
                    if retries > MAX_RETRIES {
                        return Err(e);
                    }
                    pos -= n as u64;
                    reposition = true;
                    continue 'frame;
                }
            }
        }
        // End the frame, then the file; the receiver answers ZRINIT once it has everything.
        packet.clear();
        frame::encode_subpacket(&mut packet, &[], ZCRCE, crc32);
        packet.extend(frame::encode_header(&Header::with_pos(Kind::Eof, pos), encoding));
        match ask(link, &packet, pos, Offer::Eof).await? {
            Answer::Done => break,
            Answer::Pos(resume) => {
                pos = resume;
                reposition = true;
            }
            Answer::Skip => {
                report.skipped(&file.name);
                return Ok(());
            }
        }
    }
    report.sent(&file.name, file.size);
    Ok(())
}

enum Reply {
    Header(Header),
    Timeout(anyhow::Error),
}

/// Waits for the receiver's answer to a ZCRCW subpacket that ended at `pos`: its ZACK, or a
/// header asking for something else. Anything else (a late ZACK for a subpacket sent again,
/// a repeated ZRINIT) is skipped.
async fn acknowledgement(link: &mut Link, pos: u64) -> Result<Reply> {
    loop {
        let header = match link.header(link.timeout).await {
            Ok(header) => header,
            Err(e) if is_timeout(&e) => return Ok(Reply::Timeout(e)),
            Err(e) => return Err(e),
        };
        match header.kind {
            Kind::Ack if u64::from(header.pos()) == pos & 0xffff_ffff => return Ok(Reply::Header(header)),
            Kind::Rpos | Kind::Skip | Kind::Can | Kind::Abort => return Ok(Reply::Header(header)),
            _ => {}
        }
    }
}

/// What `ask` is waiting for an answer to.
#[derive(PartialEq)]
enum Offer {
    File,
    Eof,
}

enum Answer {
    /// Send (again) from this position.
    Pos(u64),
    /// The receiver declined the file.
    Skip,
    /// The receiver has the whole file and is ready for the next one.
    Done,
}

/// Sends `message` (a file offer or its end) and waits for the receiver's answer, sending it
/// again when the receiver asks or doesn't answer. `pos` is where the data stands, to make
/// sense of 32-bit positions.
async fn ask(link: &mut Link, message: &[u8], pos: u64, offer: Offer) -> Result<Answer> {
    let mut retries = 0;
    'send: loop {
        link.send(message).await?;
        link.flush().await?;
        loop {
            let header = match link.header(link.timeout).await {
                Ok(header) => header,
                Err(e) if is_timeout(&e) && retries < MAX_RETRIES => {
                    retries += 1;
                    continue 'send;
                }
                Err(e) => return Err(e),
            };
            match header.kind {
                Kind::Rpos => return Ok(Answer::Pos(unwrap_pos(header.pos(), pos))),
                Kind::Skip => return Ok(Answer::Skip),
                // At a file offer, a ZRINIT means the offer was lost (or is the receiver
                // repeating itself from before); at the end, that the file is complete.
                Kind::Rinit if offer == Offer::Eof => return Ok(Answer::Done),
                Kind::Rinit | Kind::Nak => continue 'send,
                Kind::Can | Kind::Abort | Kind::Fin => bail!(Error::new("zmodem.remoteCancelled")),
                // A late ZACK, or something not used here (ZCRC needs an option we don't set).
                _ => {}
            }
        }
    }
}

/// Ends the session: ZFIN both ways, then "OO".
async fn finish(link: &mut Link) -> Result<()> {
    let mut retries = 0;
    loop {
        link.send_header(Header::new(Kind::Fin), Encoding::Hex).await?;
        match link.header(link.timeout).await {
            Ok(header) if header.kind == Kind::Fin => break,
            Ok(_) => {}
            Err(e) if is_timeout(&e) && retries < MAX_RETRIES => retries += 1,
            Err(e) => return Err(e),
        }
    }
    link.send(b"OO").await?;
    link.flush().await
}

/// Fills `block` as far as the file allows; 0 at the end.
async fn read_block(reader: &mut BufReader<tokio::fs::File>, block: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < block.len() {
        let n = reader.read(&mut block[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// A 32-bit position from the wire as a file offset, taking the one nearest to `near` (files
/// over 4 GiB).
fn unwrap_pos(wire: u32, near: u64) -> u64 {
    let base = near & !0xffff_ffff;
    [base.wrapping_sub(1 << 32), base, base + (1 << 32)]
        .into_iter()
        .map(|b| b | u64::from(wire))
        .min_by_key(|p| p.abs_diff(near))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_positions_past_4_gib() {
        assert_eq!(unwrap_pos(100, 50), 100);
        assert_eq!(unwrap_pos(10, (1 << 32) + 20), (1 << 32) + 10);
        assert_eq!(unwrap_pos(0xffff_fff0, (1 << 32) + 5), 0xffff_fff0);
    }
}
