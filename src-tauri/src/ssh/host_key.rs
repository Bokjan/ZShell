//! Server host key verification against `~/.ssh/known_hosts` (shared with OpenSSH).
//!
//! russh calls [`super::handler::ClientHandler::check_server_key`] from its own task during
//! key exchange, so the handler cannot talk to the terminal directly. Instead it sends a
//! [`HostKeyQuery`] to the session task, which asks the user and replies.

use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{HashAlg, PublicKey};
use tokio::sync::oneshot;

use super::known_hosts::{self, host_pattern};
use crate::session::TermIo;

pub enum HostKeyStatus {
    Unknown,
    Changed { line: usize },
    /// Marked `@revoked` in known_hosts.
    Revoked,
}

pub struct HostKeyQuery {
    pub key: PublicKey,
    pub status: HostKeyStatus,
    pub reply: oneshot::Sender<bool>,
}

/// Asks the user about an unknown or changed host key; returns whether to proceed.
pub async fn confirm(io: &mut TermIo, host: &str, port: u16, query: &HostKeyQuery) -> bool {
    let algorithm = query.key.algorithm();
    let fingerprint = query.key.fingerprint(HashAlg::Sha256);
    match query.status {
        HostKeyStatus::Changed { line } => {
            let banner = t!("hostKey.changedBanner");
            // The real path, as OpenSSH shows it, rather than `~` (unfamiliar on Windows).
            let path = known_hosts::path().unwrap_or_default();
            let details = t!(
                "hostKey.changedDetails",
                algorithm = algorithm,
                fingerprint = fingerprint,
                line = line,
                path = path.display(),
                host = host_pattern(host, port)
            );
            io.print(&format!("\x1b[1;31m{banner}\x1b[0m\n{details}\n"));
            false
        }
        HostKeyStatus::Revoked => {
            let banner = t!("hostKey.revokedBanner");
            let path = known_hosts::path().unwrap_or_default();
            let details = t!("hostKey.revokedDetails", algorithm = algorithm, host = host_pattern(host, port), path = path.display());
            io.print(&format!("\x1b[1;31m{banner}\x1b[0m\n{details}\n"));
            false
        }
        HostKeyStatus::Unknown => {
            io.print(&t!("hostKey.unknown", host = host, algorithm = algorithm, fingerprint = fingerprint));
            // The answer words stay "yes"/"no" in every language, as in OpenSSH.
            io.print(" (yes/no)? ");
            loop {
                match io.read_line(true).await.as_deref().map(str::trim) {
                    Some(answer) if answer.eq_ignore_ascii_case("yes") => break,
                    Some(answer) if answer.eq_ignore_ascii_case("no") => return false,
                    None => return false,
                    Some(_) => io.print(&format!("{} ", t!("hostKey.typeYesOrNo"))),
                }
            }
            match learn_known_hosts(host, port, &query.key) {
                Ok(()) => io.print(&format!("{}\n", t!("hostKey.added", host = host, algorithm = algorithm))),
                Err(e) => io.print(&format!("\x1b[33m{}\x1b[0m\n", t!("hostKey.saveFailed", error = e))),
            }
            true
        }
    }
}
