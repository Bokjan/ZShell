//! Local shells in a pseudo terminal (portable-pty; ConPTY on Windows).
//!
//! On Unix the session's tasks drive the terminal (`unix.rs`): tokio waits for it to be
//! readable or writable and for the shell to exit. ConPTY's pipes only block, so on Windows
//! each session runs three threads beside its task (`windows.rs`): one reads output (pausing
//! while the frontend is behind, see [`Flow`]), one writes input (writes block while the
//! shell isn't reading), and one waits for the shell to exit. The session ends when the shell
//! exits rather than at the end of the output: background jobs can keep the terminal open
//! after that, and ConPTY only closes its output when the pseudo console is closed.
//!
//! Once the session has shut down, reading stops, and with it our last handle to the
//! terminal, which then closes for whatever still has it open. macOS closes it anyway when
//! the shell exits (the shell's session loses its terminal), Linux doesn't: a program that
//! ignores the hangup (`nohup`) would keep the reader waiting as long as it runs.

mod shell;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::time::Duration;

use portable_pty::{ExitStatus, PtySize};
use tokio::sync::oneshot;

use crate::error::Error;
use crate::session::{Flow, Outcome, SessionEvent, TermIo};
pub use shell::{default_shell, local_shells_allowed, Shell};
#[cfg(unix)]
use unix::Pty;
#[cfg(windows)]
use windows::Pty;

/// How long to wait for the rest of the output once the shell has exited.
#[cfg(all(unix, not(test)))]
const DRAIN_TIMEOUT: Duration = Duration::from_millis(200);
/// Tests check all the output of a shell that exits, which a busy machine can take longer
/// than that to read; the reader's reaching the end cuts the wait short anyway.
#[cfg(all(unix, test))]
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a shell gets to exit after its terminal is closed before it is killed.
const KILL_GRACE: Duration = Duration::from_secs(2);
const READ_BUFFER: usize = 64 << 10;
/// How long a closed local session takes at most to let go of its output (and its log): its
/// shell is killed after `KILL_GRACE`, and reading stops then (on Windows, as the pseudo
/// console closes, before that).
pub const RELEASED_WITHIN: Duration = KILL_GRACE;

/// Session backend: runs `shell` in a pseudo terminal until it exits.
pub async fn run(shell: Shell, mut io: TermIo) {
    let mut pty = match Pty::start(shell, &io) {
        Ok(pty) => pty,
        Err(e) => return io.finish(Outcome::Failed(Error::from(e))),
    };
    io.event(SessionEvent::Connected);
    let Some(status) = pty.bridge(&mut io).await else {
        return;
    };
    pty.drain().await;
    let signal = status.as_ref().and_then(|s| s.signal()).map(str::to_owned);
    let code = status.as_ref().filter(|s| s.signal().is_none()).map(ExitStatus::exit_code);
    // The pseudo terminal is closed first (dropping it), as the other backends release what
    // they hold before reporting.
    drop(pty);
    io.finish(Outcome::ProcessEnded { code, signal });
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize { cols, rows, pixel_width: 0, pixel_height: 0 }
}

/// After the shell has exited, gives the reader a moment to pass on the rest of the output
/// (`done` completes at its end; a background job may keep the terminal open indefinitely).
/// Time the reader spends waiting for the frontend to catch up (`cat big.log; exit`) doesn't
/// count: the rest of the output would otherwise follow the message that the shell exited.
async fn drain_output(mut done: oneshot::Receiver<()>, flow: &Flow) {
    const STEP: Duration = Duration::from_millis(20);
    let mut left = DRAIN_TIMEOUT;
    while !left.is_zero() {
        if flow.is_paused() {
            tokio::select! {
                _ = &mut done => return,
                () = flow.ready() => continue,
            }
        }
        let step = left.min(STEP);
        tokio::select! {
            _ = &mut done => return,
            () = tokio::time::sleep(step) => left -= step,
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    use portable_pty::CommandBuilder;

    use super::*;
    use crate::session::SessionInput;

    /// How long a test waits for something before failing.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// Waits until `done` holds; fails if it doesn't for a while.
    async fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn sh(script: &str) -> Shell {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        shell::configure(&mut command);
        Shell { command, program: "/bin/sh".to_owned() }
    }

    /// An interactive shell, which runs commands in their own process groups (job control).
    fn sh_interactive() -> Shell {
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-i");
        shell::configure(&mut command);
        Shell { command, program: "/bin/sh".to_owned() }
    }

    fn text(output: &Receiver<Vec<u8>>) -> String {
        String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_output_and_exit_code() {
        let (io, _input, output, events) = TermIo::detached((80, 24));
        run(sh(r#"printf '%s %s %s' "$TERM" "$COLORTERM" "$(stty size)"; exit 3"#), io).await;
        let text = text(&output);
        assert!(text.contains("xterm-256color truecolor 24 80"), "{text:?}");
        assert!(text.contains(&t!("terminal.processExitedWithCode", code = 3)), "{text:?}");
        let events: Vec<String> = events.try_iter().collect();
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(events[0].contains(r#""type":"connected""#));
        assert!(events[1].contains(r#""reason":"exited""#) && events[1].contains(r#""status":3"#), "{events:?}");
    }

    /// Output still waiting for the frontend to catch up when the shell exits comes before
    /// the message that it exited, however long the frontend takes.
    #[tokio::test(flavor = "multi_thread")]
    async fn output_held_back_comes_before_the_exit_message() {
        let (io, _input, output, _events) = TermIo::detached((80, 24));
        let flow = io.sink().flow();
        // A little more than the flow control lets through unacknowledged, so that the shell
        // gets to exit while the reader waits.
        let script = format!("head -c {} /dev/zero | tr '\\000' x; printf END; exit 0", (2 << 20) + 500);
        let session = tokio::spawn(run(sh(&script), io));
        // Catching up only well after the drain timeout. (A busy machine slow to run the shell
        // could leave less than that, and the test would then pass when it shouldn't.)
        tokio::time::sleep(DRAIN_TIMEOUT + Duration::from_secs(1)).await;
        let mut shown = Vec::new();
        while !session.is_finished() {
            for chunk in output.try_iter() {
                flow.ack(chunk.len());
                shown.extend(chunk);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        shown.extend(output.try_iter().flatten());
        let shown = String::from_utf8_lossy(&shown);
        let end = shown.find("END").expect("all the output");
        let exited = shown.find(&t!("terminal.processExited").to_string()).expect("the exit message");
        assert!(end < exited, "{:?}", &shown[shown.len() - 200..]);
    }

    /// A stream of output reaches the terminal whole. (How many messages it takes depends on
    /// how fast the shell writes it: see `reads_on_while_more_is_ready`.)
    #[tokio::test(flavor = "multi_thread")]
    async fn sends_what_is_ready_together() {
        const SIZE: usize = 4 << 20;
        let (io, _input, output, _events) = TermIo::detached((80, 24));
        let flow = io.sink().flow();
        let script = format!("head -c {SIZE} /dev/zero | tr '\\000' x; exit 0");
        let session = tokio::spawn(run(sh(&script), io));
        let (mut messages, mut bytes) = (0, 0);
        loop {
            let finished = session.is_finished();
            for chunk in output.try_iter() {
                flow.ack(chunk.len());
                messages += 1;
                bytes += chunk.len();
            }
            if finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(bytes >= SIZE && messages > 0, "{bytes}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn passes_input_and_resizes() {
        let (io, input, output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(run(sh("read line; stty size; echo \"got $line\""), io));
        input.send(SessionInput::Resize { cols: 100, rows: 30 }).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        input.send(SessionInput::Data(b"hello\r".to_vec())).unwrap();
        session.await.unwrap();
        let text = text(&output);
        assert!(text.contains("30 100") && text.contains("got hello"), "{text:?}");
    }

    /// Input the shell doesn't read doesn't hold up resizes.
    #[tokio::test(flavor = "multi_thread")]
    async fn resizes_while_input_is_backed_up() {
        let (io, input, output, _events) = TermIo::detached((80, 24));
        // The shell waits for the resize itself; the timeout below fails the test if it never
        // comes.
        let script = r#"stty raw -echo; while [ "$(stty size)" != "30 100" ]; do sleep 0.05; done; stty size; exit 4"#;
        let session = tokio::spawn(run(sh(script), io));
        for _ in 0..200 {
            input.send(SessionInput::Data(vec![b'x'; 16 << 10])).unwrap();
        }
        input.send(SessionInput::Resize { cols: 100, rows: 30 }).unwrap();
        tokio::time::timeout(Duration::from_secs(10), session).await.unwrap().unwrap();
        let text = text(&output);
        assert!(text.contains("30 100"), "{text:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_failure_is_reported() {
        let (io, _input, _output, events) = TermIo::detached((80, 24));
        let shell = Shell { command: CommandBuilder::new("/nonexistent/shell"), program: "/nonexistent/shell".to_owned() };
        run(shell, io).await;
        let events: Vec<String> = events.try_iter().collect();
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(events[0].contains(r#""reason":"failed""#) && events[0].contains(r#""code":"pty.spawnFailed""#), "{events:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_foreground_programs() {
        let (io, input, _output, _events) = TermIo::detached((80, 24));
        let foreground = io.foreground();
        let session = tokio::spawn(run(sh_interactive(), io));
        // The shell is in the foreground from the start; the sleep only gives it time to start.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(foreground.get(), None);
        // Long enough not to end on its own before it is seen.
        input.send(SessionInput::Data(b"sleep 30\r".to_vec())).unwrap();
        until("sleep in the foreground", || foreground.get().as_deref() == Some("sleep")).await;
        input.send(SessionInput::Data(b"\x03".to_vec())).unwrap();
        until("the shell back in the foreground", || foreground.get().is_none()).await;
        input.send(SessionInput::Data(b"exit\r".to_vec())).unwrap();
        session.await.unwrap();
        assert_eq!(foreground.get(), None);
    }

    /// Closing the tab must not submit a line that was typed but not entered, here to a
    /// program in the foreground (which gets SIGHUP only after the shell).
    #[tokio::test(flavor = "multi_thread")]
    async fn closing_does_not_submit_typed_line() {
        let out = std::env::temp_dir().join(format!("zshell-pty-close-{}", uuid::Uuid::new_v4()));
        let (io, input, _output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(run(sh_interactive(), io));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let program = format!("/bin/sh -c 'read line; echo \"$line\" > {}'\r", out.display());
        input.send(SessionInput::Data(program.into_bytes())).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        input.send(SessionInput::Data(b"rm -rf important".to_vec())).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        session.abort();
        tokio::time::sleep(KILL_GRACE / 2).await;
        let submitted = out.exists();
        let _ = std::fs::remove_file(&out);
        assert!(!submitted, "the typed line was submitted");
    }

    /// Closing the tab hangs up on the shell, and kills it once `KILL_GRACE` has passed if
    /// it ignores the hangup.
    #[tokio::test(flavor = "multi_thread")]
    async fn closing_kills_a_shell_that_ignores_the_hangup() {
        let (io, _input, output, _events) = TermIo::detached((80, 24));
        let session = tokio::spawn(run(sh("trap '' HUP; echo \"pid $$\"; exec sleep 30"), io));
        let mut shown = String::new();
        let pid = loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            shown.push_str(&text(&output));
            if let Some(pid) = shown.split_whitespace().skip_while(|word| *word != "pid").nth(1) {
                break pid.parse::<libc::pid_t>().unwrap();
            }
        };
        // SAFETY: plain syscalls; signal 0 only asks whether the process is there.
        let running = || unsafe { libc::kill(pid, 0) } == 0;
        // Before the abort, so that the grace period (whose timer never fires early) can't
        // start before it.
        let since = Instant::now();
        session.abort();
        while running() {
            assert!(since.elapsed() < KILL_GRACE + PATIENCE, "still running (or not reaped) long after the grace period");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(since.elapsed() >= KILL_GRACE, "killed before the grace period");
    }

    /// Without acknowledgements the reader stops after about `FLOW_HIGH` bytes, and the
    /// shell blocks instead of flooding the frontend.
    #[tokio::test(flavor = "multi_thread")]
    async fn output_waits_for_acknowledgements() {
        const TOTAL: usize = 8 << 20;
        let (io, _input, output, events) = TermIo::detached((80, 24));
        let flow = io.sink().flow();
        let session = tokio::spawn(run(sh(&format!("head -c {TOTAL} /dev/zero")), io));

        let mut received: usize = 0;
        until("reading paused", || {
            received += output.try_iter().map(|chunk| chunk.len()).sum::<usize>();
            flow.is_paused()
        })
        .await;
        // A reader that read on regardless would do so meanwhile.
        tokio::time::sleep(Duration::from_millis(200)).await;
        received += output.try_iter().map(|chunk| chunk.len()).sum::<usize>();
        assert!(received > 0 && received < (3 << 20), "{received}");
        assert!(events.try_iter().all(|e| !e.contains("closed")));

        let mut acked = 0;
        while !session.is_finished() {
            received += output.try_iter().map(|chunk| chunk.len()).sum::<usize>();
            flow.ack(received - acked);
            acked = received;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        received += output.try_iter().map(|chunk| chunk.len()).sum::<usize>();
        assert!(received >= TOTAL, "{received}");
    }
}
