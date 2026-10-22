//! Linux: the connections to the system and session buses, made on first use and kept for as
//! long as the app runs, which the app lock and keeping the system awake share.

use std::sync::OnceLock;

use zbus::blocking::Connection;

/// The system bus, or why there is none.
pub fn system() -> Result<&'static Connection, &'static str> {
    static BUS: OnceLock<Result<Connection, String>> = OnceLock::new();
    BUS.get_or_init(|| Connection::system().map_err(|e| e.to_string())).as_ref().map_err(String::as_str)
}

/// The session bus, or why there is none.
pub fn session() -> Result<&'static Connection, &'static str> {
    static BUS: OnceLock<Result<Connection, String>> = OnceLock::new();
    BUS.get_or_init(|| Connection::session().map_err(|e| e.to_string())).as_ref().map_err(String::as_str)
}
