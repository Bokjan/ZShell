use serde::{Serialize, Serializer};

use crate::session::SessionId;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session {0} not found")]
    SessionNotFound(SessionId),
    #[error("会话 {0} 尚未建立 SSH 连接")]
    NotConnected(SessionId),
    #[error("profile {0} not found")]
    ProfileNotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
    #[error("credential store: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(format!("{e:#}"))
    }
}

impl From<russh_sftp::client::error::Error> for Error {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        Error::Other(e.to_string())
    }
}

// Commands return errors to the frontend as plain message strings.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
