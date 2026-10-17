//! What the tests of port forwarding (here and in `crate::connections`) need: a tab watching
//! the states of a session's rules, a rule, and a port to listen on.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::{ForwardKind, ForwardRule};
use crate::session::{SessionInput, TermIo};

/// How long a test waits for something before failing.
const PATIENCE: Duration = Duration::from_secs(10);

/// A tab without a frontend, as the forwarding code reports to it.
pub struct Tab {
    pub io: TermIo,
    events: Receiver<String>,
    _input: UnboundedSender<SessionInput>,
    _output: Receiver<Vec<u8>>,
}

impl Tab {
    pub fn new() -> Self {
        let (io, input, output, events) = TermIo::detached((80, 24));
        Self { io, events, _input: input, _output: output }
    }

    /// The next state reported for rule `rule_id` (`{"type": "active", …}`), skipping other
    /// events; fails if none comes for a while.
    pub async fn next_state(&self, rule_id: &str) -> Value {
        let deadline = Instant::now() + PATIENCE;
        loop {
            while let Ok(json) = self.events.try_recv() {
                let event: Value = serde_json::from_str(&json).unwrap();
                if event["type"] == "forward" && event["ruleId"] == rule_id {
                    return event["state"].clone();
                }
            }
            assert!(Instant::now() < deadline, "no state reported for {rule_id}");
            // The rule tasks run on Tauri's runtime, the connection on this one.
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    /// The type of the next state reported for rule `rule_id` (see [`Tab::next_state`]).
    pub async fn next_kind(&self, rule_id: &str) -> String {
        self.next_state(rule_id).await["type"].as_str().unwrap().to_owned()
    }

    /// The types of the states reported for rule `rule_id` so far and not looked at yet,
    /// skipping other events.
    pub fn reported(&self, rule_id: &str) -> Vec<String> {
        let events = self.events.try_iter().map(|json| serde_json::from_str::<Value>(&json).unwrap());
        events
            .filter(|event| event["type"] == "forward" && event["ruleId"] == rule_id)
            .map(|event| event["state"]["type"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Whether nothing more has been reported so far.
    pub fn quiet(&self) -> bool {
        self.events.try_recv().is_err()
    }
}

/// Waits until `done` holds; fails if it doesn't for a while.
pub async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// A local rule (`-L`) listening on 127.0.0.1:`port`.
pub fn local_rule(id: &str, port: u16) -> ForwardRule {
    ForwardRule {
        id: id.into(),
        kind: ForwardKind::Local,
        bind_host: "127.0.0.1".into(),
        bind_port: port,
        target_host: "127.0.0.1".into(),
        target_port: 9,
        description: String::new(),
        auto_start: false,
    }
}

/// A port nothing listens on now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
