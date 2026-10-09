//! Whole transfers: our sender against our receiver, and both against lrzsz's `lsz` / `lrz`
//! when installed (skipped otherwise, e.g. on CI).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tokio::sync::{mpsc, watch};

use super::frame::{self, Encoding, Header, Kind, CANFC32, CANFDX, ZCRCE, ZCRCW};
use super::link::{Link, TIMEOUT};
use super::{detect, receive, send, Direction, Report};

#[derive(Default)]
struct Log {
    events: Vec<String>,
}

impl Report for Log {
    fn start(&mut self, name: &str, size: Option<u64>) {
        self.events.push(format!("start {name} {size:?}"));
    }
    fn progress(&mut self, _done: u64) {}
    fn received(&mut self, path: &Path, size: u64) {
        self.events.push(format!("received {} {size}", path.file_name().unwrap().to_string_lossy()));
    }
    fn sent(&mut self, name: &str, size: u64) {
        self.events.push(format!("sent {name} {size}"));
    }
    fn skipped(&mut self, name: &str) {
        self.events.push(format!("skipped {name}"));
    }
    fn failed(&mut self, error: anyhow::Error) {
        self.events.push(format!("failed {error:#}"));
    }
}

/// The far ends of a link's channels: what to feed it, what it sent, and its cancel switch
/// (which has to stay alive).
type Ends = (mpsc::UnboundedSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>, watch::Sender<bool>);

fn link() -> (Link, Ends) {
    let (incoming_tx, incoming) = mpsc::unbounded_channel();
    let (outgoing, outgoing_rx) = mpsc::channel(4);
    let (cancel, cancel_rx) = watch::channel(false);
    (Link::new(incoming, outgoing, cancel_rx), (incoming_tx, outgoing_rx, cancel))
}

/// Forwards one link's output to another's input.
fn pipe(mut from: mpsc::Receiver<Vec<u8>>, to: mpsc::UnboundedSender<Vec<u8>>) {
    tokio::spawn(async move {
        while let Some(chunk) = from.recv().await {
            if to.send(chunk).is_err() {
                break;
            }
        }
    });
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zshell-zmodem-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Test files: every byte value (so every escape), empty, and sizes around the block size.
fn make_files(dir: &Path) -> Vec<PathBuf> {
    let mut seed = 0x2545_f491_u32;
    let mut random = |len: usize| -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect()
    };
    let files = [
        ("all-bytes.bin", (0..=255u8).cycle().take(5000).collect()),
        ("empty.txt", Vec::new()),
        ("exact-blocks.bin", random(4096)),
        ("large.bin", random(300_000)),
    ];
    files
        .into_iter()
        .map(|(name, data)| {
            let path = dir.join(name);
            std::fs::write(&path, data).unwrap();
            path
        })
        .collect()
}

fn assert_same_files(sent: &[PathBuf], dir: &Path) {
    for path in sent {
        let received = dir.join(path.file_name().unwrap());
        assert_eq!(std::fs::read(path).unwrap(), std::fs::read(&received).unwrap(), "{}", received.display());
    }
}

#[test]
fn detects_transfers_in_output() {
    // sz: "rz\r" then ZRQINIT; the header starts at its first ZPAD.
    assert_eq!(detect(b"rz\r**\x18B00000000000000\r\x8a\x11").map(|(i, d)| (i, d == Direction::Receive)), Some((3, true)));
    // rz: its message, then ZRINIT.
    let output = b"rz waiting to receive.**\x18B0100000023be50\r\x8a\x11";
    assert_eq!(detect(output).map(|(i, d)| (i, d == Direction::Send)), Some((22, true)));
    assert!(detect(b"ls -l\r\nfile *.txt").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn our_sender_and_receiver_agree() {
    let src = temp_dir("loop-src");
    let dst = temp_dir("loop-dst");
    let files = make_files(&src);
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, _cancel_r)) = link();
    pipe(from_sender, to_receiver);
    pipe(from_receiver, to_sender);

    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move {
        let mut log = Log::default();
        receive::receive(&mut receiver, &dst2, &mut log).await.map(|()| log)
    });
    let rinit = sender.header(TIMEOUT).await.unwrap();
    assert_eq!(rinit.kind, Kind::Rinit);
    let mut log = Log::default();
    send::send(&mut sender, rinit, &files, &mut log).await.unwrap();
    let received = receiving.await.unwrap().unwrap();

    assert_same_files(&files, &dst);
    assert_eq!(log.events.iter().filter(|e| e.starts_with("sent ")).count(), files.len(), "{:?}", log.events);
    assert_eq!(received.events.iter().filter(|e| e.starts_with("received ")).count(), files.len(), "{:?}", received.events);
    // Existing names get a suffix rather than being overwritten.
    let (mut sender, (to_sender, from_sender, _c1)) = link();
    let (mut receiver, (to_receiver, from_receiver, _c2)) = link();
    pipe(from_sender, to_receiver);
    pipe(from_receiver, to_sender);
    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move { receive::receive(&mut receiver, &dst2, &mut Log::default()).await });
    let rinit = sender.header(TIMEOUT).await.unwrap();
    send::send(&mut sender, rinit, &files[..1], &mut Log::default()).await.unwrap();
    receiving.await.unwrap().unwrap();
    assert!(dst.join("all-bytes (1).bin").exists());

    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

/// A receiver that wants each subpacket acknowledged (no CANOVIO, as with a limited buffer).
fn acknowledging(rinit: Header) -> Header {
    assert_eq!(rinit.kind, Kind::Rinit);
    Header::with_flags(Kind::Rinit, CANFDX | CANFC32)
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_with_an_acknowledgement_per_subpacket() {
    let src = temp_dir("ack-src");
    let dst = temp_dir("ack-dst");
    let files = make_files(&src);
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, _cancel_r)) = link();
    pipe(from_sender, to_receiver);
    pipe(from_receiver, to_sender);

    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move { receive::receive(&mut receiver, &dst2, &mut Log::default()).await });
    let rinit = acknowledging(sender.header(TIMEOUT).await.unwrap());
    let mut log = Log::default();
    send::send(&mut sender, rinit, &files, &mut log).await.unwrap();
    receiving.await.unwrap().unwrap();
    assert_same_files(&files, &dst);
    assert_eq!(log.events.iter().filter(|e| e.starts_with("sent ")).count(), files.len(), "{:?}", log.events);
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

/// Like `pipe`, passing each chunk through `change` (which may hold it up).
fn pipe_through<F, Fut>(mut from: mpsc::Receiver<Vec<u8>>, to: mpsc::UnboundedSender<Vec<u8>>, mut change: F)
where
    F: FnMut(Vec<u8>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Vec<u8>> + Send,
{
    tokio::spawn(async move {
        while let Some(chunk) = from.recv().await {
            if to.send(change(chunk).await).is_err() {
                break;
            }
        }
    });
}

const SHORT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);

#[tokio::test(flavor = "multi_thread")]
async fn receiving_survives_a_pause_in_the_data() {
    let src = temp_dir("pause-src");
    let dst = temp_dir("pause-dst");
    let files = make_files(&src);
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, _cancel_r)) = link();
    sender.timeout = SHORT_TIMEOUT;
    receiver.timeout = SHORT_TIMEOUT;
    // Partway through the large file, nothing arrives for a while.
    let mut forwarded = 0;
    pipe_through(from_sender, to_receiver, move |chunk| {
        let pause = forwarded < 100_000 && forwarded + chunk.len() >= 100_000;
        forwarded += chunk.len();
        async move {
            if pause {
                tokio::time::sleep(SHORT_TIMEOUT * 3).await;
            }
            chunk
        }
    });
    pipe(from_receiver, to_sender);

    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move { receive::receive(&mut receiver, &dst2, &mut Log::default()).await });
    let rinit = sender.header(TIMEOUT).await.unwrap();
    send::send(&mut sender, rinit, &files, &mut Log::default()).await.unwrap();
    receiving.await.unwrap().unwrap();
    assert_same_files(&files, &dst);
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn receiving_gives_up_on_data_damaged_every_time() {
    const MARKER: &[u8] = b"DAMAGED-HERE-EVERY-TIME";
    let src = temp_dir("damage-src");
    let dst = temp_dir("damage-dst");
    let path = src.join("damaged.txt");
    let mut contents = vec![b'a'; 20_000];
    contents[100..100 + MARKER.len()].copy_from_slice(MARKER);
    std::fs::write(&path, contents).unwrap();
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, _cancel_r)) = link();
    pipe_through(from_sender, to_receiver, |mut chunk| async move {
        if let Some(at) = chunk.windows(MARKER.len()).position(|w| w == MARKER) {
            chunk[at] = b'X';
        }
        chunk
    });
    pipe(from_receiver, to_sender);

    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move { receive::receive(&mut receiver, &dst2, &mut Log::default()).await });
    let rinit = sender.header(TIMEOUT).await.unwrap();
    let sending = tokio::spawn(async move { send::send(&mut sender, rinit, &[path], &mut Log::default()).await });
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), receiving).await.expect("still retrying");
    let error = result.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains(&t!("errors.zmodem.corrupt")), "{error:#}");
    sending.abort();
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_deletes_the_partial_file() {
    let src = temp_dir("cancel-src");
    let dst = temp_dir("cancel-dst");
    let files = make_files(&src);
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, cancel)) = link();
    pipe(from_sender, to_receiver);
    pipe(from_receiver, to_sender);

    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move { receive::receive(&mut receiver, &dst2, &mut Log::default()).await });
    let rinit = sender.header(TIMEOUT).await.unwrap();
    let sending = tokio::spawn(async move { send::send(&mut sender, rinit, &files[3..], &mut Log::default()).await });
    // Partway through the large file.
    while std::fs::read_dir(&dst).unwrap().next().is_none() {
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    cancel.send(true).unwrap();
    let error = receiving.await.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains(&t!("errors.zmodem.cancelled")), "{error:#}");
    assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 0);
    sending.abort();
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_left_for_another_is_deleted_and_reported() {
    let dst = temp_dir("switch-dst");
    let (mut sender, (to_sender, from_sender, _cancel_s)) = link();
    let (mut receiver, (to_receiver, from_receiver, _cancel_r)) = link();
    pipe(from_sender, to_receiver);
    pipe(from_receiver, to_sender);
    let dst2 = dst.clone();
    let receiving = tokio::spawn(async move {
        let mut log = Log::default();
        receive::receive(&mut receiver, &dst2, &mut log).await.map(|()| log.events)
    });

    // A sender that offers a.txt, sends part of it, then offers b.txt instead.
    sender.header(TIMEOUT).await.unwrap();
    for (name, data, eof) in [("a.txt", b"hello".as_slice(), false), ("b.txt", b"world".as_slice(), true)] {
        let mut packet = frame::encode_header(&Header::new(Kind::File), Encoding::Bin32);
        frame::encode_subpacket(&mut packet, format!("{name}\0{}\0", if eof { 5 } else { 10 }).as_bytes(), ZCRCW, true);
        sender.send(&packet).await.unwrap();
        sender.flush().await.unwrap();
        assert_eq!(sender.header(TIMEOUT).await.unwrap(), Header::with_pos(Kind::Rpos, 0));
        let mut packet = frame::encode_header(&Header::with_pos(Kind::Data, 0), Encoding::Bin32);
        frame::encode_subpacket(&mut packet, data, ZCRCE, true);
        if eof {
            packet.extend(frame::encode_header(&Header::with_pos(Kind::Eof, 5), Encoding::Bin32));
        }
        sender.send(&packet).await.unwrap();
        sender.flush().await.unwrap();
    }
    assert_eq!(sender.header(TIMEOUT).await.unwrap().kind, Kind::Rinit);
    sender.send_header(Header::new(Kind::Fin), Encoding::Hex).await.unwrap();
    assert_eq!(sender.header(TIMEOUT).await.unwrap().kind, Kind::Fin);
    sender.send(b"OO").await.unwrap();
    sender.flush().await.unwrap();

    let events = receiving.await.unwrap().unwrap();
    let incomplete = t!("errors.zmodem.incomplete", name = "a.txt");
    assert!(events.iter().any(|e| e.contains(&*incomplete)), "{events:?}");
    let names: Vec<_> = std::fs::read_dir(&dst).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["b.txt"]);
    assert_eq!(std::fs::read(dst.join("b.txt")).unwrap(), b"world");
    std::fs::remove_dir_all(&dst).unwrap();
}

/// An lrzsz program (`lsz` / `lrz`, as Homebrew and most distributions install them), if any.
fn lrzsz(name: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    let found = dirs
        .into_iter()
        .chain([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")])
        .map(|d| d.join(name))
        .find(|p| p.is_file());
    if found.is_none() {
        eprintln!("{name} not found (lrzsz is not installed); skipping");
    }
    found
}

/// Runs `program args` with its stdin and stdout connected to a link (and the link's cancel
/// switch, which has to stay alive).
fn spawn_with_link(program: &Path, args: &[&Path], cwd: &Path) -> (Link, std::process::Child, watch::Sender<bool>) {
    let (link, (incoming, mut outgoing, cancel)) = link();
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        while let Ok(n) = stdout.read(&mut buffer) {
            if n == 0 || incoming.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    std::thread::spawn(move || {
        while let Some(chunk) = outgoing.blocking_recv() {
            if stdin.write_all(&chunk).and_then(|()| stdin.flush()).is_err() {
                break;
            }
        }
    });
    (link, child, cancel)
}

#[tokio::test(flavor = "multi_thread")]
async fn receives_from_lsz() {
    let Some(lsz) = lrzsz("lsz") else { return };
    let src = temp_dir("lsz-src");
    let dst = temp_dir("lsz-dst");
    let files = make_files(&src);
    let args: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let (mut link, mut child, _cancel) = spawn_with_link(&lsz, &args, &src);
    assert_eq!(link.header(TIMEOUT).await.unwrap().kind, Kind::Rqinit);
    let mut log = Log::default();
    receive::receive(&mut link, &dst, &mut log).await.unwrap();
    assert!(child.wait().unwrap().success());
    assert_same_files(&files, &dst);
    assert_eq!(log.events.iter().filter(|e| e.starts_with("received ")).count(), files.len(), "{:?}", log.events);
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_to_lrz() {
    let Some(lrz) = lrzsz("lrz") else { return };
    let src = temp_dir("lrz-src");
    let dst = temp_dir("lrz-dst");
    let files = make_files(&src);
    let (mut link, mut child, _cancel) = spawn_with_link(&lrz, &[], &dst);
    let rinit = link.header(TIMEOUT).await.unwrap();
    assert_eq!(rinit.kind, Kind::Rinit);
    let mut log = Log::default();
    send::send(&mut link, rinit, &files, &mut log).await.unwrap();
    assert!(child.wait().unwrap().success());
    assert_same_files(&files, &dst);
    assert_eq!(log.events.iter().filter(|e| e.starts_with("sent ")).count(), files.len(), "{:?}", log.events);

    // Acknowledging each subpacket, into a fresh folder.
    let dst_ack = temp_dir("lrz-ack-dst");
    let (mut link, mut child, _cancel) = spawn_with_link(&lrz, &[], &dst_ack);
    let rinit = acknowledging(link.header(TIMEOUT).await.unwrap());
    send::send(&mut link, rinit, &files, &mut Log::default()).await.unwrap();
    assert!(child.wait().unwrap().success());
    assert_same_files(&files, &dst_ack);
    std::fs::remove_dir_all(&dst_ack).unwrap();

    // lrz refuses files that exist (without -y / -E).
    let (mut link, mut child, _cancel) = spawn_with_link(&lrz, &[], &dst);
    let rinit = link.header(TIMEOUT).await.unwrap();
    let mut log = Log::default();
    send::send(&mut link, rinit, &files[..1], &mut log).await.unwrap();
    child.wait().unwrap();
    assert_eq!(log.events, ["skipped all-bytes.bin"]);
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

/// Through a pseudo terminal, as in a local terminal tab: `rz` changes the terminal's modes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn sends_to_lrz_in_a_pty() {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let Some(lrz) = lrzsz("lrz") else { return };
    let src = temp_dir("lrz-pty-src");
    let dst = temp_dir("lrz-pty-dst");
    let files = make_files(&src);
    let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut command = CommandBuilder::new(&lrz);
    command.cwd(&dst);
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let (mut link, (incoming, mut outgoing, _cancel)) = link();
    let mut reader = pair.master.try_clone_reader().unwrap();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 || incoming.send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut writer = pair.master.take_writer().unwrap();
    std::thread::spawn(move || {
        while let Some(chunk) = outgoing.blocking_recv() {
            if writer.write_all(&chunk).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });
    let rinit = link.header(TIMEOUT).await.unwrap();
    send::send(&mut link, rinit, &files, &mut Log::default()).await.unwrap();
    assert!(child.wait().unwrap().success());
    assert_same_files(&files, &dst);
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}
