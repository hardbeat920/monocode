//! Session reminders: the table, the due check, and delivery preferences.
//! Moved from src-tauri/src/reminders.rs. Window choice and the poller thread
//! stay with the app.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::StoreEvents;
use crate::session_store::{SessionStore, now_millis, validate_id};

/// Notification identifiers for reminders start with this prefix, followed by
/// `<session id>:<due at>`.
pub const NOTIFICATION_PREFIX: &str = "reminder:";

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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPreferences {
    notifications_enabled: bool,
    sound: bool,
    #[serde(default)]
    project_rules: HashMap<String, ProjectDeliveryRule>,
}

#[derive(Clone, Deserialize)]
pub struct ProjectDeliveryRule {
    enabled: bool,
    after: i64,
}

impl DeliveryPreferences {
    fn allows(&self, reminder: &Reminder) -> bool {
        self.notifications_enabled
            && self
                .project_rules
                .get(&reminder.session_id)
                .is_some_and(|rule| rule.enabled && reminder.due_at > rule.after)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenReminder {
    pub session_id: String,
    pub due_at: i64,
    /// The owner (window) chosen to open the reminder.
    #[serde(skip)]
    pub window_label: Option<String>,
}

/// Delivery preferences, the reminder an owner should open next, and the
/// sessions each owner (window) shows.
#[derive(Default)]
pub struct ReminderService {
    pub preferences: Mutex<Option<DeliveryPreferences>>,
    pub pending_open: Mutex<Option<OpenReminder>>,
    pub window_sessions: Mutex<HashMap<String, Vec<String>>>,
}

/// A reminder that is due and allowed by the delivery preferences.
pub struct ReminderNotification {
    pub identifier: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub sound: bool,
}

pub fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
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
fn take_due(
    conn: &mut Connection,
    now: i64,
    is_ready: impl Fn(&Reminder) -> bool,
) -> rusqlite::Result<Vec<Reminder>> {
    let tx = conn.transaction()?;
    let due = list(&tx)?
        .into_iter()
        .filter(|reminder| {
            reminder.due_at <= now && reminder.fired_at.is_none() && is_ready(reminder)
        })
        .collect::<Vec<_>>();
    for reminder in &due {
        tx.execute(
            "UPDATE session_reminders SET fired_at = ?1 WHERE session_id = ?2",
            params![now, reminder.session_id],
        )?;
    }
    tx.commit()?;
    Ok(due)
}

pub fn reminder_list(store: &SessionStore) -> Result<Vec<Reminder>, String> {
    let conn = store.lock_conn()?;
    list(&conn).map_err(|error| error.to_string())
}

pub fn reminder_set(
    store: &SessionStore,
    events: &dyn StoreEvents,
    session_ids: Vec<String>,
    due_at: i64,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    set(&mut conn, &session_ids, due_at, now_millis())?;
    drop(conn);
    events.reminders_changed();
    Ok(())
}

pub fn reminder_clear(
    store: &SessionStore,
    events: &dyn StoreEvents,
    session_ids: Vec<String>,
    expected_due_at: Option<i64>,
) -> Result<(), String> {
    let mut conn = store.lock_conn()?;
    clear(&mut conn, &session_ids, expected_due_at)?;
    drop(conn);
    events.reminders_changed();
    Ok(())
}

pub fn reminder_configure(
    service: &ReminderService,
    preferences: DeliveryPreferences,
) -> Result<(), String> {
    *service
        .preferences
        .lock()
        .map_err(|error| error.to_string())? = Some(preferences);
    Ok(())
}

pub fn reminder_register_window(
    service: &ReminderService,
    owner: &str,
    session_ids: Vec<String>,
) -> Result<(), String> {
    service
        .window_sessions
        .lock()
        .map_err(|error| error.to_string())?
        .insert(owner.to_string(), session_ids);
    Ok(())
}

/// Hand the pending open request to `owner` if it was chosen, or if the
/// chosen owner no longer exists.
pub fn reminder_take_open(
    service: &ReminderService,
    owner: &str,
    owner_exists: impl Fn(&str) -> bool,
) -> Result<Option<OpenReminder>, String> {
    let mut pending = service
        .pending_open
        .lock()
        .map_err(|error| error.to_string())?;
    if pending.as_ref().is_some_and(|request| {
        request
            .window_label
            .as_deref()
            .is_none_or(|label| label == owner || !owner_exists(label))
    }) {
        return Ok(pending.take());
    }
    Ok(None)
}

/// Parse a notification identifier (without the prefix) into a session id and
/// due time.
pub fn parse_notification(identifier: &str) -> Option<(String, i64)> {
    let (session_id, due_at) = identifier.rsplit_once(':')?;
    let due_at = due_at.parse::<i64>().ok()?;
    if validate_id(session_id, "session").is_err() {
        return None;
    }
    Some((session_id.to_string(), due_at))
}

/// One pass of the reminder poller: claim due reminders, announce the change,
/// and hand each deliverable one to `notify`. The caller runs this every five
/// seconds on a background thread.
pub fn poll_due(
    service: &ReminderService,
    store: &SessionStore,
    events: &dyn StoreEvents,
    mut notify: impl FnMut(ReminderNotification),
) {
    let preferences = service
        .preferences
        .lock()
        .ok()
        .and_then(|value| value.clone());
    let Some(preferences) = preferences else {
        return;
    };
    let due = store.lock_conn().and_then(|mut conn| {
        // Resolve project preferences before claiming a reminder, including
        // during launch and immediately after a reminder is scheduled.
        take_due(&mut conn, now_millis(), |reminder| {
            preferences.project_rules.contains_key(&reminder.session_id)
        })
        .map_err(|error| error.to_string())
    });
    let due = match due {
        Ok(due) => due,
        Err(error) => {
            eprintln!("Could not check reminders: {error}");
            return;
        }
    };
    if due.is_empty() {
        return;
    }
    events.reminders_changed();
    for reminder in due {
        // A prior platform notification may have blocked while preferences
        // changed. Take a fresh snapshot without holding the lock for delivery.
        let preferences = service
            .preferences
            .lock()
            .ok()
            .and_then(|value| value.clone());
        let Some(preferences) = preferences else {
            continue;
        };
        if !preferences.allows(&reminder) {
            continue;
        }
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
        notify(ReminderNotification {
            identifier,
            title: "MonoCode".into(),
            subtitle: reminder.title,
            body: "Reminder: continue this conversation.".into(),
            sound: preferences.sound,
        });
    }
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
        assert!(take_due(&mut conn, 199, |_| true).unwrap().is_empty());
        let due = take_due(&mut conn, 200, |_| true).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].session_id, "first");
        assert!(take_due(&mut conn, 300, |_| true).unwrap().is_empty());
        assert_eq!(list(&conn).unwrap().len(), 2);
        assert_eq!(take_due(&mut conn, 500, |_| true).unwrap()[0].cwd, "/two");
    }

    #[test]
    fn project_rules_wait_for_resolution_and_suppress_due_reminders_without_replay() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "muted", "/one");
        seed(&conn, "timed", "/two");
        set(&mut conn, &["muted".into(), "timed".into()], 200, 100).unwrap();
        let mut preferences: DeliveryPreferences =
            serde_json::from_str(r#"{"notificationsEnabled":true,"sound":false}"#).unwrap();
        assert!(
            take_due(&mut conn, 300, |reminder| {
                preferences.project_rules.contains_key(&reminder.session_id)
            })
            .unwrap()
            .is_empty()
        );
        assert!(
            list(&conn)
                .unwrap()
                .iter()
                .all(|reminder| reminder.fired_at.is_none())
        );

        preferences.project_rules.insert(
            "muted".into(),
            ProjectDeliveryRule {
                enabled: false,
                after: 0,
            },
        );
        let due = take_due(&mut conn, 300, |reminder| {
            preferences.project_rules.contains_key(&reminder.session_id)
        })
        .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].session_id, "muted");
        assert!(!preferences.allows(&due[0]));

        preferences.project_rules.insert(
            "timed".into(),
            ProjectDeliveryRule {
                enabled: true,
                after: 200,
            },
        );
        let due = take_due(&mut conn, 300, |reminder| {
            preferences.project_rules.contains_key(&reminder.session_id)
        })
        .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].session_id, "timed");
        assert!(!preferences.allows(&due[0]));
        assert!(
            list(&conn)
                .unwrap()
                .iter()
                .all(|reminder| reminder.fired_at.is_some())
        );

        preferences.project_rules.get_mut("muted").unwrap().enabled = true;
        assert!(take_due(&mut conn, 400, |_| true).unwrap().is_empty());
        set(&mut conn, &["timed".into()], 500, 400).unwrap();
        let due = take_due(&mut conn, 500, |_| true).unwrap();
        assert_eq!(due.len(), 1);
        assert!(preferences.allows(&due[0]));
    }

    #[test]
    fn reschedule_rearms_and_old_notification_cannot_clear_it() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut conn = store.lock_conn().unwrap();
        seed(&conn, "session", "/one");
        let ids = ["session".into()];
        set(&mut conn, &ids, 200, 100).unwrap();
        assert_eq!(take_due(&mut conn, 200, |_| true).unwrap().len(), 1);
        set(&mut conn, &ids, 500, 200).unwrap();
        clear(&mut conn, &ids, Some(200)).unwrap();
        let remaining = list(&conn).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].due_at, 500);
        assert!(remaining[0].fired_at.is_none());
        assert_eq!(take_due(&mut conn, 500, |_| true).unwrap().len(), 1);
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
        assert!(take_due(&mut conn, 300, |_| true).unwrap().is_empty());
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
            assert_eq!(take_due(&mut conn, 300, |_| true).unwrap().len(), 1);
        }
        {
            let store = SessionStore::open(path.join("test.db")).unwrap();
            let mut conn = store.lock_conn().unwrap();
            let due = take_due(&mut conn, 900, |_| true).unwrap();
            assert_eq!(due.len(), 1);
            assert_eq!(due[0].session_id, "missed");
            assert_eq!(list(&conn).unwrap().len(), 2);
            assert!(take_due(&mut conn, 1000, |_| true).unwrap().is_empty());
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}
