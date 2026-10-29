use super::*;

fn entry(source: LogSource, message: &str) -> LogEntry {
    LogEntry { seq: 0, source, time: 0, level: LogLevel::Debug, target: "ssh".into(), connection: None, message: message.into() }
}

#[test]
fn a_ring_keeps_the_latest_within_its_lines_and_bytes() {
    let mut ring = Ring::new(3);
    for i in 0..5 {
        ring.push(entry(LogSource::Backend, &i.to_string()));
    }
    let kept: Vec<&str> = ring.entries.iter().map(|entry| entry.message.as_str()).collect();
    assert_eq!(kept, ["2", "3", "4"]);
    assert_eq!(ring.bytes, ring.entries.iter().map(LogEntry::size).sum::<usize>());

    let mut ring = Ring::new(BACKEND_LINES);
    let big = "x".repeat(MAX_MESSAGE);
    for _ in 0..MAX_BYTES / MAX_MESSAGE + 10 {
        ring.push(entry(LogSource::Backend, &big));
    }
    assert!(ring.bytes <= MAX_BYTES);
    assert!(ring.entries.len() < MAX_BYTES / MAX_MESSAGE);
    ring.clear();
    assert_eq!((ring.entries.len(), ring.bytes), (0, 0));
}

#[test]
fn long_messages_are_cut_at_a_character() {
    assert_eq!(cut("short".into()), "short");
    let long = "é".repeat(MAX_MESSAGE);
    let total = long.len();
    let cut = cut(long);
    assert!(cut.starts_with('é'));
    assert!(cut.ends_with(&format!(" [cut short: {total} bytes]")));
    assert!(cut.len() <= MAX_MESSAGE + 40);
}

#[test]
fn only_the_records_worth_keeping_are() {
    let message = |text: &'static str| move || text.to_owned();
    assert!(wanted("zshell_lib::ssh", Level::Debug, message("connecting")));
    assert!(!wanted("zshell_lib::ssh", Level::Trace, message("connecting")));
    assert!(wanted("russh::client::kex", Level::Debug, message("negotiated algorithms")));
    assert!(wanted("russh_sftp::client", Level::Debug, message("read half of sftp stream ended")));
    // Every packet, or what the agent signs.
    assert!(!wanted("russh::sshbuffer", Level::Debug, message("> msg type 94, len 9")));
    assert!(!wanted("russh::client", Level::Debug, message("< msg type 94, seqn 48, len 20")));
    assert!(!wanted("russh::keys::agent::client", Level::Debug, message("sign_request: [1, 2]")));
    assert!(wanted("russh::keys::agent::client", Level::Error, message("unexpected agent signature type")));
    assert!(!wanted("russh::client::encrypted", Level::Debug, message("channel_extended_data")));
    assert!(!wanted("russh::client::encrypted", Level::Debug, message("channel_window_adjust amount: 1")));
    assert!(!wanted("russh_sftp::protocol", Level::Debug, message("packet type 101")));
    assert!(wanted("russh::client::encrypted", Level::Debug, message("userauth_failure")));
    // Other crates from `warn` only.
    assert!(!wanted("tauri::manager", Level::Info, message("loaded")));
    assert!(wanted("tao::platform", Level::Warn, message("failed")));
}

#[test]
fn the_window_gets_what_comes_after_its_snapshot() {
    let mut state = State { next_seq: 1, backend: Ring::new(10), frontend: Ring::new(10), pending: None };
    assert!(!state.push(entry(LogSource::Backend, "before")));
    state.pending = Some(Vec::new());
    assert!(state.push(entry(LogSource::Frontend, "after")));
    let pending = state.pending.as_ref().unwrap();
    assert_eq!((pending.len(), pending[0].seq, pending[0].message.as_str()), (1, 2, "after"));
    assert_eq!(state.frontend.entries[0].seq, 2);
}
