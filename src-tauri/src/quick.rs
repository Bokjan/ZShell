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
        Self { commands: Part::new(vault, |data| Self::repaired(data.commands.clone())) }
    }

    /// As read: those without an id or with one used before (hand edits) get one made from
    /// their position, which reading again gives too, rather than new ones each time that
    /// sessions' `command_group` wouldn't find. Stored once the user changes the commands.
    fn repaired(mut commands: QuickCommands) -> QuickCommands {
        crate::persist::unique_ids(commands.ids_mut());
        commands.normalize()
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn repairs_ids_the_same_way_each_time() {
        let read = || {
            let commands: QuickCommands = serde_json::from_str(
                r#"{"groups":[
                    {"name":"Docker","commands":[{"name":"ps","text":"docker ps"},{"id":"a","name":"top","text":"top"}]},
                    {"id":"a","name":"Logs","commands":[]}
                ]}"#,
            )
            .unwrap();
            QuickCommandStore::repaired(commands)
        };
        let (first, again) = (read(), read());
        assert_eq!(first, again);
        let ids: Vec<&str> =
            first.groups.iter().map(|g| g.id.as_str()).chain(first.groups[1].commands.iter().map(|c| c.id.as_str())).collect();
        assert_eq!(ids.len(), 5);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 5);
    }
}
