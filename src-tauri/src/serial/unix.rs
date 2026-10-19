//! Serial lines on Unix driven by the session's task: tokio waits for the device to be
//! readable or writable ([`AsyncFd`]), so a session takes no thread and wakes nothing while
//! the line is quiet. Reads and writes go straight to the descriptor (non-blocking, see
//! `non_blocking`): serialport's own wait for data first would hold up tokio's thread.

use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, RawFd};
use std::pin::Pin;

use serialport::{SerialPort, TTYPort};
use tokio::io::unix::AsyncFd;
use tokio::time::Sleep;

use super::{Output, BREAK_DURATION, READ_BUFFER};
use crate::error::Error;
use crate::session::{Outcome, SessionInput, TermIo};

/// Bridges the port to the terminal until the device fails or the tab closes. Input the
/// device takes no more of (its flow control holds the line) waits here, so keystrokes are
/// still read: Ctrl+C cancels a ZMODEM upload that the line stalls (see [`TermIo::recv`]).
pub(super) async fn bridge(port: AsyncFd<TTYPort>, device: &str, io: &mut TermIo) -> Outcome {
    let lost = |detail: &dyn std::fmt::Display| Outcome::Lost(Error::new("serial.disconnected").param("device", device).detail(detail));
    let sink = io.sink();
    let flow = sink.flow();
    let mut buffer = vec![0; READ_BUFFER];
    let mut pending: VecDeque<Output> = VecDeque::new();
    let mut pending_bytes = 0;
    // While a break holds the line; what was typed after it waits.
    let mut breaking: Option<Pin<Box<Sleep>>> = None;
    loop {
        // Breaks are sent in turn with the data around them.
        while breaking.is_none() && matches!(pending.front(), Some(Output::Break)) {
            pending.pop_front();
            // Not every adapter (or pseudo terminal) can send a break; that is no reason to
            // end the session.
            if port.get_ref().set_break().is_ok() {
                breaking = Some(Box::pin(tokio::time::sleep(BREAK_DURATION)));
            }
        }
        let writing = breaking.is_none() && !pending.is_empty();
        tokio::select! {
            // Not read while the terminal is behind: the driver's buffer, then flow control
            // (if the line has any), holds the device back meanwhile.
            ready = async {
                flow.ready().await;
                port.readable().await
            } => {
                let read = match ready {
                    Ok(mut ready) => match ready.try_io(|port| read_on(port.as_raw_fd(), &mut buffer)) {
                        Ok(read) => read,
                        Err(_would_block) => continue,
                    },
                    Err(e) => Err(e),
                };
                match read {
                    Ok(0) => return lost(&io::Error::from(ErrorKind::UnexpectedEof)),
                    Ok(n) => sink.output(buffer[..n].to_vec()),
                    Err(e) => return lost(&e),
                }
            }
            ready = port.writable(), if writing => {
                let Some(Output::Data(data)) = pending.front_mut() else { continue };
                let written = match ready {
                    Ok(mut ready) => match ready.try_io(|port| write_fd(port.as_raw_fd(), data)) {
                        Ok(written) => written,
                        Err(_would_block) => continue,
                    },
                    Err(e) => Err(e),
                };
                match written {
                    Ok(n) => {
                        pending_bytes -= n;
                        if n == data.len() {
                            pending.pop_front();
                        } else {
                            data.drain(..n);
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => return lost(&e),
                }
            }
            () = async { breaking.as_mut().unwrap().await }, if breaking.is_some() => {
                breaking = None;
                let _ = port.get_ref().clear_break();
            }
            input = io.recv(pending_bytes) => match input {
                Some(SessionInput::Data(data) | SessionInput::Raw(data)) => {
                    pending_bytes += data.len();
                    pending.push_back(Output::Data(data));
                }
                Some(SessionInput::Break) => pending.push_back(Output::Break),
                Some(SessionInput::Resize { .. }) => {}
                // The tab is closing.
                None => return Outcome::Exited(None),
            },
        }
    }
}

/// Reads on while more is ready: an adapter hands over a few dozen bytes at a time, and
/// each read becomes a message to the web view. Fails with `WouldBlock` only when nothing
/// was ready.
fn read_on(fd: RawFd, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        // SAFETY: the rest of `buffer` is valid for writes of its length.
        let n = unsafe { libc::read(fd, buffer[read..].as_mut_ptr().cast(), buffer.len() - read) };
        match usize::try_from(n) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => {
                let e = io::Error::last_os_error();
                match e.kind() {
                    ErrorKind::Interrupted => {}
                    _ if read == 0 => return Err(e),
                    // What was read is passed on now; a failure comes again next time.
                    _ => break,
                }
            }
        }
    }
    Ok(read)
}

fn write_fd(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    // SAFETY: `data` is valid for reads of its length.
    let written = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    usize::try_from(written).map_err(|_| io::Error::last_os_error())
}
