//! SSH shell sessions on top of russh.

mod auth;
mod connections;
mod handler;
mod host_key;
pub mod known_hosts;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelStream, ChannelWriteHalf, Disconnect};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};

use crate::config::Profile;
use crate::error::Error;
use crate::forward::{host_port, RemoteRoutes};
use crate::net::Route;
use crate::proxy::Proxy;
use crate::session::{Outcome, SessionEvent, SessionId, SessionInput, TermIo};
pub use connections::{Connection, Connections, SshHandle};
use handler::ClientHandler;

/// Session backend: connects (by `route`), authenticates and bridges a remote shell to the
/// terminal. `carry`: forwarding rules to start besides the automatic ones (those running
/// before the tab reconnected).
pub async fn run(profile: Profile, route: Route, id: SessionId, mut io: TermIo, connections: Connections, carry: Vec<String>) {
    let registered = Registered { connections: connections.clone(), id };
    let outcome = match start(&profile, &route, id, &mut io, &connections, &carry).await {
        Ok((channel, disconnect)) => bridge(channel, &mut io, disconnect).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    drop(registered);
    io.finish(outcome);
}

/// Unregisters a session's connection when its task ends, also when the task is aborted
/// (closing the tab). Closing the tab first unregisters the connection, then aborts the
/// task, which may register it in between (just after authenticating); the task is aborted
/// at its next await, dropping this.
struct Registered {
    connections: Connections,
    id: SessionId,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.connections.close(self.id);
    }
}

/// Session backend for a duplicated tab: a new shell on `connection`, which is already
/// registered for this session. No prompts, no auto-started forwards: like a second
/// OpenSSH session through a ControlMaster.
pub async fn run_shared(connection: Arc<Connection>, id: SessionId, mut io: TermIo, connections: Connections) {
    let registered = Registered { connections, id };
    let outcome = match open_shell(connection.handle(), connection.profile(), &mut io).await {
        Ok(channel) => bridge(channel, &mut io, connection.disconnect_reason()).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    drop(connection);
    drop(registered);
    io.finish(outcome);
}

/// Connections to the jump hosts a session runs through, first hop first. Dropping it
/// disconnects them, last hop first.
#[derive(Default)]
pub struct JumpChain(Vec<Arc<SshHandle>>);

impl JumpChain {
    /// Whether one of the connections has ended (which ends the tunnel through them).
    pub fn is_broken(&self) -> bool {
        self.0.iter().any(|jump| jump.is_closed())
    }
}

impl Drop for JumpChain {
    fn drop(&mut self) {
        let jumps = std::mem::take(&mut self.0);
        if jumps.is_empty() {
            return;
        }
        tauri::async_runtime::spawn(async move {
            for jump in jumps.iter().rev() {
                let _ = jump.disconnect(Disconnect::ByApplication, "", "en").await;
            }
        });
    }
}

/// A byte stream to a target through jump hosts: a `direct-tcpip` channel from the last one.
pub struct Tunnel {
    pub stream: ChannelStream<Msg>,
    /// The name of the jump host the stream comes from.
    pub via: String,
    pub jumps: JumpChain,
}

/// Connects through the route's jump hosts (each authenticating as its own profile, with its
/// prompts in the terminal) to `host:port`; `None` without jump hosts. The first one is
/// reached through the route's proxy; each opens a direct-tcpip channel to the next hop, which
/// becomes the transport for that hop's SSH session.
pub async fn tunnel(route: &Route, host: &str, port: u16, io: &mut TermIo) -> Result<Option<Tunnel>> {
    let jumps = &route.jumps;
    let mut chain = JumpChain::default();
    let mut transport = None;
    for (index, hop) in jumps.iter().enumerate() {
        let (next_host, next_port) = jumps.get(index + 1).map_or((host, port), |next| (next.host.as_str(), next.port));
        let proxy = route.proxy.as_ref().filter(|_| index == 0);
        // The agent is only forwarded to the target, where the shell runs.
        let mut session =
            connect(hop, false, transport.take(), proxy, io, RemoteRoutes::default(), watch::channel(None).0).await?;
        auth::authenticate(&mut session, hop, io).await?;
        let target = host_port(next_host, next_port);
        let channel = session
            .channel_open_direct_tcpip(next_host.to_owned(), next_port.into(), "127.0.0.1", 0)
            .await
            .context(Error::new("ssh.jumpFailed").param("jump", &hop.name).param("target", &target))?;
        chain.0.push(Arc::new(session));
        transport = Some((channel.into_stream(), hop.name.clone()));
    }
    Ok(transport.map(|(stream, via)| Tunnel { stream, via, jumps: chain }))
}

/// Connects (through the jump hosts, if any), authenticates and starts the remote shell.
/// Also returns the receiver for the reason the connection ends, reported by the client
/// handler.
async fn start(
    profile: &Profile,
    route: &Route,
    id: SessionId,
    io: &mut TermIo,
    connections: &Connections,
    carry: &[String],
) -> Result<(Channel<Msg>, watch::Receiver<Option<String>>)> {
    let (transport, chain) = match tunnel(route, &profile.host, profile.port, io).await? {
        Some(Tunnel { stream, via, jumps }) => (Some((stream, via)), jumps),
        None => (None, JumpChain::default()),
    };
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    let proxy = route.proxy.as_ref();
    let mut session = connect(profile, profile.forward_agent, transport, proxy, io, routes.clone(), disconnect_tx).await?;
    auth::authenticate(&mut session, profile, io).await?;
    let session = Arc::new(session);
    let connection = connections.insert(id, session.clone(), profile.clone(), chain, routes, disconnect.clone(), io.sink());
    connections.auto_start(&connection, carry);
    let channel = open_shell(&session, profile, io).await?;
    Ok((channel, disconnect))
}

/// Starts an interactive shell in a new channel on an authenticated connection, with the
/// profile's terminal type, environment and agent forwarding. Like OpenSSH, rejected
/// environment variables are not reported (the server's `AcceptEnv` decides).
async fn open_shell(session: &SshHandle, profile: &Profile, io: &mut TermIo) -> Result<Channel<Msg>> {
    let channel = session.channel_open_session().await.context(Error::new("ssh.channelFailed"))?;
    if profile.forward_agent {
        channel.agent_forward(false).await?;
    }
    let (cols, rows) = io.size;
    channel.request_pty(false, &profile.term_type, cols.into(), rows.into(), 0, 0, &[]).await?;
    for var in &profile.env {
        channel.set_env(false, var.name.clone(), var.value.clone()).await?;
    }
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
    let writer = Arc::new(writer);
    let mut close = CloseOnDrop(Some(writer.clone()));
    let mut exit_status = None;
    let mut writing = None;
    loop {
        let idle = writing.is_none();
        let sent = tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    io.output(data.to_vec());
                    Ok(())
                }
                Some(ChannelMsg::ExitStatus { exit_status: status }) => {
                    exit_status = Some(status);
                    Ok(())
                }
                Some(ChannelMsg::Close) => {
                    close.0 = None;
                    return Outcome::Exited(exit_status);
                }
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
                Some(SessionInput::Break) => Ok(()),
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
    let error = Error::new("net.connectionLost");
    let reason = disconnect.borrow().clone();
    Outcome::Lost(match reason {
        Some(reason) => error.detail(reason),
        None => error,
    })
}

/// Closes the shell channel when the bridge ends without the server closing it: the tab was
/// closed (the task aborted) or the terminal went away. The split halves of a channel don't
/// close it when dropped, and on a connection shared with other tabs the shell would stay
/// open on the server, counting against its `MaxSessions`.
struct CloseOnDrop(Option<Arc<ChannelWriteHalf<Msg>>>);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Some(writer) = self.0.take() {
            tauri::async_runtime::spawn(async move {
                let _ = writer.close().await;
            });
        }
    }
}

/// Completes with the write in progress, if any; pending forever otherwise (for `select!`).
async fn in_flight<F: Future + Unpin>(write: &mut Option<F>) -> F::Output {
    match write {
        Some(write) => write.await,
        None => std::future::pending().await,
    }
}

/// Connects to one hop: over TCP (through `proxy`, if any), or through `via` (a channel from
/// the previous jump host, with that host's name), then performs the SSH handshake.
/// `forward_agent`: whether the server may open agent channels.
async fn connect(
    hop: &Profile,
    forward_agent: bool,
    via: Option<(ChannelStream<Msg>, String)>,
    proxy: Option<&Proxy>,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle> {
    let (user, host, port) = (&hop.username, &hop.host, hop.port);
    let Some((stream, jump)) = via else {
        let connecting = match proxy {
            Some(proxy) => t!("terminal.connectingViaProxy", user = user, host = host, port = port, proxy = proxy.name),
            None => t!("terminal.connecting", user = user, host = host, port = port),
        };
        io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
        // SSH keepalives notice a dead connection; TCP ones aren't needed.
        let stream = crate::net::connect(host, port, user, proxy, None, io).await?;
        return handshake(hop, forward_agent, stream, io, routes, disconnect).await;
    };
    let connecting = t!("terminal.connectingVia", user = user, host = host, port = port, jump = jump);
    io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
    handshake(hop, forward_agent, stream, io, routes, disconnect).await
}

/// How long the SSH version and key exchange may take, not counting the time a host key
/// question waits for the user. A port that accepts connections but never speaks SSH (or a
/// proxy command that hangs) would otherwise leave the tab connecting for ever, and an
/// automatic reconnection would never try again.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

async fn handshake<S>(
    hop: &Profile,
    forward_agent: bool,
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
    let handler = ClientHandler::new(hop.host.clone(), hop.port, forward_agent, queries_tx, routes, disconnect);

    // Drive the handshake while answering host key questions from the handler.
    let handshake = client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    let deadline = tokio::time::sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);
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
                deadline.as_mut().reset(tokio::time::Instant::now() + HANDSHAKE_TIMEOUT);
            }
            () = &mut deadline => {
                return Err(Error::new("ssh.handshakeTimeout").param("target", host_port(&hop.host, hop.port)).into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that accepts the connection but never speaks SSH.
    #[tokio::test(start_paused = true)]
    async fn handshake_gives_up_on_a_silent_server() {
        let (stream, _server) = tokio::io::duplex(1024);
        let profile = Profile::new("t".into(), "example.com".into(), 22, "alice".into());
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let result = handshake(&profile, false, stream, &mut io, RemoteRoutes::default(), watch::channel(None).0).await;
        let error = Error::from(result.err().unwrap());
        assert_eq!(error.code(), "ssh.handshakeTimeout");
    }
}
