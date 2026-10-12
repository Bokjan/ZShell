//! Saved connection profiles, the folders they are organized in and the proxies they use,
//! kept by the vault (`vault/`: in `profiles.json`, `folders.json` and `proxies.json`, or
//! encrypted in `vault.json`). Their passwords are kept apart; see [`crate::secrets`].
//!
//! The order of each list is the order shown: a folder's subfolders and sessions appear in
//! file order (subfolders first).

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
