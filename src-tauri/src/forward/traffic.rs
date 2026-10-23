//! What a run of a rule carries, for its lights and speed in every tab of the session (see
//! `crate::traffic`): the bytes through the channels of its connections, sent being what goes
//! to the server, and which channels those are.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::session::{ForwardTraffic, FORWARD_CHANNELS_LISTED};
use crate::traffic::{ConnectionInfo, Meter, Wake};

pub(crate) struct RuleTraffic {
    pub(super) meter: Arc<Meter>,
    channels: Mutex<BTreeSet<u32>>,
    pub(super) wake: Wake,
}

impl RuleTraffic {
    pub(super) fn new() -> Self {
        let wake = Wake::default();
        Self { meter: Arc::new(Meter::new(&wake)), channels: Mutex::default(), wake }
    }

    /// Notes that `channel` carries a connection of the rule, until the guard is dropped.
    pub(super) fn carrying(self: &Arc<Self>, channel: u32) -> Carrying {
        self.channels.lock().unwrap().insert(channel);
        self.wake.wake();
        Carrying { traffic: self.clone(), channel }
    }

    pub(super) fn report(&self, connection: ConnectionInfo) -> ForwardTraffic {
        let channels = self.channels.lock().unwrap();
        ForwardTraffic {
            counts: self.meter.counts(),
            connection,
            channels: channels.iter().take(FORWARD_CHANNELS_LISTED).copied().collect(),
            channel_count: channels.len(),
        }
    }
}

pub(super) struct Carrying {
    traffic: Arc<RuleTraffic>,
    channel: u32,
}

impl Drop for Carrying {
    fn drop(&mut self) {
        self.traffic.channels.lock().unwrap().remove(&self.channel);
        self.traffic.wake.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_lowest_channels_carried() {
        let traffic = Arc::new(RuleTraffic::new());
        let carried: Vec<_> = (0..30).rev().map(|channel| traffic.carrying(channel)).collect();
        traffic.meter.sent(4);
        let connection = ConnectionInfo { id: 3, local_port: Some(50000) };
        let report = traffic.report(connection);
        assert_eq!(report.channels, (0..20).collect::<Vec<_>>());
        assert_eq!((report.channel_count, report.counts.sent), (30, 4));
        drop(carried);
        assert_eq!(traffic.report(connection).channel_count, 0);
    }
}
