//! The connections that rules released as a shell exits still carry (see
//! [`Forwards::release_all`](super::Forwards::release_all)).

use std::task::Poll;

use tauri::async_runtime::JoinHandle;
use tokio::task::JoinSet;

use super::describe;
use crate::model::ForwardRule;

/// The connections a released rule still carries, which go on until they end; dropping it
/// ends them.
#[derive(Default)]
pub struct InFlight(pub(super) JoinSet<anyhow::Result<()>>);

/// The connections that released rules still carry, with the rules, until they end.
#[derive(Default)]
pub struct Draining(Vec<(ForwardRule, InFlight)>);

impl Draining {
    /// Waits for the tasks of released rules, which end once they have let go of their ports
    /// and the server's listeners, and takes the connections they still carry.
    pub async fn collect(tasks: Vec<(ForwardRule, JoinHandle<InFlight>)>) -> Self {
        let mut draining = Vec::new();
        for (rule, task) in tasks {
            if let Ok(in_flight) = task.await {
                draining.push((rule, in_flight));
            }
        }
        Self(draining)
    }

    /// How many connections are still open.
    pub fn len(&self) -> usize {
        self.0.iter().map(|(_, in_flight)| in_flight.0.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The rules still carrying connections, OpenSSH style (`-L 127.0.0.1:8080:db:5432`).
    pub fn rules(&self) -> Vec<String> {
        let mut rules = Vec::new();
        for (rule, in_flight) in &self.0 {
            let rule = describe(rule);
            if !in_flight.0.is_empty() && !rules.contains(&rule) {
                rules.push(rule);
            }
        }
        rules
    }

    /// Adds the connections of `other`, forgetting those that have ended meanwhile.
    pub fn merge(&mut self, other: Draining) {
        self.0.extend(other.0);
        for (_, in_flight) in &mut self.0 {
            while in_flight.0.try_join_next().is_some() {}
        }
        self.0.retain(|(_, in_flight)| !in_flight.0.is_empty());
    }

    /// Completes once one of the connections has ended; pending forever while none is open.
    pub async fn next_ended(&mut self) {
        std::future::poll_fn(|cx| {
            for (_, in_flight) in &mut self.0 {
                if let Poll::Ready(Some(_)) = in_flight.0.poll_join_next(cx) {
                    return Poll::Ready(());
                }
            }
            Poll::Pending
        })
        .await;
    }
}
