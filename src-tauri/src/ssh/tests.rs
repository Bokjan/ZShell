use super::*;
use crate::model::Remote;

/// A server that accepts the connection but never speaks SSH.
#[tokio::test(start_paused = true)]
async fn handshake_gives_up_on_a_silent_server() {
    let (stream, _server) = tokio::io::duplex(1024);
    let profile = SshProfile::quick("t".into(), Remote::new("example.com".into(), 22, "alice".into()));
    let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
    let result = handshake(&profile, 0, HopOptions::jump_host(), stream, &mut io).await;
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
        _ = handshake(&profile, 0, HopOptions::jump_host(), stream, &mut io) => panic!("the handshake ended"),
        // The client's version line and key exchange offer.
        read = server.read(&mut buf) => assert!(read.unwrap() > 0),
    }
    let closed = tokio::time::timeout(Duration::from_secs(5), async { while server.read(&mut buf).await.is_ok_and(|n| n > 0) {} }).await;
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

/// A connection not yet signed in, as one waiting for the user to answer a prompt, stays open
/// however long it waits, though it sends keepalives.
#[tokio::test]
async fn a_connection_waiting_to_sign_in_stays_open() {
    let config = russh::server::Config { keys: vec![russh::keys::PrivateKey::from_openssh(TEST_KEY).unwrap()], ..Default::default() };
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(session) = russh::server::run_stream(Arc::new(config), server_side, testing::Anyone).await {
            let _ = session.await;
        }
    });
    let keepalive = Some(Duration::from_millis(100));
    let (client_side, _guard) = abortable::abortable(client_side, stall_limit(keepalive));
    let (queries, _) = mpsc::channel(1);
    let handler =
        ClientHandler::new(0, "localhost".into(), 22, false, queries, RemoteRoutes::default(), watch::channel(None).0).with_any_host_key();
    let handle = client::connect_stream(Arc::new(client_config(keepalive)), client_side, handler).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!handle.is_closed());
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

/// While the main thread waits for files dragged out of the window, it shows no output and
/// acknowledges none: output is read on rather than paused, which would stop the connection
/// the download runs on until its requests time out.
#[tokio::test]
async fn output_is_read_on_while_the_main_thread_waits_for_a_download() {
    let shell = Shell::start(true).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !shell.flow.is_paused() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("output never paused");
    // Until the bridge has stopped reading.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = shell.output.try_iter().count();

    let reading = KeepReading::hold();
    let read: usize = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let read = shell.output.try_iter().map(|bytes| bytes.len()).sum();
            if read > 0 {
                break read;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("output stayed paused");
    drop(reading);
    assert!(read > 0 && shell.flow.is_paused());
    shell.bridge.abort();
}

/// Waiting for the server counts only while russh waits for it: not while its connection task
/// is held up on this side, by a shell's output that nobody reads (an SFTP channel opening
/// then would otherwise time out, its answer unread).
#[tokio::test]
async fn waiting_for_the_server_leaves_out_holdups_on_this_side() {
    const WAIT: Duration = Duration::from_millis(300);
    let (sizes, _) = mpsc::unbounded_channel();
    let (typed, _) = mpsc::unbounded_channel();
    let connection = testing::connect(Server { flood: true, sizes, typed }, "").await;
    tokio::time::timeout(Duration::from_secs(5), connection.waited_for_server(WAIT)).await.expect("waiting never counted");

    let channel = connection.handle().channel_open_session().await.unwrap();
    channel.request_shell(false).await.unwrap();
    // Never read: the connection task fills the channel's queue and waits.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let held_up = tokio::time::timeout(WAIT * 4, connection.waited_for_server(WAIT)).await;
    assert!(held_up.is_err(), "counted while held up on this side");
    drop(channel);
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
