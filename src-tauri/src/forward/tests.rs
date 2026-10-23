use super::testing::{free_port, local_rule, until, Tab};
use super::*;
use crate::ssh::testing::{connect, Anyone};

/// The forwards of a new connection, and a tab of its session watching them.
async fn forwards() -> (Forwards, Tab) {
    let connection = connect(Anyone, "web").await;
    let forwards = Forwards::new(&connection, Arc::new(Hub::default()));
    let tab = Tab::new();
    forwards.attach(1, tab.io.sink());
    (forwards, tab)
}

/// Whether the task of rule `rule_id` has ended.
fn ended(forwards: &Forwards, rule_id: &str) -> bool {
    forwards.0.running.lock().unwrap().get(rule_id).is_some_and(|r| r.task.inner().is_finished())
}

#[tokio::test]
async fn starts_and_stops_a_rule() {
    let (forwards, tab) = forwards().await;
    forwards.start(local_rule("r1", 0), false);
    assert_eq!(tab.next_kind("r1").await, "starting");
    let active = tab.next_state("r1").await;
    assert_eq!(active["type"], "active");
    assert!(active["bound"].as_str().unwrap().starts_with("127.0.0.1:"));
    assert!(forwards.has("r1") && forwards.is_running("r1"));
    assert_eq!(forwards.running().len(), 1);

    forwards.stop("r1");
    assert_eq!(tab.next_kind("r1").await, "stopped");
    assert!(!forwards.has("r1") && !forwards.is_running("r1"));
    assert!(forwards.running().is_empty());
}

/// What a rule carries is counted, with the channels it is on, until the rule stops.
#[tokio::test]
async fn reports_what_a_rule_carries() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let connection = connect(crate::ssh::testing::Echoing, "web").await;
    let forwards = Forwards::new(&connection, Arc::new(Hub::default()));
    let port = free_port();
    forwards.start(local_rule("r1", port), false);
    until("the rule listens", || {
        forwards.0.hub.0.lock().unwrap().states.get("r1").is_some_and(|s| matches!(s, ForwardState::Active { .. }))
    })
    .await;
    let traffic = || forwards.0.hub.0.lock().unwrap().traffic.get("r1").cloned();
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    let mut echoed = [0; 5];
    client.read_exact(&mut echoed).await.unwrap();
    until("both ways are counted", || traffic().is_some_and(|t| (t.counts.sent, t.counts.received) == (5, 5))).await;
    let carried = traffic().unwrap();
    assert_eq!((carried.connection, carried.channel_count, carried.channels.len()), (connection.info(), 1, 1));

    drop(client);
    until("the channel is let go", || traffic().is_some_and(|t| t.channel_count == 0 && t.channels.is_empty())).await;
    forwards.stop("r1");
    assert!(traffic().is_none());
}

/// A rule started again on the same port: the new run binds it once the old one has let
/// it go, and the old one's reports no longer count.
#[tokio::test]
async fn restarting_a_rule_reuses_its_port() {
    let (forwards, tab) = forwards().await;
    let rule = local_rule("r1", free_port());
    forwards.start(rule.clone(), false);
    assert_eq!(tab.next_kind("r1").await, "starting");
    let first = tab.next_state("r1").await;
    assert_eq!(first["type"], "active");

    forwards.start(rule, false);
    assert_eq!(tab.next_kind("r1").await, "starting");
    let second = tab.next_state("r1").await;
    assert_eq!(second["type"], "active", "{second}");
    assert_eq!(second["bound"], first["bound"]);
    assert_eq!(forwards.running().len(), 1);

    // The replaced run's generation reports nothing; the current one's does.
    let ctx = |generation| Ctx { shared: forwards.0.clone(), rule_id: "r1".into(), generation, traffic: Arc::new(RuleTraffic::new()) };
    let current = forwards.0.next_generation.load(Ordering::Relaxed) - 1;
    assert!(tab.quiet());
    ctx(current - 1).report(ForwardState::Stopped);
    assert!(tab.quiet());
    ctx(current).report(ForwardState::Stopped);
    assert_eq!(tab.next_kind("r1").await, "stopped");
}

/// Each start takes a new generation, and a stopped rule's run reports nothing more.
#[tokio::test]
async fn a_stopped_run_no_longer_reports() {
    let (forwards, tab) = forwards().await;
    forwards.start(local_rule("r1", 0), false);
    forwards.start(local_rule("r2", 0), false);
    assert_eq!(forwards.0.next_generation.load(Ordering::Relaxed), 2);
    assert_eq!(tab.next_kind("r1").await, "starting");
    assert_eq!(tab.next_kind("r1").await, "active");

    forwards.stop("r1");
    assert_eq!(tab.next_kind("r1").await, "stopped");
    Ctx { shared: forwards.0.clone(), rule_id: "r1".into(), generation: 0, traffic: Arc::new(RuleTraffic::new()) }
        .report(ForwardState::Starting);
    until("the stopped run's end", || ended(&forwards, "r1")).await;
    // Its run has ended: nothing more can come.
    assert_eq!(tab.reported("r1"), Vec::<String>::new());
    assert!(forwards.running().iter().all(|rule| rule.id == "r2"));
}

/// Once the connection closes, its rules stop and none starts again.
#[tokio::test]
async fn nothing_starts_after_stop_all() {
    let (forwards, tab) = forwards().await;
    forwards.start(local_rule("r1", 0), false);
    assert_eq!(tab.next_kind("r1").await, "starting");
    assert_eq!(tab.next_kind("r1").await, "active");

    let (rules, tasks) = forwards.stop_all();
    assert_eq!(rules.iter().map(|rule| rule.id.as_str()).collect::<Vec<_>>(), ["r1"]);
    assert_eq!(tab.next_kind("r1").await, "stopped");
    for task in tasks {
        task.await.unwrap();
    }

    forwards.start(local_rule("r1", 0), false);
    assert_eq!(tab.next_kind("r1").await, "stopped");
    assert!(tab.quiet());
    assert!(!forwards.has("r1") && forwards.running().is_empty());
    assert!(forwards.0.running.lock().unwrap().is_empty());
}

/// A rule that failed to start is still had (until stopped), but no longer runs.
#[tokio::test]
async fn a_failed_rule_is_not_running() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let (forwards, tab) = forwards().await;
    forwards.start(local_rule("r1", port), false);
    assert_eq!(tab.next_kind("r1").await, "starting");
    let failed = tab.next_state("r1").await;
    assert_eq!(failed["type"], "failed", "{failed}");
    until("the failed run's end", || ended(&forwards, "r1")).await;
    assert!(forwards.has("r1"));
    assert!(!forwards.is_running("r1"));
    assert!(forwards.running().is_empty());
}

/// A server that answers no request for a `direct-tcpip` channel by itself, as one waiting
/// to connect to a target that drops packets: each request's channel and reply go to the
/// test.
struct Unanswering(tokio::sync::mpsc::UnboundedSender<(Channel<russh::server::Msg>, russh::server::ChannelOpenHandle)>);

impl russh::server::Handler for Unanswering {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<russh::server::Msg>,
        _host: &str,
        _port: u32,
        _originator: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.0.send((channel, reply));
        Ok(())
    }
}

/// A channel the server doesn't open in time is given up on, and closed if the server
/// opens it after all.
#[tokio::test(start_paused = true)]
async fn gives_up_on_a_channel_the_server_does_not_open_in_time() {
    let (requests, mut requested) = tokio::sync::mpsc::unbounded_channel();
    let connection = connect(Unanswering(requests), "").await;
    let opener =
        Opener { handle: connection.handle().clone(), liveness: connection.liveness().clone(), traffic: Arc::new(RuleTraffic::new()) };
    let opened = opener.open("db".into(), 5432, SocketAddr::from(([127, 0, 0, 1], 50000))).await;
    let Err(error) = opened else { panic!("the channel opened") };
    assert_eq!(error.downcast_ref::<Error>().map(Error::code), Some("forward.channelTimeout"), "{error:#}");

    let (mut channel, reply) = requested.recv().await.unwrap();
    reply.accept().await;
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
    });
    assert!(closed.await.is_ok(), "the channel is still open");
}

/// A client sending through a carried connection once the SSH connection has gone dead
/// ends it soon, rather than when keepalives give up (an hour away here).
#[tokio::test]
async fn sending_on_a_dead_connection_ends_it() {
    use tokio::io::AsyncWriteExt;

    let (connection, stall) =
        crate::ssh::testing::connect_stalling(crate::ssh::testing::Accepting::default(), Duration::from_secs(3600)).await;
    let opener =
        Opener { handle: connection.handle().clone(), liveness: connection.liveness().clone(), traffic: Arc::new(RuleTraffic::new()) };
    let channel = opener.open("db".into(), 5432, SocketAddr::from(([127, 0, 0, 1], 50000))).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
    let (ours, _) = listener.accept().await.unwrap();
    tokio::spawn(bridge(ours, channel, opener.liveness.clone(), opener.traffic.clone()));
    // Longer than the check of opening the channel takes, in tests.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let mut reason = connection.disconnect_reason();
    stall.now();
    client.write_all(b"SELECT 1;").await.unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), reason.wait_for(Option::is_some)).await;
    assert!(ended.is_ok(), "the connection is still open");
}

/// The first change is reported at once; those right after it together, once the
/// interval has passed.
#[tokio::test(start_paused = true)]
async fn reports_changes_at_most_once_an_interval() {
    let mut throttle = Throttle::default();
    let start = Instant::now();
    assert!(throttle.changed(start));
    assert_eq!(throttle.due(), None);
    assert!(!throttle.changed(start + Duration::from_millis(10)));
    assert!(!throttle.changed(start + Duration::from_millis(50)));
    assert_eq!(throttle.due(), Some(start + REPORT_INTERVAL));
    throttle.reported(start + REPORT_INTERVAL);
    assert_eq!(throttle.due(), None);
    // After a quiet interval, at once again.
    assert!(throttle.changed(start + REPORT_INTERVAL * 3));
}
