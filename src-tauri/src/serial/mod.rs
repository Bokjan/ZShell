//! Serial sessions (serialport): the terminal talks to a device over a serial line.
//!
//! On Unix the session's task drives the line (`unix.rs`). serialport's handles block on
//! Windows, so there each session reads and writes on threads of its own ([`Line`]), whose
//! reads wake up periodically to notice that the session has closed; so does a Unix device
//! that tokio can't watch (one whose driver doesn't support kqueue or epoll). A session ends
//! when the device goes away (a USB adapter unplugged); the frontend then reconnects as for
//! a lost connection, which succeeds once the device is back.

#[cfg(unix)]
mod unix;

use std::io::{ErrorKind, Read};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serialport::{SerialPort, SerialPortType};
use tokio::sync::mpsc;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::model::{FlowControl, Parity, SerialOptions};
use crate::session::{Outcome, Pending, PendingInput, SessionEvent, SessionInput, SessionSink, TermIo, Tone};

/// How long a read waits for data before checking whether the session has closed. On
/// Windows, reads and writes on the port's handle take turns, so a waiting read also delays
/// typed input: keep it short there.
#[cfg(not(windows))]
const READ_TIMEOUT: Duration = Duration::from_millis(200);
#[cfg(windows)]
const READ_TIMEOUT: Duration = Duration::from_millis(10);
/// How long a break holds the line.
const BREAK_DURATION: Duration = Duration::from_millis(250);
const READ_BUFFER: usize = 4096;
/// Input chunks waiting for the writer thread; beyond that, sending waits (a ZMODEM upload
/// is paced by the line).
const INPUT_QUEUE: usize = 16;
/// How long opening a device waits for a closed session of ours to let go of it.
const RELEASE_WAIT: Duration = Duration::from_secs(1);

/// The devices our sessions still hold, once per session. A closed session's threads let go
/// of the port within `READ_TIMEOUT` (a session in its task once the task is dropped, which
/// can come a moment after it is aborted), and only one handle can have a port open
/// (exclusive on macOS, always on Windows), so reconnecting, which opens the device again at
/// once, would otherwise find it busy.
static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A session's hold on its device, shared by its reader and writer threads (or kept with the
/// port by its task, see `unix::Held`).
struct Hold(String);

impl Hold {
    fn new(device: &str) -> Arc<Self> {
        HELD.lock().unwrap().push(device.to_owned());
        Arc::new(Self(device.to_owned()))
    }

    fn is_held(device: &str) -> bool {
        HELD.lock().unwrap().iter().any(|held| held == device)
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap();
        if let Some(i) = held.iter().position(|device| *device == self.0) {
            held.swap_remove(i);
        }
    }
}

/// A serial port found on this computer.
#[derive(Serialize, TS)]
#[ts(rename = "SerialPortInfo")]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    /// What to open: `/dev/cu.usbserial-1410`, `COM3`.
    pub name: String,
    /// The USB adapter's product or manufacturer name, if known.
    pub description: Option<String>,
}

/// The serial ports on this computer, by name. On macOS only the call-out devices
/// (`/dev/cu.*`): their `tty.*` twins wait for a carrier signal when opened.
pub fn ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|port| !cfg!(target_os = "macos") || port.port_name.starts_with("/dev/cu."))
        .map(|port| PortInfo {
            description: match port.port_type {
                SerialPortType::UsbPort(usb) => usb.product.or(usb.manufacturer),
                _ => None,
            },
            name: port.port_name,
        })
        .collect();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    ports.dedup_by(|a, b| a.name == b.name);
    ports
}

/// Session backend: opens the device and bridges it to the terminal until the device goes
/// away or the tab closes.
pub async fn run(options: SerialOptions, io: TermIo) {
    run_with(options, io, false).await;
}

/// [`run`], on threads even where the session's task could drive the line if `on_threads`.
async fn run_with(options: SerialOptions, mut io: TermIo, on_threads: bool) {
    let since = Instant::now();
    while Hold::is_held(&options.device) && since.elapsed() < RELEASE_WAIT {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Off the async workers: opening a Bluetooth serial port can take seconds.
    let opening = options.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || open(&opening)).await.unwrap_or_else(|e| Err(e.into()));
    let outcome = match opened {
        Ok(port) => {
            // Before reading starts, so that what the device already sent comes after.
            let opened = t!("terminal.serialOpened", device = &options.device, settings = options.summary());
            io.notice(Tone::Dim, &opened);
            if on_threads {
                bridge_on_threads(port, &options.device, &mut io).await
            } else {
                bridge(port, &options.device, &mut io).await
            }
        }
        Err(e) => Outcome::Failed(e),
    };
    io.finish(outcome);
}

#[cfg(unix)]
async fn bridge(port: NativePort, device: &str, io: &mut TermIo) -> Outcome {
    // SAFETY: the `AsyncFd` owns the port, whose descriptor stays open and the same until the
    // port is dropped; the session only borrows it (`get_ref`).
    match unsafe { tokio::io::unix::AsyncFd::register(port) } {
        Ok(port) => {
            io.event(SessionEvent::Connected);
            unix::bridge(port, device, io).await
        }
        Err(e) => bridge_on_threads(e.into_parts().0, device, io).await,
    }
}

#[cfg(windows)]
async fn bridge(port: NativePort, device: &str, io: &mut TermIo) -> Outcome {
    bridge_on_threads(port, device, io).await
}

async fn bridge_on_threads(port: NativePort, device: &str, io: &mut TermIo) -> Outcome {
    match Line::start(port, device, io) {
        Ok(line) => {
            io.event(SessionEvent::Connected);
            line.bridge(device, io).await
        }
        Err(e) => Outcome::Failed(Error::new("serial.openFailed").param("device", device).detail(e)),
    }
}

#[cfg(unix)]
type NativePort = serialport::TTYPort;
#[cfg(windows)]
type NativePort = serialport::COMPort;

/// Opens the device with the session's line settings.
fn open(options: &SerialOptions) -> Result<NativePort> {
    let open_failed = |e: serialport::Error| Error::new("serial.openFailed").param("device", &options.device).detail(e);
    let baud_rate = if is_pseudo_terminal(&options.device) { 0 } else { options.baud_rate };
    let builder = serialport::new(&options.device, baud_rate)
        .data_bits(match options.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        })
        .parity(match options.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        })
        .stop_bits(match options.stop_bits {
            2 => serialport::StopBits::Two,
            _ => serialport::StopBits::One,
        })
        .flow_control(match options.flow_control {
            FlowControl::None => serialport::FlowControl::None,
            FlowControl::Software => serialport::FlowControl::Software,
            FlowControl::Hardware => serialport::FlowControl::Hardware,
        })
        .timeout(READ_TIMEOUT);
    let port = builder.open_native().map_err(open_failed)?;
    // Reads and writes on Unix never block: the session's task waits for the device instead
    // (`unix.rs`). On threads (see [`Line`]), serialport waits (up to the port's timeout) for
    // room to write, then writes blocking: more than there is room for would wait as long as
    // flow control holds the line, and a closed tab's writer couldn't let go of the device.
    // Without blocking, a write takes what fits and `write_data` looks again; reads too wait
    // for data first. The setting is the open file's, which the writer thread's handle shares.
    #[cfg(unix)]
    crate::fd::set_non_blocking(std::os::fd::AsRawFd::as_raw_fd(&port))
        .map_err(|e| Error::new("serial.openFailed").param("device", &options.device).detail(e))?;
    Ok(port)
}

/// serialport duplicates the handle as inheritable, and `std::process::Command` lets child
/// processes inherit every inheritable handle: a proxy command or editor started while the
/// tab is open would keep the device busy after it closes.
#[cfg(windows)]
fn not_inherited(port: &serialport::COMPort) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    // SAFETY: the handle is the port's, open as long as `port` is.
    unsafe { SetHandleInformation(port.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) };
}

/// An open serial port and its reader and writer threads (on Windows, or for a device tokio
/// can't watch), which stop when it is dropped: the writer once its input closes, the reader
/// (whose reads time out) once nothing receives its failures.
struct Line {
    input: mpsc::Sender<Pending>,
    /// Why the reader or writer stopped, if the device failed.
    failed: mpsc::UnboundedReceiver<String>,
}

impl Line {
    /// Fails if the port can't be opened a second time, for the writer.
    fn start(port: NativePort, device: &str, io: &TermIo) -> serialport::Result<Self> {
        let writer = port.try_clone_native()?;
        #[cfg(windows)]
        not_inherited(&writer);
        let (port, writer): (Box<dyn SerialPort>, Box<dyn SerialPort>) = (Box::new(port), Box::new(writer));
        let hold = Hold::new(device);
        let (failed_tx, failed) = mpsc::unbounded_channel();
        let sink = io.sink();
        let (reader_failed, reader_hold) = (failed_tx.clone(), hold.clone());
        thread::spawn(move || {
            read_output(port, &sink, &reader_failed);
            drop(reader_hold);
        });
        let (input, inputs) = mpsc::channel(INPUT_QUEUE);
        thread::spawn(move || {
            write_input(writer, inputs, &failed_tx);
            drop(hold);
        });
        Ok(Self { input, failed })
    }

    /// Passes input to the writer thread until the device fails or the tab closes. Input the
    /// writer has no room for yet (the device's flow control holds it back) waits here, so
    /// keystrokes are still read: Ctrl+C cancels a ZMODEM upload that the line stalls (see
    /// [`TermIo::recv`]).
    async fn bridge(mut self, device: &str, io: &mut TermIo) -> Outcome {
        let mut pending = PendingInput::default();
        loop {
            tokio::select! {
                reason = self.failed.recv() => {
                    let error = Error::new("serial.disconnected").param("device", device);
                    return Outcome::Lost(match reason {
                        Some(reason) => error.detail(reason),
                        None => error,
                    });
                }
                // An error means the writer stopped; the reason arrives on `failed`.
                Ok(permit) = self.input.reserve(), if !pending.is_empty() => {
                    permit.send(pending.pop().expect("not empty"));
                }
                input = io.recv(pending.bytes()) => match input {
                    Some(SessionInput::Data(data) | SessionInput::Raw(data)) => pending.push(data),
                    Some(SessionInput::Break) => pending.push_break(),
                    Some(SessionInput::Resize { .. }) => {}
                    // The tab is closing.
                    None => return Outcome::Exited(None),
                },
            }
        }
    }
}

/// Whether `device` is a pseudo terminal (macOS `/dev/ttysNNN`, or a link to one), the
/// "serial port" of a virtual machine (QEMU's `-serial pty`) or of socat. These have no line
/// speed: macOS refuses to set one (`IOSSIOSPEED`), and serialport skips it for a baud rate
/// of 0.
fn is_pseudo_terminal(device: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let path = std::fs::canonicalize(device).unwrap_or_else(|_| device.into());
    let name = path.to_string_lossy();
    name.strip_prefix("/dev/ttys").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Reads until the device fails or the line is dropped (`failed` closes).
fn read_output(mut port: Box<dyn SerialPort>, sink: &SessionSink, failed: &mpsc::UnboundedSender<String>) {
    let mut buffer = vec![0; READ_BUFFER];
    let flow = sink.flow();
    while !failed.is_closed() {
        // Not read while the terminal is behind: the driver's buffer, then flow control (if
        // the line has any), holds the device back meanwhile.
        flow.wait_ready();
        match port.read(&mut buffer) {
            Ok(0) => {
                let _ = failed.send(std::io::Error::from(ErrorKind::UnexpectedEof).to_string());
                return;
            }
            Ok(n) => {
                let n = read_ready(&mut *port, &mut buffer, n);
                sink.output(buffer[..n].to_vec());
            }
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
            Err(e) => {
                let _ = failed.send(e.to_string());
                return;
            }
        }
    }
}

/// Reads on into `buffer` past its first `read` bytes while the port has more waiting: an
/// adapter hands over a few dozen bytes at a time, and each read becomes a message to the web
/// view. Returns how much `buffer` holds; a failure comes again on the next read.
fn read_ready(port: &mut dyn SerialPort, buffer: &mut [u8], mut read: usize) -> usize {
    while read < buffer.len() && port.bytes_to_read().is_ok_and(|waiting| waiting > 0) {
        match port.read(&mut buffer[read..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => read += n,
        }
    }
    read
}

fn write_input(mut port: Box<dyn SerialPort>, mut inputs: mpsc::Receiver<Pending>, failed: &mpsc::UnboundedSender<String>) {
    while let Some(write) = inputs.blocking_recv() {
        let result = match write {
            Pending::Data(data) => write_data(&mut *port, &data, failed),
            // Not every adapter (or pseudo terminal) can send a break; that is no reason to
            // end the session.
            Pending::Break => {
                if port.set_break().is_ok() {
                    thread::sleep(BREAK_DURATION);
                    let _ = port.clear_break();
                }
                Ok(())
            }
        };
        if let Err(e) = result {
            let _ = failed.send(e.to_string());
            return;
        }
    }
}

/// Writes all of `data`, waiting as long as flow control holds the line, unless the line is
/// dropped (`failed` closes): each write returns within the port's timeout (see
/// `open` on Unix).
fn write_data(port: &mut dyn SerialPort, mut data: &[u8], failed: &mpsc::UnboundedSender<String>) -> std::io::Result<()> {
    while !data.is_empty() && !failed.is_closed() {
        match port.write(data) {
            // On Windows a write that times out (the port's timeout is also the write
            // timeout) succeeds having written nothing.
            Ok(0) => {}
            Ok(n) => data = &data[n..],
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::CStr;
    use std::fs::File;
    use std::io::Write as _;
    use std::os::fd::FromRawFd;
    use std::sync::mpsc::Receiver;

    use super::*;

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

    /// Waits until the session reports that it is connected (it has opened the device),
    /// keeping the events it reports in `seen`; fails if it doesn't for a while.
    async fn until_connected(events: &Receiver<String>, seen: &mut Vec<String>) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            seen.extend(events.try_iter());
            if seen.iter().any(|event| event.contains(r#""type":"connected""#)) {
                return;
            }
            assert!(Instant::now() < deadline, "never connected: {seen:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A pseudo terminal standing in for a device: the session opens its slave side, the
    /// test plays the device on the master side.
    fn fake_device() -> (File, String) {
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills in two file descriptors; null name, termios and window size
        // are allowed.
        let result = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, 0);
        // SAFETY: `slave` is an open terminal; ttyname returns a C string or null.
        let name = unsafe { CStr::from_ptr(libc::ttyname(slave)) }.to_string_lossy().into_owned();
        // SAFETY: both descriptors are open and owned here; the slave is reopened by name.
        unsafe { libc::close(slave) };
        // SAFETY: `master` is open and owned here, by the file from now on.
        let master = unsafe { File::from_raw_fd(master) };
        (master, name)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn talks_to_a_device() {
        // In the session's task, then on threads (for a device tokio can't watch).
        for on_threads in [false, true] {
            let (mut device, path) = fake_device();
            let (io, input, output, events) = TermIo::detached((80, 24));
            let options = SerialOptions { device: path.clone(), ..SerialOptions::default() };
            let session = tokio::spawn(run_with(options, io, on_threads));

            // Until the session opens the device, reading it fails on Linux.
            let mut events_seen = Vec::new();
            until_connected(&events, &mut events_seen).await;
            input.send(SessionInput::Data(b"help\r".to_vec())).unwrap();
            let mut buffer = [0; 64];
            let n = device.read(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], b"help\r");
            device.write_all(b"U-Boot> ").unwrap();
            // Read before the device goes away, which discards what is still unread.
            let mut shown = Vec::new();
            until("the device's prompt shown", || {
                shown.extend(output.try_iter().flatten());
                String::from_utf8_lossy(&shown).contains("U-Boot> ")
            })
            .await;

            // The device goes away.
            drop(device);
            tokio::time::timeout(Duration::from_secs(5), session).await.unwrap().unwrap();
            shown.extend(output.try_iter().flatten());
            let text = String::from_utf8_lossy(&shown).into_owned();
            assert!(text.contains("115200 8N1") && text.contains("U-Boot> "), "{text:?}");
            events_seen.extend(events.try_iter());
            let events = events_seen;
            assert!(events[0].contains(r#""type":"connected""#), "{events:?}");
            assert!(events.last().unwrap().contains(r#""reason":"lost""#), "{events:?}");
        }
    }

    /// Reconnecting closes the session and opens the device again at once, while the old
    /// session's threads may still hold the port, which only one can open at a time.
    #[tokio::test(flavor = "multi_thread")]
    async fn reopens_a_device_just_closed() {
        // In the session's task, then on threads (for a device tokio can't watch).
        for on_threads in [false, true] {
            let (_device, path) = fake_device();
            let options = SerialOptions { device: path, ..SerialOptions::default() };
            let (io, _input, _output, events) = TermIo::detached((80, 24));
            let first = tokio::spawn(run_with(options.clone(), io, on_threads));
            until_connected(&events, &mut Vec::new()).await;
            first.abort();
            let (io, _input, _output, events) = TermIo::detached((80, 24));
            let second = tokio::spawn(run_with(options, io, on_threads));
            // Connected, or failed to open the device.
            let mut first_event = None;
            until("the second session's first event", || {
                first_event = events.try_recv().ok();
                first_event.is_some()
            })
            .await;
            assert!(first_event.as_ref().is_some_and(|e| e.contains(r#""type":"connected""#)), "{first_event:?}");
            second.abort();
        }
    }

    /// A tab closed while a write waits for a device that takes no more (flow control holding
    /// the line) lets go of the device, so that it can be opened again.
    #[tokio::test(flavor = "multi_thread")]
    async fn lets_go_of_a_device_holding_up_a_write() {
        // In the session's task, then on threads (for a device tokio can't watch).
        for on_threads in [false, true] {
            // Never read: the line fills up.
            let (_device, path) = fake_device();
            let (io, input, _output, events) = TermIo::detached((80, 24));
            let session = tokio::spawn(run_with(SerialOptions { device: path.clone(), ..SerialOptions::default() }, io, on_threads));
            until_connected(&events, &mut Vec::new()).await;
            input.send(SessionInput::Data(vec![b'x'; 1 << 20])).unwrap();
            // Time for the write to start waiting; on a busy machine the test may pass without.
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(input);
            tokio::time::timeout(Duration::from_secs(5), session).await.unwrap().unwrap();
            // What is tested is that the device is let go of, not how soon, which depends on the
            // machine's load.
            until("the device let go of", || !Hold::is_held(&path)).await;
        }
    }

    /// Input keeps being read while the device takes no more: more writes than the writer's
    /// queue holds still let the tab close (or a keystroke cancel a ZMODEM upload).
    #[tokio::test(flavor = "multi_thread")]
    async fn reads_input_while_the_device_holds_up_writes() {
        // In the session's task, then on threads (for a device tokio can't watch).
        for on_threads in [false, true] {
            let (_device, path) = fake_device();
            let (io, input, _output, _events) = TermIo::detached((80, 24));
            let session = tokio::spawn(run_with(SerialOptions { device: path, ..SerialOptions::default() }, io, on_threads));
            tokio::time::sleep(Duration::from_millis(300)).await;
            for _ in 0..INPUT_QUEUE * 4 {
                input.send(SessionInput::Data(vec![b'x'; 64 << 10])).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(input);
            tokio::time::timeout(Duration::from_secs(5), session).await.unwrap().unwrap();
        }
    }

    /// The other tests run in the session's task only if tokio can watch the fake device.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_task_drives_a_device_tokio_can_watch() {
        let (_device, path) = fake_device();
        let port = open(&SerialOptions { device: path, ..SerialOptions::default() }).unwrap();
        // SAFETY: as in `bridge`.
        assert!(unsafe { tokio::io::unix::AsyncFd::register(port) }.is_ok());
    }

    /// What is typed after a break goes out after it, and the line keeps working.
    #[tokio::test(flavor = "multi_thread")]
    async fn sends_input_after_a_break() {
        for on_threads in [false, true] {
            let (mut device, path) = fake_device();
            let (io, input, _output, events) = TermIo::detached((80, 24));
            let session = tokio::spawn(run_with(SerialOptions { device: path, ..SerialOptions::default() }, io, on_threads));
            // Until the session opens the device, reading it fails on Linux.
            until_connected(&events, &mut Vec::new()).await;
            input.send(SessionInput::Break).unwrap();
            input.send(SessionInput::Data(b"after".to_vec())).unwrap();
            let mut buffer = [0; 64];
            let n = device.read(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], b"after");
            session.abort();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_a_missing_device() {
        let (io, _input, _output, events) = TermIo::detached((80, 24));
        run(SerialOptions { device: "/dev/cu.zshell-missing".into(), ..SerialOptions::default() }, io).await;
        let events: Vec<String> = events.try_iter().collect();
        assert!(events[0].contains(r#""reason":"failed""#) && events[0].contains("serial.openFailed"), "{events:?}");
    }
}
