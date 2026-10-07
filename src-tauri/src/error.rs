use serde::{Serialize, Serializer};

use crate::session::SessionId;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session {0} not found")]
    SessionNotFound(SessionId),
}

// Commands return errors to the frontend as plain message strings.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
