//! Tauri commands over `monocode_integrations::inbox_media`.
use monocode_integrations::inbox_media;

/// Fetch an issue/PR image or video through the host, as a blob the webview
/// can render without opening `img-src` / `media-src` to GitHub's CDNs.
#[tauri::command]
pub async fn fetch_inbox_media(url: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || inbox_media::fetch_inbox_media(url))
        .await
        .map_err(|error| error.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}
