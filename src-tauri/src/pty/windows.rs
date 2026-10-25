//! Local shells on Windows: ConPTY's pipes only block, so each session runs three threads
//! beside its task (see the module comment of `pty`).

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, ChildKiller, ExitStatus, MasterPty};
use tokio::sync::{mpsc, oneshot};

use super::{drain_output, pty_size, shell, Shell, KILL_GRACE, READ_BUFFER};
use crate::error::Error;
use crate::session::{Flow, Foreground, ForegroundProbe, Pending, PendingInput, SessionInput, SessionSink, TermIo};

/// Input chunks waiting for the writer thread; beyond that, sending waits (a ZMODEM upload
/// is paced by how fast the shell reads).
const INPUT_QUEUE: usize = 16;

/// A running shell. Dropping it (the shell exited, or the tab was closed and the session
/// task aborted) ends the shell if it is still running.
pub(super) struct Pty {
    master: Option<Box<dyn MasterPty + Send>>,
    /// Feeds the writer thread; dropping it stops the thread (see [`write_input`]).
    input: Option<mpsc::Sender<Vec<u8>>>,
    /// The exit status, once the shell exits (`None` if it could not be determined).
    exit: oneshot::Receiver<Option<ExitStatus>>,
    /// Completes when the reader thread reaches the end of the output.
    output_done: Option<oneshot::Receiver<()>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    exited: Arc<Exited>,
    /// Set once the session has shut down, to stop the reader.
    shut: Arc<AtomicBool>,
    flow: Arc<Flow>,
    foreground: Foreground,
}

impl Pty {
    pub(super) fn start(shell: Shell, io: &TermIo) -> Result<Self> {
        let (cols, rows) = io.size;
        let pair = native_pty_system().openpty(pty_size(cols, rows)).context(Error::new("pty.openFailed"))?;
        let reader = pair.master.try_clone_reader().context(Error::new("pty.openFailed"))?;
        let writer = pair.master.take_writer().context(Error::new("pty.openFailed"))?;
        let mut child = pair.slave.spawn_command(shell.command).context(Error::new("pty.spawnFailed").param("shell", &shell.program))?;
        // The shell has its own handles to the terminal; ours would keep it open after the
        // shell exits (and on Windows, keep the pseudo console alive).
        drop(pair.slave);

        let sink = io.sink();
        let flow = sink.flow();
        let (output_done_tx, output_done) = oneshot::channel();
        let shut = Arc::new(AtomicBool::new(false));
        let reader_shut = shut.clone();
        thread::spawn(move || {
            read_output(reader, &sink, &reader_shut);
            let _ = output_done_tx.send(());
        });

        let exited = Arc::new(Exited::default());
        let (input, inputs) = mpsc::channel(INPUT_QUEUE);
        let writer_exited = exited.clone();
        thread::spawn(move || write_input(writer, inputs, &writer_exited));

        let killer = child.clone_killer();
        let foreground = io.foreground();
        foreground.set(foreground_probe(&*pair.master, child.process_id()));
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
            exited,
            shut,
            flow,
            foreground,
        })
    }

    /// Passes input and resizes to the shell until it exits; returns its exit status, or
    /// `None` if the tab is closing.
    ///
    /// Input the writer thread has no room for yet (the shell isn't reading) waits here, so
    /// the shell exiting and resizes are still seen (see [`TermIo::recv`]).
    pub(super) async fn bridge(&mut self, io: &mut TermIo) -> Option<Option<ExitStatus>> {
        let mut pending = PendingInput::default();
        loop {
            let writer = self.input.clone();
            tokio::select! {
                status = &mut self.exit => return Some(status.ok().flatten()),
                permit = async { writer?.reserve_owned().await.ok() }, if !pending.is_empty() => {
                    if let (Some(Pending::Data(data)), Some(permit)) = (pending.pop(), permit) {
                        permit.send(data);
                    }
                }
                input = io.recv(pending.bytes()) => match input {
                    Some(SessionInput::Data(data) | SessionInput::Raw(data)) => {
                        if self.input.is_some() {
                            pending.push(data);
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
    pub(super) async fn drain(&mut self) {
        // ConPTY keeps its output pipe open until the pseudo console is closed, which also
        // flushes what it has not sent yet. Closing blocks until that output is read.
        if let Some(master) = self.master.take() {
            thread::spawn(move || drop(master));
        }
        if let Some(done) = self.output_done.take() {
            drain_output(done, &self.flow).await;
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
        let shut = self.shut.clone();
        // Waiting for the shell to exit (and closing a pseudo console) blocks.
        thread::spawn(move || {
            shut_down(master, killer, &exited);
            shut.store(true, Ordering::Relaxed);
        });
    }
}

/// Ends the shell if it is still running, the way closing a terminal window does.
fn shut_down(master: Option<Box<dyn MasterPty + Send>>, mut killer: Box<dyn ChildKiller + Send + Sync>, exited: &Exited) {
    // Closing the pseudo console sends CTRL_CLOSE_EVENT to the processes attached to it,
    // so they can exit cleanly.
    drop(master);
    if !exited.wait(KILL_GRACE) {
        let _ = killer.kill();
    }
}

/// A child process of the shell. Windows has no foreground process groups, and ConPTY's
/// own conhost is not the shell's child.
fn foreground_probe(_master: &dyn MasterPty, shell: Option<u32>) -> Option<ForegroundProbe> {
    let shell = shell?;
    Some(Box::new(move || child_process_name(shell)))
}

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
        // Windows reuses process ids and doesn't give orphans a new parent: a process whose
        // parent was an earlier holder of the shell's id started before the shell.
        let shell_created = created(parent);
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let is_child = entry.th32ParentProcessID == parent
                && !matches!((shell_created, created(entry.th32ProcessID)), (Some(shell), Some(child)) if child < shell);
            if is_child {
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

/// When process `pid` started (in 100 ns units), if that can be asked.
fn created(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: plain Win32 calls with valid out-pointers; the process handle is closed once.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        CloseHandle(process);
        ok.then(|| (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
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

    /// Waits until the shell exits or `timeout` passes; returns whether it exited.
    fn wait(&self, timeout: Duration) -> bool {
        let (exited, _) = self.changed.wait_timeout_while(self.exited.lock().unwrap(), timeout, |exited| !*exited).unwrap();
        *exited
    }
}

/// Reads until the end of the output, or until the session has shut down (`shut`).
fn read_output(mut reader: Box<dyn Read + Send>, sink: &SessionSink, shut: &AtomicBool) {
    let flow = sink.flow();
    let mut buffer = vec![0; READ_BUFFER];
    loop {
        flow.wait_ready();
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => sink.output(buffer[..n].to_vec()),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                if shut.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Writes input until the session closes. The writer is then kept until the shell has
/// exited: `shut_down` ends it within `KILL_GRACE`.
fn write_input(mut writer: Box<dyn Write + Send>, mut inputs: mpsc::Receiver<Vec<u8>>, exited: &Exited) {
    while let Some(data) = inputs.blocking_recv() {
        if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
            break;
        }
    }
    if !exited.wait(KILL_GRACE * 2) {
        // The shell outlived being killed: leaking the handle is the lesser harm.
        std::mem::forget(writer);
    }
}
