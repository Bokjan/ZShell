//! SSH shell sessions on top of russh.

mod auth;
mod connections;
mod handler;
mod host_key;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

use crate::config::Profile;
use crate::error::Error;
use crate::forward::RemoteRoutes;
use crate::session::{CloseReason, SessionEvent, SessionId, SessionInput, TermIo};
pub use connections::{Connections, SshHandle};
use handler::ClientHandler;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How a session ended.
enum Outcome {
    /// The remote shell exited or closed the channel, with its exit status if reported.
    Exited(Option<u32>),
    /// The connection broke after the shell had started.
    Lost(Error),
    /// Connecting, authenticating or starting the shell failed.
    Failed(Error),
}

/// Session backend: connects, authenticates and bridges a remote shell to the terminal.
pub async fn run(profile: Profile, id: SessionId, mut io: TermIo, connections: Connections) {
    let outcome = match start(&profile, id, &mut io, &connections).await {
        Ok((channel, disconnect)) => bridge(channel, &mut io, disconnect).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    connections.close(id);
    let (reason, error) = match outcome {
        Outcome::Exited(status) => {
            let message = match status {
                Some(status) => t!("terminal.closedWithStatus", status = status),
                None => t!("terminal.closed"),
            };
            io.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
            (CloseReason::Exited, None)
        }
        Outcome::Lost(e) => {
            // The shell may have left the cursor mid-line.
            io.print(&format!("\n\x1b[31m{e}\x1b[0m\n"));
            (CloseReason::Lost, Some(e))
        }
        Outcome::Failed(e) => {
            io.print(&format!("\x1b[31m{e}\x1b[0m\n"));
            (CloseReason::Failed, Some(e))
        }
    };
    io.event(SessionEvent::Closed { reason, error });
}

/// Connects, authenticates and starts the remote shell. Also returns the receiver for the
/// reason the connection ends, reported by the client handler.
async fn start(
    profile: &Profile,
    id: SessionId,
    io: &mut TermIo,
    connections: &Connections,
) -> Result<(Channel<Msg>, watch::Receiver<Option<String>>)> {
    let connecting = t!("terminal.connecting", user = profile.username, host = profile.host, port = profile.port);
    io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    let mut session = connect(profile, io, routes.clone(), disconnect_tx).await?;
    auth::authenticate(&mut session, profile, io).await?;
    let session = Arc::new(session);
    let connection = connections.insert(id, session.clone(), routes, io.sink());
    for rule in profile.forwards.iter().filter(|rule| rule.auto_start) {
        connection.forwards().start(rule.clone(), true);
    }

    let channel = session.channel_open_session().await.context(Error::new("ssh.channelFailed"))?;
    let (cols, rows) = io.size;
    channel.request_pty(false, "xterm-256color", cols.into(), rows.into(), 0, 0, &[]).await?;
    channel.request_shell(false).await?;
    io.event(SessionEvent::Connected);
    Ok((channel, disconnect))
}

/// Copies between the shell channel and the terminal until either side ends.
async fn bridge(channel: Channel<Msg>, io: &mut TermIo, mut disconnect: watch::Receiver<Option<String>>) -> Outcome {
    let (mut reader, writer) = channel.split();
    let mut exit_status = None;
    loop {
        let sent = tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    io.write(data.to_vec());
                    Ok(())
                }
                Some(ChannelMsg::ExitStatus { exit_status: status }) => {
                    exit_status = Some(status);
                    Ok(())
                }
                Some(ChannelMsg::Close) => return Outcome::Exited(exit_status),
                // The connection ended without closing the channel.
                None => break,
                Some(_) => Ok(()),
            },
            input = io.recv() => match input {
                Some(SessionInput::Data(data)) => writer.data_bytes(data).await,
                Some(SessionInput::Resize { cols, rows }) => writer.window_change(cols.into(), rows.into(), 0, 0).await,
                // The tab is closing.
                None => return Outcome::Exited(exit_status),
            },
        };
        if sent.is_err() {
            break;
        }
    }
    // The handler records the reason as the connection task winds down, which can be just
    // after the channel notices; give it a moment.
    let _ = tokio::time::timeout(Duration::from_millis(500), disconnect.wait_for(Option::is_some)).await;
    let error = Error::new("ssh.connectionLost");
    let reason = disconnect.borrow().clone();
    Outcome::Lost(match reason {
        Some(reason) => error.detail(reason),
        None => error,
    })
}

async fn connect(
    profile: &Profile,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle> {
    let target = format!("{}:{}", profile.host, profile.port);
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&target))
        .await
        .map_err(|_| Error::new("ssh.connectTimeout").param("target", &target))?
        .context(Error::new("ssh.connectFailed").param("target", &target))?;
    stream.set_nodelay(true)?;

    let config = Arc::new(client::Config {
        keepalive_interval: (profile.keepalive_interval > 0).then(|| Duration::from_secs(profile.keepalive_interval.into())),
        keepalive_max: 3,
        ..Default::default()
    });
    let (queries_tx, mut queries) = mpsc::channel(1);
    let handler = ClientHandler::new(profile.host.clone(), profile.port, queries_tx, routes, disconnect);

    // Drive the handshake while answering host key questions from the handler.
    let handshake = client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    loop {
        tokio::select! {
            result = &mut handshake => {
                return result.map_err(|e| match e {
                    russh::Error::UnknownKey => anyhow!(Error::new("ssh.hostKeyRejected")),
                    e => anyhow!(e).context(Error::new("ssh.handshakeFailed")),
                });
            }
            Some(query) = queries.recv() => {
                let accepted = host_key::confirm(io, &profile.host, profile.port, &query).await;
                let _ = query.reply.send(accepted);
            }
        }
    }
}
