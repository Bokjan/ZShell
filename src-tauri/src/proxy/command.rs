//! Command proxies (OpenSSH's `ProxyCommand`): a local process whose stdin and stdout carry
//! the connection, like a jump host's channel. What it writes to stderr is shown in the
//! terminal, dimmed, as `ssh` lets it through; dropping the stream kills the process.
//!
//! On macOS the command runs in the user's shell (`$SHELL -c "exec …"`, as OpenSSH does), with
//! the PATH of a login shell: apps started from Finder get a minimal PATH from launchd, without
//! `/opt/homebrew/bin`. That PATH is looked up once, rather than starting every command in a
//! login shell. On Windows the program is started directly, without `cmd`, so that dropping the
//! stream ends the program itself rather than a shell around it; it has no console window.

use std::io;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context as TaskContext, Poll};

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use super::Proxy;
use crate::error::Error;
use crate::net::Stream;
use crate::session::SessionSink;

/// Starts the proxy command for `host:port` (and `user`, for `%r`).
pub async fn spawn(proxy: &Proxy, host: &str, port: u16, user: &str, sink: SessionSink) -> Result<Box<dyn Stream>> {
    let line = expand(&proxy.command, host, port, user)?;
    let mut command = build(&line).await;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = command.spawn().with_context(|| Error::new("proxy.commandFailed").param("command", &line))?;
    let (stdin, stdout) = (child.stdin.take().expect("piped"), child.stdout.take().expect("piped"));
    let mut stderr = child.stderr.take().expect("piped");
    tauri::async_runtime::spawn(async move {
        let mut buffer = vec![0; 4096];
        while let Ok(n @ 1..) = stderr.read(&mut buffer).await {
            let text = String::from_utf8_lossy(&buffer[..n]);
            sink.print(&format!("\x1b[2m{text}\x1b[0m"));
        }
    });
    Ok(Box::new(CommandStream { _child: child, stdin: Some(stdin), stdout }))
}

/// The name of the program a command runs, without its directory and extension.
pub fn program_name(command: &str) -> String {
    let program = split_program(command).0;
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    name.strip_suffix(".exe").unwrap_or(name).to_owned()
}

/// Characters a host or user name put into the command must not contain, besides whitespace
/// (which would split it into more arguments) and control characters: the shell interprets
/// them. A session imported from someone else's file could otherwise run any command when it
/// connects; OpenSSH refuses the same since CVE-2023-51385.
const SHELL_SPECIAL: &str = "'`\"$\\;&<>|(){}";

fn check_name(name: &str) -> Result<&str, Error> {
    let unsafe_char = |c: char| c.is_whitespace() || c.is_control() || SHELL_SPECIAL.contains(c);
    if name.starts_with('-') || name.chars().any(unsafe_char) {
        return Err(Error::new("proxy.unsafeName").param("name", name));
    }
    Ok(name)
}

/// Replaces `%h`, `%p`, `%r` and `%%`; other `%` sequences are kept as they are.
fn expand(command: &str, host: &str, port: u16, user: &str) -> Result<String, Error> {
    let mut line = String::with_capacity(command.len());
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            line.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => line.push_str(check_name(host)?),
            Some('p') => line.push_str(&port.to_string()),
            Some('r') => line.push_str(check_name(user)?),
            Some('%') => line.push('%'),
            Some(other) => {
                line.push('%');
                line.push(other);
            }
            None => line.push('%'),
        }
    }
    Ok(line)
}

/// The program and the rest of a command line; the program may be in double quotes.
fn split_program(command: &str) -> (&str, &str) {
    let command = command.trim_start();
    match command.strip_prefix('"') {
        Some(rest) => rest.split_once('"').unwrap_or((rest, "")),
        None => command.split_once(char::is_whitespace).unwrap_or((command, "")),
    }
}

#[cfg(unix)]
async fn build(line: &str) -> Command {
    let shell = portable_pty::CommandBuilder::new_default_prog().get_shell();
    let mut command = Command::new(shell);
    command.arg("-c").arg(format!("exec {line}"));
    if let Some(path) = login_path().await {
        command.env("PATH", path);
    }
    command
}

#[cfg(windows)]
async fn build(line: &str) -> Command {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let (program, args) = split_program(line);
    let mut command = Command::new(program);
    command.raw_arg(args.trim_start());
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// The PATH a login shell sets up, looked up once; `None` if that fails (the inherited PATH
/// is used then).
#[cfg(unix)]
async fn login_path() -> Option<String> {
    static PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();
    PATH.get_or_init(|| async {
        const MARKER: &str = "__ZSHELL_PATH__";
        let shell = portable_pty::CommandBuilder::new_default_prog().get_shell();
        // Startup files may print something too; the markers find the value.
        let output = Command::new(shell)
            .args(["-l", "-c", &format!("printf '{MARKER}%s{MARKER}' \"$PATH\"")])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = tokio::time::timeout(std::time::Duration::from_secs(5), output).await.ok()?.ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let path = text.split(MARKER).nth(1)?;
        (!path.is_empty()).then(|| path.to_owned())
    })
    .await
    .clone()
}

/// The process's stdout to read from and stdin to write to.
struct CommandStream {
    /// Killed when dropped.
    _child: Child,
    /// Closed on shutdown: tokio's `ChildStdin::poll_shutdown` leaves the pipe open, so the
    /// process would never see the end of its input.
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
}

fn closed() -> io::Error {
    io::Error::from(io::ErrorKind::BrokenPipe)
}

impl AsyncRead for CommandStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for CommandStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.stdin {
            Some(stdin) => Pin::new(stdin).poll_write(cx, buf),
            None => Poll::Ready(Err(closed())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match &mut self.stdin {
            Some(stdin) => Pin::new(stdin).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        if let Some(stdin) = &mut self.stdin {
            std::task::ready!(Pin::new(stdin).poll_flush(cx))?;
        }
        self.stdin = None;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tokens() {
        assert_eq!(expand("nc -X 5 -x proxy:1080 %h %p", "db", 22, "alice").unwrap(), "nc -X 5 -x proxy:1080 db 22");
        assert_eq!(expand("ssh -W %h:%p %r@jump", "db", 2222, "bob").unwrap(), "ssh -W db:2222 bob@jump");
        assert_eq!(expand("echo 100%% %x %", "h", 1, "").unwrap(), "echo 100% %x %");
        assert_eq!(expand("nc %h %p", "fe80::1%en0", 22, "").unwrap(), "nc fe80::1%en0 22");
        // Names the shell would interpret are refused, wherever the session came from.
        for (host, user) in [("db$(curl x|sh)", "a"), ("db;reboot", "a"), ("db", "a`id`"), ("db\nx", "a"), ("-oProxyCommand=x", "a"), ("db", "a b")] {
            assert_eq!(expand("ssh -W %h:%p %r@jump", host, 22, user).unwrap_err().code(), "proxy.unsafeName", "{host:?} {user:?}");
        }
    }

    #[test]
    fn names_programs() {
        assert_eq!(program_name("/opt/homebrew/bin/cloudflared access ssh --hostname %h"), "cloudflared");
        assert_eq!(program_name(r#""C:\Program Files\Git\usr\bin\connect.exe" -S proxy:1080 %h %p"#), "connect");
        assert_eq!(split_program(r#""C:\a b\nc.exe" -x p %h"#), (r"C:\a b\nc.exe", " -x p %h"));
        assert_eq!(split_program("nc"), ("nc", ""));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn carries_the_connection_over_stdin_and_stdout() {
        use tokio::io::AsyncWriteExt;

        let mut command = build("tr a-z A-Z").await;
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut stream = CommandStream { stdin: child.stdin.take(), stdout: child.stdout.take().unwrap(), _child: child };
        stream.write_all(b"hello").await.unwrap();
        stream.shutdown().await.unwrap();
        let mut output = String::new();
        stream.read_to_string(&mut output).await.unwrap();
        assert_eq!(output, "HELLO");
    }
}
