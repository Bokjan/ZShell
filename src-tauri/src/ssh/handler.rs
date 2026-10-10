//! russh client callbacks: server host key verification (answered by the session task, see
//! [`super::host_key`]), channels the server opens for remote port forwards and agent
//! forwarding, and why the connection ended.

use russh::client::{self, ChannelOpenHandle, DisconnectReason, Msg, Session};
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::{Channel, ChannelOpenFailure};
use tokio::sync::{mpsc, oneshot, watch};

use super::host_key::{HostKeyQuery, HostKeyStatus};
use super::known_hosts::{self, Check};
use crate::forward::{Incoming, RemoteRoutes};

pub struct ClientHandler {
    host: String,
    port: u16,
    /// Whether we asked for agent forwarding; agent channels are refused otherwise.
    forward_agent: bool,
    queries: mpsc::Sender<HostKeyQuery>,
    /// The key accepted during the initial key exchange. Re-keying later in the session must
    /// present the same key, and is checked without involving the user.
    verified: Option<PublicKey>,
    routes: RemoteRoutes,
    /// Receives a description of why the connection ended (untranslated technical detail).
    disconnect: watch::Sender<Option<String>>,
}

impl ClientHandler {
    pub fn new(
        host: String,
        port: u16,
        forward_agent: bool,
        queries: mpsc::Sender<HostKeyQuery>,
        routes: RemoteRoutes,
        disconnect: watch::Sender<Option<String>>,
    ) -> Self {
        Self { host, port, forward_agent, queries, verified: None, routes, disconnect }
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

        let status = match known_hosts::check(&self.host, self.port, &key) {
            Ok(Check::Known) => {
                self.verified = Some(key);
                return Ok(true);
            }
            Ok(Check::Unknown) => HostKeyStatus::Unknown { others: Vec::new() },
            Ok(Check::OtherTypesKnown(others)) => HostKeyStatus::Unknown { others },
            Ok(Check::Changed { path, line }) => HostKeyStatus::Changed { path, line },
            Ok(Check::Revoked { path, .. }) => HostKeyStatus::Revoked { path },
            Err(e) => return Err(std::io::Error::other(e.to_string()).into()),
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

    /// Connects each agent channel to the local agent (a connection of its own), off russh's
    /// connection task. Refused when we didn't ask for forwarding (russh would accept it) or
    /// there is no local agent.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.forward_agent {
            reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
            return Ok(());
        }
        tauri::async_runtime::spawn(async move {
            match super::auth::agent_stream().await {
                Ok(mut agent) => {
                    reply.accept().await;
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut agent).await;
                }
                Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
            }
        });
        Ok(())
    }

    async fn disconnected(&mut self, reason: DisconnectReason<Self::Error>) -> Result<(), Self::Error> {
        match reason {
            DisconnectReason::ReceivedDisconnect(info) => {
                let message = if info.message.is_empty() { format!("{:?}", info.reason_code) } else { info.message };
                self.disconnect.send_replace(Some(message));
                Ok(())
            }
            DisconnectReason::Error(e) => {
                self.disconnect.send_replace(Some(e.to_string()));
                // Returned so that the connection task ends with it, as russh expects.
                Err(e)
            }
        }
    }
}
