//! What a session does once its shell has exited, if no other tab uses its connection.
//!
//! Its forwarding rules stop listening, moving to another connection of the saved session if
//! there is one. The connections they carry are not cut: like OpenSSH, which waits for them
//! before it exits, the session stays open (the tab says so) until they end. Enter starts
//! another shell on the connection instead (the rules that stopped start again, and the
//! connections go on, to be waited for again when that shell exits), Ctrl+C ends them at once,
//! and so does closing the tab.

use tokio::time::Instant;

use super::Connections;
use crate::forward::{at, Draining, Throttle};
use crate::session::{SessionEvent, SessionId, SessionInput, TermIo, Tone};
use crate::ssh;

/// Session `id`'s [`ssh::ShellExit`].
pub struct ShellExit {
    connections: Connections,
    id: SessionId,
}

impl Connections {
    /// What session `id` does once its shell has exited (see [`crate::connections::exit`]).
    pub fn shell_exit(&self, id: SessionId) -> ShellExit {
        ShellExit { connections: self.clone(), id }
    }
}

impl ssh::ShellExit for ShellExit {
    async fn exited(&mut self, io: &mut TermIo) -> bool {
        // Not when the tab is closing, which has unregistered it.
        let Some((connection, draining, stopped)) = self.connections.release_forwards(self.id).await else {
            return false;
        };
        if draining.is_empty() {
            return false;
        }
        let Some(draining) = wait(draining, io).await else { return false };
        self.connections.restart_forwards(&connection, draining, stopped);
        true
    }
}

/// Waits for the connections of `draining` to end, reporting how many are left, unless the
/// user presses Enter (gives them back: another shell, and they go on) or Ctrl+C, or the tab
/// closes (they end).
async fn wait(mut draining: Draining, io: &mut TermIo) -> Option<Draining> {
    let message = t!("terminal.waitingForForwards", forwards = draining.rules().join(", "));
    let hint = t!("terminal.waitingHint");
    io.print("\n");
    io.notice(Tone::Dim, &format!("{message}\n{hint}"));
    let report = |io: &TermIo, draining: &Draining| io.event(SessionEvent::Waiting { connections: draining.len() });
    let mut throttle = Throttle::default();
    throttle.changed(Instant::now());
    report(io, &draining);
    let new_shell = loop {
        tokio::select! {
            () = draining.next_ended() => {
                if draining.is_empty() {
                    return None;
                }
                if throttle.changed(Instant::now()) {
                    report(io, &draining);
                }
            }
            () = at(throttle.due()) => {
                throttle.reported(Instant::now());
                report(io, &draining);
            }
            input = io.recv(0) => match input {
                Some(SessionInput::Data(bytes)) if bytes == b"\r" => break true,
                Some(SessionInput::Data(bytes)) if bytes == b"\x03" => break false,
                Some(_) => {}
                None => break false,
            },
        }
    };
    new_shell.then_some(draining)
}
