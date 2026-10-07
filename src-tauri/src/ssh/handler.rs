//! russh client callbacks: server host key verification (answered by the session task, see
//! [`super::host_key`]) and channels the server opens for remote port forwards.

use russh::client::{self, ChannelOpenHandle, Msg, Session};
use russh::keys::known_hosts::check_known_hosts;
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::Channel;
use tokio::sync::{mpsc, oneshot};

use super::host_key::{HostKeyQuery, HostKeyStatus};
use crate::forward::{Incoming, RemoteRoutes};

pub struct ClientHandler {
    host: String,
    port: u16,
    queries: mpsc::Sender<HostKeyQuery>,
    /// The key accepted during the initial key exchange. Re-keying later in the session must
    /// present the same key, and is checked without involving the user.
    verified: Option<PublicKey>,
    routes: RemoteRoutes,
}

impl ClientHandler {
    pub fn new(host: String, port: u16, queries: mpsc::Sender<HostKeyQuery>, routes: RemoteRoutes) -> Self {
        Self { host, port, queries, verified: None, routes }
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

    /// Hands the channel to its rule without waiting: this runs on russh's connection task,
    /// and the rule first connects to its target before accepting.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.routes.route(connected_address, connected_port, Incoming { channel, reply });
        Ok(())
    }
}
