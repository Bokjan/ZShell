//! Saved connection profiles, the folders they are organized in and the proxies they use,
//! kept by the vault (`vault/`) with their passwords, which are written with them.
//!
//! The order they are shown in is a list of its own (`order`), where sessions and folders mix:
//! each folder's contents appear in that order. The lists of sessions and of folders are
//! kept in that order too, which is how older versions, which don't read it, show them
//! (subfolders first).

mod model;
mod store;
#[cfg(test)]
mod tests;

pub use model::{
    local_username, AuthMethod, Connection, EnvVar, FlowControl, Folder, Item, Parity, Profile, Protocol, Remote, SerialOptions,
    SshOptions, SshProfile,
};
// Written into the frontend's bindings.
#[cfg(test)]
pub use model::default_term_type;
pub use store::{Additions, ProfileStore, Snapshot};
