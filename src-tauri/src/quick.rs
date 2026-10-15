//! The store of the quick commands ([`QuickCommands`]), kept by the vault. The frontend edits
//! them as a whole; the backend stores and validates.

use std::sync::{Arc, Mutex};

use crate::error::Result;
use crate::model::QuickCommands;
use crate::vault::Vault;

pub struct QuickCommandStore {
    /// Where they are kept; empty while it is locked, and loaded again once unlocked.
    vault: Arc<Vault>,
    commands: Mutex<QuickCommands>,
}

impl QuickCommandStore {
    /// A missing or unreadable file (see `JsonFile::load`) starts with just the empty
    /// default group.
    pub fn load(vault: Arc<Vault>) -> Self {
        let commands = Self::read(&vault);
        Self { vault, commands: Mutex::new(commands) }
    }

    /// Takes what the vault has again, once it is unlocked or reset.
    pub fn reload(&self) {
        *self.commands.lock().unwrap() = Self::read(&self.vault);
    }

    fn read(vault: &Vault) -> QuickCommands {
        vault.data().map(|data| data.commands).unwrap_or_default().normalize()
    }

    pub fn get(&self) -> QuickCommands {
        self.commands.lock().unwrap().clone()
    }

    /// Validates, stores and returns the commands as stored (with ids for new ones).
    pub fn set(&self, commands: QuickCommands) -> Result<QuickCommands> {
        let commands = commands.normalize();
        let mut current = self.commands.lock().unwrap();
        self.vault.update(|data| {
            data.commands.clone_from(&commands);
            Ok(())
        })?;
        *current = commands.clone();
        Ok(commands)
    }
}
