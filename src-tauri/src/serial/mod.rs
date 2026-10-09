//! Serial sessions (serialport): the terminal talks to a device over a serial line.
//!
//! serialport's handles block, so each session reads and writes on threads of its own, like
//! local terminals. Reads wake up periodically to notice that the session has closed. A
//! session ends when the device goes away (a USB adapter unplugged); the frontend then
//! reconnects as for a lost connection, which succeeds once the device is back.

use std::io::{ErrorKind, Read};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;
use serialport::{SerialPort, SerialPortType};
use tokio::sync::mpsc;

use crate::config::{FlowControl, Parity, SerialOptions};
use crate::error::{Error, Result};
use crate::session::{Outcome, SessionEvent, SessionInput, SessionSink, TermIo};

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

/// The devices our sessions' threads still hold, once per session. A closed session's
/// threads let go of the port within `READ_TIMEOUT`, and only one handle can have a port
/// open (exclusive on macOS, always on Windows), so reconnecting, which opens the device
/// again at once, would otherwise find it busy.
static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A session's hold on its device, shared by its reader and writer threads.
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
pub async fn run(options: SerialOptions, mut io: TermIo) {
    let since = Instant::now();
    while Hold::is_held(&options.device) && since.elapsed() < RELEASE_WAIT {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let outcome = match open(&options) {
        Ok((port, writer)) => {
            // Before the reader starts, so that what the device already sent comes after.
            let opened = t!("terminal.serialOpened", device = &options.device, settings = options.summary());
            io.print(&format!("\x1b[2m{opened}\x1b[0m\n"));
            let line = Line::start(port, writer, &options.device, &io);
            io.event(SessionEvent::Connected);
            line.bridge(&options.device, &mut io).await
        }
        Err(e) => Outcome::Failed(e),
    };
    io.finish(outcome);
}

/// Opens the device with the session's line settings: the port, and a second handle to it
/// for the writer thread.
fn open(options: &SerialOptions) -> Result<(Box<dyn SerialPort>, Box<dyn SerialPort>)> {
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
    #[cfg(windows)]
    {
        let port = builder.open_native().map_err(open_failed)?;
        let writer = port.try_clone_native().map_err(open_failed)?;
        not_inherited(&writer);
        let (port, writer): (Box<dyn SerialPort>, Box<dyn SerialPort>) = (Box::new(port), Box::new(writer));
        Ok((port, writer))
    }
    #[cfg(not(windows))]
    {
        let port = builder.open().map_err(open_failed)?;
        let writer = port.try_clone().map_err(open_failed)?;
        Ok((port, writer))
    }
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

/// What the writer thread sends to the device.
enum Output {
    Data(Vec<u8>),
    Break,
}

/// An open serial port and its reader and writer threads, which stop when it is dropped:
/// the writer once its input closes, the reader (whose reads time out) once nothing receives
/// its failures.
struct Line {
    input: mpsc::Sender<Output>,
    /// Why the reader or writer stopped, if the device failed.
    failed: mpsc::UnboundedReceiver<String>,
}

impl Line {
    fn start(port: Box<dyn SerialPort>, writer: Box<dyn SerialPort>, device: &str, io: &TermIo) -> Self {
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
        Self { input, failed }
    }

    async fn bridge(mut self, device: &str, io: &mut TermIo) -> Outcome {
        loop {
            let write = tokio::select! {
                reason = self.failed.recv() => {
                    let error = Error::new("serial.disconnected").param("device", device);
                    return Outcome::Lost(match reason {
                        Some(reason) => error.detail(reason),
                        None => error,
                    });
                }
                input = io.recv() => match input {
                    Some(SessionInput::Data(data)) => Output::Data(data),
                    Some(SessionInput::Break) => Output::Break,
                    Some(SessionInput::Resize { .. }) => continue,
                    // The tab is closing.
                    None => return Outcome::Exited(None),
                },
            };
            // An error means the writer stopped; the reason arrives on `failed`.
            let _ = self.input.send(write).await;
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
            Ok(n) => sink.output(buffer[..n].to_vec()),
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
            Err(e) => {
                let _ = failed.send(e.to_string());
                return;
            }
        }
    }
}

fn write_input(mut port: Box<dyn SerialPort>, mut inputs: mpsc::Receiver<Output>, failed: &mpsc::UnboundedSender<String>) {
    while let Some(write) = inputs.blocking_recv() {
        let result = match write {
            Output::Data(data) => write_data(&mut *port, &data, failed),
            // Not every adapter (or pseudo terminal) can send a break; that is no reason to
            // end the session.
            Output::Break => {
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
/// dropped (`failed` closes).
fn write_data(port: &mut dyn SerialPort, mut data: &[u8], failed: &mpsc::UnboundedSender<String>) -> std::io::Result<()> {
    while !data.is_empty() && !failed.is_closed() {
        match port.write(data) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
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

    use super::*;

    /// A pseudo terminal standing in for a device: the session opens its slave side, the
    /// test plays the device on the master side.
    fn fake_device() -> (File, String) {
        let (mut master, mut slave) = (0, 0);
        // SAFETY: openpty fills in two file descriptors; null name, termios and window size
        // are allowed.
        let result = unsafe {
            libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        assert_eq!(result, 0);
        // SAFETY: `slave` is an open terminal; ttyname returns a C string or null.
        let name = unsafe { CStr::from_ptr(libc::ttyname(slave)) }.to_string_lossy().into_owned();
        // SAFETY: both descriptors are open and owned here; the slave is reopened by name.
        unsafe { libc::close(slave) };
        (unsafe { File::from_raw_fd(master) }, name)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn talks_to_a_device() {
        let (mut device, path) = fake_device();
        let (io, input, output, events) = TermIo::detached((80, 24));
        let options = SerialOptions { device: path.clone(), ..SerialOptions::default() };
        let session = tokio::spawn(run(options, io));

        tokio::time::sleep(Duration::from_millis(300)).await;
        input.send(SessionInput::Data(b"help\r".to_vec())).unwrap();
        let mut buffer = [0; 64];
        let n = device.read(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"help\r");
        device.write_all(b"U-Boot> ").unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        // The device goes away.
        drop(device);
        tokio::time::timeout(Duration::from_secs(5), session).await.unwrap().unwrap();
        let text = String::from_utf8_lossy(&output.try_iter().flatten().collect::<Vec<_>>()).into_owned();
        assert!(text.contains("115200 8N1") && text.contains("U-Boot> "), "{text:?}");
        let events: Vec<String> = events.try_iter().collect();
        assert!(events[0].contains(r#""type":"connected""#), "{events:?}");
        assert!(events.last().unwrap().contains(r#""reason":"lost""#), "{events:?}");
    }

    /// Reconnecting closes the session and opens the device again at once, while the old
    /// session's threads may still hold the port, which only one can open at a time.
    #[tokio::test(flavor = "multi_thread")]
    async fn reopens_a_device_just_closed() {
        let (_device, path) = fake_device();
        let options = SerialOptions { device: path, ..SerialOptions::default() };
        let (io, _input, _output, _events) = TermIo::detached((80, 24));
        let first = tokio::spawn(run(options.clone(), io));
        tokio::time::sleep(Duration::from_millis(300)).await;
        first.abort();
        let (io, _input, _output, events) = TermIo::detached((80, 24));
        let second = tokio::spawn(run(options, io));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let events: Vec<String> = events.try_iter().collect();
        assert!(events.first().is_some_and(|e| e.contains(r#""type":"connected""#)), "{events:?}");
        second.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_a_missing_device() {
        let (io, _input, _output, events) = TermIo::detached((80, 24));
        run(SerialOptions { device: "/dev/cu.zshell-missing".into(), ..SerialOptions::default() }, io).await;
        let events: Vec<String> = events.try_iter().collect();
        assert!(events[0].contains(r#""reason":"failed""#) && events[0].contains("serial.openFailed"), "{events:?}");
    }
}
