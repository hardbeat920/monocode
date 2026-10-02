//! Tauri commands over `monocode_store::reminders`, plus the window choice
//! for opening a reminder and the poller thread.
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::session_store::{SessionStore, TauriStoreEvents};
use monocode_store::reminders::{
    self, DeliveryPreferences, OpenReminder, Reminder, ReminderService,
};
use monocode_store::session_store::validate_id;

pub(crate) const CHANGED: &str = "monocode:reminders-changed";
const OPEN: &str = "monocode:reminder-open";
pub(crate) use monocode_store::reminders::NOTIFICATION_PREFIX;

#[tauri::command(async)]
pub fn reminder_list(store: State<'_, SessionStore>) -> Result<Vec<Reminder>, String> {
    reminders::reminder_list(&store)
}

#[tauri::command(async)]
pub fn reminder_set(
    store: State<'_, SessionStore>,
    app: AppHandle,
    session_ids: Vec<String>,
    due_at: i64,
) -> Result<(), String> {
    reminders::reminder_set(&store, &TauriStoreEvents(app.clone()), session_ids, due_at)
}

#[tauri::command(async)]
pub fn reminder_clear(
    store: State<'_, SessionStore>,
    app: AppHandle,
    session_ids: Vec<String>,
    expected_due_at: Option<i64>,
) -> Result<(), String> {
    reminders::reminder_clear(
        &store,
        &TauriStoreEvents(app.clone()),
        session_ids,
        expected_due_at,
    )
}

#[tauri::command]
pub fn reminder_configure(
    service: State<'_, ReminderService>,
    preferences: DeliveryPreferences,
) -> Result<(), String> {
    reminders::reminder_configure(&service, preferences)
}

#[tauri::command]
pub fn reminder_register_window(
    window: WebviewWindow,
    service: State<'_, ReminderService>,
    session_ids: Vec<String>,
) -> Result<(), String> {
    reminders::reminder_register_window(&service, window.label(), session_ids)
}

#[tauri::command]
pub fn reminder_open(app: AppHandle, session_id: String, due_at: i64) -> Result<(), String> {
    validate_id(&session_id, "session")?;
    queue_open(&app, session_id, due_at)
}

#[tauri::command]
pub fn reminder_take_open(
    app: AppHandle,
    window: WebviewWindow,
    service: State<'_, ReminderService>,
) -> Result<Option<OpenReminder>, String> {
    reminders::reminder_take_open(&service, window.label(), |label| {
        app.get_webview_window(label).is_some()
    })
}

/// A notification can outlive its window. Keep the request until the chosen
/// window has mounted and attached its listeners, then let only that window act.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) fn open_from_notification(app: &AppHandle, identifier: &str) {
    let Some((session_id, due_at)) = reminders::parse_notification(identifier) else {
        return;
    };
    let _ = queue_open(app, session_id, due_at);
}

fn queue_open(app: &AppHandle, session_id: String, due_at: i64) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let mut windows = handle.webview_windows();
        windows.retain(|label, _| crate::window::is_workspace_window(label));
        let service = handle.state::<ReminderService>();
        let owners = service.window_sessions.lock().ok();
        let owns_session = |window: &&WebviewWindow| {
            owners.as_ref().is_some_and(|owners| {
                owners
                    .get(window.label())
                    .is_some_and(|ids| ids.contains(&session_id))
            })
        };
        let target = windows
            .values()
            .filter(owns_session)
            .find(|window| window.is_focused().unwrap_or(false))
            .or_else(|| {
                windows
                    .values()
                    .filter(owns_session)
                    .min_by_key(|window| window.label())
            })
            .or_else(|| {
                windows
                    .values()
                    .find(|window| window.is_focused().unwrap_or(false))
            })
            .or_else(|| windows.get("main"))
            .or_else(|| windows.values().min_by_key(|window| window.label()));
        let request = OpenReminder {
            session_id,
            due_at,
            window_label: target.map(|window| window.label().to_string()),
        };
        drop(owners);
        if let Ok(mut pending) = service.pending_open.lock() {
            *pending = Some(request);
        }
        if let Some(window) = target {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit(OPEN, ());
        } else {
            let _ = crate::window::open_new_window(&handle);
        }
    })
    .map_err(|error| error.to_string())
}

pub(crate) fn init(app: &AppHandle) {
    app.manage(ReminderService::default());
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let service = app.state::<ReminderService>();
        let store = app.state::<SessionStore>();
        reminders::poll_due(&service, &store, &TauriStoreEvents(app.clone()), |notice| {
            let _ = tauri::async_runtime::block_on(crate::notifications::show_notification(
                app.clone(),
                notice.identifier,
                notice.title,
                notice.subtitle,
                notice.body,
                notice.sound,
            ));
        });
    });
}
