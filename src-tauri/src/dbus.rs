//! Linux: the connections to the system and session buses, made on first use and kept for as
//! long as the app runs, which the app lock and keeping the system awake share.

use std::sync::OnceLock;
use std::time::Duration;

use zbus::blocking::connection::Builder;
use zbus::blocking::Connection;

/// How long a method call may wait for its answer, as libdbus waits by default: zbus would
/// wait for ever, so a service that hangs (gnome-session, PowerDevil) would hold up the thread
/// that keeps the system awake, which then couldn't let it sleep either.
const METHOD_TIMEOUT: Duration = Duration::from_secs(25);

/// The system bus, or why there is none.
pub fn system() -> Result<&'static Connection, &'static str> {
    static BUS: OnceLock<Result<Connection, String>> = OnceLock::new();
    BUS.get_or_init(|| connect(Builder::system())).as_ref().map_err(String::as_str)
}

/// The session bus, or why there is none.
pub fn session() -> Result<&'static Connection, &'static str> {
    static BUS: OnceLock<Result<Connection, String>> = OnceLock::new();
    BUS.get_or_init(|| connect(Builder::session())).as_ref().map_err(String::as_str)
}

fn connect(builder: zbus::Result<Builder<'static>>) -> Result<Connection, String> {
    builder.and_then(|builder| builder.method_timeout(METHOD_TIMEOUT).build()).map_err(|e| e.to_string())
}
