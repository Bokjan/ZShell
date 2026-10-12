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
    routes: RemoteRoutes,
    /// Receives why the connection ended.
    disconnect: watch::Sender<Option<EndReason>>,
}

/// Why a connection ended.
#[derive(Clone, Debug)]
pub struct EndReason {
    /// Untranslated technical detail: the server's message, or what failed.
    pub detail: String,
    /// The server ended it (`SSH_MSG_DISCONNECT`), rather than the connection failing.
    pub by_server: bool,
}

impl EndReason {
    pub fn of(reason: &DisconnectReason<russh::Error>) -> Self {
        match reason {
            DisconnectReason::ReceivedDisconnect(info) => {
                let detail = if info.message.is_empty() { format!("{:?}", info.reason_code) } else { info.message.clone() };
                Self { detail, by_server: true }
            }
            DisconnectReason::Error(e) => Self { detail: e.to_string(), by_server: false },
        }
    }
}

impl ClientHandler {
    pub fn new(
        host: String,
        port: u16,
        forward_agent: bool,
        queries: mpsc::Sender<HostKeyQuery>,
        routes: RemoteRoutes,
        disconnect: watch::Sender<Option<EndReason>>,
    ) -> Self {
        Self { host, port, forward_agent, queries, routes, disconnect }
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    /// Called for the initial key exchange only: russh 0.64 doesn't ask again when keys are
    /// exchanged anew later in the session, so a different key then goes unnoticed (see
    /// docs/TODO.md).
    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };

        let status = match known_hosts::check(&self.host, self.port, &key) {
            Ok(Check::Known) => return Ok(true),
            Ok(Check::Unknown) => HostKeyStatus::Unknown { others: Vec::new() },
            Ok(Check::OtherTypesKnown(others)) => HostKeyStatus::Unknown { others },
            Ok(Check::Changed { path, line }) => HostKeyStatus::Changed { path, line },
            Ok(Check::Revoked { path, .. }) => HostKeyStatus::Revoked { path },
            Err(e) => return Err(std::io::Error::other(e.to_string()).into()),
        };

        let (reply, answer) = oneshot::channel();
        let query = HostKeyQuery { key, status, reply };
        if self.queries.send(query).await.is_err() {
            return Ok(false);
        }
        Ok(answer.await.unwrap_or(false))
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
        self.disconnect.send_replace(Some(EndReason::of(&reason)));
        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            // Returned so that the connection task ends with it, as russh expects.
            DisconnectReason::Error(e) => Err(e),
        }
    }
}
