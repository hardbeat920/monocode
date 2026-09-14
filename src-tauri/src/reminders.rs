use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::session_store::{now_millis, validate_id, SessionStore};

pub(crate) const CHANGED: &str = "monocode:reminders-changed";
const OPEN: &str = "monocode:reminder-open";
pub(crate) const NOTIFICATION_PREFIX: &str = "reminder:";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    session_id: String,
    due_at: i64,
    fired_at: Option<i64>,
    title: String,
    harness: String,
    cwd: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPreferences {
    notifications_enabled: bool,
    sound: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReminder {
    session_id: String,
    due_at: i64,
    #[serde(skip)]
    window_label: Option<String>,
}

#[derive(Default)]
pub struct ReminderService {
    preferences: Mutex<Option<DeliveryPreferences>>,
    pending_open: Mutex<Option<OpenReminder>>,
    window_sessions: Mutex<HashMap<String, Vec<String>>>,
}

pub(crate) fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_reminders (
           session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
           due_at INTEGER NOT NULL CHECK (due_at > 0),
           fired_at INTEGER
         );
         CREATE INDEX IF NOT EXISTS session_reminders_pending
         ON session_reminders (due_at) WHERE fired_at IS NULL;",
    )
}

fn list(conn: &Connection) -> rusqlite::Result<Vec<Reminder>> {
    let mut statement = conn.prepare(
        "SELECT r.session_id, r.due_at, r.fired_at, s.title, s.harness, s.cwd
         FROM session_reminders r JOIN sessions s ON s.id = r.session_id
         ORDER BY r.due_at, r.session_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Reminder {
            session_id: row.get(0)?,
            due_at: row.get(1)?,
            fired_at: row.get(2)?,
            title: row.get(3)?,
            harness: row.get(4)?,
            cwd: row.get(5)?,
        })
    })?;
    rows.collect()
}

fn set(conn: &mut Connection, ids: &[String], due_at: i64, now: i64) -> Result<(), String> {
    if due_at <= now || due_at > 8_640_000_000_000_000 {
        return Err("Choose a reminder time in the future.".into());
    }
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for id in ids {
        validate_id(id, "session")?;
        let valid: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions
                 WHERE id = ?1 AND has_user_message = 1 AND inbox_ask IS NULL)",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if !valid {
            return Err("This conversation must be saved before adding a reminder.".into());
        }
        tx.execute(
            "INSERT INTO session_reminders (session_id, due_at) VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET due_at = excluded.due_at, fired_at = NULL",
            params![id, due_at],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

fn clear(
    conn: &mut Connection,
    ids: &[String],
    expected_due_at: Option<i64>,
) -> Result<(), String> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    for id in ids {
        validate_id(id, "session")?;
        tx.execute(
            "DELETE FROM session_reminders WHERE session_id = ?1
             AND (?2 IS NULL OR due_at = ?2)",
            params![id, expected_due_at],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

/// Claim in the database before dispatch, so restarts and multiple windows
/// cannot repeatedly announce the same reminder. Due items remain until handled,
/// including when OS delivery fails or the process exits during dispatch.
fn take_due(conn: &mut Connection, now: i64) -> rusqlite::Result<Vec<Reminder>> {
    let tx = conn.transaction()?;
    let due = list(&tx)?
        .into_iter()
        .filter(|reminder| reminder.due_at <= now && reminder.fired_at.is_none())
        .collect::<Vec<_>>();
    tx.execute(
        "UPDATE session_reminders SET fired_at = ?1 WHERE due_at <= ?1 AND fired_at IS NULL",
        [now],
    )?;
    tx.commit()?;
    Ok(due)
}

#[tauri::command(async)]
pub fn reminder_list(store: State<'_, SessionStore>) -> Result<Vec<Reminder>, String> {
    let conn = store.lock_conn()?;
    list(&conn).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub fn reminder_set(
    app: AppHandle,
    store: State<'_, SessionStore>,
    session_ids: Vec<String>,
    due_at: i64,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    set(&mut conn, &session_ids, due_at, now_millis())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

#[tauri::command(async)]
pub fn reminder_clear(
    app: AppHandle,
    store: State<'_, SessionStore>,
    session_ids: Vec<String>,
    expected_due_at: Option<i64>,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    clear(&mut conn, &session_ids, expected_due_at)?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn reminder_configure(
    service: State<'_, ReminderService>,
    preferences: DeliveryPreferences,
) -> Result<(), String> {
    *service
        .preferences
        .lock()
        .map_err(|error| error.to_string())? = Some(preferences);
    Ok(())
}

#[tauri::command]
pub fn reminder_register_window(
    window: WebviewWindow,
    service: State<'_, ReminderService>,
    session_ids: Vec<String>,
) -> Result<(), String> {
    service
        .window_sessions
        .lock()
        .map_err(|error| error.to_string())?
        .insert(window.label().to_string(), session_ids);
    Ok(())
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
    let mut pending = service
        .pending_open
        .lock()
        .map_err(|error| error.to_string())?;
    if pending.as_ref().is_some_and(|request| {
        request
            .window_label
            .as_deref()
            .is_none_or(|label| label == window.label() || app.get_webview_window(label).is_none())
    }) {
        return Ok(pending.take());
    }
    Ok(None)
}

/// A notification can outlive its window. Keep the request until the chosen
/// window has mounted and attached its listeners, then let only that window act.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn open_from_notification(app: &AppHandle, identifier: &str) {
    let Some((session_id, due_at)) = identifier.rsplit_once(':') else {
        return;
    };
    let Ok(due_at) = due_at.parse::<i64>() else {
        return;
    };
    if validate_id(session_id, "session").is_err() {
        return;
    }
    let _ = queue_open(app, session_id.to_string(), due_at);
}

fn queue_open(app: &AppHandle, session_id: String, due_at: i64) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let windows = handle.webview_windows();
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
        let preferences = app
            .state::<ReminderService>()
            .preferences
            .lock()
            .ok()
            .and_then(|value| *value);
        let Some(preferences) = preferences else {
            continue;
        };
        let store = app.state::<SessionStore>();
        let due = store.lock_conn().and_then(|mut conn| {
            take_due(&mut conn, now_millis()).map_err(|error| error.to_string())
        });
        let due = match due {
            Ok(due) => due,
            Err(error) => {
                eprintln!("Could not check reminders: {error}");
                continue;
            }
        };
        if due.is_empty() {
            continue;
        }
        let _ = app.emit(CHANGED, ());
        if !preferences.notifications_enabled {
            continue;
        }
        for reminder in due {
            // Cancellation or rescheduling may happen while another banner is
            // being delivered. Do not announce a stale snapshot of the queue.
            let current = store.lock_conn().and_then(|conn| conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_reminders WHERE session_id = ?1 AND due_at = ?2 AND fired_at IS NOT NULL)",
                params![reminder.session_id, reminder.due_at], |row| row.get::<_, bool>(0),
            ).map_err(|error| error.to_string())).unwrap_or(false);
            if !current {
                continue;
            }
            let identifier = format!(
                "{NOTIFICATION_PREFIX}{}:{}",
                reminder.session_id, reminder.due_at
            );
            let _ = tauri::async_runtime::block_on(crate::notifications::show_notification(
                app.clone(),
                identifier,
                "MonoCode".into(),
                reminder.title,
                "Reminder: continue this conversation.".into(),
                preferences.sound,
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(conn: &Connection, id: &str, cwd: &str) {
        conn.execute(
            "INSERT INTO sessions (id, cwd, harness, model, runtime_mode, title,
             created_at, updated_at, has_user_message) VALUES (?1, ?2, 'codex', '',
             'supervised', 'A saved conversation', 1, 1, 1)",
            params![id, cwd],
        )
        .unwrap();
    }

    #[test]
    fn due_reminders_are_claimed_once_across_projects_and_remain_visible() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "first", "/one");
        seed(&conn, "second", "/two");
        set(&mut conn, &["first".into()], 200, 100).unwrap();
        set(&mut conn, &["second".into()], 400, 100).unwrap();
        assert!(take_due(&mut conn, 199).unwrap().is_empty());
        let due = take_due(&mut conn, 200).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].session_id, "first");
        assert!(take_due(&mut conn, 300).unwrap().is_empty());
        assert_eq!(list(&conn).unwrap().len(), 2);
        assert_eq!(take_due(&mut conn, 500).unwrap()[0].cwd, "/two");
    }

    #[test]
    fn reschedule_rearms_and_old_notification_cannot_clear_it() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "session", "/one");
        let ids = ["session".into()];
        set(&mut conn, &ids, 200, 100).unwrap();
        assert_eq!(take_due(&mut conn, 200).unwrap().len(), 1);
        set(&mut conn, &ids, 500, 200).unwrap();
        clear(&mut conn, &ids, Some(200)).unwrap();
        let remaining = list(&conn).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].due_at, 500);
        assert!(remaining[0].fired_at.is_none());
        assert_eq!(take_due(&mut conn, 500).unwrap().len(), 1);
        clear(&mut conn, &ids, Some(500)).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn cancellation_and_session_deletion_remove_reminders() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "first", "/one");
        seed(&conn, "second", "/two");
        set(&mut conn, &["first".into(), "second".into()], 200, 100).unwrap();
        clear(&mut conn, &["first".into()], None).unwrap();
        conn.execute("DELETE FROM sessions WHERE id = 'second'", [])
            .unwrap();
        assert!(take_due(&mut conn, 300).unwrap().is_empty());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn invalid_batch_and_past_time_do_not_partially_save() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "session", "/one");
        assert!(set(&mut conn, &["session".into(), "missing".into()], 200, 100).is_err());
        assert!(list(&conn).unwrap().is_empty());
        assert!(set(&mut conn, &["session".into()], 100, 100).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn reopening_catches_missed_reminders_without_reannouncing_fired_ones() {
        let path = std::env::temp_dir().join(format!(
            "monocode-reminders-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let store = SessionStore::open(path.join("test.db")).unwrap();
            let mut conn = store.lock_conn().unwrap();
            seed(&conn, "missed", "/one");
            seed(&conn, "fired", "/two");
            set(&mut conn, &["fired".into()], 200, 100).unwrap();
            set(&mut conn, &["missed".into()], 400, 100).unwrap();
            assert_eq!(take_due(&mut conn, 300).unwrap().len(), 1);
        }
        {
            let store = SessionStore::open(path.join("test.db")).unwrap();
            let mut conn = store.lock_conn().unwrap();
            let due = take_due(&mut conn, 900).unwrap();
            assert_eq!(due.len(), 1);
            assert_eq!(due[0].session_id, "missed");
            assert_eq!(list(&conn).unwrap().len(), 2);
            assert!(take_due(&mut conn, 1000).unwrap().is_empty());
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}
