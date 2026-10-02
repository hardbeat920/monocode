//! Tauri commands over `monocode_store::cursor_store`.
use std::collections::HashMap;

use monocode_store::cursor_store::{self, CursorSubagentRun, CursorToolCall};

/// Cursor's ACP transport omits child interaction updates. The child's own
/// store identifies its parent and spawn call in meta[0].subagentInfo.
#[tauri::command]
pub async fn cursor_subagent_runs(
    session_id: String,
    tool_call_ids: Vec<String>,
    known_revisions: Option<HashMap<String, String>>,
) -> Result<Vec<CursorSubagentRun>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        cursor_store::cursor_subagent_runs(session_id, tool_call_ids, known_revisions)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Recover tool arguments that Cursor currently omits from its ACP events.
///
/// Cursor persists the complete call in a per-session SQLite store before
/// sending the corresponding result. MonoCode only opens that store read-only.
#[tauri::command]
pub async fn cursor_tool_calls(
    session_id: String,
    tool_call_ids: Vec<String>,
) -> Result<Vec<CursorToolCall>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        cursor_store::cursor_tool_calls(session_id, tool_call_ids)
    })
    .await
    .map_err(|e| e.to_string())?
}
