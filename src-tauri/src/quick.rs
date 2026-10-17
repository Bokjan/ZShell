//! The store of the quick commands ([`QuickCommands`]), kept by the vault. The frontend edits
//! them as a whole; the backend stores and validates.

use std::sync::Arc;

use crate::error::Result;
use crate::model::QuickCommands;
use crate::vault::{Part, Vault};

pub struct QuickCommandStore {
    /// Read again once the vault is unlocked; the default group alone while it is locked.
    commands: Part<QuickCommands>,
}

impl QuickCommandStore {
    pub fn load(vault: Arc<Vault>) -> Self {
        Self { commands: Part::new(vault, |data| data.commands.clone().normalize()) }
    }

    pub fn get(&self) -> QuickCommands {
        self.commands.read().clone()
    }

    /// Validates, stores and returns the commands as stored (with ids for new ones).
    pub fn set(&self, commands: QuickCommands) -> Result<QuickCommands> {
        let commands = commands.normalize();
        self.commands.update(|part, data| {
            part.clone_from(&commands);
            data.commands.clone_from(&commands);
            Ok(())
        })?;
        Ok(commands)
    }
}
