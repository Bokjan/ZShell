//! Terminal sessions.
//!
//! A session is a background task that streams output bytes to the frontend through a
//! [`tauri::ipc::Channel`] and consumes [`SessionInput`] from an mpsc queue. Dropping the
//! queue's sender (via [`SessionManager::remove`]) tells the task to shut down.
//!
//! M0 only ships the [`loopback`] backend; SSH shells (M1) and local PTYs (M5) will be
//! additional backends behind the same interface.

pub mod loopback;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::error::{Error, Result};

pub type SessionId = u32;

#[derive(Debug)]
pub enum SessionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<SessionId, mpsc::UnboundedSender<SessionInput>>>,
}

impl SessionManager {
    /// Allocates a session id and returns the input queue its backend task should consume.
    pub fn register(&self) -> (SessionId, mpsc::UnboundedReceiver<SessionInput>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::unbounded_channel();
        self.sessions.lock().unwrap().insert(id, tx);
        (id, rx)
    }

    pub fn send(&self, id: SessionId, input: SessionInput) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let tx = sessions.get(&id).ok_or(Error::SessionNotFound(id))?;
        // A send error means the backend already exited; it will be removed on close.
        let _ = tx.send(input);
        Ok(())
    }

    pub fn remove(&self, id: SessionId) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .remove(&id)
            .map(drop)
            .ok_or(Error::SessionNotFound(id))
    }
}
