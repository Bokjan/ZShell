//! Terminal sessions.
//!
//! A session is a background task that owns a [`TermIo`]: it streams output bytes to the
//! frontend through a [`tauri::ipc::Channel`], reports lifecycle [`SessionEvent`]s, and
//! consumes [`SessionInput`] (keystrokes, resizes) from the frontend.
//!
//! A session closes only through [`SessionManager`]: [`SessionManager::remove`] as its tab
//! closes, [`SessionManager::remove_webview`] as its page reloads, its window goes away or the
//! app quits, both the same way: it releases what the session holds, then aborts its
//! task. What a session holds beyond its task registers its release on the session's
//! [`Lifetime`], which also runs when the session ends by itself ([`TermIo::finish`]) or its
//! task is dropped, whichever comes first, so nothing depends on the order of these.
//!
//! Port forwards report their state through the same event channel ([`SessionSink`]).
//!
//! Backends: remote shells over SSH (`ssh::run`), Telnet (`telnet::run`), serial lines
//! (`serial::run`) and local shells in a pseudo terminal (`pty::run`).
//!
//! Remote output goes through [`SessionSink::output`], where ZMODEM transfers are detected and
//! take over the session until they end (see [`crate::zmodem`]). What the terminal shows is then
//! decoded from the session's character encoding ([`SessionSink::remote`]), and keystrokes are
//! encoded on the way back ([`TermIo::recv`]); ZMODEM data and our own messages are not.
//!
//! Output is flow controlled: the frontend acknowledges the bytes xterm.js has processed
//! ([`SessionManager::ack`]), and every backend waits on [`Flow`] before reading more while
//! the terminal is too far behind (the remote side is then held back by TCP, the SSH window
//! or the serial line's flow control).
//!
//! A session belongs to the webview whose tab shows it, and can move to another window's tab
//! ([`SessionManager::adopt`]): its output goes to the new tab's channel from then on, and the
//! old tab hears [`SessionEvent::Moved`] after the last output it was sent, which is when it
//! hands its screen over to the new one. Each move is named by the tab it moves to, so that
//! a screen handed over late for an earlier move is never taken for this one's.

mod flow;
mod lifetime;
mod manager;
mod sink;
mod term;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::Error;
use crate::traffic::{ChannelInfo, ConnectionInfo, Counts};
use crate::zmodem;
pub use flow::Flow;
#[cfg(test)]
pub use flow::ACK_BATCH;
pub use lifetime::Lifetime;
pub use manager::SessionManager;
pub use sink::SessionSink;
pub use term::{Foreground, ForegroundProbe, TermIo};

pub type SessionId = u32;

/// What a session's terminal showed in the tab it moved from: written to the new tab's
/// terminal before the output that came since.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: SessionId,
    /// The move it was taken for (see [`SessionEvent::Moved`]).
    pub move_id: String,
    /// The screen and the scrollback, as escape sequences that draw them again.
    pub data: String,
    /// The size of the terminal it was taken from, which it is written into before fitting.
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug)]
pub enum SessionInput {
    /// Typed text (UTF-8), converted to the session's encoding on the way.
    Data(Vec<u8>),
    /// Bytes to pass on as they are, not text: the terminal's mouse reports in the default
    /// (X10) encoding, which past column 95 are not UTF-8. Backends treat them as `Data`;
    /// prompts and ZMODEM transfers ignore them.
    Raw(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// A break signal: on a serial line, or Telnet's `BRK`. Other backends ignore it.
    Break,
}

#[derive(Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    Connected,
    /// `status`: the exit status of the shell, if it reported one. `signal`: the signal that
    /// ended it, if one did.
    Closed {
        reason: CloseReason,
        error: Option<Error>,
        status: Option<u32>,
        signal: Option<String>,
    },
    Forward {
        rule_id: String,
        state: ForwardState,
    },
    /// A ZMODEM transfer needs an answer, is running, or has ended.
    Zmodem {
        phase: zmodem::Phase,
    },
    /// The session's log started (`path`), stopped (neither), or couldn't start (`error`).
    Log {
        path: Option<String>,
        error: Option<Error>,
    },
    /// The shell has exited, and the session waits for the connections its forwarding rules
    /// still carry to end: `connections` of them (see `crate::connections`).
    Waiting {
        connections: usize,
    },
    /// What the session's terminal has sent and received (see [`crate::traffic`]), and the
    /// channel it runs on, for an SSH session once its shell has started.
    TerminalTraffic {
        counts: Counts,
        channel: Option<ChannelInfo>,
    },
    /// What the SFTP channels of the session's connection have carried.
    SftpTraffic {
        connection: ConnectionInfo,
        browse: SftpChannelTraffic,
        transfer: SftpChannelTraffic,
    },
    /// What forwarding rule `rule_id` of the session's saved session carries in its current
    /// run, on whichever of the session's connections it runs.
    ForwardTraffic {
        rule_id: String,
        traffic: ForwardTraffic,
    },
    /// The session moved to another window's tab: the last message on this channel.
    /// `move_id`: what the new tab named the move, to pass back with the screen.
    Moved {
        move_id: String,
    },
}

/// The state of a port forwarding rule, as reported to the tabs of its session.
#[derive(Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ForwardState {
    Starting,
    Active {
        /// The address actually listened on (with the real port when 0 was requested).
        bound: String,
        connections: usize,
        /// Why the most recent connection through this rule failed, if one did.
        last_error: Option<Error>,
    },
    Failed {
        error: Error,
    },
    Stopped,
}

/// One of a connection's SFTP channels (browsing, or transfers), for [`SessionEvent::SftpTraffic`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SftpChannelTraffic {
    pub counts: Counts,
    /// The channel it is on now, if it has been opened. Transfers share the browsing channel
    /// when the server refuses them one of their own, and then have its counts too.
    pub channel: Option<u32>,
}

/// A forwarding rule's run, for [`SessionEvent::ForwardTraffic`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ForwardTraffic {
    pub counts: Counts,
    /// The connection it runs on.
    pub connection: ConnectionInfo,
    /// The channels of the connections it carries now, lowest first: the first
    /// [`FORWARD_CHANNELS_LISTED`] of `channel_count`.
    pub channels: Vec<u32>,
    pub channel_count: usize,
}

/// How many of a rule's channels [`ForwardTraffic`] lists.
pub const FORWARD_CHANNELS_LISTED: usize = 20;

#[derive(Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CloseReason {
    /// The shell exited or the remote side closed the session.
    Exited,
    /// The connection broke after the session had started.
    Lost,
    /// Connecting, authenticating or starting the shell failed.
    Failed,
}

/// How a remote session (SSH, Telnet, serial) ended; see [`TermIo::finish`].
pub enum Outcome {
    /// The remote side closed the session, with the shell's exit status if it reported one.
    Exited(Option<u32>),
    /// The remote side closed the session after its shell was ended by the signal named.
    Signaled(String),
    /// A local shell exited, with its exit code, or the signal that ended it.
    ProcessEnded { code: Option<u32>, signal: Option<String> },
    /// The connection broke (or the device went away) after the session had started.
    Lost(Error),
    /// Connecting, authenticating or starting the session failed.
    Failed(Error),
}
