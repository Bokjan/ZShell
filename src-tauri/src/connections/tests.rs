use super::*;
use crate::forward::testing::{free_port, local_rule, until, Tab};
use crate::model::ForwardKind;
use crate::session::Outcome;
use crate::ssh::testing::{connect, Anyone, Echoing};

/// Two connections of saved session "web", by tabs 1 and 2, with rule r1 running on the
/// first; tab 2 watches the rule.
async fn two_connections(port: u16) -> (Connections, [Arc<ssh::Connection>; 2], [Tab; 2]) {
    let connections = Connections::default();
    let ssh = [connect(Anyone, "web").await, connect(Anyone, "web").await];
    let tabs = [Tab::new(), Tab::new()];
    connections.connected(1, ssh[0].clone(), &[], &tabs[0].io);
    connections.connected(2, ssh[1].clone(), &[], &tabs[1].io);
    connections.start_forward(1, local_rule("r1", port)).unwrap();
    assert_eq!(tabs[1].next_kind("r1").await, "starting");
    assert_eq!(tabs[1].next_kind("r1").await, "active");
    assert!(connections.get(1).unwrap().forwards.is_running("r1"));
    (connections, ssh, tabs)
}

/// Closing the connection running a rule the user chose to keep: the rule moves to the
/// session's other connection, on the same port, once the first has let it go.
#[tokio::test]
async fn hands_rules_over_when_a_connection_closes() {
    let port = free_port();
    let (connections, ssh, tabs) = two_connections(port).await;
    assert_eq!(ids(&connections.forwards_to_keep(&[1])), ["r1"]);
    connections.keep_forwards(&[1]);
    let closing = connections.get(1).unwrap();
    tabs[0].io.finish(Outcome::Exited(None));

    assert_eq!(tabs[1].next_kind("r1").await, "stopped");
    assert_eq!(tabs[1].next_kind("r1").await, "starting");
    let active = tabs[1].next_state("r1").await;
    assert_eq!(active["type"], "active", "{active}");
    assert_eq!(active["bound"], format!("127.0.0.1:{port}"));
    until("the closing task's end", || Arc::strong_count(&closing) == 1).await;
    until("the connection's end", || ssh[0].handle().is_closed()).await;
    assert!(connections.get(1).is_err());
    assert!(connections.get(2).unwrap().forwards.is_running("r1"));
}

/// The tabs using a connection that runs the session's rules, whichever tab started them.
#[tokio::test]
async fn finds_the_tabs_forwarding_for_a_session() {
    let (connections, ssh, tabs) = two_connections(0).await;
    let duplicate = Tab::new();
    connections.attach(3, connections.get(1).unwrap(), &duplicate.io);
    assert_eq!(connections.forwarding_sessions("web"), [1, 3]);
    assert!(connections.forwarding_sessions("other").is_empty());
    connections.stop_saved_forwards("web", None);
    assert_eq!(tabs[1].next_kind("r1").await, "stopped");
    assert!(connections.forwarding_sessions("web").is_empty());
    drop(ssh);
}

/// Without the user choosing to keep them, a closing connection's rules just stop.
#[tokio::test]
async fn stops_rules_not_kept_when_a_connection_closes() {
    let (connections, ssh, tabs) = two_connections(0).await;
    let closing = connections.get(1).unwrap();
    tabs[0].io.finish(Outcome::Exited(None));
    assert_eq!(tabs[1].next_kind("r1").await, "stopped");
    // The task that disconnects it, and would hand the rules over, has let it go.
    until("the closing task's end", || Arc::strong_count(&closing) == 1).await;
    until("the connection's end", || ssh[0].handle().is_closed()).await;
    assert!(!connections.get(2).unwrap().forwards.has("r1"));
    assert_eq!(tabs[1].reported("r1"), Vec::<String>::new());
}

/// An SFTP command on a connection gone dead ends it soon, rather than waiting for its
/// channel or its requests until keepalives give up.
#[tokio::test]
async fn sftp_on_a_dead_connection_ends_it() {
    let connections = Connections::default();
    let (ssh, stall) = crate::ssh::testing::connect_stalling(Anyone, std::time::Duration::from_secs(3600)).await;
    let tab = Tab::new();
    connections.connected(1, ssh.clone(), &[], &tab.io);
    stall.now();
    let sftp = tokio::time::timeout(std::time::Duration::from_secs(5), connections.get(1).unwrap().sftp()).await;
    assert!(sftp.expect("the connection is still open").is_err());
    assert!(ssh.disconnect_reason().borrow().is_some());
}

/// The rules of a connection that closed (lost) are given once to its tab, to reconnect with.
#[tokio::test]
async fn gives_a_closed_connections_rules_to_carry_once() {
    let (connections, _ssh, tabs) = two_connections(0).await;
    assert_eq!(connections.forwards_to_carry(1), ["r1"]);
    tabs[0].io.finish(Outcome::Exited(None));
    until("the session to close", || connections.get(1).is_err()).await;
    assert_eq!(connections.forwards_to_carry(1), ["r1"]);
    assert!(connections.forwards_to_carry(1).is_empty());
    assert!(connections.forwards_to_carry(2).is_empty());
}

/// A connection another tab still uses (a duplicated tab) stays open with its rules.
#[tokio::test]
async fn keeps_rules_while_another_tab_uses_the_connection() {
    let (connections, ssh, tabs) = two_connections(0).await;
    let duplicate = Tab::new();
    connections.attach(3, connections.get(1).unwrap(), &duplicate.io);
    tabs[0].io.finish(Outcome::Exited(None));
    assert!(connections.get(1).is_err());
    assert!(connections.get(3).unwrap().forwards.is_running("r1"));
    assert!(!ssh[0].handle().is_closed());
    assert!(tabs[1].quiet());
}

/// A client of rule r1 on `port`, connected through it (the server sends back what it
/// gets).
async fn client(port: u16) -> tokio::net::TcpStream {
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert!(echoes(&mut client, b"ping").await, "the connection doesn't work");
    client
}

/// Whether `client`'s connection still sends back what it sends.
async fn echoes(client: &mut tokio::net::TcpStream, data: &[u8]) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut back = vec![0; data.len()];
    let round_trip = async {
        client.write_all(data).await?;
        client.read_exact(&mut back).await
    };
    matches!(tokio::time::timeout(std::time::Duration::from_secs(5), round_trip).await, Ok(Ok(_))) && back == data
}

/// Waits for `tab` to hear that rule r1 stopped, past the counts of its connections.
async fn stopped_in(tab: &Tab) {
    while tab.next_kind("r1").await != "stopped" {}
}

/// A connection of saved session "web" by tab 1, running rule r1 on `port`.
async fn forwarding(port: u16) -> (Connections, Tab) {
    let connections = Connections::default();
    let tab = Tab::new();
    connections.connected(1, connect(Echoing, "web").await, &[], &tab.io);
    connections.start_forward(1, local_rule("r1", port)).unwrap();
    assert_eq!(tab.next_kind("r1").await, "starting");
    assert_eq!(tab.next_kind("r1").await, "active");
    (connections, tab)
}

/// The shell of the connection's last tab exits: its rule stops listening, and the
/// connection it carries goes on until it ends.
#[tokio::test]
async fn keeps_the_connections_of_rules_released_as_the_shell_exits() {
    let port = free_port();
    let (connections, tab) = forwarding(port).await;
    let mut client = client(port).await;
    until("the connection to be counted", || connections.forward_connections(&[1]) == 1).await;

    let (connection, mut draining, stopped) = connections.release_forwards(1).await.unwrap();
    assert_eq!(ids(&stopped), ["r1"]);
    stopped_in(&tab).await;
    assert_eq!(draining.len(), 1);
    assert_eq!(draining.rules(), [format!("-L 127.0.0.1:{port}:127.0.0.1:9")]);
    // The port is free, the connection still works.
    drop(std::net::TcpListener::bind(("127.0.0.1", port)).unwrap());
    assert!(echoes(&mut client, b"pong").await);

    drop(client);
    tokio::time::timeout(std::time::Duration::from_secs(5), draining.next_ended()).await.unwrap();
    assert!(draining.is_empty());
    // Another shell: the rule starts again.
    connections.restart_forwards(&connection, draining, stopped);
    assert_eq!(tab.next_kind("r1").await, "starting");
    assert_eq!(tab.next_kind("r1").await, "active");
}

/// With another connection of the session, the rule moves there as the shell exits; the
/// connection it carried stays on the first.
#[tokio::test]
async fn hands_rules_over_as_the_shell_exits() {
    let port = free_port();
    let (connections, tab) = forwarding(port).await;
    let other = Tab::new();
    connections.connected(2, connect(Echoing, "web").await, &[], &other.io);
    let mut client = client(port).await;

    let (_, draining, stopped) = connections.release_forwards(1).await.unwrap();
    assert!(stopped.is_empty());
    assert_eq!(draining.len(), 1);
    stopped_in(&tab).await;
    assert_eq!(tab.next_kind("r1").await, "starting");
    let active = tab.next_state("r1").await;
    assert_eq!(active["bound"], format!("127.0.0.1:{port}"), "{active}");
    assert!(connections.get(2).unwrap().forwards.is_running("r1"));
    assert!(echoes(&mut client, b"pong").await);
    // New connections go through the other one.
    let _second = self::client(port).await;
    until("the new connection to be counted", || connections.forward_connections(&[2]) == 1).await;
}

/// A connection another tab still uses keeps its rules when one of its shells exits.
#[tokio::test]
async fn releases_nothing_while_another_tab_uses_the_connection() {
    let (connections, _tab) = forwarding(0).await;
    let duplicate = Tab::new();
    connections.attach(3, connections.get(1).unwrap(), &duplicate.io);
    assert!(connections.release_forwards(1).await.is_none());
    assert!(connections.get(3).unwrap().forwards.is_running("r1"));
}

/// Enter while waiting starts another shell, with the rule running again; the connections
/// it carried go on, and are waited for again when that shell exits.
#[tokio::test]
async fn starts_another_shell_on_enter_while_waiting() {
    use crate::ssh::ShellExit as _;

    let port = free_port();
    let (connections, mut tab) = forwarding(port).await;
    let mut client = client(port).await;
    tab.typed("x");
    tab.typed("\r");
    assert!(connections.shell_exit(1).exited(&mut tab.io).await);
    stopped_in(&tab).await;
    assert_eq!(tab.next_kind("r1").await, "starting");
    assert_eq!(tab.next_kind("r1").await, "active");
    assert!(echoes(&mut client, b"pong").await);
    // Closing the tab now would cut it too.
    assert_eq!(connections.forward_connections(&[1]), 1);
    let second = self::client(port).await;
    until("the new connection to be counted", || connections.forward_connections(&[1]) == 2).await;
    drop(second);
    until("the new connection to end", || connections.forward_connections(&[1]) == 1).await;

    tab.typed("\r");
    assert!(connections.shell_exit(1).exited(&mut tab.io).await, "the connection wasn't waited for");
    assert!(echoes(&mut client, b"pong").await);
}

/// Without Enter, the session waits until the connections have ended; Ctrl+C ends them.
#[tokio::test]
async fn waits_for_the_connections_to_end() {
    use crate::ssh::ShellExit as _;

    let port = free_port();
    let (connections, mut tab) = forwarding(port).await;
    let client = client(port).await;
    let closing = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        drop(client);
    });
    let waited = tokio::time::timeout(std::time::Duration::from_secs(5), connections.shell_exit(1).exited(&mut tab.io)).await;
    assert_eq!(waited, Ok(false));
    assert!(closing.is_finished());

    connections.restart_forwards(&connections.get(1).unwrap(), Draining::default(), vec![local_rule("r1", port)]);
    // Listening once active: it counts as running while it is still starting.
    while tab.next_kind("r1").await != "starting" {}
    assert_eq!(tab.next_kind("r1").await, "active");
    let mut client = self::client(port).await;
    tab.typed("\x03");
    assert!(!connections.shell_exit(1).exited(&mut tab.io).await);
    assert!(!echoes(&mut client, b"pong").await);
}

/// A connection of saved session `session` running `running`.
struct Fake {
    session: &'static str,
    running: Vec<ForwardRule>,
}

impl Holder for Fake {
    fn session(&self) -> &str {
        self.session
    }
    fn running(&self) -> Vec<ForwardRule> {
        self.running.clone()
    }
    fn is_running(&self, rule_id: &str) -> bool {
        self.running.iter().any(|rule| rule.id == rule_id)
    }
}

fn rule(id: &str, auto_start: bool) -> ForwardRule {
    ForwardRule {
        id: id.into(),
        kind: ForwardKind::Local,
        bind_host: "localhost".into(),
        bind_port: 8080,
        target_host: "localhost".into(),
        target_port: 80,
        description: String::new(),
        auto_start,
    }
}

fn connection(session: &'static str, running: &[&str]) -> Arc<Fake> {
    Arc::new(Fake { session, running: running.iter().map(|id| rule(id, false)).collect() })
}

fn ids(rules: &[ForwardRule]) -> Vec<&str> {
    let mut ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    ids.sort();
    ids
}

/// Tab 1 and its duplicate (tab 2) share a connection running r1; tab 3 is another
/// connection of the same session; tab 4 one of another session.
fn tabs() -> (Map<Fake>, Arc<Fake>, Arc<Fake>) {
    let shared = connection("web", &["r1"]);
    let other = connection("web", &[]);
    let map = HashMap::from([(1, shared.clone()), (2, shared.clone()), (3, other.clone()), (4, connection("db", &["r9"]))]);
    (map, shared, other)
}

#[test]
fn starts_a_session_s_rules_once() {
    let (map, shared, other) = tabs();
    let rules = [rule("r1", true), rule("r2", true), rule("r3", false), rule("r4", false)];
    // r1 runs on the session's other connection; r4 ran before reconnecting.
    assert_eq!(ids(&rules_to_start(&map, &other, &rules, &["r4".into()])), ["r2", "r4"]);
    assert_eq!(ids(&rules_to_start(&map, &shared, &rules, &[])), ["r1", "r2"]);
    // A quick connection shares rules with nothing.
    let quick = connection("", &[]);
    assert_eq!(ids(&rules_to_start(&HashMap::from([(5, quick.clone()), (6, connection("", &["r1"]))]), &quick, &rules, &[])), ["r1", "r2"]);
}

#[test]
fn offers_to_keep_rules_another_connection_could_take_over() {
    let (map, _, _) = tabs();
    // Closing one of the shared connection's tabs closes nothing.
    assert!(rules_to_keep(&map, &[1]).is_empty());
    // Closing both closes it, and tab 3 stays.
    assert_eq!(ids(&rules_to_keep(&map, &[1, 2])), ["r1"]);
    // Closing every tab of the session leaves nothing to take them over.
    assert!(rules_to_keep(&map, &[1, 2, 3]).is_empty());
    // Another session's tab staying open doesn't count.
    assert!(rules_to_keep(&HashMap::from([(1, connection("web", &["r1"])), (4, connection("db", &[]))]), &[1]).is_empty());
}

#[test]
fn finds_the_connections_that_close() {
    let (map, shared, other) = tabs();
    // Tab 2 still uses the shared connection.
    assert!(closing(&map, &[1, 3]).iter().all(|c| Arc::ptr_eq(c, &other)));
    assert_eq!(closing(&map, &[1, 3]).len(), 1);
    let both = closing(&map, &[1, 2]);
    assert_eq!(both.len(), 1);
    assert!(Arc::ptr_eq(&both[0], &shared));
}

#[test]
fn carries_rules_over_a_reconnection_by_the_last_user() {
    let (mut map, _, _) = tabs();
    // Tab 2 still uses the connection: tab 1 reconnecting takes nothing with it.
    assert!(rules_to_carry(&map, 1).is_empty());
    map.remove(&2);
    assert_eq!(rules_to_carry(&map, 1), ["r1"]);
    assert!(rules_to_carry(&map, 7).is_empty());
}

#[test]
fn hands_rules_over_to_another_connection_of_the_session() {
    let (mut map, shared, other) = tabs();
    map.remove(&1);
    map.remove(&2);
    let (target, rules) = rules_to_adopt(&map, &shared, vec![rule("r1", false)]).unwrap();
    assert!(Arc::ptr_eq(&target, &other));
    assert_eq!(ids(&rules), ["r1"]);
    // None left of the session.
    map.remove(&3);
    assert!(rules_to_adopt(&map, &shared, vec![rule("r1", false)]).is_none());
}
