//! Tauri commands over `monocode_platform::notifications`. A click on a
//! reminder notification opens the reminder; any other click is emitted to
//! every window as `monocode:notification-click`.
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use monocode_platform::notifications::{self, ClickHandler, Permission};

/// Emitted to every window when the user clicks a notification. Payload is
/// the session id; the window that owns that session handles it.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub const CLICK_EVENT: &str = "monocode:notification-click";

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn handle_click(app: &AppHandle, identifier: &str) {
    if let Some(reminder) = identifier.strip_prefix(crate::reminders::NOTIFICATION_PREFIX) {
        crate::reminders::open_from_notification(app, reminder);
    } else {
        let _ = app.emit(CLICK_EVENT, identifier);
    }
}

fn click_handler(app: &AppHandle) -> ClickHandler {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let app = app.clone();
        Arc::new(move |identifier: &str| handle_click(&app, identifier))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = app;
        Arc::new(|_: &str| {})
    }
}

fn app_id(app: &AppHandle) -> String {
    app.config().identifier.clone()
}

#[cfg(target_os = "macos")]
pub fn install_delegate(app: &AppHandle) {
    notifications::install_delegate(click_handler(app));
}

#[tauri::command]
pub async fn notification_permission(app: AppHandle) -> Permission {
    let app_id = app_id(&app);
    tauri::async_runtime::spawn_blocking(move || notifications::notification_permission(&app_id))
        .await
        .unwrap_or_else(|_| notifications::permission_fallback())
}

#[tauri::command]
pub async fn request_notification_permission(app: AppHandle) -> Permission {
    let app_id = app_id(&app);
    tauri::async_runtime::spawn_blocking(move || {
        notifications::request_notification_permission(&app_id)
    })
    .await
    .unwrap_or_else(|_| notifications::permission_fallback())
}

/// Resolves only once the platform reports the banner as scheduled: the
/// frontend skips its own turn-finished cue on success, so returning early
/// would silence a turn that never got a notification.
#[tauri::command]
pub async fn show_notification(
    app: AppHandle,
    session_id: String,
    title: String,
    subtitle: String,
    body: String,
    sound: bool,
) -> Result<(), String> {
    let app_id = app_id(&app);
    let on_click = click_handler(&app);
    tauri::async_runtime::spawn_blocking(move || {
        notifications::show_notification(
            &app_id,
            &on_click,
            &session_id,
            &title,
            &subtitle,
            &body,
            sound,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Opens the app's page in the OS notification settings, where the user can
/// re-enable alerts after declining the prompt.
#[tauri::command]
pub fn open_notification_settings(app: AppHandle) -> Result<(), String> {
    notifications::open_notification_settings(&app_id(&app))
}
