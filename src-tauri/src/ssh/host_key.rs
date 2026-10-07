//! Server host key verification against `~/.ssh/known_hosts` (shared with OpenSSH).
//!
//! russh calls [`ClientHandler::check_server_key`] from its own task during key exchange,
//! so the handler cannot talk to the terminal directly. Instead it sends a
//! [`HostKeyQuery`] to the session task, which asks the user and replies.

use russh::client;
use russh::keys::known_hosts::{check_known_hosts, learn_known_hosts};
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use tokio::sync::{mpsc, oneshot};

use crate::session::TermIo;

pub enum HostKeyStatus {
    Unknown,
    Changed { line: usize },
}

pub struct HostKeyQuery {
    pub key: PublicKey,
    pub status: HostKeyStatus,
    pub reply: oneshot::Sender<bool>,
}

pub struct ClientHandler {
    host: String,
    port: u16,
    queries: mpsc::Sender<HostKeyQuery>,
    /// The key accepted during the initial key exchange. Re-keying later in the session must
    /// present the same key, and is checked without involving the user.
    verified: Option<PublicKey>,
}

impl ClientHandler {
    pub fn new(host: String, port: u16, queries: mpsc::Sender<HostKeyQuery>) -> Self {
        Self { host, port, queries, verified: None }
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };
        if let Some(verified) = &self.verified {
            return Ok(*verified == key);
        }

        let status = match check_known_hosts(&self.host, self.port, &key) {
            Ok(true) => {
                self.verified = Some(key);
                return Ok(true);
            }
            Ok(false) => HostKeyStatus::Unknown,
            Err(russh::keys::Error::KeyChanged { line }) => HostKeyStatus::Changed { line },
            Err(e) => return Err(e.into()),
        };

        let (reply, answer) = oneshot::channel();
        let query = HostKeyQuery { key: key.clone(), status, reply };
        if self.queries.send(query).await.is_err() {
            return Ok(false);
        }
        let accepted = answer.await.unwrap_or(false);
        if accepted {
            self.verified = Some(key);
        }
        Ok(accepted)
    }
}

/// Asks the user about an unknown or changed host key; returns whether to proceed.
pub async fn confirm(io: &mut TermIo, host: &str, port: u16, query: &HostKeyQuery) -> bool {
    let algorithm = query.key.algorithm();
    let fingerprint = query.key.fingerprint(HashAlg::Sha256);
    match query.status {
        HostKeyStatus::Changed { line } => {
            io.print(&format!(
                "\x1b[1;31m@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
                 @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
                 @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\x1b[0m\n\
                 Someone could be eavesdropping on you right now (man-in-the-middle attack)!\n\
                 It is also possible that the host key has just been changed.\n\
                 The {algorithm} key fingerprint sent by the remote host is:\n  {fingerprint}\n\
                 It does not match line {line} of ~/.ssh/known_hosts.\n\
                 If the key change is expected, remove that line and reconnect.\n"
            ));
            false
        }
        HostKeyStatus::Unknown => {
            io.print(&format!(
                "The authenticity of host '{host}' can't be established.\n\
                 {algorithm} key fingerprint is {fingerprint}.\n\
                 Are you sure you want to continue connecting (yes/no)? "
            ));
            loop {
                match io.read_line(true).await.as_deref().map(str::trim) {
                    Some(answer) if answer.eq_ignore_ascii_case("yes") => break,
                    Some(answer) if answer.eq_ignore_ascii_case("no") => return false,
                    None => return false,
                    Some(_) => io.print("Please type 'yes' or 'no': "),
                }
            }
            match learn_known_hosts(host, port, &query.key) {
                Ok(()) => io.print(&format!("Permanently added '{host}' ({algorithm}) to the list of known hosts.\n")),
                Err(e) => io.print(&format!("\x1b[33mCould not write to known_hosts: {e}\x1b[0m\n")),
            }
            true
        }
    }
}
