//! Local shells on Unix, driven by tasks: tokio waits for the terminal to be readable or
//! writable ([`AsyncFd`]) and for the shell to exit (SIGCHLD), so an idle session takes no
//! thread and wakes nothing.

use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, ExitStatus, MasterPty};
use tokio::io::unix::AsyncFd;
use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::oneshot;

use super::{drain_output, pty_size, Shell, KILL_GRACE, READ_BUFFER};
use crate::error::Error;
use crate::fd::{read_on, set_non_blocking, write_fd};
use crate::session::{Flow, Foreground, ForegroundProbe, SessionInput, SessionSink, TermIo};

/// A running shell. Dropping it (the shell exited, or the tab was closed and the session
/// task aborted) ends the shell if it is still running.
pub(super) struct Pty {
    /// portable-pty's handle, for resizing and the foreground probe; closed last.
    master: Option<Box<dyn MasterPty + Send>>,
    /// A non-blocking handle of our own to the terminal: input is written here, and the
    /// reading task reads the output from it. Not portable-pty's writer, which sends a newline
    /// and EOF when dropped: that would submit a line typed but not entered when the tab
    /// closes (a program in the foreground gets SIGHUP only after the shell does).
    terminal: Arc<AsyncFd<OwnedFd>>,
    /// The shell, until it has been reaped.
    process: Option<Process>,
    /// Completes when the reading task reaches the end of the output.
    output_done: Option<oneshot::Receiver<()>>,
    /// Dropped to stop the reading task once the session has shut down.
    stop_reading: Option<oneshot::Sender<()>>,
    /// Where the session runs, for shutting the shell down after the session is dropped.
    runtime: tokio::runtime::Handle,
    flow: Arc<Flow>,
    foreground: Foreground,
}

impl Pty {
    pub(super) fn start(shell: Shell, io: &TermIo) -> Result<Self> {
        let (cols, rows) = io.size;
        let pair = native_pty_system().openpty(pty_size(cols, rows)).context(Error::new("pty.openFailed"))?;
        let terminal = pair
            .master
            .as_raw_fd()
            .ok_or_else(|| io::Error::from(ErrorKind::Unsupported))
            .and_then(non_blocking_copy)
            .and_then(AsyncFd::new)
            .context(Error::new("pty.openFailed"))?;
        let terminal = Arc::new(terminal);
        // Before the shell starts, so that its exit can't come before we listen for it.
        let exits = signal(SignalKind::child()).context(Error::new("pty.openFailed"))?;
        let child = pair.slave.spawn_command(shell.command).context(Error::new("pty.spawnFailed").param("shell", &shell.program))?;
        // The shell has its own handles to the terminal; ours would keep it open after the
        // shell exits.
        drop(pair.slave);

        let foreground = io.foreground();
        foreground.set(foreground_probe(&*pair.master, child.process_id()));
        let sink = io.sink();
        let flow = sink.flow();
        let (output_done_tx, output_done) = oneshot::channel();
        let (stop_reading, stop) = oneshot::channel();
        tokio::spawn({
            let terminal = terminal.clone();
            async move {
                read_output(&terminal, &sink, stop).await;
                let _ = output_done_tx.send(());
            }
        });

        Ok(Self {
            master: Some(pair.master),
            terminal,
            process: Some(Process { child, exits }),
            output_done: Some(output_done),
            stop_reading: Some(stop_reading),
            runtime: tokio::runtime::Handle::current(),
            flow,
            foreground,
        })
    }

    /// Passes input and resizes to the shell until it exits; returns its exit status, or
    /// `None` if the tab is closing.
    ///
    /// Input the shell isn't reading yet waits here, so the shell exiting and resizes are
    /// still seen (see [`TermIo::recv`]).
    pub(super) async fn bridge(&mut self, io: &mut TermIo) -> Option<Option<ExitStatus>> {
        let process = self.process.as_mut()?;
        let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
        let mut pending_bytes = 0;
        // Until writing fails: the terminal is gone, and input goes nowhere.
        let mut writing = true;
        let status = loop {
            tokio::select! {
                status = process.exit() => break status,
                ready = self.terminal.writable(), if !pending.is_empty() => {
                    let written = match ready {
                        Ok(mut ready) => match ready.try_io(|terminal| write_fd(terminal.as_raw_fd(), &pending[0])) {
                            Ok(written) => written,
                            Err(_would_block) => continue,
                        },
                        Err(e) => Err(e),
                    };
                    match written {
                        Ok(n) => {
                            pending_bytes -= n;
                            if n == pending[0].len() {
                                pending.pop_front();
                            } else {
                                pending[0].drain(..n);
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => {}
                        Err(_) => {
                            writing = false;
                            pending.clear();
                            pending_bytes = 0;
                        }
                    }
                }
                input = io.recv(pending_bytes) => match input {
                    Some(SessionInput::Data(data) | SessionInput::Raw(data)) => {
                        if writing {
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
        };
        // Reaped: its pid may soon be another process's.
        self.process = None;
        Some(status)
    }

    /// See [`drain_output`].
    pub(super) async fn drain(&mut self) {
        if let Some(done) = self.output_done.take() {
            drain_output(done, &self.flow).await;
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Before the terminal is closed: the probe uses its handle.
        self.foreground.set(None);
        // The frontend no longer acknowledges anything; what is still read goes to the log.
        self.flow.close();
        let process = self.process.take();
        let master = self.master.take();
        let stop_reading = self.stop_reading.take();
        self.runtime.spawn(async move {
            let killed = match process {
                Some(process) => process.hang_up().await,
                None => None,
            };
            drop(master);
            drop(stop_reading);
            if let Some(mut killed) = killed {
                // Not a zombie for as long as the app runs.
                killed.exit().await;
            }
        });
    }
}

/// The shell, with what tells when it exits.
struct Process {
    child: Box<dyn Child + Send + Sync>,
    /// SIGCHLD: some child of ours exited (or stopped), maybe the shell.
    exits: Signal,
}

impl Process {
    /// Waits for the shell to exit, and reaps it; `None` when its status can't be had.
    async fn exit(&mut self) -> Option<ExitStatus> {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            }
            if self.exits.recv().await.is_none() {
                return std::future::pending().await;
            }
        }
    }

    /// Ends the shell the way closing a terminal window does: SIGHUP, which shells pass on to
    /// their jobs, then SIGKILL if it is still running after [`KILL_GRACE`]. Returns the shell
    /// if it had to be killed, to be reaped.
    async fn hang_up(mut self) -> Option<Self> {
        let pid = self.child.process_id().and_then(|pid| libc::pid_t::try_from(pid).ok())?;
        // SAFETY: plain syscall; the shell has not been reaped (`Pty::bridge` drops it once it
        // is), so the pid still refers to it.
        unsafe { libc::kill(pid, libc::SIGHUP) };
        if tokio::time::timeout(KILL_GRACE, self.exit()).await.is_ok() {
            return None;
        }
        // SAFETY: as above; the wait timed out, so it has not been reaped.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        Some(self)
    }
}

/// A non-blocking handle of our own to the terminal `fd`. The flag is on the open file, which
/// portable-pty's master shares: it only resizes and asks for the foreground process group,
/// which don't block either way. The shell's side of the terminal stays blocking.
fn non_blocking_copy(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: plain syscall; `fd` is open, and the new descriptor is ours alone.
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if copy < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `copy` was just opened, and nothing else owns it.
    let copy = unsafe { OwnedFd::from_raw_fd(copy) };
    set_non_blocking(copy.as_raw_fd())?;
    Ok(copy)
}

/// Passes the output on until its end, or until `stop` (the session has shut down).
async fn read_output(terminal: &AsyncFd<OwnedFd>, sink: &SessionSink, mut stop: oneshot::Receiver<()>) {
    let flow = sink.flow();
    let mut buffer = vec![0; READ_BUFFER];
    loop {
        let read = tokio::select! {
            _ = &mut stop => return,
            read = async {
                flow.ready().await;
                read_ready(terminal, &mut buffer).await
            } => read,
        };
        match read {
            Ok(0) => return,
            Ok(n) => sink.output(buffer[..n].to_vec()),
            // EIO once the shell and its children have all closed the terminal.
            Err(_) => return,
        }
    }
}

/// Waits for output, then reads what is ready, up to the buffer's size.
async fn read_ready(terminal: &AsyncFd<OwnedFd>, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        let mut ready = terminal.readable().await?;
        if let Ok(read) = ready.try_io(|terminal| read_on(terminal.as_raw_fd(), buffer)) {
            return read;
        }
    }
}

/// The terminal's foreground process group, when it is not the shell's: a program started
/// from the shell is running (background jobs don't count).
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

#[cfg(not(target_os = "macos"))]
fn process_name(pid: libc::pid_t) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|name| name.trim_end().to_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading stops once the session has shut down, though something still has the
    /// terminal open (on Linux, a program that ignores the hangup).
    #[tokio::test(flavor = "multi_thread")]
    async fn reading_stops_once_shut_down() {
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills in two descriptors; null name, termios and size are allowed.
        let result = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, 0);
        // SAFETY: just opened, and owned by nothing else.
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        let terminal = AsyncFd::new(non_blocking_copy(master.as_raw_fd()).unwrap()).unwrap();
        let (io, _input, output, _events) = TermIo::detached((80, 24));
        let sink = io.sink();
        let (stop_reading, stop) = oneshot::channel();
        let reading = tokio::spawn(async move { read_output(&terminal, &sink, stop).await });
        write_fd(slave.as_raw_fd(), b"out").unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()), "out");
        assert!(!reading.is_finished());
        drop(stop_reading);
        tokio::time::timeout(std::time::Duration::from_secs(1), reading).await.expect("reading stopped").unwrap();
    }
}
