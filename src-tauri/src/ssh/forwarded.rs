//! The `forwarded-tcpip` channels the server opens for remote forwards (`-R`), handed by the
//! client handler to whichever rule asked the server to listen on that address.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::client::{ChannelOpenHandle, Msg};
use russh::Channel;
use tokio::sync::mpsc;

/// A connection the server accepted on a remote forward, not yet confirmed.
pub struct Incoming {
    pub channel: Channel<Msg>,
    /// Dropping it rejects the channel.
    pub reply: ChannelOpenHandle,
}

/// Remote forwards active on a connection, keyed by the address and port the server
/// listens on (see `crate::forward`, which adds and removes them).
#[derive(Clone, Default)]
pub struct RemoteRoutes(Arc<Mutex<HashMap<(String, u32), Route>>>);

/// Where the channels for one remote forward go.
pub type Route = mpsc::UnboundedSender<Incoming>;

impl RemoteRoutes {
    /// Delivers a `forwarded-tcpip` channel to its rule; unknown ones are rejected.
    pub fn route(&self, address: &str, port: u32, incoming: Incoming) {
        if let Some(rule) = find_route(&self.0.lock().unwrap(), address, port) {
            let _ = rule.send(incoming);
        }
    }

    /// Sends the channels for `address:port` to `route` from now on.
    pub fn insert(&self, address: String, port: u32, route: Route) {
        self.0.lock().unwrap().insert((address, port), route);
    }

    pub fn remove(&self, address: &str, port: u32) {
        self.0.lock().unwrap().remove(&(address.to_owned(), port));
    }
}

/// The route for a channel. Servers normally echo the requested address (OpenSSH does), but
/// one that spells it differently (`localhost` for `127.0.0.1`, say) is matched on the port
/// alone, as long as only one rule listens on it: with two, the connection may be meant for
/// either, and is rejected rather than sent to the wrong one.
fn find_route<'a, R>(routes: &'a HashMap<(String, u32), R>, address: &str, port: u32) -> Option<&'a R> {
    if let Some(route) = routes.get(&(address.to_owned(), port)) {
        return Some(route);
    }
    let mut on_port = routes.iter().filter(|((_, p), _)| *p == port).map(|(_, route)| route);
    match (on_port.next(), on_port.next()) {
        (Some(route), None) => Some(route),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_by_address_then_by_port_alone_when_unambiguous() {
        let mut routes = HashMap::new();
        routes.insert(("127.0.0.1".to_owned(), 9000), "loopback");
        assert_eq!(find_route(&routes, "127.0.0.1", 9000), Some(&"loopback"));
        assert_eq!(find_route(&routes, "localhost", 9000), Some(&"loopback"));
        assert_eq!(find_route(&routes, "127.0.0.1", 9001), None);

        routes.insert(("10.0.0.1".to_owned(), 9000), "lan");
        assert_eq!(find_route(&routes, "10.0.0.1", 9000), Some(&"lan"));
        assert_eq!(find_route(&routes, "localhost", 9000), None);
    }
}
