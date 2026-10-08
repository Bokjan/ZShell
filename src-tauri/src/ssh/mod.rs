//! SSH shell sessions on top of russh.

mod auth;
mod connections;
mod handler;
mod host_key;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelStream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

use crate::config::Profile;
use crate::error::Error;
use crate::forward::{host_port, RemoteRoutes};
use crate::session::{CloseReason, SessionEvent, SessionId, SessionInput, TermIo};
pub use connections::{Connection, Connections, SshHandle};
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
/// `jumps` are the profiles of the jump hosts to connect through, first hop first.
pub async fn run(profile: Profile, jumps: Vec<Profile>, id: SessionId, mut io: TermIo, connections: Connections) {
    let outcome = match start(&profile, &jumps, id, &mut io, &connections).await {
        Ok((channel, disconnect)) => bridge(channel, &mut io, disconnect).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    connections.close(id);
    finish(outcome, &io);
}

/// Session backend for a duplicated tab: a new shell on `connection`, which is already
/// registered for this session. No prompts, no auto-started forwards: like a second
/// OpenSSH session through a ControlMaster.
pub async fn run_shared(connection: Arc<Connection>, id: SessionId, mut io: TermIo, connections: Connections) {
    let outcome = match open_shell(connection.handle(), &mut io).await {
        Ok(channel) => bridge(channel, &mut io, connection.disconnect_reason()).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    drop(connection);
    connections.close(id);
    finish(outcome, &io);
}

/// Reports how the session ended, in the terminal and as its `Closed` event.
fn finish(outcome: Outcome, io: &TermIo) {
    let (reason, error, status) = match outcome {
        Outcome::Exited(status) => {
            let message = match status {
                Some(status) => t!("terminal.closedWithStatus", status = status),
                None => t!("terminal.closed"),
            };
            io.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
            (CloseReason::Exited, None, status)
        }
        Outcome::Lost(e) => {
            // The shell may have left the cursor mid-line.
            io.print(&format!("\n\x1b[31m{e}\x1b[0m\n"));
            (CloseReason::Lost, Some(e), None)
        }
        Outcome::Failed(e) => {
            io.print(&format!("\x1b[31m{e}\x1b[0m\n"));
            (CloseReason::Failed, Some(e), None)
        }
    };
    io.event(SessionEvent::Closed { reason, error, status });
}

/// Connects (through the jump hosts, if any), authenticates and starts the remote shell.
/// Also returns the receiver for the reason the connection ends, reported by the client
/// handler.
async fn start(
    profile: &Profile,
    jumps: &[Profile],
    id: SessionId,
    io: &mut TermIo,
    connections: &Connections,
) -> Result<(Channel<Msg>, watch::Receiver<Option<String>>)> {
    // Each jump host opens a direct-tcpip channel to the next hop, which becomes the
    // transport for that hop's SSH session.
    let mut jump_handles = Vec::with_capacity(jumps.len());
    let mut transport = None;
    for (index, hop) in jumps.iter().enumerate() {
        let next = jumps.get(index + 1).unwrap_or(profile);
        let mut session = connect(hop, transport.take(), io, RemoteRoutes::default(), watch::channel(None).0).await?;
        auth::authenticate(&mut session, hop, io).await?;
        let target = host_port(&next.host, next.port);
        let channel = session
            .channel_open_direct_tcpip(next.host.clone(), next.port.into(), "127.0.0.1", 0)
            .await
            .context(Error::new("ssh.jumpFailed").param("jump", &hop.name).param("target", &target))?;
        transport = Some((channel.into_stream(), hop.name.clone()));
        jump_handles.push(Arc::new(session));
    }

    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    let mut session = connect(profile, transport, io, routes.clone(), disconnect_tx).await?;
    auth::authenticate(&mut session, profile, io).await?;
    let session = Arc::new(session);
    let connection = connections.insert(id, session.clone(), jump_handles, routes, disconnect.clone(), io.sink());
    for rule in profile.forwards.iter().filter(|rule| rule.auto_start) {
        connection.forwards().start(rule.clone(), true);
    }
    let channel = open_shell(&session, io).await?;
    Ok((channel, disconnect))
}

/// Starts an interactive shell in a new channel on an authenticated connection.
async fn open_shell(session: &SshHandle, io: &mut TermIo) -> Result<Channel<Msg>> {
    let channel = session.channel_open_session().await.context(Error::new("ssh.channelFailed"))?;
    let (cols, rows) = io.size;
    channel.request_pty(false, "xterm-256color", cols.into(), rows.into(), 0, 0, &[]).await?;
    channel.request_shell(false).await?;
    io.event(SessionEvent::Connected);
    Ok(channel)
}

/// Copies between the shell channel and the terminal until either side ends.
///
/// Output keeps being read while a write waits for the server's window: russh delivers output
/// into a bounded queue from its connection task, so a full queue would stall that task, and
/// with it the window adjustments the write is waiting for (a large paste or a ZMODEM upload
/// while the remote program prints). New input waits until the write is done.
async fn bridge(channel: Channel<Msg>, io: &mut TermIo, mut disconnect: watch::Receiver<Option<String>>) -> Outcome {
    let (mut reader, writer) = channel.split();
    let mut exit_status = None;
    let mut writing = None;
    loop {
        let idle = writing.is_none();
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
            result = in_flight(&mut writing) => {
                writing = None;
                result
            }
            input = io.recv(), if idle => match input {
                Some(SessionInput::Data(data)) => {
                    writing = Some(Box::pin(writer.data_bytes(data)));
                    Ok(())
                }
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

/// Completes with the write in progress, if any; pending forever otherwise (for `select!`).
async fn in_flight<F: Future + Unpin>(write: &mut Option<F>) -> F::Output {
    match write {
        Some(write) => write.await,
        None => std::future::pending().await,
    }
}

/// Connects to one hop: over TCP, or through `via` (a channel from the previous jump host,
/// with that host's name), then performs the SSH handshake.
async fn connect(
    hop: &Profile,
    via: Option<(ChannelStream<Msg>, String)>,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle> {
    let (user, host, port) = (&hop.username, &hop.host, hop.port);
    let Some((stream, jump)) = via else {
        io.print(&format!("\x1b[2m{}\x1b[0m\n", t!("terminal.connecting", user = user, host = host, port = port)));
        let target = host_port(host, port);
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| Error::new("ssh.connectTimeout").param("target", &target))?
            .context(Error::new("ssh.connectFailed").param("target", &target))?;
        stream.set_nodelay(true)?;
        return handshake(hop, stream, io, routes, disconnect).await;
    };
    let connecting = t!("terminal.connectingVia", user = user, host = host, port = port, jump = jump);
    io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
    handshake(hop, stream, io, routes, disconnect).await
}

async fn handshake<S>(
    hop: &Profile,
    stream: S,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let config = Arc::new(client::Config {
        keepalive_interval: (hop.keepalive_interval > 0).then(|| Duration::from_secs(hop.keepalive_interval.into())),
        keepalive_max: 3,
        ..Default::default()
    });
    let (queries_tx, mut queries) = mpsc::channel(1);
    let handler = ClientHandler::new(hop.host.clone(), hop.port, queries_tx, routes, disconnect);

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
                let accepted = host_key::confirm(io, &hop.host, hop.port, &query).await;
                let _ = query.reply.send(accepted);
            }
        }
    }
}
