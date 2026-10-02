//! Tauri commands over `monocode_terminal::pty`.
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

pub(crate) use monocode_terminal::pty::PtyHost;
use monocode_terminal::pty::{self, PtyEvents, PtyStatus};

const DATA_EVENT: &str = "pty-data";
const EXIT_EVENT: &str = "pty-exit";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyData {
    id: String,
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyExit {
    id: String,
    code: Option<i32>,
}

/// Emits terminal output to every webview as base64 text.
pub(crate) struct TauriPtyEvents(pub AppHandle);

impl PtyEvents for TauriPtyEvents {
    fn data(&self, id: &str, bytes: &[u8]) {
        let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
        let _ = self.0.emit(
            DATA_EVENT,
            PtyData {
                id: id.to_string(),
                data,
            },
        );
    }

    fn exit(&self, id: &str, code: Option<i32>) {
        let _ = self.0.emit(
            EXIT_EVENT,
            PtyExit {
                id: id.to_string(),
                code,
            },
        );
    }
}

#[tauri::command]
pub fn pty_spawn(
    host: State<PtyHost>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    pty::pty_spawn(&host, id, cwd, cols, rows)
}

#[tauri::command]
pub fn pty_write(host: State<PtyHost>, id: String, data: String) -> Result<(), String> {
    pty::pty_write(&host, id, data)
}

#[tauri::command]
pub fn pty_resize(host: State<PtyHost>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    pty::pty_resize(&host, id, cols, rows)
}

/// Off the main thread: this forks `ps`, and the title poll calls it once a
/// second for every open terminal.
#[tauri::command(async)]
pub fn pty_status(host: State<'_, PtyHost>, id: String) -> Result<PtyStatus, String> {
    pty::pty_status(&host, id)
}

#[tauri::command]
pub fn pty_kill(host: State<PtyHost>, id: String) -> Result<(), String> {
    pty::pty_kill(&host, id)
}

/// Off the main thread: `kill_all` waits for the shells to die before it
/// returns, and a window close calls this while the app keeps running.
#[tauri::command(async)]
pub fn pty_kill_all(host: State<'_, PtyHost>) -> Result<(), String> {
    pty::pty_kill_all(&host)
}
