//! Quick commands: named texts sent to the terminal from a button, the terminal's menu or the
//! command palette, kept in groups by the vault (see `crate::quick`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The group that always exists, first, for commands not put in a group of their own. Its
/// name is shown translated, so it is stored empty.
pub const DEFAULT_GROUP: &str = "default";

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct QuickCommands {
    pub groups: Vec<CommandGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroup {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub commands: Vec<QuickCommand>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
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
    /// The ids of the groups other than the default one, and of the commands, in order.
    pub(crate) fn ids_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.groups.iter_mut().flat_map(|CommandGroup { id, commands, .. }| {
            let id = (id != DEFAULT_GROUP).then_some(id);
            id.into_iter().chain(commands.iter_mut().map(|command| &mut command.id))
        })
    }

    /// Gives new groups and commands ids (and fixes duplicates from hand edits), and puts the
    /// default group first, creating it if needed.
    pub(crate) fn normalize(mut self) -> Self {
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
