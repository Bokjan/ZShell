//! Quick commands: named texts sent to the terminal from a button, the terminal's menu or the
//! command palette, kept in groups in `commands.json` next to `profiles.json`. The frontend
//! edits them as a whole; the backend stores and validates.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::{load_json, write_json_atomic, SetAside};
use crate::error::Result;

/// The group that always exists, first, for commands not put in a group of their own. Its
/// name is shown translated, so it is stored empty.
pub const DEFAULT_GROUP: &str = "default";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuickCommands {
    pub groups: Vec<CommandGroup>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub commands: Vec<QuickCommand>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommand {
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Sent as typed; line breaks are Enter.
    pub text: String,
    /// Press Enter after the text; otherwise it is left on the command line to be finished.
    #[serde(default = "default_true")]
    pub enter: bool,
}

fn default_true() -> bool {
    true
}

impl QuickCommands {
    /// Gives new groups and commands ids (and fixes duplicates from hand edits), and puts the
    /// default group first, creating it if needed.
    fn normalize(mut self) -> Self {
        let mut ids = HashSet::new();
        let mut fresh = |id: &mut String| {
            if id.is_empty() || !ids.insert(id.clone()) {
                *id = uuid::Uuid::new_v4().to_string();
                ids.insert(id.clone());
            }
        };
        for group in &mut self.groups {
            if group.id != DEFAULT_GROUP {
                fresh(&mut group.id);
            }
            group.name = group.name.trim().to_owned();
            for command in &mut group.commands {
                fresh(&mut command.id);
                command.name = command.name.trim().to_owned();
            }
        }
        // Several default groups (hand edits) merge into the first.
        let mut default = CommandGroup { id: DEFAULT_GROUP.to_owned(), name: String::new(), commands: Vec::new() };
        self.groups.retain_mut(|group| {
            if group.id != DEFAULT_GROUP {
                return true;
            }
            default.commands.append(&mut group.commands);
            false
        });
        self.groups.insert(0, default);
        self
    }
}

pub struct QuickCommandStore {
    path: PathBuf,
    commands: Mutex<QuickCommands>,
}

impl QuickCommandStore {
    /// A missing or unreadable file (see [`load_json`]) starts with just the empty default
    /// group.
    pub fn load(path: PathBuf, set_aside: &SetAside) -> Self {
        let commands = load_json::<QuickCommands>(&path, set_aside).normalize();
        Self { path, commands: Mutex::new(commands) }
    }

    pub fn get(&self) -> QuickCommands {
        self.commands.lock().unwrap().clone()
    }

    /// Validates, stores and returns the commands as stored (with ids for new ones).
    pub fn set(&self, commands: QuickCommands) -> Result<QuickCommands> {
        let commands = commands.normalize();
        let mut current = self.commands.lock().unwrap();
        write_json_atomic(&self.path, &commands)?;
        *current = commands.clone();
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_one_default_group_first_and_assigns_ids() {
        let commands: QuickCommands = serde_json::from_str(
            r#"{"groups":[
                {"name":"Docker","commands":[{"name":"ps","text":"docker ps"}]},
                {"id":"default","commands":[{"id":"a","name":"top","text":"top"}]},
                {"id":"default","commands":[{"id":"a","name":"df","text":"df -h","enter":false}]}
            ]}"#,
        )
        .unwrap();
        let commands = commands.normalize();
        assert_eq!(commands.groups.len(), 2);
        let default = &commands.groups[0];
        assert_eq!(default.id, DEFAULT_GROUP);
        assert_eq!(default.commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["top", "df"]);
        assert_ne!(default.commands[0].id, default.commands[1].id);
        assert!(!default.commands[1].enter);
        let docker = &commands.groups[1];
        assert!(!docker.id.is_empty() && docker.commands[0].enter);
        assert!(!docker.commands[0].id.is_empty());
    }
}
