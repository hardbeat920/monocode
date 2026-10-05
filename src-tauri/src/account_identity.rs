//! Tauri commands over `monocode_integrations::account_identity`.
use tauri::AppHandle;

use monocode_integrations::account_identity::{self, ProviderAccountIdentity};

/// Read the signed-in identity a provider CLI already cached on disk, so
/// no token is sent anywhere. Returns `None` when the profile is not signed in.
#[tauri::command]
pub async fn provider_account_identity(
    app: AppHandle,
    provider: String,
    account_id: Option<String>,
) -> Result<Option<ProviderAccountIdentity>, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        account_identity::provider_account_identity(&data_dir, provider, account_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
