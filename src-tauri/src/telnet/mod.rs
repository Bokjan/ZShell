//! Telnet sessions: a TCP connection (or a tunnel through SSH jump hosts) carrying the
//! Telnet protocol ([`protocol`]).
//!
//! A saved user name and password are typed at the server's login prompts ([`AutoLogin`]);
//! otherwise the server asks and the user answers, as with the `telnet` command. When the
//! server leaves echoing to ZShell, the password typed at its prompt is not shown, and so
//! not logged either ([`LocalEcho`]).

mod protocol;

use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::error::Error;
use crate::model::Remote;
use crate::net::{Route, Stream};
use crate::session::{Outcome, SessionEvent, SessionInput, TermIo};
use crate::ssh::{self, JumpChain};
use protocol::{Telnet, BREAK};

const READ_BUFFER: usize = 16 << 10;
/// What waits to be written to the server; beyond that, new input waits (a ZMODEM upload is
/// paced by the connection), and so does reading the server's output (see [`bridge`]).
const OUTGOING_LIMIT: usize = 64 << 10;

/// Session backend: connects to the session's host (by `route`) and bridges the Telnet
/// session to the terminal. `password` is the saved one, if any, looked up when the server
/// asks for it.
pub async fn run(remote: Remote, route: Route, password: impl FnOnce() -> Option<String> + Send, mut io: TermIo) {
    let outcome = match connect(&remote, &route, &mut io).await {
        Ok((stream, jumps)) => {
            let login = AutoLogin::new(&remote.username, password);
            bridge(stream, jumps, &remote.term_type, login, &mut io).await
        }
        Err(e) => Outcome::Failed(e.into()),
    };
    io.finish(outcome);
}

async fn connect(remote: &Remote, route: &Route, io: &mut TermIo) -> Result<(Box<dyn Stream>, JumpChain)> {
    let (host, port) = (remote.host.as_str(), remote.port);
    if let Some(tunnel) = ssh::tunnel(route, host, port, io).await? {
        let connecting = t!("terminal.connectingToVia", host = host, port = port, jump = tunnel.via);
        io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
        return Ok((Box::new(tunnel.stream), tunnel.jumps));
    }
    let proxy = route.proxy.as_ref();
    let connecting = match proxy {
        Some(proxy) => t!("terminal.connectingToViaProxy", host = host, port = port, proxy = proxy.name),
        None => t!("terminal.connectingTo", host = host, port = port),
    };
    io.print(&format!("\x1b[2m{connecting}\x1b[0m\n"));
    let stream = crate::net::connect(host, port, &remote.username, proxy, &route.passwords, remote.keepalive(), io).await?;
    Ok((stream, JumpChain::default()))
}

/// Passes data between the connection and the terminal until either side ends.
///
/// Writes happen on their own task, so the server's output keeps being read while a write
/// waits (a large paste or a ZMODEM upload while the server prints).
async fn bridge(
    stream: Box<dyn Stream>,
    jumps: JumpChain,
    term_type: &str,
    mut login: AutoLogin<impl FnOnce() -> Option<String>>,
    io: &mut TermIo,
) -> Outcome {
    let lost = |detail: Option<String>| {
        let error = Error::new("net.connectionLost");
        Outcome::Lost(match detail {
            Some(detail) => error.detail(detail),
            None => error,
        })
    };
    let (mut reader, writer) = tokio::io::split(stream);
    let (tx, rx) = mpsc::channel(1);
    // Stopped when the session closes: a server that no longer reads would keep a write
    // waiting for ever, and with it the connection (the stream closes once both halves are
    // dropped) and a proxy command's process.
    let writing = tokio::spawn(write_all(writer, rx));
    io.on_close(move || writing.abort());

    let mut telnet = Telnet::new(term_type, io.size);
    let mut echo = LocalEcho::default();
    let mut outgoing = telnet.start();
    io.event(SessionEvent::Connected);
    let mut buffer = vec![0; READ_BUFFER];
    let flow = io.sink().flow();
    loop {
        // Not read while the terminal is behind: TCP holds the server back meanwhile. Nor
        // while what we send waits: the server's negotiation needs answers, and one that
        // doesn't read them would otherwise have them pile up here.
        let paused = flow.is_paused();
        let reading = !paused && outgoing.len() < OUTGOING_LIMIT;
        tokio::select! {
            () = flow.ready(), if paused => {}
            read = reader.read(&mut buffer), if reading => match read {
                // A tunnel also ends when its jump host connection does.
                Ok(0) if jumps.is_broken() => return lost(None),
                Ok(0) => return Outcome::Exited(None),
                Ok(n) => {
                    let received = telnet.receive(&buffer[..n]);
                    outgoing.extend(received.reply);
                    if !received.data.is_empty() {
                        login.output(&received.data);
                        echo.output(&received.data);
                        io.output(received.data);
                    }
                }
                Err(e) => return lost(Some(e.to_string())),
            },
            permit = tx.reserve(), if !outgoing.is_empty() => match permit {
                Ok(permit) => permit.send(std::mem::take(&mut outgoing)),
                // The writer failed.
                Err(_) => return lost(None),
            },
            () = login.due() => {
                if let Some(answer) = login.answer(io.sink().encoding()) {
                    outgoing.extend(telnet.encode(&answer));
                }
            },
            input = io.recv(), if outgoing.len() < OUTGOING_LIMIT => match input {
                // A ZMODEM transfer's data is neither typing to echo nor a login answer.
                Some(SessionInput::Data(data)) if io.is_transfer_data() => outgoing.extend(telnet.encode(&data)),
                Some(SessionInput::Data(data)) => {
                    login.typed();
                    if !telnet.remote_echoes() {
                        let shown = echo.typed(&data);
                        if !shown.is_empty() {
                            // Whole characters, decoded on their own: the output's decoder may
                            // hold the first byte of a character the server's output stopped
                            // in the middle of, which a typed `a` would complete as GBK.
                            let sink = io.sink();
                            sink.write(crate::encoding::decode(sink.encoding(), &shown).into_bytes());
                        }
                    }
                    outgoing.extend(telnet.encode(&data));
                }
                // Not typing: not echoed, and not a login answer.
                Some(SessionInput::Raw(data)) => outgoing.extend(telnet.encode(&data)),
                Some(SessionInput::Resize { cols, rows }) => outgoing.extend(telnet.resize(cols, rows)),
                Some(SessionInput::Break) => outgoing.extend(BREAK),
                // The tab is closing.
                None => return Outcome::Exited(None),
            },
        }
    }
}

/// Writes what the session sends until the session ends or a write fails.
async fn write_all(mut writer: tokio::io::WriteHalf<Box<dyn Stream>>, mut rx: mpsc::Receiver<Vec<u8>>) {
    while let Some(bytes) = rx.recv().await {
        if writer.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = writer.shutdown().await;
}

/// Echoes typed input when the server doesn't (NVT's default), except for a line typed at a
/// password prompt, which would otherwise be on the screen (as `telnet` and PuTTY show it)
/// and in the session's log. Only the server's output counts as a prompt, so that typing
/// `password:` hides nothing.
#[derive(Default)]
struct LocalEcho {
    line: LastLine,
    /// Enter was typed since the server's output last changed: the prompt has its answer.
    entered: bool,
}

impl LocalEcho {
    fn output(&mut self, data: &[u8]) {
        self.line.push(data);
        self.entered = false;
    }

    /// What the terminal shows of `data`, typed.
    fn typed(&mut self, data: &[u8]) -> Vec<u8> {
        if self.entered || !self.line.text().ends_with(PASSWORD_PROMPT) {
            return local_echo(data);
        }
        // Hidden up to Enter, which shows as a new line.
        let Some(at) = data.iter().position(|&b| b == b'\r') else {
            return Vec::new();
        };
        self.entered = true;
        local_echo(&data[at..])
    }
}

/// What the terminal shows of typed input when the server doesn't echo: printable text,
/// Enter as a new line and Backspace erasing; escape sequences (arrow keys) show nothing. In
/// the session's encoding, like the input.
fn local_echo(data: &[u8]) -> Vec<u8> {
    if data.first() == Some(&0x1b) {
        return Vec::new();
    }
    let mut echo = Vec::with_capacity(data.len());
    for &byte in data {
        match byte {
            b'\r' => echo.extend(b"\r\n"),
            0x7f | 0x08 => echo.extend(b"\x08 \x08"),
            b'\t' => echo.push(byte),
            byte if byte >= 0x20 => echo.push(byte),
            _ => {}
        }
    }
    echo
}

/// How long the output must pause at a login prompt before it is answered.
const PROMPT_IDLE: Duration = Duration::from_millis(300);
/// Prompts later than this after connecting are left to the user.
const LOGIN_WINDOW: Duration = Duration::from_secs(30);
/// The end of the output kept for recognizing prompts.
const TAIL: usize = 128;
/// How a password prompt ends, in lower case.
const PASSWORD_PROMPT: &str = "password:";

/// The server's output since the last line break, up to [`TAIL`] bytes: where a prompt is.
#[derive(Default)]
struct LastLine(Vec<u8>);

impl LastLine {
    fn push(&mut self, data: &[u8]) {
        let line = match data.iter().rposition(|&b| b == b'\n' || b == b'\r') {
            Some(at) => {
                self.0.clear();
                &data[at + 1..]
            }
            None => data,
        };
        self.0.extend(&line[line.len().saturating_sub(TAIL)..]);
        if self.0.len() > TAIL {
            self.0.drain(..self.0.len() - TAIL);
        }
    }

    /// The line in lower case, without trailing white space.
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0).trim_end().to_lowercase()
    }
}

#[derive(Debug, PartialEq)]
enum Prompt {
    User,
    Password,
}

/// Types the saved user name and password at the server's login prompts, each at most
/// once, and only until the user types something or the login window has passed.
struct AutoLogin<F> {
    user: Option<String>,
    password: Option<F>,
    tail: LastLine,
    /// When to answer the prompt the output ends with.
    due: Option<Instant>,
    until: Instant,
}

impl<F: FnOnce() -> Option<String>> AutoLogin<F> {
    fn new(user: &str, password: F) -> Self {
        Self {
            user: Some(user.to_owned()).filter(|user| !user.is_empty()),
            password: Some(password),
            tail: LastLine::default(),
            due: None,
            until: Instant::now() + LOGIN_WINDOW,
        }
    }

    fn active(&self) -> bool {
        (self.user.is_some() || self.password.is_some()) && Instant::now() < self.until
    }

    fn output(&mut self, data: &[u8]) {
        if !self.active() {
            self.due = None;
            return;
        }
        self.tail.push(data);
        self.due = self.prompt().map(|_| Instant::now() + PROMPT_IDLE);
    }

    /// The prompt the output ends with, if there is something to answer it with.
    fn prompt(&self) -> Option<Prompt> {
        let text = self.tail.text();
        if self.user.is_some() && ["login:", "username:", "user name:"].iter().any(|p| text.ends_with(p)) {
            return Some(Prompt::User);
        }
        if self.password.is_some() && text.ends_with(PASSWORD_PROMPT) {
            return Some(Prompt::Password);
        }
        None
    }

    /// Completes when a prompt is to be answered; pending otherwise (for `select!`).
    async fn due(&self) {
        match self.due {
            Some(due) => tokio::time::sleep_until(due).await,
            None => std::future::pending().await,
        }
    }

    /// The answer to the prompt, with Enter, in `encoding`; `None` if there is none (the
    /// password is not saved).
    fn answer(&mut self, encoding: &'static encoding_rs::Encoding) -> Option<Vec<u8>> {
        self.due = None;
        let answer = match self.prompt()? {
            Prompt::User => self.user.take(),
            Prompt::Password => self.password.take().and_then(|password| password()),
        }?;
        self.tail = LastLine::default();
        let mut bytes = encoding.encode(&answer).0.into_owned();
        bytes.push(b'\r');
        Some(bytes)
    }

    /// The user typed something: the login is theirs from now on.
    fn typed(&mut self) {
        self.user = None;
        self.password = None;
        self.due = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn answers_login_prompts_once() {
        let mut login = AutoLogin::new("alice", || Some("secret".to_owned()));
        login.output(b"Welcome\r\n\r\nrouter login");
        assert_eq!(login.due, None);
        login.output(b": ");
        login.due().await;
        assert_eq!(login.answer(encoding_rs::UTF_8).as_deref(), Some(&b"alice\r"[..]));

        login.output(b"alice\r\nPassword: ");
        login.due().await;
        assert_eq!(login.answer(encoding_rs::UTF_8).as_deref(), Some(&b"secret\r"[..]));

        // A second prompt (wrong password) is left to the user.
        login.output(b"\r\nLogin incorrect\r\nrouter login: ");
        assert_eq!(login.due, None);
    }

    #[tokio::test(start_paused = true)]
    async fn leaves_the_login_to_the_user() {
        // Without a saved password, the password prompt gets nothing.
        let mut login = AutoLogin::new("", || None);
        login.output(b"Username: ");
        assert_eq!(login.due, None);
        login.output(b"\r\nPassword:");
        login.due().await;
        assert_eq!(login.answer(encoding_rs::UTF_8), None);

        // Once the user types, nothing more is answered.
        let mut login = AutoLogin::new("bob", || Some("pw".to_owned()));
        login.typed();
        login.output(b"login: ");
        assert_eq!(login.due, None);

        // Nor after the login window.
        let mut login = AutoLogin::new("bob", || Some("pw".to_owned()));
        tokio::time::advance(LOGIN_WINDOW).await;
        login.output(b"login: ");
        assert_eq!(login.due, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn typing_does_not_complete_a_character_of_the_output() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // "中" in GBK, in two pieces with the user typing in between.
            stream.write_all(&[0xd6]).await.unwrap();
            let mut buffer = [0; 64];
            while !buffer.contains(&b'a') {
                assert_ne!(stream.read(&mut buffer).await.unwrap(), 0);
            }
            stream.write_all(&[0xd0]).await.unwrap();
        });

        let remote = Remote::new("127.0.0.1".into(), port, String::new());
        let (io, input, output, _events) = TermIo::detached_with((80, 24), encoding_rs::GBK);
        let session = tokio::spawn(run(remote, Route::default(), || None, io));
        tokio::time::sleep(Duration::from_millis(300)).await;
        input.send(SessionInput::Data(b"a".to_vec())).unwrap();
        server.await.unwrap();
        session.await.unwrap();
        let text = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
        assert!(text.contains("a中"), "{text:?}");
    }

    #[tokio::test]
    async fn talks_to_a_server() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // Ask for the window size, offer to echo, and prompt for the user name.
            stream.write_all(&[255, 253, 31, 255, 251, 1]).await.unwrap();
            stream.write_all(b"\r\nlogin: ").await.unwrap();
            let mut received: Vec<u8> = Vec::new();
            let mut buffer = [0; 1024];
            while !received.windows(7).any(|w| w == b"alice\r\0") {
                let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
                assert_ne!(n, 0);
                received.extend(&buffer[..n]);
            }
            stream.write_all("欢迎\r\n".as_bytes()).await.unwrap();
            received
        });

        let remote = Remote::new("127.0.0.1".into(), port, "alice".into());
        let (io, _input, output, events) = TermIo::detached((80, 24));
        run(remote, Route::default(), || None, io).await;

        let received = server.await.unwrap();
        assert!(received.windows(9).any(|w| w == [255, 250, 31, 0, 80, 0, 24, 255, 240]), "{received:?}");
        let text = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
        assert!(text.contains("login: ") && text.contains("欢迎") && text.contains(&t!("terminal.closed")), "{text:?}");
        let events: Vec<String> = events.try_iter().collect();
        assert!(events[0].contains(r#""type":"connected""#));
        assert!(events.last().unwrap().contains(r#""reason":"exited""#), "{events:?}");
    }

    /// A server that never offers to echo: the user name typed shows, the password doesn't.
    #[tokio::test]
    async fn does_not_show_a_password_typed_without_remote_echo() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut received: Vec<u8> = Vec::new();
            let mut buffer = [0; 1024];
            for (prompt, answer) in [(&b"login: "[..], &b"bob\r"[..]), (b"Password: ", b"hunter2\r")] {
                stream.write_all(prompt).await.unwrap();
                while !received.windows(answer.len()).any(|w| w == answer) {
                    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
                    assert_ne!(n, 0);
                    received.extend(&buffer[..n]);
                }
            }
            stream.write_all(b"\r\n$ ").await.unwrap();
        });

        let remote = Remote::new("127.0.0.1".into(), port, String::new());
        let (io, input, output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(run(remote, Route::default(), || None, io));
        tokio::time::sleep(Duration::from_millis(200)).await;
        input.send(SessionInput::Data(b"bob\r".to_vec())).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        input.send(SessionInput::Data(b"hunter2\r".to_vec())).unwrap();
        server.await.unwrap();
        session.await.unwrap();
        let text = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
        assert!(text.contains("login: bob\r\nPassword: \r\n\r\n$ "), "{text:?}");
        assert!(!text.contains("hunter2"), "{text:?}");
    }

    /// Closing the tab while a write waits for a server that doesn't read closes the
    /// connection all the same.
    #[tokio::test]
    async fn closing_stops_a_write_the_server_never_reads() {
        let (client, mut server) = tokio::io::duplex(64);
        let (mut io, input, _output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(async move {
            let outcome = bridge(Box::new(client), JumpChain::default(), "xterm", AutoLogin::new("", || None), &mut io).await;
            io.finish(outcome);
        });
        input.send(SessionInput::Data(vec![b'x'; 4096])).unwrap();
        // The paste is being written when the tab closes.
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(input);
        session.await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(server.write_all(b"y").await.is_err());
    }

    /// A server that sends negotiation without end and never reads the answers: they don't
    /// pile up, as reading stops once they wait (and the server's writes then wait too).
    #[tokio::test]
    async fn stops_reading_while_answers_wait() {
        let (client, mut server) = tokio::io::duplex(64);
        let (mut io, _input, _output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(async move {
            let outcome = bridge(Box::new(client), JumpChain::default(), "xterm", AutoLogin::new("", || None), &mut io).await;
            io.finish(outcome);
        });
        // IAC WILL 5 (status), refused each time with IAC DONT 5.
        let flood = [255, 251, 5].repeat(1 << 20);
        let sent = tokio::time::timeout(Duration::from_secs(2), server.write_all(&flood)).await;
        assert!(sent.is_err(), "all of the negotiation was read");
        session.abort();
    }

    #[test]
    fn hides_the_password_typed_at_its_prompt() {
        let mut echo = LocalEcho::default();
        echo.output(b"\r\nlogin: ");
        assert_eq!(echo.typed(b"alice"), b"alice");
        assert_eq!(echo.typed(b"\r"), b"\r\n");
        echo.output(b"Password: ");
        assert_eq!(echo.typed(b"se"), b"");
        assert_eq!(echo.typed(b"\x7f"), b"");
        // A paste, Enter and what is typed ahead of the server's answer.
        assert_eq!(echo.typed(b"cret\rls"), b"\r\nls");
        assert_eq!(echo.typed(b" -l"), b" -l");

        // The server's output, not the user's typing, makes a prompt.
        echo.output(b"\r\n$ ");
        assert_eq!(echo.typed(b"echo Password: "), b"echo Password: ");
        assert_eq!(echo.typed(b"x"), b"x");
        echo.output(b"\r\nOld password:\r\nNew password:  ");
        assert_eq!(echo.typed(b"x"), b"");
    }

    #[test]
    fn echoes_locally() {
        assert_eq!(local_echo(b"ls -l\r"), b"ls -l\r\n");
        assert_eq!(local_echo(b"\x7f"), b"\x08 \x08");
        assert_eq!(local_echo(b"\x1b[A"), b"");
        assert_eq!(local_echo(b"\x03"), b"");
    }
}
