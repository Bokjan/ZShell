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
                 @    警告：远程主机标识已改变！                           @\n\
                 @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\x1b[0m\n\
                 可能有人正在进行中间人攻击，也可能只是主机密钥被更换了。\n\
                 服务器当前的 {algorithm} 密钥指纹为：\n  {fingerprint}\n\
                 与 ~/.ssh/known_hosts 第 {line} 行记录的不一致。\n\
                 如确认密钥更换是正常的，请删除该行后重新连接。\n"
            ));
            false
        }
        HostKeyStatus::Unknown => {
            io.print(&format!(
                "无法确认主机 '{host}' 的真实性。\n\
                 {algorithm} 密钥指纹为 {fingerprint}。\n\
                 确定要继续连接吗 (yes/no)? "
            ));
            loop {
                match io.read_line(true).await.as_deref().map(str::trim) {
                    Some(answer) if answer.eq_ignore_ascii_case("yes") => break,
                    Some(answer) if answer.eq_ignore_ascii_case("no") => return false,
                    None => return false,
                    Some(_) => io.print("请输入 'yes' 或 'no': "),
                }
            }
            match learn_known_hosts(host, port, &query.key) {
                Ok(()) => io.print(&format!("已将 '{host}' ({algorithm}) 添加到 known_hosts。\n")),
                Err(e) => io.print(&format!("\x1b[33m无法写入 known_hosts：{e}\x1b[0m\n")),
            }
            true
        }
    }
}
