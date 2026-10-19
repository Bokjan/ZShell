//! Serial lines on Unix driven by the session's task: tokio waits for the device to be
//! readable or writable ([`AsyncFd`]), so a session takes no thread and wakes nothing while
//! the line is quiet. Reads and writes go straight to the descriptor (non-blocking, see
//! `open`): serialport's own wait for data first would hold up tokio's thread.

use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::os::fd::AsRawFd;
use std::pin::Pin;
use std::sync::Arc;

use serialport::{SerialPort, TTYPort};
use tokio::io::unix::AsyncFd;
use tokio::time::Sleep;

use super::{Hold, Output, BREAK_DURATION, READ_BUFFER};
use crate::error::Error;
use crate::fd::{read_on, write_fd};
use crate::session::{Outcome, SessionInput, TermIo};

/// The port and our hold on it, let go of only once the port is closed (fields drop in
/// order): closing a tab aborts its task, which is dropped on some worker thread maybe after
/// a reconnecting session has looked whether the device is free.
struct Held {
    port: AsyncFd<TTYPort>,
    _hold: Arc<Hold>,
}

/// Bridges the port to the terminal until the device fails or the tab closes. Input the
/// device takes no more of (its flow control holds the line) waits here, so keystrokes are
/// still read: Ctrl+C cancels a ZMODEM upload that the line stalls (see [`TermIo::recv`]).
pub(super) async fn bridge(port: AsyncFd<TTYPort>, device: &str, io: &mut TermIo) -> Outcome {
    let held = Held { port, _hold: Hold::new(device) };
    let port = &held.port;
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
