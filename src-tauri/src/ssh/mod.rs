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

use crate::config::SshProfile;
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
pub async fn run(profile: SshProfile, route: Route, id: SessionId, mut io: TermIo, connections: Connections, carry: Vec<String>) {
    let outcome = match start(&profile, &route, id, &mut io, &connections, &carry).await {
        Ok((channel, disconnect)) => bridge(channel, &mut io, disconnect).await,
        Err(e) => Outcome::Failed(e.into()),
    };
    io.finish(outcome);
}

/// Session backend for a duplicated tab: a new shell on `connection`, which is already
/// registered for this session. No prompts, no auto-started forwards: like a second
/// OpenSSH session through a ControlMaster.
pub async fn run_shared(connection: Arc<Connection>, mut io: TermIo) {
    let outcome = match open_shell(connection.handle(), connection.profile(), &mut io).await {
        Ok(channel) => bridge(channel, &mut io, connection.disconnect_reason()).await,
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
        let (next_host, next_port) = jumps.get(index + 1).map_or((host, port), |next| (next.ssh.remote.host.as_str(), next.ssh.remote.port));
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
    profile: &SshProfile,
    route: &Route,
    id: SessionId,
    io: &mut TermIo,
    connections: &Connections,
    carry: &[String],
) -> Result<(Channel<Msg>, watch::Receiver<Option<String>>)> {
    let remote = &profile.ssh.remote;
    let (transport, chain) = match tunnel(route, &remote.host, remote.port, io).await? {
        Some(Tunnel { stream, via, jumps }) => (Some((stream, via)), jumps),
        None => (None, JumpChain::default()),
    };
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    let proxy = route.proxy.as_ref();
    let mut session = connect(profile, profile.ssh.forward_agent, transport, proxy, io, routes.clone(), disconnect_tx).await?;
    auth::authenticate(&mut session, profile, io).await?;
    let session = Arc::new(session);
    let connection = connections.insert(id, session.clone(), profile.clone(), chain, routes, disconnect.clone(), io);
    connections.auto_start(&connection, carry);
    let channel = open_shell(&session, profile, io).await?;
    Ok((channel, disconnect))
}

/// Starts an interactive shell in a new channel on an authenticated connection, with the
/// profile's terminal type, environment and agent forwarding. Like OpenSSH, rejected
/// environment variables are not reported (the server's `AcceptEnv` decides).
async fn open_shell(session: &SshHandle, profile: &SshProfile, io: &mut TermIo) -> Result<Channel<Msg>> {
    let channel = session.channel_open_session().await.context(Error::new("ssh.channelFailed"))?;
    if profile.ssh.forward_agent {
        channel.agent_forward(false).await?;
    }
    let (cols, rows) = io.size;
    channel.request_pty(false, &profile.ssh.remote.term_type, cols.into(), rows.into(), 0, 0, &[]).await?;
    for var in &profile.ssh.env {
        channel.set_env(false, var.name.clone(), var.value.clone()).await?;
    }
    channel.request_shell(false).await?;
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
/// is in flight at a time ([`Sending`]); while it is, one keystroke or paste waits, and window
/// changes are merged into the latest size (dragging the window edge sends many).
async fn bridge(channel: Channel<Msg>, io: &mut TermIo, mut disconnect: watch::Receiver<Option<String>>) -> Outcome {
    let (mut reader, writer) = channel.split();
    let writer = Arc::new(writer);
    let mut close = CloseOnDrop(Some(writer.clone()));
    let flow = io.sink().flow();
    let mut exit_status = None;
    let mut sending: Option<Sending> = None;
    let mut data = None;
    let mut resize = None;
    loop {
        if sending.is_none() {
            sending = match (data.take(), resize.take()) {
                (Some(bytes), size) => {
                    resize = size;
                    Some(Box::pin(writer.data_bytes(bytes)))
                }
                (None, Some((cols, rows))) => Some(Box::pin(writer.window_change(cols, rows, 0, 0))),
                (None, None) => None,
            };
        }
        let paused = flow.is_paused();
        let sent = tokio::select! {
            () = flow.ready(), if paused => Ok(()),
            msg = reader.wait(), if !paused => match msg {
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
            result = in_flight(&mut sending) => {
                sending = None;
                result
            }
            input = io.recv(), if data.is_none() => match input {
                Some(SessionInput::Data(bytes)) => {
                    data = Some(bytes);
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

/// A message to the server on its way to russh's connection task: data or a window change.
type Sending<'a> = std::pin::Pin<Box<dyn Future<Output = Result<(), russh::Error>> + Send + 'a>>;

/// Completes with the message in flight, if any; pending forever otherwise (for `select!`).
async fn in_flight<F: Future + Unpin>(sending: &mut Option<F>) -> F::Output {
    match sending {
        Some(sending) => sending.await,
        None => std::future::pending().await,
    }
}

/// Connects to one hop: over TCP (through `proxy`, if any), or through `via` (a channel from
/// the previous jump host, with that host's name), then performs the SSH handshake.
/// `forward_agent`: whether the server may open agent channels.
async fn connect(
    hop: &SshProfile,
    forward_agent: bool,
    via: Option<(ChannelStream<Msg>, String)>,
    proxy: Option<&Proxy>,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle> {
    let remote = &hop.ssh.remote;
    let (user, host, port) = (&remote.username, &remote.host, remote.port);
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
    hop: &SshProfile,
    forward_agent: bool,
    stream: S,
    io: &mut TermIo,
    routes: RemoteRoutes,
    disconnect: watch::Sender<Option<String>>,
) -> Result<SshHandle>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let remote = &hop.ssh.remote;
    let preferred = russh::Preferred::default();
    let key = known_hosts::prefer_known(&preferred.key, &remote.host, remote.port).into();
    let config = Arc::new(client::Config {
        keepalive_interval: (remote.keepalive_interval > 0).then(|| Duration::from_secs(remote.keepalive_interval.into())),
        keepalive_max: 3,
        preferred: russh::Preferred { key, ..preferred },
        ..Default::default()
    });
    let (queries_tx, mut queries) = mpsc::channel(1);
    let handler = ClientHandler::new(remote.host.clone(), remote.port, forward_agent, queries_tx, routes, disconnect);

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
                let accepted = host_key::confirm(io, &remote.host, remote.port, &query).await;
                let _ = query.reply.send(accepted);
                deadline.as_mut().reset(tokio::time::Instant::now() + HANDSHAKE_TIMEOUT);
            }
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
    use crate::config::Remote;

    /// A server that accepts the connection but never speaks SSH.
    #[tokio::test(start_paused = true)]
    async fn handshake_gives_up_on_a_silent_server() {
        let (stream, _server) = tokio::io::duplex(1024);
        let profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let result = handshake(&profile, false, stream, &mut io, RemoteRoutes::default(), watch::channel(None).0).await;
        let error = Error::from(result.err().unwrap());
        assert_eq!(error.code(), "ssh.handshakeTimeout");
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

        async fn data(&mut self, _channel: russh::ChannelId, data: &[u8], _session: &mut russh::server::Session) -> Result<(), Self::Error> {
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

    struct AnyHostKey;

    impl client::Handler for AnyHostKey {
        type Error = russh::Error;

        async fn check_server_key(&mut self, _key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

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
            let config = russh::server::Config {
                methods: russh::MethodSet::from(&[russh::MethodKind::None][..]),
                keys: vec![russh::keys::PrivateKey::from_openssh(TEST_KEY).unwrap()],
                ..Default::default()
            };
            let (client_side, server_side) = tokio::io::duplex(1 << 16);
            let (sizes_tx, sizes) = mpsc::unbounded_channel();
            let (typed_tx, typed) = mpsc::unbounded_channel();
            let server = Server { flood, sizes: sizes_tx, typed: typed_tx };
            tokio::spawn(async move {
                if let Ok(session) = russh::server::run_stream(Arc::new(config), server_side, server).await {
                    let _ = session.await;
                }
            });
            let mut session = client::connect_stream(Arc::new(client::Config::default()), client_side, AnyHostKey).await.unwrap();
            assert!(session.authenticate_none("alice").await.unwrap().success());
            let channel = session.channel_open_session().await.unwrap();
            channel.request_shell(false).await.unwrap();
            let (mut io, input, output, _events) = TermIo::detached((80, 24));
            let flow = io.sink().flow();
            let bridge = tokio::spawn(async move { bridge(channel, &mut io, watch::channel(None).1).await });
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
                panic!("the connection is stuck: got {} of {} bytes (same up to {same}), size {latest:?}, {shown} bytes shown", got.len(), typed.len())
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
