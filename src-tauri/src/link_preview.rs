//! Tauri commands over `monocode_integrations::link_preview`.
use monocode_integrations::link_preview::{self, LinkPreviewMetadata};

/// Fetch metadata in the native host so the webview's deliberately narrow CSP
/// can remain intact. Every hop is checked before a request is made.
#[tauri::command]
pub async fn fetch_link_preview(url: String) -> Result<LinkPreviewMetadata, String> {
    tauri::async_runtime::spawn_blocking(move || link_preview::fetch_link_preview(url))
        .await
        .map_err(|error| error.to_string())?
}
