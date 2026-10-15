//! The store of the saved sessions, the folders they are organized in and the proxies they
//! use (their types are in `crate::model`), kept by the vault (`vault/`) with their passwords,
//! which are written with them.
//!
//! The order they are shown in is a list of its own (`order`), where sessions and folders mix:
//! each folder's contents appear in that order. The lists of sessions and of folders are
//! kept in that order too, which is how older versions, which don't read it, show them
//! (subfolders first).

mod store;
#[cfg(test)]
mod tests;

pub use store::{Additions, ProfileStore, Snapshot};
