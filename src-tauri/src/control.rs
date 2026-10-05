//! Tauri commands over `monocode_process::control`. The owner id of a grant is
//! the webview window label.
use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use monocode_process::control::{self as control, ControlEvents, ControlHost, ControlRequest};

const REQUEST_EVENT: &str = "monocode-control-request";

/// Sends each control request to the window that owns the grant.
struct TauriControlEvents(AppHandle);

impl ControlEvents for TauriControlEvents {
    fn request(&self, owner: &str, request: ControlRequest) -> Result<(), String> {
        self.0
            .emit_to(owner, REQUEST_EVENT, request)
            .map_err(|error| error.to_string())
    }
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let host = control::init(Arc::new(TauriControlEvents(app.clone())))?;
    app.manage(host);
    Ok(())
}

#[tauri::command]
pub fn control_enable(
    window: WebviewWindow,
    host: State<'_, ControlHost>,
    session_id: String,
    cwd: String,
) -> Result<String, String> {
    control::control_enable(&host, window.label(), session_id, cwd)
}

#[tauri::command]
pub fn control_disable(
    window: WebviewWindow,
    host: State<'_, ControlHost>,
    session_id: String,
) -> Result<(), String> {
    control::control_disable(&host, window.label(), session_id)
}

#[tauri::command]
pub fn control_attach_worker(
    window: WebviewWindow,
    host: State<'_, ControlHost>,
    lead_id: String,
    session_id: String,
) -> Result<String, String> {
    control::control_attach_worker(&host, window.label(), lead_id, session_id)
}

#[tauri::command]
pub fn control_authorize_turn(
    window: WebviewWindow,
    host: State<'_, ControlHost>,
    session_id: String,
    cwd: String,
    app_access: bool,
) -> Result<(), String> {
    control::control_authorize_turn(&host, window.label(), session_id, cwd, app_access)
}

#[tauri::command]
pub fn control_turn_finished(host: State<'_, ControlHost>, session_id: String) {
    control::control_turn_finished(&host, session_id)
}

pub fn window_closed(app: &AppHandle, label: &str) {
    let host = app.state::<ControlHost>();
    let harness = app.state::<crate::harness::HarnessHost>();
    control::owner_closed(&host, &harness, label);
}

#[tauri::command]
pub fn app_cli_path() -> Result<String, String> {
    control::app_cli_path()
}

#[tauri::command]
pub fn control_reply(
    window: WebviewWindow,
    host: State<'_, ControlHost>,
    id: String,
    response: Value,
) -> Result<(), String> {
    control::control_reply(&host, window.label(), id, response)
}

#[tauri::command]
pub fn control_save(
    store: State<'_, crate::session_store::SessionStore>,
    lead_id: String,
    state: String,
) -> Result<(), String> {
    monocode_store::session_store::control_save(&store, lead_id, state)
}

#[tauri::command]
pub fn control_load(
    store: State<'_, crate::session_store::SessionStore>,
    lead_id: String,
) -> Result<Option<String>, String> {
    monocode_store::session_store::control_load(&store, lead_id)
}

/// Resolve reported writes as well as scopes: aliases and symlinks must not
/// turn a private scratch directory into an exemption for another worker's files.
#[tauri::command]
pub fn control_write_path(path: String) -> Result<String, String> {
    control::control_write_path(path)
}

#[tauri::command]
pub fn control_scopes(cwd: String, files: Vec<String>) -> Result<Vec<String>, String> {
    control::control_scopes(cwd, files)
}
