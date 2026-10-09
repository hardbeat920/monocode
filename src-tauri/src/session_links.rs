//! Links between two sessions whose agents may read and message each other.
//!
//! A link is stored once per unordered pair. `agent_messages` counts messages
//! one agent sent to the other since a user last wrote in either session, so a
//! pair of agents cannot keep each other running forever. `budget_epoch` goes
//! up on every reset, so a late release of a message counted before the reset
//! cannot lower the new count.
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::session_store::{now_millis, validate_id, SessionStore};

pub(crate) const CHANGED: &str = "monocode:session-links-changed";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionLink {
    a: String,
    b: String,
    a_title: String,
    b_title: String,
    agent_messages: i64,
    created_at: i64,
}

/// One counted agent message. Pass `epoch` back to release it.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkReservation {
    count: i64,
    epoch: i64,
}

pub(crate) fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_links (
           a TEXT NOT NULL,
           b TEXT NOT NULL,
           agent_messages INTEGER NOT NULL DEFAULT 0,
           budget_epoch INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL,
           PRIMARY KEY (a, b),
           CHECK (a < b)
         );
         CREATE INDEX IF NOT EXISTS session_links_b ON session_links (b);",
    )
}

fn pair<'a>(first: &'a str, second: &'a str) -> Result<(&'a str, &'a str), String> {
    validate_id(first, "session")?;
    validate_id(second, "session")?;
    if first == second {
        return Err("A session cannot be linked to itself".into());
    }
    Ok(if first < second {
        (first, second)
    } else {
        (second, first)
    })
}

fn list(conn: &Connection) -> rusqlite::Result<Vec<SessionLink>> {
    let mut statement = conn.prepare(
        "SELECT l.a, l.b, COALESCE(sa.title, ''), COALESCE(sb.title, ''),
                l.agent_messages, l.created_at
         FROM session_links l
         LEFT JOIN sessions sa ON sa.id = l.a
         LEFT JOIN sessions sb ON sb.id = l.b
         ORDER BY l.created_at, l.a, l.b",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SessionLink {
            a: row.get(0)?,
            b: row.get(1)?,
            a_title: row.get(2)?,
            b_title: row.get(3)?,
            agent_messages: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

fn link(conn: &Connection, first: &str, second: &str) -> Result<(), String> {
    let (a, b) = pair(first, second)?;
    // A random first epoch, so a release left over from an earlier link
    // between the same sessions cannot match this one.
    conn.execute(
        "INSERT INTO session_links (a, b, created_at, budget_epoch)
         VALUES (?1, ?2, ?3, abs(random() % 1000000000000000))
         ON CONFLICT(a, b) DO NOTHING",
        params![a, b, now_millis()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn unlink(conn: &Connection, first: &str, second: &str) -> Result<(), String> {
    let (a, b) = pair(first, second)?;
    conn.execute(
        "DELETE FROM session_links WHERE a = ?1 AND b = ?2",
        params![a, b],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Count one agent message on a link, or refuse once `limit` is reached.
fn record_agent_message(
    conn: &Connection,
    first: &str,
    second: &str,
    limit: i64,
) -> Result<LinkReservation, String> {
    let (a, b) = pair(first, second)?;
    let changed = conn
        .execute(
            "UPDATE session_links SET agent_messages = agent_messages + 1
             WHERE a = ?1 AND b = ?2 AND agent_messages < ?3",
            params![a, b, limit],
        )
        .map_err(|e| e.to_string())?;
    let reservation = conn
        .query_row(
            "SELECT agent_messages, budget_epoch FROM session_links WHERE a = ?1 AND b = ?2",
            params![a, b],
            |row| {
                Ok(LinkReservation {
                    count: row.get(0)?,
                    epoch: row.get(1)?,
                })
            },
        )
        .ok();
    match (changed, reservation) {
        (_, None) => Err("These sessions are not linked".into()),
        (0, Some(_)) => Err(format!(
            "The agents in these linked sessions have sent each other {limit} messages since the user last wrote. Ask the user to send a message in either session before sending more."
        )),
        (_, Some(reservation)) => Ok(reservation),
    }
}

/// Give back a message counted by `record_agent_message` that the target
/// session did not accept. A reset since then already cleared it.
fn release_agent_message(
    conn: &Connection,
    first: &str,
    second: &str,
    epoch: i64,
) -> Result<(), String> {
    let (a, b) = pair(first, second)?;
    conn.execute(
        "UPDATE session_links SET agent_messages = agent_messages - 1
         WHERE a = ?1 AND b = ?2 AND agent_messages > 0 AND budget_epoch = ?3",
        params![a, b, epoch],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// A user message in either session gives its links a fresh budget.
fn reset_agent_messages(conn: &Connection, session: &str) -> Result<usize, String> {
    validate_id(session, "session")?;
    conn.execute(
        "UPDATE session_links SET agent_messages = 0, budget_epoch = budget_epoch + 1
         WHERE (a = ?1 OR b = ?1) AND agent_messages > 0",
        params![session],
    )
    .map_err(|e| e.to_string())
}

pub(crate) fn delete_for_session(conn: &Connection, session: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM session_links WHERE a = ?1 OR b = ?1",
        params![session],
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn session_links_list(store: State<'_, SessionStore>) -> Result<Vec<SessionLink>, String> {
    let conn = store.lock_conn()?;
    list(&conn).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn session_link(
    app: AppHandle,
    store: State<'_, SessionStore>,
    first: String,
    second: String,
) -> Result<(), String> {
    let conn = store.lock_conn()?;
    link(&conn, &first, &second)?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

#[tauri::command(async)]
pub fn session_unlink(
    app: AppHandle,
    store: State<'_, SessionStore>,
    first: String,
    second: String,
) -> Result<(), String> {
    let conn = store.lock_conn()?;
    unlink(&conn, &first, &second)?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

#[tauri::command(async)]
pub fn session_link_record_message(
    store: State<'_, SessionStore>,
    first: String,
    second: String,
    limit: i64,
) -> Result<LinkReservation, String> {
    let conn = store.lock_conn()?;
    record_agent_message(&conn, &first, &second, limit.clamp(1, 100))
}

#[tauri::command(async)]
pub fn session_link_release_message(
    store: State<'_, SessionStore>,
    first: String,
    second: String,
    epoch: i64,
) -> Result<(), String> {
    let conn = store.lock_conn()?;
    release_agent_message(&conn, &first, &second, epoch)
}

#[tauri::command(async)]
pub fn session_links_reset(
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<usize, String> {
    let conn = store.lock_conn()?;
    reset_agent_messages(&conn, &session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        SessionStore::open_in_memory().unwrap()
    }

    #[test]
    fn links_are_unordered_and_unique() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        link(&conn, "b-2", "a-1").unwrap();
        link(&conn, "a-1", "b-2").unwrap();
        let links = list(&conn).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].a, "a-1");
        assert_eq!(links[0].b, "b-2");
        assert!(link(&conn, "a-1", "a-1").is_err());
        assert!(link(&conn, "a-1", "bad/id").is_err());
        unlink(&conn, "b-2", "a-1").unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn agent_messages_stop_at_the_limit_until_a_user_writes() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        assert!(record_agent_message(&conn, "a", "b", 5).is_err());
        link(&conn, "a", "b").unwrap();
        for expected in 1..=5 {
            assert_eq!(
                record_agent_message(&conn, "b", "a", 5).unwrap().count,
                expected
            );
        }
        let error = record_agent_message(&conn, "a", "b", 5).unwrap_err();
        assert!(error.contains("5 messages"));
        reset_agent_messages(&conn, "b").unwrap();
        assert_eq!(record_agent_message(&conn, "a", "b", 5).unwrap().count, 1);
    }

    #[test]
    fn a_released_message_returns_to_the_budget() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        link(&conn, "a", "b").unwrap();
        let first = record_agent_message(&conn, "a", "b", 5).unwrap();
        assert_eq!(first.count, 1);
        release_agent_message(&conn, "b", "a", first.epoch).unwrap();
        release_agent_message(&conn, "a", "b", first.epoch).unwrap();
        assert_eq!(record_agent_message(&conn, "a", "b", 5).unwrap().count, 1);
    }

    #[test]
    fn a_release_from_before_a_reset_leaves_the_new_budget_alone() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        link(&conn, "a", "b").unwrap();
        let stale = record_agent_message(&conn, "a", "b", 5).unwrap();
        reset_agent_messages(&conn, "a").unwrap();
        for _ in 0..5 {
            record_agent_message(&conn, "a", "b", 5).unwrap();
        }
        release_agent_message(&conn, "a", "b", stale.epoch).unwrap();
        assert!(record_agent_message(&conn, "a", "b", 5).is_err());
    }

    #[test]
    fn a_release_from_an_earlier_link_leaves_a_relink_alone() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        link(&conn, "a", "b").unwrap();
        let stale = record_agent_message(&conn, "a", "b", 5).unwrap();
        unlink(&conn, "a", "b").unwrap();
        link(&conn, "a", "b").unwrap();
        for _ in 0..5 {
            record_agent_message(&conn, "a", "b", 5).unwrap();
        }
        release_agent_message(&conn, "a", "b", stale.epoch).unwrap();
        assert!(record_agent_message(&conn, "a", "b", 5).is_err());
    }

    #[test]
    fn deleting_a_session_drops_its_links() {
        let store = store();
        let conn = store.lock_conn().unwrap();
        link(&conn, "a", "b").unwrap();
        link(&conn, "a", "c").unwrap();
        link(&conn, "b", "c").unwrap();
        delete_for_session(&conn, "a").unwrap();
        let links = list(&conn).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].a.as_str(), links[0].b.as_str()), ("b", "c"));
    }
}
