//! russh client callbacks: server host key verification (answered by the session task, see
//! [`super::host_key`]), channels the server opens for remote port forwards and agent
//! forwarding, and why the connection ended.

use russh::client::{self, ChannelOpenHandle, DisconnectReason, Msg, Session};
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::{Channel, ChannelOpenFailure};
use tokio::sync::{mpsc, oneshot, watch};

use super::forwarded::{Incoming, RemoteRoutes};
use super::host_key::{HostKeyMessage, HostKeyQuery, HostKeyStatus};
use super::known_hosts::{self, CertificateCheck, Check, Invalid};

pub struct ClientHandler {
    host: String,
    port: u16,
    /// Whether we asked for agent forwarding; agent channels are refused otherwise.
    forward_agent: bool,
    queries: mpsc::Sender<HostKeyMessage>,
    routes: RemoteRoutes,
    /// Receives why the connection ended.
    disconnect: watch::Sender<Option<EndReason>>,
    /// Takes any host key without reading known_hosts: for connections to the test server
    /// (see `super::testing`), which must not touch the user's files.
    #[cfg(test)]
    any_host_key: bool,
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
        queries: mpsc::Sender<HostKeyMessage>,
        routes: RemoteRoutes,
        disconnect: watch::Sender<Option<EndReason>>,
    ) -> Self {
        Self {
            host,
            port,
            forward_agent,
            queries,
            routes,
            disconnect,
            #[cfg(test)]
            any_host_key: false,
        }
    }

    /// Takes any host key, reading no known_hosts file (see `any_host_key`).
    #[cfg(test)]
    pub(super) fn with_any_host_key(self) -> Self {
        Self { any_host_key: true, ..self }
    }

    /// Fails the handshake as `error` says: sent to the session task, which reports it, since
    /// russh keeps only the text of the error returned.
    async fn failed(&self, error: crate::error::Error) -> russh::Error {
        let text = error.to_string();
        let _ = self.queries.send(HostKeyMessage::Failed(error)).await;
        std::io::Error::other(text).into()
    }

    /// Has the session task tell or ask the user about `key`; whether to go on.
    async fn ask(&self, key: PublicKey, status: HostKeyStatus) -> bool {
        let (reply, answer) = oneshot::channel();
        let query = HostKeyQuery { key, status, reply };
        if self.queries.send(HostKeyMessage::Query(query)).await.is_err() {
            return false;
        }
        answer.await.unwrap_or(false)
    }
}

/// Why a certificate from `host` is set aside, as a warning.
fn invalid_certificate(why: &Invalid, host: &str) -> String {
    match why {
        Invalid::SignatureAlgorithm(algorithm) => t!("hostKey.certificateSignatureAlgorithm", host = host, algorithm = algorithm),
        Invalid::Signature => t!("hostKey.certificateBadSignature", host = host),
        Invalid::NotHost => t!("hostKey.certificateNotHost", host = host),
        Invalid::NotYetValid => t!("hostKey.certificateNotYetValid", host = host),
        Invalid::Expired => t!("hostKey.certificateExpired", host = host),
        Invalid::NoPrincipals => t!("hostKey.certificateNoPrincipals", host = host),
        Invalid::WrongHost => t!("hostKey.certificateWrongHost", host = host),
        Invalid::CriticalOptions => t!("hostKey.certificateCriticalOptions", host = host),
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    /// Called for the initial key exchange only: russh 0.64 doesn't ask again when keys are
    /// exchanged anew later in the session, so a different key then goes unnoticed (see
    /// docs/TODO.md).
    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        #[cfg(test)]
        if self.any_host_key {
            return Ok(true);
        }
        let key = server_key.public_key();
        // As OpenSSH: a certificate that isn't trusted, or not valid, is checked as the key in
        // it, unless it is revoked or its signature is bad.
        if let PublicKeyOrCertificate::Certificate(certificate) = server_key {
            let (host, port, certificate) = (self.host.clone(), self.port, certificate.clone());
            let checked = tokio::task::spawn_blocking(move || known_hosts::check_certificate(&host, port, &certificate))
                .await
                .map_err(std::io::Error::other)?;
            match checked {
                Ok(CertificateCheck::Trusted) => return Ok(true),
                Ok(CertificateCheck::Untrusted) => {}
                Ok(CertificateCheck::Revoked { path, authority: false, .. }) => {
                    return Ok(self.ask(key, HostKeyStatus::Revoked { path }).await)
                }
                Ok(CertificateCheck::Revoked { path, authority: true, .. }) => {
                    return Ok(self.ask(key, HostKeyStatus::RevokedAuthority { path }).await)
                }
                Ok(CertificateCheck::Invalid(Invalid::Signature)) => {
                    return Ok(self.ask(key, HostKeyStatus::BadCertificateSignature).await)
                }
                Ok(CertificateCheck::Invalid(why)) => {
                    let warning = invalid_certificate(&why, &self.host);
                    let _ = self.queries.send(HostKeyMessage::Warning(warning)).await;
                }
                Err(e) => return Err(self.failed(e).await),
            }
        }

        // Reading the files (and waiting for a write to them), off russh's connection task.
        let (host, port, checked) = (self.host.clone(), self.port, key.clone());
        let checked =
            tokio::task::spawn_blocking(move || known_hosts::check(&host, port, &checked)).await.map_err(std::io::Error::other)?;
        let status = match checked {
            Ok(Check::Known) => return Ok(true),
            Ok(Check::Unknown) => HostKeyStatus::Unknown { others: Vec::new() },
            Ok(Check::OtherTypesKnown(others)) => HostKeyStatus::Unknown { others },
            Ok(Check::Changed { path, line }) => HostKeyStatus::Changed { path, line },
            Ok(Check::Revoked { path, .. }) => HostKeyStatus::Revoked { path },
            Err(e) => return Err(self.failed(e).await),
        };
        Ok(self.ask(key, status).await)
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

    // Channels ZShell never asks the server for, which russh accepts by default (and then
    // leaves open, connected to nothing): refused, as OpenSSH does.

    async fn server_channel_open_session(&mut self, _: Channel<Msg>, reply: ChannelOpenHandle, _: &mut Session) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_direct_tcpip(
        &mut self,
        _: Channel<Msg>,
        _: &str,
        _: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        _: Channel<Msg>,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        _: Channel<Msg>,
        _: &str,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_direct_streamlocal(
        &mut self,
        _: Channel<Msg>,
        _: &str,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
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
