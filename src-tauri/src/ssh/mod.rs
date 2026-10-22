//! SSH shell sessions on top of russh.

mod abortable;
mod auth;
mod connection;
mod forwarded;
mod handler;
mod host_key;
pub mod known_hosts;
mod liveness;
#[cfg(test)]
pub(crate) mod testing;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::FutureExt;
use russh::client::{self, Msg};
use russh::{Channel, ChannelMsg, ChannelStream, ChannelWriteHalf, Disconnect, Sig};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};

use crate::error::Error;
use crate::model::host_port;
use crate::model::Proxy;
use crate::model::SshProfile;
use crate::net::Route;
use crate::session::{Outcome, SessionEvent, SessionId, SessionInput, TermIo};
use crate::vault::SavedPasswords;
pub use auth::{default_key_path, key_file_exists};
pub use connection::{Connection, Shells, SshHandle};
pub use forwarded::{Incoming, RemoteRoutes, ROUTE_QUEUE};
use handler::{ClientHandler, EndReason};
use host_key::HostKeyMessage;
pub use liveness::Liveness;

/// Session backend: connects (by `route`), authenticates and bridges a remote shell to the
/// terminal. `connected` is given the connection once it is authenticated, before the shell
/// opens on it, for the other features to use (see `crate::connections`).
pub async fn run(
    profile: SshProfile,
    route: Route,
    id: SessionId,
    mut io: TermIo,
    connected: impl FnOnce(Arc<Connection>, &TermIo) + Send,
) {
    let outcome = match start(&profile, &route, id, &mut io, connected).await {
        Ok((channel, connection)) => bridge(channel, &mut io, connection.disconnect_reason(), connection.liveness()).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    io.finish(outcome);
}

/// Session backend for a duplicated tab: a new shell on `connection`, which is already
/// registered for this session (see `crate::connections`). No prompts, no auto-started
/// forwards: like a second OpenSSH session through a ControlMaster.
pub async fn run_shared(connection: Arc<Connection>, id: SessionId, mut io: TermIo) {
    let outcome = match open_shell(&connection, id, &mut io).await {
        Ok(channel) => bridge(channel, &mut io, connection.disconnect_reason(), connection.liveness()).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    drop(connection);
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
        let (next_host, next_port) =
            jumps.get(index + 1).map_or((host, port), |next| (next.ssh.remote.host.as_str(), next.ssh.remote.port));
        let proxy = route.proxy.as_ref().filter(|_| index == 0);
        // The agent is only forwarded to the target, where the shell runs.
        let options = HopOptions::jump_host();
        let ended = options.disconnect.subscribe();
        // The target's checks find a jump host gone, as nothing comes through it any more.
        let (mut session, _) = connect(hop, options, transport.take(), proxy, &route.passwords, io).await?;
        auth::authenticate(&mut session, hop, &route.passwords, io, ended).await?;
        let target = host_port(next_host, next_port);
        let failed = || Error::new("ssh.jumpFailed").param("jump", &hop.name).param("target", &target);
        // Given as long as a handshake: a target that drops packets would otherwise leave the
        // tab connecting until the jump host's kernel gives up (two minutes on Linux).
        let opening = session.channel_open_direct_tcpip(next_host.to_owned(), next_port.into(), "127.0.0.1", 0);
        let channel = match tokio::time::timeout(HANDSHAKE_TIMEOUT, opening).await {
            Ok(opened) => opened.context(failed())?,
            Err(elapsed) => return Err(anyhow!(elapsed).context(failed())),
        };
        chain.0.push(Arc::new(session));
        transport = Some((channel.into_stream(), hop.name.clone()));
    }
    Ok(transport.map(|(stream, via)| Tunnel { stream, via, jumps: chain }))
}

/// Connects (through the jump hosts, if any), authenticates and starts the remote shell.
/// Also returns the connection, given to `connected` too.
async fn start(
    profile: &SshProfile,
    route: &Route,
    id: SessionId,
    io: &mut TermIo,
    connected: impl FnOnce(Arc<Connection>, &TermIo),
) -> Result<(Channel<Msg>, Arc<Connection>)> {
    let remote = &profile.ssh.remote;
    let (transport, chain) = match tunnel(route, &remote.host, remote.port, io).await? {
        Some(Tunnel { stream, via, jumps }) => (Some((stream, via)), jumps),
        None => (None, JumpChain::default()),
    };
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    let proxy = route.proxy.as_ref();
    let options = HopOptions { forward_agent: profile.ssh.forward_agent, routes: routes.clone(), disconnect: disconnect_tx };
    let (mut session, link) = connect(profile, options, transport, proxy, &route.passwords, io).await?;
    // No time limit, unlike connecting: the user may be answering prompts, and the server
    // has its own (`LoginGraceTime`).
    auth::authenticate(&mut session, profile, &route.passwords, io, disconnect.clone()).await?;
    let connection = Connection::new(session, link, profile.clone(), chain, routes, disconnect);
    connected(connection.clone(), io);
    let channel = open_shell(&connection, id, io).await?;
    Ok((channel, connection))
}

/// Starts an interactive shell in a new channel on an authenticated connection, with the
/// profile's terminal type, environment and agent forwarding. Like OpenSSH, rejected
/// environment variables are not reported (the server's `AcceptEnv` decides).
async fn open_shell(connection: &Connection, id: SessionId, io: &mut TermIo) -> Result<Channel<Msg>> {
    let profile = connection.profile();
    let channel = connection.handle().channel_open_session().await.context(Error::new("ssh.channelFailed"))?;
    // These requests wait for no answer: they fail only when the connection is gone. A server
    // that won't start the shell closes the channel, which ends the session as usual.
    let failed = || Error::new("net.connectionLost");
    if profile.ssh.forward_agent {
        channel.agent_forward(false).await.context(failed())?;
    }
    let (cols, rows) = io.size;
    channel.request_pty(false, &profile.ssh.remote.term_type, cols.into(), rows.into(), 0, 0, &[]).await.context(failed())?;
    for var in &profile.ssh.env {
        channel.set_env(false, var.name.clone(), var.value.clone()).await.context(failed())?;
    }
    // In the order the server starts the shells, which is how their folders are found.
    connection.shell_opened(id, io);
    channel.request_shell(false).await.context(failed())?;
    io.event(SessionEvent::Connected);
    Ok(channel)
}

/// Copies between the shell channel and the terminal until either side ends.
///
/// Output is not read while the terminal is too far behind ([`crate::session::Flow`]): the
/// channel's window then stays closed and the remote program waits. That stops the whole
/// connection (russh's connection task waits for the channel to be read), including SFTP and
/// forwards on it and the shells of duplicated tabs, for as long as the slowest of its
/// terminals is behind: a bounded wait, where reading on would grow memory without bound.
///
/// Output keeps being read while a write waits for the server's window: russh delivers output
/// into a bounded queue from its connection task, so a full queue would stall that task, and
/// with it the window adjustments the write is waiting for (a large paste or a ZMODEM upload
/// while the remote program prints).
///
/// So nothing sent to russh is awaited in a `select!` branch, window changes included: every
/// message goes through russh's bounded queue to the connection task, which may itself be
/// waiting for this channel to be read (output paused while the terminal is behind), and the
/// branch would keep the reader and the flow control from being polled for good. One message
/// is in flight at a time ([`Sending`]); while it is, input is gathered to send next (past a
/// limit, a ZMODEM upload's data waits in its own queue while keystrokes are still read, see
/// [`TermIo::recv`]), and window changes are merged into the latest size (dragging the
/// window edge sends many).
///
/// What is typed is answered, echoed or otherwise: `liveness` checks the connection when
/// nothing comes back.
async fn bridge(
    channel: Channel<Msg>,
    io: &mut TermIo,
    mut disconnect: watch::Receiver<Option<EndReason>>,
    liveness: &Liveness,
) -> Outcome {
    let (mut reader, writer) = channel.split();
    let writer = Arc::new(writer);
    let mut close = CloseOnDrop(Some(writer.clone()));
    let flow = io.sink().flow();
    let mut exit_status = None;
    let mut exit_signal = None;
    let mut sending: Option<Sending> = None;
    let mut data: Vec<u8> = Vec::new();
    let mut resize = None;
    // A message taken while gathering output (see `gather`), to handle next.
    let mut next = None;
    loop {
        if let Some(msg) = next.take() {
            match received(msg, io, &mut close, &mut exit_status, &mut exit_signal) {
                Some(Ok(())) => continue,
                Some(Err(outcome)) => return outcome,
                None => break,
            }
        }
        if sending.is_none() {
            sending = match (std::mem::take(&mut data), resize.take()) {
                (bytes, size) if !bytes.is_empty() => {
                    resize = size;
                    Some(Box::pin(writer.data_bytes(bytes)))
                }
                (_, Some((cols, rows))) => Some(Box::pin(writer.window_change(cols, rows, 0, 0))),
                (_, None) => None,
            };
        }
        let paused = flow.is_paused();
        let sent = tokio::select! {
            () = flow.ready(), if paused => Ok(()),
            msg = reader.wait(), if !paused => {
                let msg = match msg {
                    Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => {
                        let (output, after) = gather(|| reader.wait().now_or_never(), data.to_vec());
                        next = after;
                        io.output(output);
                        continue;
                    }
                    msg => msg,
                };
                match received(msg, io, &mut close, &mut exit_status, &mut exit_signal) {
                    Some(Ok(())) => Ok(()),
                    Some(Err(outcome)) => return outcome,
                    None => break,
                }
            },
            result = in_flight(&mut sending) => {
                sending = None;
                result
            }
            input = io.recv(data.len()) => match input {
                Some(SessionInput::Data(bytes) | SessionInput::Raw(bytes)) => {
                    data.extend_from_slice(&bytes);
                    liveness.expect_reply();
                    Ok(())
                }
                Some(SessionInput::Resize { cols, rows }) => {
                    resize = Some((cols.into(), rows.into()));
                    Ok(())
                }
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
        Some(reason) => error.detail(reason.detail),
        None => error,
    })
}

/// Output messages from the channel are passed on together up to this many bytes.
const GATHER: usize = 64 << 10;

/// `output` and the output messages already waiting after it (`waiting`: the next one, if
/// one is), up to [`GATHER`] bytes, with the next message if another kind was taken (to handle
/// after the output, in order). Each message is a packet from the server, often a few KiB, and
/// the terminal gets each of its own as a message to the web view, which takes the main thread.
fn gather(mut waiting: impl FnMut() -> Option<Option<ChannelMsg>>, mut output: Vec<u8>) -> (Vec<u8>, Option<Option<ChannelMsg>>) {
    while output.len() < GATHER {
        match waiting() {
            Some(Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. })) => output.extend_from_slice(&data),
            Some(msg) => return (output, Some(msg)),
            None => break,
        }
    }
    (output, None)
}

/// Handles a message from the shell channel other than output: `Some(Err)` when the session
/// ended, `None` when the connection did.
fn received(
    msg: Option<ChannelMsg>,
    io: &TermIo,
    close: &mut CloseOnDrop,
    exit_status: &mut Option<u32>,
    exit_signal: &mut Option<Sig>,
) -> Option<Result<(), Outcome>> {
    match msg {
        Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => io.output(data.to_vec()),
        Some(ChannelMsg::ExitStatus { exit_status: status }) => *exit_status = Some(status),
        Some(ChannelMsg::ExitSignal { signal_name, .. }) => *exit_signal = Some(signal_name),
        Some(ChannelMsg::Close) => {
            close.0 = None;
            return Some(Err(match exit_signal {
                Some(signal) => Outcome::Signaled(signal_label(signal)),
                None => Outcome::Exited(*exit_status),
            }));
        }
        // The connection ended without closing the channel.
        None => return None,
        Some(_) => {}
    }
    Some(Ok(()))
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

/// A message to the server on its way to russh's connection task: data or a window change.
type Sending<'a> = std::pin::Pin<Box<dyn Future<Output = Result<(), russh::Error>> + Send + 'a>>;

/// Completes with the message in flight, if any; pending forever otherwise (for `select!`).
async fn in_flight<F: Future + Unpin>(sending: &mut Option<F>) -> F::Output {
    match sending {
        Some(sending) => sending.await,
        None => std::future::pending().await,
    }
}

/// How a signal the server names is shown, as OpenSSH's `SIG` names. A server's own name
/// (an extension) is cut short: it ends up in the terminal.
fn signal_label(signal: &russh::Sig) -> String {
    use russh::Sig;
    let name = match signal {
        Sig::ABRT => "ABRT",
        Sig::ALRM => "ALRM",
        Sig::FPE => "FPE",
        Sig::HUP => "HUP",
        Sig::ILL => "ILL",
        Sig::INT => "INT",
        Sig::KILL => "KILL",
        Sig::PIPE => "PIPE",
        Sig::QUIT => "QUIT",
        Sig::SEGV => "SEGV",
        Sig::TERM => "TERM",
        Sig::USR1 => "USR1",
        Sig::Custom(name) => return name.chars().take(32).collect(),
    };
    format!("SIG{name}")
}

/// What a connection to one hop takes besides its address: what the server may open, and
/// where to report why the connection ended. The shell's host has its own; a jump host none.
struct HopOptions {
    /// Whether the server may open agent channels.
    forward_agent: bool,
    /// Where the channels of remote forwarding rules go.
    routes: RemoteRoutes,
    /// Receives why the connection ended.
    disconnect: watch::Sender<Option<EndReason>>,
}

impl HopOptions {
    fn jump_host() -> Self {
        Self { forward_agent: false, routes: RemoteRoutes::default(), disconnect: watch::channel(None).0 }
    }
}

/// Connects to one hop: over TCP (through `proxy`, if any, with its saved password in
/// `passwords`), or through `via` (a channel from the previous jump host, with that host's
/// name), then performs the SSH handshake. Also returns the link to its transport.
async fn connect(
    hop: &SshProfile,
    options: HopOptions,
    via: Option<(ChannelStream<Msg>, String)>,
    proxy: Option<&Proxy>,
    passwords: &SavedPasswords,
    io: &mut TermIo,
) -> Result<(SshHandle, abortable::Link)> {
    let remote = &hop.ssh.remote;
    let (user, host, port) = (&remote.username, &remote.host, remote.port);
    let Some((stream, jump)) = via else {
        let connecting = match proxy {
            Some(proxy) => t!("terminal.connectingViaProxy", user = user, host = host, port = port, proxy = proxy.name),
            None => t!("terminal.connecting", user = user, host = host, port = port),
        };
        io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
        // SSH keepalives notice a dead connection once authenticated (russh sends none
        // before); TCP ones cover the time until then, as OpenSSH's `TCPKeepAlive` does.
        let stream = crate::net::connect(host, port, user, proxy, passwords, remote.keepalive(), io).await?;
        return handshake(hop, options, stream, io).await;
    };
    let connecting = t!("terminal.connectingVia", user = user, host = host, port = port, jump = jump);
    io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
    handshake(hop, options, stream, io).await
}

/// Keepalives unanswered in a row that end a connection, as OpenSSH's `ServerAliveCountMax`.
const KEEPALIVE_MAX: u32 = 3;

/// The least `inactivity_timeout`, which also limits each of russh's writes: on a slow link,
/// writing what a channel's window allows takes a minute (2 MiB at 256 kbit/s). None in tests,
/// which wait for stalled connections to end.
const INACTIVITY_MIN: Duration = Duration::from_secs(if cfg!(test) { 0 } else { 120 });

/// russh's settings for a connection sending keepalives every `keepalive`, if at all.
///
/// russh waits for its writes with no limit but `inactivity_timeout`. Once a peer gone silently
/// has left the system's send buffer full (with what a forward carries, say), the connection
/// task waits on a write for good, and its keepalives with it, until the system gives up
/// retransmitting many minutes later. Anything that happens on the connection but our own
/// keepalives starts the timeout again, a server's answer to them included, so an idle
/// connection stays open; it is a keepalive longer than the keepalives take to give up, which
/// stay the reason given for a server that stops answering, and no less than
/// [`INACTIVITY_MIN`].
fn client_config(keepalive: Option<Duration>) -> client::Config {
    client::Config {
        keepalive_interval: keepalive,
        keepalive_max: KEEPALIVE_MAX as usize,
        inactivity_timeout: keepalive.map(|interval| (interval * (KEEPALIVE_MAX + 2)).max(INACTIVITY_MIN)),
        ..Default::default()
    }
}

/// How long the SSH version and key exchange may take, not counting the time a host key
/// question waits for the user. A port that accepts connections but never speaks SSH (or a
/// proxy command that hangs) would otherwise leave the tab connecting for ever, and an
/// automatic reconnection would never try again. Giving up (or the tab closing) ends the
/// connection, which russh would otherwise keep (see [`abortable`]).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

async fn handshake<S>(hop: &SshProfile, options: HopOptions, stream: S, io: &mut TermIo) -> Result<(SshHandle, abortable::Link)>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let remote = &hop.ssh.remote;
    let preferred = russh::Preferred::default();
    // Reads the known_hosts files, in the blocking pool.
    let (algorithms, host, port) = (preferred.key.to_vec(), remote.host.clone(), remote.port);
    let algorithms = tokio::task::spawn_blocking(move || known_hosts::host_key_algorithms(&algorithms, &host, port))
        .await
        .unwrap_or_else(|_| known_hosts::HostKeyAlgorithms { keys: preferred.key.to_vec(), certificates: Vec::new() });
    let config = Arc::new(client::Config {
        preferred: russh::Preferred { key: algorithms.keys.into(), host_key_certificates: algorithms.certificates.into(), ..preferred },
        ..client_config(remote.keepalive())
    });
    let (queries_tx, mut queries) = mpsc::channel(1);
    let HopOptions { forward_agent, routes, disconnect } = options;
    let handler = ClientHandler::new(remote.host.clone(), remote.port, forward_agent, queries_tx, routes, disconnect);

    // Drive the handshake while answering host key questions from the handler.
    let (stream, abort) = abortable::abortable(stream);
    let handshake = client::connect_stream(config, stream, handler);
    tokio::pin!(handshake);
    let deadline = tokio::time::sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);
    // Why the handler couldn't check the host key, which is the error then.
    let mut failure = None;
    loop {
        tokio::select! {
            result = &mut handshake => {
                // Warnings sent just before the key was accepted, which may not have been
                // received yet.
                while let Ok(message) = queries.try_recv() {
                    match message {
                        HostKeyMessage::Warning(warning) => host_key::warn(io, &warning),
                        HostKeyMessage::Failed(e) => failure = Some(e),
                        HostKeyMessage::Query(_) => {}
                    }
                }
                let session = result.map_err(|e| match (e, failure) {
                    (_, Some(failure)) => anyhow!(failure),
                    (russh::Error::UnknownKey, None) => anyhow!(Error::new("ssh.hostKeyRejected")),
                    (e, None) => anyhow!(e).context(Error::new("ssh.handshakeFailed")),
                })?;
                return Ok((session, abort.keep()));
            }
            Some(message) = queries.recv() => match message {
                HostKeyMessage::Warning(warning) => host_key::warn(io, &warning),
                HostKeyMessage::Failed(e) => failure = Some(e),
                HostKeyMessage::Query(query) => {
                    let accepted = host_key::confirm(io, &remote.host, remote.port, &query).await;
                    let _ = query.reply.send(accepted);
                    deadline.as_mut().reset(tokio::time::Instant::now() + HANDSHAKE_TIMEOUT);
                }
            },
            () = &mut deadline => {
                return Err(Error::new("ssh.handshakeTimeout").param("target", host_port(&remote.host, remote.port)).into());
            }
        }
    }
}

/// A private key for tests: the server's host key, and the user's key where one is needed.
#[cfg(test)]
const TEST_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBmJacSIfoWm34XIu1XxBNljXN6rq4lFsD5uSBwBECvQAAAAIiHA/GKhwPx
igAAAAtzc2gtZWQyNTUxOQAAACBmJacSIfoWm34XIu1XxBNljXN6rq4lFsD5uSBwBECvQA
AAAEBQtLIgjSDC4h72YyOg7rcfkBUD/Fm2W/HoNlMi5m03MmYlpxIh+habfhci7VfEE2WN
c3quriUWwPm5IHAEQK9AAAAAAAECAwQF
-----END OPENSSH PRIVATE KEY-----
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Remote;

    /// A server that accepts the connection but never speaks SSH.
    #[tokio::test(start_paused = true)]
    async fn handshake_gives_up_on_a_silent_server() {
        let (stream, _server) = tokio::io::duplex(1024);
        let profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let result = handshake(&profile, HopOptions::jump_host(), stream, &mut io).await;
        let error = Error::from(result.err().unwrap());
        assert_eq!(error.code(), "ssh.handshakeTimeout");
    }

    /// A server that sends its version line, then nothing. russh has started its connection
    /// task by then; giving up on the handshake (the tab closing, here) must end it too, or the
    /// connection stays open.
    #[tokio::test]
    async fn giving_up_on_a_handshake_closes_the_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (stream, mut server) = tokio::io::duplex(1 << 16);
        server.write_all(b"SSH-2.0-silent\r\n").await.unwrap();
        let profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let mut buf = vec![0; 4096];
        tokio::select! {
            _ = handshake(&profile, HopOptions::jump_host(), stream, &mut io) => panic!("the handshake ended"),
            // The client's version line and key exchange offer.
            read = server.read(&mut buf) => assert!(read.unwrap() > 0),
        }
        let closed =
            tokio::time::timeout(Duration::from_secs(5), async { while server.read(&mut buf).await.is_ok_and(|n| n > 0) {} }).await;
        assert!(closed.is_ok(), "the connection is still open");
    }

    /// A connection whose transport stalls, as one to a peer gone silently does once the send
    /// buffer is full, ends after a while: russh's connection task waits on a write by then, and
    /// sends no more keepalives.
    #[tokio::test]
    async fn a_stalled_connection_ends() {
        let (connection, stall) = testing::connect_stalling(testing::Anyone, Duration::from_millis(100)).await;
        let mut reason = connection.disconnect_reason();
        stall.now();
        let ended = tokio::time::timeout(Duration::from_secs(5), reason.wait_for(Option::is_some)).await;
        assert!(ended.is_ok(), "the connection is still open");
    }

    /// A server whose shell prints without end if `flood` (and nothing otherwise), and that
    /// reports what it is sent: window changes and typed data.
    struct Server {
        flood: bool,
        sizes: mpsc::UnboundedSender<(u32, u32)>,
        typed: mpsc::UnboundedSender<Vec<u8>>,
    }

    impl russh::server::Handler for Server {
        type Error = russh::Error;

        async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
            Ok(russh::server::Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn shell_request(&mut self, channel: russh::ChannelId, session: &mut russh::server::Session) -> Result<(), Self::Error> {
            if self.flood {
                let handle = session.handle();
                tokio::spawn(async move { while handle.data(channel, vec![b'y'; 1 << 14]).await.is_ok() {} });
            }
            Ok(())
        }

        async fn data(&mut self, channel: russh::ChannelId, data: &[u8], session: &mut russh::server::Session) -> Result<(), Self::Error> {
            // The shell is killed, or exits.
            match data {
                b"kill" => {
                    session.exit_signal_request(channel, russh::Sig::KILL, false, "", "")?;
                    session.close(channel)?;
                }
                b"exit" => {
                    session.exit_status_request(channel, 3)?;
                    session.close(channel)?;
                }
                // Many small packets of output at once, then the end.
                b"burst" => {
                    for i in 0..BURST {
                        session.data(channel, format!("{i}\n").into_bytes())?;
                    }
                    session.exit_status_request(channel, 0)?;
                    session.close(channel)?;
                }
                _ => {}
            }
            let _ = self.typed.send(data.to_vec());
            Ok(())
        }

        async fn window_change_request(
            &mut self,
            _channel: russh::ChannelId,
            cols: u32,
            rows: u32,
            _pix_width: u32,
            _pix_height: u32,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            let _ = self.sizes.send((cols, rows));
            Ok(())
        }
    }

    /// Packets of output the test server sends for `burst`.
    const BURST: usize = 2000;

    /// A shell on a [`Server`], bridged to a terminal that acknowledges nothing by itself.
    struct Shell {
        input: mpsc::UnboundedSender<SessionInput>,
        output: std::sync::mpsc::Receiver<Vec<u8>>,
        flow: Arc<crate::session::Flow>,
        sizes: mpsc::UnboundedReceiver<(u32, u32)>,
        typed: mpsc::UnboundedReceiver<Vec<u8>>,
        bridge: tokio::task::JoinHandle<Outcome>,
    }

    impl Shell {
        async fn start(flood: bool) -> Self {
            let (sizes_tx, sizes) = mpsc::unbounded_channel();
            let (typed_tx, typed) = mpsc::unbounded_channel();
            let server = Server { flood, sizes: sizes_tx, typed: typed_tx };
            let connection = testing::connect(server, "").await;
            let channel = connection.handle().channel_open_session().await.unwrap();
            channel.request_shell(false).await.unwrap();
            let (mut io, input, output, _events) = TermIo::detached((80, 24));
            let flow = io.sink().flow();
            let bridge = tokio::spawn(async move { bridge(channel, &mut io, connection.disconnect_reason(), connection.liveness()).await });
            Self { input, output, flow, sizes, typed, bridge }
        }

        /// Acknowledges the output shown so far; returns how much there was.
        fn catch_up(&self) -> usize {
            let shown = self.output.try_iter().map(|bytes| bytes.len()).sum();
            self.flow.ack(shown);
            shown
        }

        /// Keeps up with the output until the server has got `typed` and the latest size is
        /// `size`, and output has been shown if `output`; fails after a while.
        async fn expect(&mut self, typed: &[u8], size: (u32, u32), output: bool) {
            let (mut got, mut latest, mut shown) = (Vec::new(), None, 0);
            tokio::time::timeout(Duration::from_secs(10), async {
                while got != typed || latest != Some(size) || (output && shown == 0) {
                    shown += self.catch_up();
                    while let Ok(bytes) = self.typed.try_recv() {
                        got.extend(bytes);
                    }
                    while let Ok(size) = self.sizes.try_recv() {
                        latest = Some(size);
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                let same = got.iter().zip(typed).take_while(|(a, b)| a == b).count();
                panic!(
                    "the connection is stuck: got {} of {} bytes (same up to {same}), size {latest:?}, {shown} bytes shown",
                    got.len(),
                    typed.len()
                )
            });
        }
    }

    /// Resizing the window while output is paused: the window changes wait for russh's
    /// connection task, which waits for the paused channel to be read. Once the terminal
    /// catches up, output goes on and the server gets the latest size.
    #[tokio::test]
    async fn window_changes_while_output_is_paused() {
        let mut shell = Shell::start(true).await;
        // Nothing is acknowledged, so the bridge stops reading; then the connection task fills
        // the channel's queue and waits.
        tokio::time::timeout(Duration::from_secs(10), async {
            while !shell.flow.is_paused() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("output never paused");
        tokio::time::sleep(Duration::from_millis(500)).await;
        // More window changes than russh queues for the connection task, and a keystroke.
        for cols in 100..150 {
            shell.input.send(SessionInput::Resize { cols, rows: 30 }).unwrap();
        }
        shell.input.send(SessionInput::Data(b"q".to_vec())).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        shell.expect(b"q", (149, 30), true).await;
        shell.bridge.abort();
    }

    /// Typing while output is paused gets no echo, and russh reads nothing meanwhile, but the
    /// hold-up is on this side: the connection is not taken for a dead one.
    #[tokio::test]
    async fn typing_while_output_is_paused_keeps_the_connection() {
        let mut shell = Shell::start(true).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !shell.flow.is_paused() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("output never paused");
        // Until the connection task has filled the channel's queue, and reads no more.
        tokio::time::sleep(Duration::from_millis(500)).await;
        shell.input.send(SessionInput::Data(b"q".to_vec())).unwrap();
        shell.input.send(SessionInput::Resize { cols: 100, rows: 30 }).unwrap();
        // Longer than a check after typing takes, in tests.
        tokio::time::sleep(Duration::from_millis(2000)).await;
        assert!(!shell.bridge.is_finished(), "the connection ended");
        shell.expect(b"q", (100, 30), true).await;
        shell.bridge.abort();
    }

    /// A shell the server reports as killed by a signal is told apart from one that exited.
    #[tokio::test]
    async fn a_shell_ended_by_a_signal_is_reported_as_such() {
        let shell = Shell::start(false).await;
        shell.input.send(SessionInput::Data(b"kill".to_vec())).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(10), shell.bridge).await.unwrap().unwrap();
        assert!(matches!(&outcome, Outcome::Signaled(signal) if signal == "SIGKILL"));

        let shell = Shell::start(false).await;
        shell.input.send(SessionInput::Data(b"exit".to_vec())).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(10), shell.bridge).await.unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Exited(Some(3))));
    }

    /// Output messages waiting are passed on together, up to `GATHER` bytes, and another kind
    /// of message among them waits for its turn.
    #[test]
    fn gathers_the_output_waiting() {
        let data = |text: &str| Some(Some(ChannelMsg::Data { data: text.as_bytes().to_vec().into() }));
        let mut waiting = std::collections::VecDeque::from([data("b"), data("c"), Some(Some(ChannelMsg::Eof)), data("d")]);
        let (output, after) = gather(|| waiting.pop_front().unwrap_or(None), b"a".to_vec());
        assert_eq!(output, b"abc");
        assert!(matches!(after, Some(Some(ChannelMsg::Eof))));
        let (output, after) = gather(|| waiting.pop_front().unwrap_or(None), Vec::new());
        assert_eq!(output, b"d");
        assert!(after.is_none());

        let packet = vec![b'x'; 4096];
        let mut packets = std::iter::repeat_with(|| Some(Some(ChannelMsg::Data { data: packet.clone().into() })));
        let (output, after) = gather(|| packets.next().unwrap(), Vec::new());
        assert_eq!(output.len(), GATHER);
        assert!(after.is_none());
    }

    /// Output in many small packets reaches the terminal whole and in order, and before the
    /// shell's end. (How many messages it takes depends on how fast it is read.)
    #[tokio::test]
    async fn output_waiting_is_passed_on_together() {
        let shell = Shell::start(false).await;
        shell.input.send(SessionInput::Data(b"burst".to_vec())).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(10), shell.bridge).await.unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Exited(Some(0))));
        let messages: Vec<Vec<u8>> = shell.output.try_iter().collect();
        let expected: String = (0..BURST).map(|i| format!("{i}\n")).collect();
        assert_eq!(String::from_utf8(messages.concat()).unwrap(), expected);
    }

    /// Typing and pastes reach the shell whole and in order, with window changes in between.
    #[tokio::test]
    async fn typing_reaches_the_shell_in_order() {
        let mut shell = Shell::start(false).await;
        let mut typed = Vec::new();
        for i in 0..200u16 {
            let line = format!("line {i}\r").into_bytes();
            typed.extend(&line);
            shell.input.send(SessionInput::Data(line)).unwrap();
            if i % 7 == 0 {
                shell.input.send(SessionInput::Resize { cols: 80 + i, rows: 24 }).unwrap();
            }
        }
        let paste = vec![b'p'; 1 << 20];
        typed.extend(&paste);
        shell.input.send(SessionInput::Data(paste)).unwrap();
        shell.input.send(SessionInput::Resize { cols: 120, rows: 40 }).unwrap();
        shell.expect(&typed, (120, 40), false).await;
        shell.bridge.abort();
    }
}
