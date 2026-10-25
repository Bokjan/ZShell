//! The data as it is saved: the sessions, folders, proxies and forwarding rules, the quick
//! commands and the app lock's settings (all kept by the vault, `crate::vault`), and the
//! app-wide settings (`settings.json`, `crate::settings`), with the checks and tidying done
//! when they are saved or imported.
//!
//! These types depend on nothing but serde, ts-rs and the crate's basic helpers (errors,
//! encodings, printable text): the stores, the import and export, and the features that run
//! what is saved (`ssh`, `forward`, `proxy`, `applock`…) depend on them, never the other way
//! round.

mod app_lock;
mod forward;
mod profile;
mod proxy;
mod quick;
mod settings;
pub mod time_format;

pub use app_lock::AppLockSettings;
#[cfg(test)]
pub(crate) use app_lock::DEFAULT_IDLE_MINUTES;
pub use forward::{host_port, ForwardKind, ForwardRule, ALL_ADDRESSES};
// Written into the frontend's bindings.
#[cfg(test)]
pub use profile::default_term_type;
pub use profile::{
    is_plain_host, local_username, AuthMethod, Connection, EnvVar, FlowControl, Folder, Item, Parity, Profile, Protocol, Remote,
    SerialOptions, SshOptions, SshProfile,
};
#[cfg(windows)]
pub(crate) use proxy::split_program;
pub use proxy::{Proxy, ProxyKind};
pub use quick::QuickCommands;
#[cfg(test)]
pub use quick::DEFAULT_GROUP;
pub use settings::*;
