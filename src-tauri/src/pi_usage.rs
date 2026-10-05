//! Tauri commands over `monocode_integrations::pi_usage`.
use monocode_integrations::pi_usage::{self, PiUsageProvider, PiUsageResult};

#[tauri::command]
pub async fn fetch_pi_usage(provider: PiUsageProvider) -> PiUsageResult {
    tauri::async_runtime::spawn_blocking(move || pi_usage::fetch_pi_usage(provider))
        .await
        .unwrap_or_else(|_| pi_usage::fetch_pi_usage_failed())
}
