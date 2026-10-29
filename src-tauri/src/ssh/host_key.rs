//! Server host key verification against `~/.ssh/known_hosts` (shared with OpenSSH) and the
//! other files OpenSSH reads.
//!
//! russh calls [`super::handler::ClientHandler::check_server_key`] from its own task during
//! key exchange, so the handler cannot talk to the terminal directly. Instead it sends a
//! [`HostKeyQuery`] to the session task, which asks the user and replies, and warnings for
//! the session task to show.

use std::path::PathBuf;

use russh::keys::{HashAlg, PublicKey};
use tokio::sync::oneshot;

use super::known_hosts::{self, host_pattern, OtherKey};
use crate::session::TermIo;

pub enum HostKeyStatus {
    /// Not recorded; `others` are the keys of other types recorded for the host.
    Unknown { others: Vec<OtherKey> },
    /// Another key is recorded on this line of this file.
    Changed { path: PathBuf, line: usize },
    /// Marked `@revoked` in this file.
    Revoked { path: PathBuf },
    /// A certificate whose authority is marked `@revoked` in this file.
    RevokedAuthority { path: PathBuf },
    /// A certificate from an authority trusted for the host, with a signature that doesn't
    /// verify.
    BadCertificateSignature,
}

/// What the handler sends the session task.
pub enum HostKeyMessage {
    Query(HostKeyQuery),
    /// A translated warning to show, e.g. about a certificate set aside.
    Warning(String),
    /// Why the host key couldn't be checked (a known_hosts file can't be read), with its
    /// code: russh only passes on the `io::Error` the handler fails with.
    Failed(crate::error::Error),
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
    match &query.status {
        HostKeyStatus::Changed { path, line } => {
            log::warn!("host key refused: {algorithm} {fingerprint} differs from the one on line {line} of {}", path.display());
            let banner = t!("hostKey.changedBanner");
            // The real path, as OpenSSH shows it, rather than `~` (unfamiliar on Windows).
            // Settings only list the user's own file. The `ssh-keygen -f "<path>" -R "<host>"`
            // the message suggests has double quotes, which cmd, PowerShell and POSIX shells
            // all take.
            let key = if known_hosts::path().as_ref() == Some(path) { "hostKey.changedDetails" } else { "hostKey.changedDetailsElsewhere" };
            let details = t!(
                key,
                algorithm = algorithm,
                fingerprint = fingerprint,
                line = line,
                path = path.display(),
                host = host_pattern(host, port)
            );
            io.print(&format!("\x1b[1;31m{banner}\x1b[0m\n{details}\n"));
            false
        }
        HostKeyStatus::Revoked { path } => {
            log::warn!("host key refused: {algorithm} {fingerprint} is revoked in {}", path.display());
            let banner = t!("hostKey.revokedBanner");
            let details = t!("hostKey.revokedDetails", algorithm = algorithm, host = host_pattern(host, port), path = path.display());
            io.print(&format!("\x1b[1;31m{banner}\x1b[0m\n{details}\n"));
            false
        }
        HostKeyStatus::RevokedAuthority { path } => {
            log::warn!("host certificate refused: its authority is revoked in {}", path.display());
            // OpenSSH shows the revoked key banner here too, with the certificate's type.
            let banner = t!("hostKey.revokedBanner");
            let details = t!("hostKey.revokedAuthorityDetails", host = host_pattern(host, port), path = path.display());
            io.print(&format!("\x1b[1;31m{banner}\x1b[0m\n{details}\n"));
            false
        }
        HostKeyStatus::BadCertificateSignature => {
            log::warn!("host certificate refused: its signature doesn't verify");
            io.print(&format!("\x1b[1;31m{}\x1b[0m\n", t!("hostKey.certificateBadSignature", host = host)));
            false
        }
        HostKeyStatus::Unknown { others } => {
            log::debug!("unknown host key {algorithm} {fingerprint} ({} of other types known): asking the user", others.len());
            // As OpenSSH: the keys of other types it has for the host, then a question that
            // says so, since a server the user trusts would normally show one of those.
            for other in others {
                let found = t!(
                    "hostKey.otherTypeFound",
                    kind = other.kind,
                    host = host_pattern(host, port),
                    path = other.path.display(),
                    line = other.line,
                    fingerprint = other.fingerprint
                );
                io.print(&format!("\x1b[33m{found}\x1b[0m\n"));
            }
            let question = if others.is_empty() { "hostKey.unknown" } else { "hostKey.unknownOtherTypes" };
            io.print(&t!(question, host = host, algorithm = algorithm, fingerprint = fingerprint));
            // The answer words stay "yes"/"no" in every language, as in OpenSSH.
            io.print(" (yes/no)? ");
            loop {
                match io.read_line(true).await.as_deref().map(str::trim) {
                    Some(answer) if answer.eq_ignore_ascii_case("yes") => break,
                    Some(answer) if answer.eq_ignore_ascii_case("no") => {
                        log::info!("host key rejected by the user");
                        return false;
                    }
                    None => return false,
                    Some(_) => io.print(&format!("{} ", t!("hostKey.typeYesOrNo"))),
                }
            }
            let (learned, key) = (host.to_owned(), query.key.clone());
            let learned = tokio::task::spawn_blocking(move || known_hosts::learn(&learned, port, &key)).await;
            match learned.unwrap_or_else(|e| Err(crate::error::Error::new("unexpected").detail(e))) {
                Ok(()) => {
                    log::info!("host key accepted by the user and added to known_hosts");
                    io.print(&format!("{}\n", t!("hostKey.added", host = host, algorithm = algorithm)));
                }
                Err(e) => {
                    log::warn!("host key accepted by the user, but can't be saved: {}", e.logged());
                    io.print(&format!("\x1b[33m{}\x1b[0m\n", t!("hostKey.saveFailed", error = e)));
                }
            }
            true
        }
    }
}

pub fn warn(io: &mut TermIo, warning: &str) {
    io.print(&format!("\x1b[33m{warning}\x1b[0m\n"));
}
