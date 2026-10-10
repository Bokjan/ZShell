//! Local shells in a pseudo terminal (portable-pty; ConPTY on Windows).
//!
//! portable-pty's handles block, so each session runs three threads beside its task: one
//! reads output (pausing while the frontend is behind, see [`Flow`]), one writes input
//! (writes block while the shell isn't reading), and one waits for the shell to exit. The
//! session ends when the shell exits rather than at the end of the output: background jobs
//! can keep the terminal open after that, and ConPTY only closes its output when the
//! pseudo console is closed.

mod shell;

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, ExitStatus, MasterPty, PtySize};
use tokio::sync::{mpsc, oneshot};

use crate::error::Error;
use crate::session::{Flow, Foreground, ForegroundProbe, Outcome, SessionEvent, SessionInput, SessionSink, TermIo};
pub use shell::{default_shell, local_shells_allowed, Shell};

/// How long to wait for the rest of the output once the shell has exited.
#[cfg(unix)]
const DRAIN_TIMEOUT: Duration = Duration::from_millis(200);
#[cfg(windows)]
const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a shell gets to exit after its terminal is closed before it is killed.
const KILL_GRACE: Duration = Duration::from_secs(2);
const READ_BUFFER: usize = 64 << 10;
/// Input chunks waiting for the writer thread; beyond that, sending waits (a ZMODEM upload
/// is paced by how fast the shell reads).
const INPUT_QUEUE: usize = 16;
/// Input bytes kept waiting in the session task once the writer's queue is full.
const INPUT_PENDING: usize = 64 << 10;

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

/// A running shell. Dropping it (the shell exited, or the tab was closed and the session
/// task aborted) ends the shell if it is still running.
struct Pty {
    master: Option<Box<dyn MasterPty + Send>>,
    /// Feeds the writer thread; dropping it stops the thread (see [`write_input`]).
    input: Option<mpsc::Sender<Vec<u8>>>,
    /// The exit status, once the shell exits (`None` if it could not be determined).
    exit: oneshot::Receiver<Option<ExitStatus>>,
    /// Completes when the reader thread reaches the end of the output.
    output_done: Option<oneshot::Receiver<()>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    exited: Arc<Exited>,
    flow: Arc<Flow>,
    foreground: Foreground,
}

impl Pty {
    fn start(shell: Shell, io: &TermIo) -> Result<Self> {
        let (cols, rows) = io.size;
        let pair = native_pty_system().openpty(pty_size(cols, rows)).context(Error::new("pty.openFailed"))?;
        let reader = pair.master.try_clone_reader().context(Error::new("pty.openFailed"))?;
        let writer = pair.master.take_writer().context(Error::new("pty.openFailed"))?;
        let mut child =
            pair.slave.spawn_command(shell.command).context(Error::new("pty.spawnFailed").param("shell", &shell.program))?;
        // The shell has its own handles to the terminal; ours would keep it open after the
        // shell exits (and on Windows, keep the pseudo console alive).
        drop(pair.slave);

        let sink = io.sink();
        let flow = sink.flow();
        let (output_done_tx, output_done) = oneshot::channel();
        thread::spawn(move || {
            read_output(reader, &sink);
            let _ = output_done_tx.send(());
        });

        let exited = Arc::new(Exited::default());
        let (input, inputs) = mpsc::channel(INPUT_QUEUE);
        let writer_exited = exited.clone();
        thread::spawn(move || write_input(writer, inputs, &writer_exited));

        let killer = child.clone_killer();
        let pid = child.process_id();
        let foreground = io.foreground();
        foreground.set(foreground_probe(&*pair.master, pid));
        let (exit_tx, exit) = oneshot::channel();
        let exited_flag = exited.clone();
        thread::spawn(move || {
            let status = child.wait().ok();
            exited_flag.set();
            let _ = exit_tx.send(status);
        });

        Ok(Self {
            master: Some(pair.master),
            input: Some(input),
            exit,
            output_done: Some(output_done),
            killer,
            pid,
            exited,
            flow,
            foreground,
        })
    }

    /// Passes input and resizes to the shell until it exits; returns its exit status, or
    /// `None` if the tab is closing.
    ///
    /// Input the writer thread has no room for yet (the shell isn't reading) waits here, so
    /// the shell exiting and resizes are still seen. Past [`INPUT_PENDING`] bytes, a ZMODEM
    /// upload's data is left to wait in its own bounded queue.
    async fn bridge(&mut self, io: &mut TermIo) -> Option<Option<ExitStatus>> {
        let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
        let mut pending_bytes = 0;
        loop {
            let writer = self.input.clone();
            let backed_up = pending_bytes >= INPUT_PENDING;
            tokio::select! {
                status = &mut self.exit => return Some(status.ok().flatten()),
                permit = async { writer?.reserve_owned().await.ok() }, if !pending.is_empty() => {
                    let data = pending.pop_front().unwrap_or_default();
                    pending_bytes -= data.len();
                    if let Some(permit) = permit {
                        permit.send(data);
                    }
                }
                input = async { if backed_up { io.recv_typed().await } else { io.recv().await } } => match input {
                    Some(SessionInput::Data(data)) => {
                        if self.input.is_some() {
                            pending_bytes += data.len();
                            pending.push_back(data);
                        }
                    }
                    Some(SessionInput::Resize { cols, rows }) => {
                        if let Some(master) = &self.master {
                            let _ = master.resize(pty_size(cols, rows));
                        }
                    }
                    Some(SessionInput::Break) => {}
                    None => return None,
                },
            }
        }
    }

    /// After the shell has exited, gives the reader a moment to pass on the rest of the
    /// output (a background job may keep the terminal open indefinitely). Time the reader
    /// spends waiting for the frontend to catch up (`cat big.log; exit`) doesn't count: the
    /// rest of the output would otherwise follow the message that the shell exited.
    async fn drain(&mut self) {
        // ConPTY keeps its output pipe open until the pseudo console is closed, which also
        // flushes what it has not sent yet. Closing blocks until that output is read.
        #[cfg(windows)]
        if let Some(master) = self.master.take() {
            thread::spawn(move || drop(master));
        }
        let Some(mut done) = self.output_done.take() else { return };
        const STEP: Duration = Duration::from_millis(20);
        let mut left = DRAIN_TIMEOUT;
        while !left.is_zero() {
            if self.flow.is_paused() {
                tokio::select! {
                    _ = &mut done => return,
                    () = self.flow.ready() => continue,
                }
            }
            let step = left.min(STEP);
            tokio::select! {
                _ = &mut done => return,
                () = tokio::time::sleep(step) => left -= step,
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Before the terminal is closed: the probe uses its handle.
        self.foreground.set(None);
        // The reader must keep reading: closing a pseudo console blocks until its output is
        // read, and the frontend no longer acknowledges anything.
        self.flow.close();
        self.input.take();
        let master = self.master.take();
        let killer = self.killer.clone_killer();
        let exited = self.exited.clone();
        let pid = self.pid;
        // Waiting for the shell to exit (and closing a pseudo console) blocks.
        thread::spawn(move || shut_down(master, killer, &exited, pid));
    }
}

/// Ends the shell if it is still running, the way closing a terminal window does.
#[cfg(unix)]
fn shut_down(master: Option<Box<dyn MasterPty + Send>>, mut killer: Box<dyn ChildKiller + Send + Sync>, exited: &Exited, pid: Option<u32>) {
    if !exited.get() {
        // portable-pty sends SIGHUP, which shells pass on to their jobs.
        let _ = killer.kill();
        if !exited.wait(KILL_GRACE) {
            if let Some(pid) = pid.and_then(|pid| libc::pid_t::try_from(pid).ok()) {
                // SAFETY: plain syscall; the shell has not been reaped (`exited` is unset), so
                // the pid still refers to it.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }
    drop(master);
}

/// Ends the shell if it is still running, the way closing a terminal window does.
#[cfg(windows)]
fn shut_down(master: Option<Box<dyn MasterPty + Send>>, mut killer: Box<dyn ChildKiller + Send + Sync>, exited: &Exited, _pid: Option<u32>) {
    // Closing the pseudo console sends CTRL_CLOSE_EVENT to the processes attached to it,
    // so they can exit cleanly.
    drop(master);
    if !exited.wait(KILL_GRACE) {
        let _ = killer.kill();
    }
}

/// The terminal's foreground process group, when it is not the shell's: a program started
/// from the shell is running (background jobs don't count).
#[cfg(unix)]
fn foreground_probe(master: &dyn MasterPty, shell: Option<u32>) -> Option<ForegroundProbe> {
    let fd = master.as_raw_fd()?;
    let shell = libc::pid_t::try_from(shell?).ok()?;
    Some(Box::new(move || {
        // SAFETY: plain syscall on the terminal's descriptor, which stays open while the probe
        // is installed (`Pty::drop` removes it first).
        let group = unsafe { libc::tcgetpgrp(fd) };
        (group > 0 && group != shell).then(|| process_name(group))
    }))
}

#[cfg(target_os = "macos")]
fn process_name(pid: libc::pid_t) -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is valid for writes of its full length.
    let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    String::from_utf8_lossy(&buffer[..usize::try_from(len).unwrap_or(0)]).into_owned()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_name(pid: libc::pid_t) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|name| name.trim_end().to_owned()).unwrap_or_default()
}

/// A child process of the shell. Windows has no foreground process groups, and ConPTY's
/// own conhost is not the shell's child.
#[cfg(windows)]
fn foreground_probe(_master: &dyn MasterPty, shell: Option<u32>) -> Option<ForegroundProbe> {
    let shell = shell?;
    Some(Box::new(move || child_process_name(shell)))
}

#[cfg(windows)]
fn child_process_name(parent: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    // SAFETY: plain Win32 calls; `entry` is initialized with its size as the API requires, and
    // the snapshot handle is closed once.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            if entry.th32ParentProcessID == parent {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                found = Some(shell::display_name(&String::from_utf16_lossy(&entry.szExeFile[..len])));
                break;
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

/// Whether the shell has exited, which the threads shutting the terminal down wait for.
#[derive(Default)]
struct Exited {
    exited: Mutex<bool>,
    changed: Condvar,
}

impl Exited {
    fn set(&self) {
        *self.exited.lock().unwrap() = true;
        self.changed.notify_all();
    }

    #[cfg(unix)]
    fn get(&self) -> bool {
        *self.exited.lock().unwrap()
    }

    /// Waits until the shell exits or `timeout` passes; returns whether it exited.
    fn wait(&self, timeout: Duration) -> bool {
        let (exited, _) = self.changed.wait_timeout_while(self.exited.lock().unwrap(), timeout, |exited| !*exited).unwrap();
        *exited
    }
}

fn read_output(mut reader: Box<dyn Read + Send>, sink: &SessionSink) {
    let flow = sink.flow();
    let mut buffer = vec![0; READ_BUFFER];
    loop {
        flow.wait_ready();
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => sink.output(buffer[..n].to_vec()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            // EIO on macOS once the shell and its children have all closed the terminal.
            Err(_) => break,
        }
    }
}

/// Writes input until the session closes. The writer is then kept until the shell has
/// exited: portable-pty's Unix writer sends a newline and EOF when dropped, which would
/// submit a line typed but not entered (a program in the foreground gets SIGHUP only after
/// the shell does). `shut_down` kills the shell within `KILL_GRACE`.
fn write_input(mut writer: Box<dyn Write + Send>, mut inputs: mpsc::Receiver<Vec<u8>>, exited: &Exited) {
    while let Some(data) = inputs.blocking_recv() {
        if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
            break;
        }
    }
    if !exited.wait(KILL_GRACE * 2) {
        // The shell outlived SIGKILL (no pid to kill): leaking the handle is the lesser harm.
        std::mem::forget(writer);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::mpsc::Receiver;

    use portable_pty::CommandBuilder;

    use super::*;

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
        // Catching up only well after the drain timeout.
        tokio::time::sleep(DRAIN_TIMEOUT * 3).await;
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
        let session = tokio::spawn(run(sh("stty raw -echo; sleep 1; stty size; exit 4"), io));
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
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(foreground.get(), None);
        input.send(SessionInput::Data(b"sleep 5\r".to_vec())).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(foreground.get().as_deref(), Some("sleep"));
        input.send(SessionInput::Data(b"\x03".to_vec())).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(foreground.get(), None);
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

    /// Without acknowledgements the reader stops after about `FLOW_HIGH` bytes, and the
    /// shell blocks instead of flooding the frontend.
    #[tokio::test(flavor = "multi_thread")]
    async fn output_waits_for_acknowledgements() {
        const TOTAL: usize = 8 << 20;
        let (io, _input, output, events) = TermIo::detached((80, 24));
        let flow = io.sink().flow();
        let session = tokio::spawn(run(sh(&format!("head -c {TOTAL} /dev/zero")), io));

        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut received: usize = output.try_iter().map(|chunk| chunk.len()).sum();
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
