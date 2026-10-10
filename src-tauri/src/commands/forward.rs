//! Commands for port forwarding rules on a connection.

use tauri::State;

use crate::error::Result;
use crate::forward::ForwardRule;
use crate::session::SessionId;
use crate::ssh::Connections;

/// Starts (or restarts with a new definition) a forwarding rule of the session's saved
/// session: where it runs (perhaps another tab's connection), else on the session's
/// connection. Progress is reported to the saved session's tabs as session events.
#[tauri::command]
pub fn forward_start(connections: State<'_, Connections>, id: SessionId, rule: ForwardRule) -> Result<()> {
    connections.start_forward(id, rule.normalize()?)
}

/// Stops a forwarding rule of the session's saved session, wherever it runs.
#[tauri::command]
pub fn forward_stop(connections: State<'_, Connections>, id: SessionId, rule_id: String) -> Result<()> {
    connections.stop_forward(id, &rule_id)
}

/// The forwarding rules that closing sessions `ids` would stop, although another tab of their
/// saved session stays connected and could keep them running.
#[tauri::command]
pub fn forward_keep_candidates(connections: State<'_, Connections>, ids: Vec<SessionId>) -> Vec<ForwardRule> {
    connections.forwards_to_keep(&ids)
}

/// Keeps the forwarding rules of sessions `ids` running on another tab of their saved session
/// once they close.
#[tauri::command]
pub fn forward_keep(connections: State<'_, Connections>, ids: Vec<SessionId>) {
    connections.keep_forwards(&ids);
}

/// The forwarding rules to start again when session `id` reconnects (see [`super::session::SessionSpec::Profile`]).
#[tauri::command]
pub fn forward_carry(connections: State<'_, Connections>, id: SessionId) -> Vec<String> {
    connections.forwards_to_carry(id)
}
