//! Commands for port forwarding rules on a connection.

use tauri::{State, Webview};

use crate::connections::Connections;
use crate::error::Result;
use crate::model::ForwardRule;
use crate::session::{SessionId, SessionManager};

/// Starts (or restarts with a new definition) a forwarding rule of the session's saved
/// session: where it runs (perhaps another tab's connection), else on the session's
/// connection. Progress is reported to the saved session's tabs as session events.
#[tauri::command]
pub fn forward_start(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    rule: ForwardRule,
) -> Result<()> {
    sessions.check(webview.label(), id)?;
    connections.start_forward(id, rule.normalize()?)
}

/// Stops a forwarding rule of the session's saved session, wherever it runs.
#[tauri::command]
pub fn forward_stop(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    rule_id: String,
) -> Result<()> {
    sessions.check(webview.label(), id)?;
    connections.stop_forward(id, &rule_id)
}

/// The forwarding rules that closing sessions `ids` (this window's) would stop, although
/// another tab of their saved session stays connected and could keep them running.
#[tauri::command]
pub fn forward_keep_candidates(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    mut ids: Vec<SessionId>,
) -> Vec<ForwardRule> {
    ids.retain(|&id| sessions.owns(webview.label(), id));
    connections.forwards_to_keep(&ids)
}

/// Keeps the forwarding rules of sessions `ids` (this window's) running on another tab of
/// their saved session once they close.
#[tauri::command]
pub fn forward_keep(webview: Webview, sessions: State<'_, SessionManager>, connections: State<'_, Connections>, mut ids: Vec<SessionId>) {
    ids.retain(|&id| sessions.owns(webview.label(), id));
    connections.keep_forwards(&ids);
}

/// The forwarding rules to start again when session `id` reconnects (see [`super::session::SessionSpec::Profile`]).
#[tauri::command]
pub fn forward_carry(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
) -> Vec<String> {
    match sessions.owns(webview.label(), id) {
        true => connections.forwards_to_carry(id),
        false => Vec::new(),
    }
}
