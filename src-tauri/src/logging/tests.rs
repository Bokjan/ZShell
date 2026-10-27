use super::index::{file_name, ActiveLogs};
use super::text::{MAX_CELL_LEN, MAX_COLUMNS};
use super::*;

fn text(chunks: &[&[u8]]) -> String {
    let mut parser = vte::Parser::new();
    let mut line = Line::default();
    for chunk in chunks {
        parser.advance(&mut line, chunk);
    }
    line.finish();
    String::from_utf8(line.out).unwrap()
}

#[test]
fn plain_text_follows_the_cursor_within_a_line() {
    // Colors, a corrected typo, a progress bar, CRLF line ends, a sequence split
    // across chunks.
    let log = text(&[
        b"\x1b[32muser@host\x1b[0m:~$ lss\x08 \x08 -l\r\n",
        b"10%\r50%\r100%\r\n",
        b"\x1b[1",
        b";31mred\x1b[0m done\r\n",
        // zsh redrawing its command line: back to the start, over the prompt, rewrite.
        b"% echo loggedx\x08\x1b[K\r\x1b[2Cecho logged\r\n",
        "中文 ok\r\x1b[5C!\r\n".as_bytes(),
        b"$ ",
    ]);
    assert_eq!(log, "user@host:~$ ls -l\n100%\nred done\n% echo logged\n中文 !k\n$\n");
}

/// A line's time is when it started showing what it ends with, not when it ended.
#[test]
fn lines_start_with_the_time_they_started() {
    let mut parser = vte::Parser::new();
    let times = TimeFormat::parse("%Y-%m-%d %H:%M:%S.%3f").unwrap();
    let mut line = Line { times: Some(times), ..Line::default() };
    let stamp = |line: &mut Line| {
        let out = String::from_utf8(std::mem::take(&mut line.out)).unwrap();
        chrono::NaiveDateTime::parse_from_str(&out[..23], "%Y-%m-%d %H:%M:%S%.3f").unwrap()
    };
    let wait = Duration::from_millis(300);

    let before = Local::now().naive_local();
    parser.advance(&mut line, b"slow");
    thread::sleep(wait);
    parser.advance(&mut line, b" line\r\n");
    assert!(stamp(&mut line) - before < chrono::TimeDelta::from_std(wait).unwrap() / 2);

    // A progress line written over starts again.
    parser.advance(&mut line, b"10%");
    thread::sleep(wait);
    let rewritten = Local::now().naive_local();
    parser.advance(&mut line, b"\r\x1b[Kdone\r\n");
    assert!(stamp(&mut line) >= rewritten - chrono::TimeDelta::milliseconds(1));

    // A blank line finished at a note, left out, doesn't start the next one.
    parser.advance(&mut line, b"  ");
    line.finish();
    thread::sleep(wait);
    let next = Local::now().naive_local();
    parser.advance(&mut line, b"next\r\n");
    assert!(stamp(&mut line) >= next - chrono::TimeDelta::milliseconds(1));
}

#[test]
fn an_endless_osc_sequence_stays_bounded() {
    // vte with its `std` feature keeps an OSC sequence in a `Vec`, growing without limit; it
    // is then the size of a few pointers.
    assert!(std::mem::size_of::<vte::Parser>() > 1024, "vte is built with `std`");
    let mut endless = b"\x1b]0;".to_vec();
    endless.extend(b"y\n".repeat(1 << 20));
    assert_eq!(text(&[b"before\r\n", &endless, b"\x07after\r\n"]), "before\nafter\n");
}

#[test]
fn long_lines_and_far_cursor_moves_stay_bounded() {
    let long = "a".repeat(3 * MAX_COLUMNS + 10);
    assert_eq!(text(&[long.as_bytes(), b"\r\nnext\r\n"]), format!("{long}\nnext\n"));

    let mut parser = vte::Parser::new();
    let mut line = Line::default();
    for _ in 0..1000 {
        parser.advance(&mut line, b"\x1b[65535C\x1b[65535@x\x1b[65535X\t");
    }
    assert!(line.cells.len() <= MAX_COLUMNS && line.cursor <= MAX_COLUMNS);
    parser.advance(&mut line, b"e");
    parser.advance(&mut line, "\u{301}".repeat(10_000).as_bytes());
    assert!(line.cells.iter().all(|cell| cell.as_str().len() <= MAX_CELL_LEN));
    assert!(line.out.len() < 100 * MAX_COLUMNS, "{}", line.out.len());
}

#[test]
fn file_names_are_safe() {
    let info = LogInfo { session: "web/01: prod".into(), host: "10.0.0.1".into(), user: "root".into(), profile: None };
    let name = file_name("{session}_{user}@{host}.log", &info);
    assert_eq!(name, "web_01_ prod_root@10.0.0.1.log");
    assert_eq!(file_name("..", &info), "session.log");
    let device = LogInfo { session: "COM1".into(), ..info };
    assert_eq!(file_name("{session}.log", &device), if cfg!(windows) { "_COM1.log" } else { "COM1.log" });
}

#[test]
fn output_reaches_the_file_within_the_flush_interval() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-flush-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    let settings = LogSettings { format: LogFormat::Raw, ..LogSettings::default() };
    let slot = LogSlot::new(LogInfo::default());
    let path = logs.start(&slot, &settings).unwrap();
    let header = std::fs::read(&path).unwrap();
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&path.metadata().unwrap().permissions()) & 0o777, 0o600);

    // Flushed once the output pauses, while the log goes on (see the next test for the chunks
    // in between).
    slot.write(b"one ");
    slot.write(b"two");
    until("the output is flushed", || std::fs::read(&path).unwrap() == [header.as_slice(), b"one two"].concat());

    // Its writer ends with the slot.
    drop(slot);
    logs.active.wait_closed(&path, Duration::from_secs(5));
    assert!(logs.active.paths.lock().unwrap().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Our own messages start with the time once, whether or not the log starts its lines with it.
#[tokio::test]
async fn messages_are_stamped_once() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-notice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    for timestamps in [true, false] {
        let (io, _input, _output, _events) = crate::session::TermIo::detached((80, 24));
        let settings = LogSettings { timestamps, ..LogSettings::default() };
        let path = logs.start(io.sink().log(), &settings).unwrap();
        io.notice(crate::session::Tone::Dim, "Connecting");
        logs.stop(io.sink().log());
        logs.active.wait_closed(&path, Duration::from_secs(5));
        let text = std::fs::read_to_string(&path).unwrap();
        let line = text.lines().last().unwrap();
        assert!(line.starts_with("[20") && line.ends_with("] Connecting") && line.matches("[20").count() == 1, "{line:?}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Waits until `done`, for long enough that only a writer that never gets there fails.
fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Chunks of output stay in the buffer until the flush interval has passed since the last flush.
#[test]
fn output_is_not_flushed_with_every_chunk() {
    let path = std::env::temp_dir().join(format!("zshell-log-chunks-{}", std::process::id()));
    let mut writer = LogWriter {
        file: BufWriter::new(File::create(&path).unwrap()),
        text: None,
        // Just flushed, as far as it can tell, however long the test takes.
        flushed: Instant::now() + Duration::from_secs(3600),
        dirty: false,
        _active: Arc::new(ActiveLogs::default()).add(path.clone()),
        failure: Arc::default(),
        left_out: Arc::default(),
    };
    writer.write(b"one ").unwrap();
    writer.write(b"two").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"");
    writer.flushed = Instant::now() - FLUSH_INTERVAL;
    writer.write(b"!").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"one two!");
    drop(writer);
    std::fs::remove_file(&path).unwrap();
}

/// Output the writer has no room for is left out, and the log says how much.
#[test]
fn output_is_left_out_while_the_writer_is_behind() {
    let (chunks, queued) = sync_channel(2);
    let failure = Arc::new(Mutex::new(None));
    let queue = Queue {
        chunks,
        dropped: 0,
        path: PathBuf::from("web.log"),
        failure: failure.clone(),
        left_out: Arc::default(),
        stamps_lines: false,
    };
    let slot = LogSlot { info: LogInfo::default(), queue: Mutex::new(Some(queue)) };
    for chunk in [b"a".as_slice(), b"b", b"cc", b"ddd"] {
        slot.write(chunk);
    }
    let taken = |queued: &std::sync::mpsc::Receiver<Chunk>| match queued.try_recv().unwrap() {
        Chunk::Output(bytes) => String::from_utf8(bytes).unwrap(),
        Chunk::Dropped(bytes) => format!("<{bytes} dropped>"),
    };
    assert_eq!((taken(&queued), taken(&queued)), ("a".into(), "b".into()));
    slot.write(b"e");
    assert_eq!((taken(&queued), taken(&queued)), ("<5 dropped>".into(), "e".into()));
    // Room for the note of what was left out, but not for the output after it.
    for chunk in [b"h".as_slice(), b"i", b"jj"] {
        slot.write(chunk);
    }
    assert_eq!(taken(&queued), "h");
    slot.write(b"k");
    assert_eq!((taken(&queued), taken(&queued)), ("i".into(), "<2 dropped>".into()));
    slot.write(b"l");
    assert_eq!((taken(&queued), taken(&queued)), ("<1 dropped>".into(), "l".into()));
    // A writer that failed ends the log, which says so once.
    *failure.lock().unwrap() = Some("No space left on device".into());
    drop(queued);
    let error = slot.write(b"f").unwrap();
    assert_eq!(error.code(), "log.writeFailed");
    assert!(error.to_string().contains("web.log") && error.to_string().contains("No space left"), "{error}");
    assert!(slot.queue.lock().unwrap().is_none());
    assert!(slot.write(b"g").is_none());
}

/// Output left out just before the log stops is noted at its end, not lost with the queue.
#[test]
fn output_left_out_at_the_end_is_noted() {
    let path = std::env::temp_dir().join(format!("zshell-log-left-out-{}", std::process::id()));
    let left_out = Arc::new(AtomicUsize::new(0));
    let (chunks, queued) = sync_channel(1);
    let queue = Queue { chunks, dropped: 0, path: path.clone(), failure: Arc::default(), left_out: left_out.clone(), stamps_lines: false };
    let slot = LogSlot { info: LogInfo::default(), queue: Mutex::new(Some(queue)) };
    slot.write(b"kept");
    slot.write(b"lost");
    let writer = LogWriter {
        file: BufWriter::new(File::create(&path).unwrap()),
        text: None,
        flushed: Instant::now(),
        dirty: false,
        _active: Arc::new(ActiveLogs::default()).add(path.clone()),
        failure: Arc::default(),
        left_out,
    };
    // As `Logs::stop` does.
    slot.queue.lock().unwrap().take();
    writer.run(queued);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("kept{}\n", t!("log.dropped", bytes = 4)));
    std::fs::remove_file(&path).unwrap();
}

/// A log that can't be listed in the index (so would never be cleaned up) isn't written.
#[test]
fn a_log_is_only_written_once_it_is_in_the_index() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Can't be read, so is never written.
    std::fs::create_dir_all(dir.join("logs.json")).unwrap();
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    let slot = LogSlot::new(LogInfo::default());
    assert_eq!(logs.start(&slot, &LogSettings::default()).unwrap_err().code(), "config.unreadable");
    assert!(slot.queue.lock().unwrap().is_none());
    assert_eq!(std::fs::read_dir(dir.join("Logs")).unwrap().count(), 0);
    assert_eq!(logs.summary().count, 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A writer that can't write stops and leaves why.
#[test]
fn a_failing_writer_says_why() {
    let path = std::env::temp_dir().join(format!("zshell-log-read-only-{}", std::process::id()));
    std::fs::write(&path, b"").unwrap();
    let failure = Arc::new(Mutex::new(None));
    let active = Arc::new(ActiveLogs::default()).add(path.clone());
    // Opened for reading only: writes fail.
    let file = File::open(&path).unwrap();
    let writer = LogWriter {
        file: BufWriter::new(file),
        text: None,
        flushed: Instant::now(),
        dirty: false,
        _active: active,
        failure: failure.clone(),
        left_out: Arc::default(),
    };
    let (chunks, queued) = sync_channel(2);
    chunks.send(Chunk::Output(vec![b'x'; 64 << 10])).unwrap();
    writer.run(queued);
    assert!(failure.lock().unwrap().is_some());
    std::fs::remove_file(&path).unwrap();
}

/// A log is written while any of its writers is: a reconnection's may start before the
/// previous one's has finished.
#[test]
fn a_log_is_active_until_its_last_writer_ends() {
    let set = Arc::new(ActiveLogs::default());
    let path = PathBuf::from("web.log");
    let (first, second) = (set.add(path.clone()), set.add(path.clone()));
    drop(first);
    assert!(set.contains(&path));
    drop(second);
    assert!(!set.contains(&path));
}

/// Going on with a log whose previous writer hasn't finished (a slow network drive)
/// starts at once, and writes only after it, so the two don't mix.
#[test]
fn appending_waits_for_the_previous_writer() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-append-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    let settings = LogSettings { format: LogFormat::Raw, ..LogSettings::default() };
    let slot = LogSlot::new(LogInfo::default());
    let path = logs.start(&slot, &settings).unwrap();
    logs.stop(&slot);
    logs.active.wait_closed(&path, Duration::from_secs(5));
    let before = std::fs::read(&path).unwrap();

    // Still writing.
    let previous = logs.active.add(path.clone());
    let slot = LogSlot::new(LogInfo::default());
    // Waiting for `previous`, which this thread holds, would never end.
    logs.open(&slot, LogOpen::Append { path: path.clone() }, false, &settings).unwrap().unwrap();
    slot.write(b"after");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(std::fs::read(&path).unwrap(), before);

    drop(previous);
    drop(slot);
    logs.active.wait_closed(&path, Duration::from_secs(5));
    let content = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert!(content.ends_with("after"), "{content:?}");
    assert_eq!(content.lines().count(), before.iter().filter(|&&b| b == b'\n').count() + 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn writes_appends_and_deletes_only_its_own_logs() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    let settings = LogSettings::default();
    let info = LogInfo { session: "web".into(), host: "h".into(), user: "u".into(), profile: Some("p1".into()) };

    let slot = LogSlot::new(info.clone());
    let path = logs.start(&slot, &settings).unwrap();
    slot.write(b"\x1b[1mhello\x1b[0m\r\n");
    logs.stop(&slot);
    // A reconnection goes on with the same file.
    let slot = LogSlot::new(info);
    logs.open(&slot, LogOpen::Append { path: path.clone() }, false, &settings).unwrap().unwrap();
    slot.write(b"again\r\n");

    // Not deleted while written; another file in the folder is never touched.
    std::fs::write(dir.join("Logs").join("mine.txt"), "keep").unwrap();
    assert_eq!(logs.delete_for("p1"), 0);
    drop(slot);
    logs.active.wait_closed(&path, Duration::from_secs(5));
    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 4);
    assert_eq!((lines[1], lines[3]), ("hello", "again"));

    // Going on with a file that isn't one of the logs starts a new log instead.
    let other = LogSlot::new(LogInfo { profile: None, ..LogInfo::default() });
    let mine = dir.join("Logs").join("mine.txt");
    let started = logs.open(&other, LogOpen::Append { path: mine.clone() }, false, &settings).unwrap().unwrap();
    assert_ne!(started, mine);
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "keep");
    drop(other);
    // On Windows a folder can't go while a file in it is still open.
    logs.active.wait_closed(&started, Duration::from_secs(5));
    std::fs::remove_file(started).unwrap();
    assert_eq!(logs.count("p1"), 1);
    assert_eq!(logs.delete_for("p1"), 1);
    assert!(!path.exists() && dir.join("Logs").join("mine.txt").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn waits_for_every_log_to_close() {
    let set = Arc::new(ActiveLogs::default());
    let (a, b) = (set.add("a.log".into()), set.add("b.log".into()));
    let closing = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        drop(a);
        drop(b);
    });
    let since = Instant::now();
    set.wait_empty(Duration::from_secs(5));
    assert!(set.paths.lock().unwrap().is_empty() && since.elapsed() < Duration::from_secs(5));
    closing.join().unwrap();
    // One that never closes: given up on.
    let _stuck = set.add("c.log".into());
    set.wait_empty(Duration::from_millis(20));
    assert!(set.contains(Path::new("c.log")));
}

/// Two reconnections that go on with a log while an earlier writer still writes it each
/// wait only for the writers before them, in turn: waiting to be the only writer left, the
/// two waited for each other forever, and the log silently stopped.
#[test]
fn appenders_take_turns_after_a_slow_writer() {
    let dir = std::env::temp_dir().join(format!("zshell-logs-turns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
    let settings = LogSettings { format: LogFormat::Raw, ..LogSettings::default() };
    let slot = LogSlot::new(LogInfo::default());
    let path = logs.start(&slot, &settings).unwrap();
    logs.stop(&slot);
    logs.active.wait_closed(&path, Duration::from_secs(5));

    // Still writing.
    let previous = logs.active.add(path.clone());
    for text in [b"second", b"third!"] {
        let slot = LogSlot::new(LogInfo::default());
        logs.open(&slot, LogOpen::Append { path: path.clone() }, false, &settings).unwrap().unwrap();
        slot.write(text);
    }
    drop(previous);
    logs.active.wait_closed(&path, Duration::from_secs(5));
    let content = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert!(!logs.active.contains(&path), "writers still waiting: {content:?}");
    let (second, third) = (content.find("second"), content.find("third!"));
    assert!(second.is_some() && third.is_some() && second < third, "{content:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}
