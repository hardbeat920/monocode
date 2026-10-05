//! Tauri commands over `monocode_platform::pasteboard`.
use monocode_platform::pasteboard;

/// Paths for files copied in a file manager, empty when it holds none.
#[tauri::command(async)]
pub fn clipboard_file_paths() -> Result<Vec<String>, String> {
    pasteboard::clipboard_file_paths()
}

#[tauri::command]
pub fn copy_file_to_clipboard(path: String) -> Result<(), String> {
    pasteboard::copy_file_to_clipboard(path)
}

/// Read an image off the native clipboard as PNG bytes, sent to the webview
/// as an ArrayBuffer.
#[tauri::command]
pub async fn clipboard_image() -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(pasteboard::clipboard_image)
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
