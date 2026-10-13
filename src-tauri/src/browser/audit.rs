//! Audit (spec 11.5, decision D10): what the agent did and where, never what
//! it saw. Three tables in the app's SQLite database:
//!
//! - `browser_audit`: one row per agent action with redacted parameters.
//! - `browser_events`: connection and permission events (who caused them).
//! - `browser_leases`: tabs the module opened, for diagnostics after a crash.
//!   Tabs from an earlier run are never re-adopted (spec 4 step 7).
//!
//! Retention is 30 days, pruned at startup; the user can clear everything
//! from Settings.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::policy::{audit_url, now_ms};
use super::protocol::{ActionKind, AuditEntry, BrowserSettings, Epoch, Outcome, Reason, TabId};

/// 30 days, spec 11.5.
pub const RETENTION_MS: i64 = 30 * 24 * 3600 * 1000;
/// Settings shows the last 50 rows per session (spec 13).
pub const DEFAULT_LIST_LIMIT: u32 = 50;
pub const MAX_LIST_LIMIT: u32 = 1000;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS browser_audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  session_id TEXT NOT NULL,
  request_id TEXT NOT NULL,
  global_epoch TEXT NOT NULL,
  session_epoch TEXT NOT NULL,
  action TEXT NOT NULL,
  tab_id TEXT,
  url TEXT,
  params TEXT NOT NULL,
  outcome TEXT NOT NULL,
  reason TEXT,
  duration_ms INTEGER NOT NULL,
  commands_sent INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS browser_audit_session_ts ON browser_audit (session_id, ts DESC);
CREATE INDEX IF NOT EXISTS browser_audit_ts ON browser_audit (ts);
CREATE TABLE IF NOT EXISTS browser_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  actor TEXT NOT NULL,
  session_id TEXT,
  detail TEXT
);
CREATE INDEX IF NOT EXISTS browser_events_ts ON browser_events (ts);
CREATE TABLE IF NOT EXISTS browser_leases (
  tab_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  target_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  opened_by TEXT,
  url TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  closed_at INTEGER
);
CREATE TABLE IF NOT EXISTS browser_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

const SETTINGS_KEY: &str = "settings";
const RUN_COUNTER_KEY: &str = "run_counter";

/// Create the tables. Idempotent; called from `browser::init`.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

/// Who caused a `browser_events` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    User,
    Agent,
    System,
    Error,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Actor::User => "user",
            Actor::Agent => "agent",
            Actor::System => "system",
            Actor::Error => "error",
        }
    }
}

/// One `browser_audit` row before it is written.
#[derive(Clone, Debug)]
pub struct AuditRecord<'a> {
    pub session_id: &'a str,
    pub request_id: &'a str,
    pub global_epoch: Epoch,
    pub session_epoch: Epoch,
    pub action: &'a str,
    pub tab: Option<&'a TabId>,
    /// Already stripped of query and fragment (see [`audit_url`]).
    pub url: Option<&'a str>,
    /// Already redacted (see [`redact_params`]).
    pub params: &'a Value,
    pub outcome: Outcome,
    pub reason: Option<Reason>,
    pub duration_ms: i64,
    pub commands_sent: i64,
}

pub fn record(conn: &Connection, row: &AuditRecord<'_>) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO browser_audit (ts, session_id, request_id, global_epoch, session_epoch, action, tab_id, url, params, outcome, reason, duration_ms, commands_sent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            now_ms(),
            row.session_id,
            row.request_id,
            row.global_epoch.to_string(),
            row.session_epoch.to_string(),
            row.action,
            row.tab.map(|t| t.0.as_str()),
            row.url,
            row.params.to_string(),
            outcome_str(row.outcome),
            row.reason.map(Reason::as_str),
            row.duration_ms,
            row.commands_sent,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn outcome_str(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Done => "done",
        Outcome::Blocked => "blocked",
        Outcome::Failed => "failed",
        Outcome::Unknown => "unknown",
    }
}

fn parse_outcome(text: &str) -> Outcome {
    match text {
        "done" => Outcome::Done,
        "blocked" => Outcome::Blocked,
        "failed" => Outcome::Failed,
        _ => Outcome::Unknown,
    }
}

fn parse_reason(text: &str) -> Option<Reason> {
    Reason::ALL.iter().copied().find(|r| r.as_str() == text)
}

/// Newest first; `session` narrows to one session.
pub fn list(
    conn: &Connection,
    session: Option<&str>,
    limit: u32,
) -> rusqlite::Result<Vec<AuditEntry>> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT) as i64;
    let mut statement = conn.prepare(
        "SELECT id, ts, session_id, request_id, global_epoch, session_epoch, action, tab_id, url, params, outcome, reason, duration_ms, commands_sent
         FROM browser_audit
         WHERE (?1 IS NULL OR session_id = ?1)
         ORDER BY id DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![session, limit], |row| {
        let params_text: String = row.get(9)?;
        let outcome: String = row.get(10)?;
        let reason: Option<String> = row.get(11)?;
        Ok(AuditEntry {
            id: row.get(0)?,
            ts_ms: row.get(1)?,
            session_id: row.get(2)?,
            request_id: row.get(3)?,
            global_epoch: row.get(4)?,
            session_epoch: row.get(5)?,
            action: row.get(6)?,
            tab: row.get(7)?,
            url: row.get(8)?,
            params: serde_json::from_str(&params_text).unwrap_or(Value::Null),
            outcome: parse_outcome(&outcome),
            reason: reason.as_deref().and_then(parse_reason),
            duration_ms: row.get(12)?,
            commands_sent: row.get(13)?,
        })
    })?;
    rows.collect()
}

/// Settings "clear audit": everything, including events and lease history.
pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM browser_audit; DELETE FROM browser_events; DELETE FROM browser_leases;",
    )
}

/// Drop rows older than the retention window. Returns rows removed.
pub fn prune(conn: &Connection, now_ms: i64) -> rusqlite::Result<usize> {
    let cutoff = now_ms - RETENTION_MS;
    let mut removed = conn.execute("DELETE FROM browser_audit WHERE ts < ?1", params![cutoff])?;
    removed += conn.execute("DELETE FROM browser_events WHERE ts < ?1", params![cutoff])?;
    removed += conn.execute(
        "DELETE FROM browser_leases WHERE COALESCE(closed_at, created_at) < ?1",
        params![cutoff],
    )?;
    Ok(removed)
}

/// A connection or permission event (spec 11.5 `browser_events`).
pub fn event(
    conn: &Connection,
    kind: &str,
    actor: Actor,
    session: Option<&str>,
    detail: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO browser_events (ts, kind, actor, session_id, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now_ms(), kind, actor.as_str(), session, detail],
    )?;
    Ok(())
}

/// One `browser_events` row as Settings lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EventEntry {
    pub id: i64,
    pub ts_ms: i64,
    pub kind: String,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub fn events(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<EventEntry>> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT) as i64;
    let mut statement = conn.prepare(
        "SELECT id, ts, kind, actor, session_id, detail FROM browser_events ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = statement.query_map(params![limit], |row| {
        Ok(EventEntry {
            id: row.get(0)?,
            ts_ms: row.get(1)?,
            kind: row.get(2)?,
            actor: row.get(3)?,
            session_id: row.get(4)?,
            detail: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn lease_opened(
    conn: &Connection,
    run_id: u64,
    tab: &TabId,
    target_id: &str,
    session: &str,
    opened_by: Option<&TabId>,
    url: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO browser_leases (tab_id, run_id, target_id, session_id, opened_by, url, created_at, closed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
        params![
            tab.0,
            format!("{run_id:x}"),
            target_id,
            session,
            opened_by.map(|t| t.0.as_str()),
            audit_url(url),
            now_ms(),
        ],
    )?;
    Ok(())
}

pub fn lease_closed(conn: &Connection, tab: &TabId) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE browser_leases SET closed_at = ?2 WHERE tab_id = ?1 AND closed_at IS NULL",
        params![tab.0, now_ms()],
    )?;
    Ok(())
}

/// Close every open row of `run_id` whose tab is not in `live`.
pub fn close_leases_except(
    conn: &Connection,
    run_id: u64,
    live: &std::collections::HashSet<String>,
) -> rusqlite::Result<usize> {
    let open: Vec<String> = {
        let mut statement = conn
            .prepare("SELECT tab_id FROM browser_leases WHERE closed_at IS NULL AND run_id = ?1")?;
        let rows = statement.query_map(params![format!("{run_id:x}")], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let now = now_ms();
    let mut closed = 0;
    for tab in open.iter().filter(|tab| !live.contains(*tab)) {
        closed += conn.execute(
            "UPDATE browser_leases SET closed_at = ?2 WHERE tab_id = ?1 AND closed_at IS NULL",
            params![tab, now],
        )?;
    }
    Ok(closed)
}

pub fn lease_transferred(conn: &Connection, tab: &TabId, to: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE browser_leases SET session_id = ?2 WHERE tab_id = ?1",
        params![tab.0, to],
    )?;
    Ok(())
}

/// Leases of earlier runs still marked open: closed on startup because the
/// module never re-adopts tabs (spec 4). Returns how many were closed out.
pub fn close_stale_leases(conn: &Connection, run_id: u64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE browser_leases SET closed_at = ?2 WHERE closed_at IS NULL AND run_id <> ?1",
        params![format!("{run_id:x}"), now_ms()],
    )
}

pub fn open_lease_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM browser_leases WHERE closed_at IS NULL",
        [],
        |row| row.get(0),
    )
    .optional()
    .map(|count| count.unwrap_or(0))
}

/// Persisted module settings (spec 13.1). The main toggle is deliberately
/// not persisted: every launch starts OFF and a new connection always means a
/// new consent dialog.
pub fn load_settings(conn: &Connection) -> rusqlite::Result<Option<BrowserSettings>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM browser_settings WHERE key = ?1",
            params![SETTINGS_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

pub fn save_settings(conn: &Connection, settings: &BrowserSettings) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO browser_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            SETTINGS_KEY,
            serde_json::to_string(settings).unwrap_or_default()
        ],
    )?;
    Ok(())
}

/// Monotonic app-run counter so an epoch from an earlier run never equals a
/// current one (spec 14.2). Returns the id for this run.
pub fn next_run_id(conn: &Connection) -> rusqlite::Result<u64> {
    let previous: Option<String> = conn
        .query_row(
            "SELECT value FROM browser_settings WHERE key = ?1",
            params![RUN_COUNTER_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let next = previous
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_add(1);
    conn.execute(
        "INSERT INTO browser_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![RUN_COUNTER_KEY, next.to_string()],
    )?;
    Ok(next)
}

/// Parameters as the audit stores them (spec 11.5): `text` becomes
/// `{len, sha256_8}`, `url` loses query and fragment, and the `value` of
/// `wait {for: text}` is hashed the same way. `ref`, `paths`, `tab` and
/// everything else stay.
pub fn redact_params(action: ActionKind, input: &Value) -> Value {
    let Some(object) = input.as_object() else {
        return Value::Object(Map::new());
    };
    let waits_for_text = object.get("for").and_then(Value::as_str) == Some("text");
    let mut out = Map::with_capacity(object.len());
    for (key, value) in object {
        let redacted = match (action, key.as_str()) {
            // `type` text, `dialog` prompt text: length and a short hash only.
            (_, "text") => hash_text(value),
            (ActionKind::Wait, "value") if waits_for_text => hash_text(value),
            (_, "url") => match value {
                Value::String(url) => Value::String(audit_url(url)),
                other => other.clone(),
            },
            _ => value.clone(),
        };
        out.insert(key.clone(), redacted);
    }
    Value::Object(out)
}

fn hash_text(value: &Value) -> Value {
    let text = match value {
        Value::String(text) => text.as_str(),
        other => return other.clone(),
    };
    let digest = Sha256::digest(text.as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    json!({ "len": text.chars().count(), "sha256_8": hex })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn records_list_newest_first_and_filter_by_session() {
        let conn = conn();
        let tab = TabId("t_0000abcd".into());
        let params = json!({ "tab": "t_0000abcd", "ref": "e3", "text": { "len": 5, "sha256_8": "deadbeef" } });
        for (i, session) in ["s1", "s1", "s2"].iter().enumerate() {
            let id = record(
                &conn,
                &AuditRecord {
                    session_id: session,
                    request_id: &format!("r{i}"),
                    global_epoch: Epoch { run: 0xab, seq: 1 },
                    session_epoch: Epoch {
                        run: 0xab,
                        seq: i as u64,
                    },
                    action: "type",
                    tab: Some(&tab),
                    url: Some("https://example.com/path"),
                    params: &params,
                    outcome: if i == 2 {
                        Outcome::Blocked
                    } else {
                        Outcome::Done
                    },
                    reason: (i == 2).then_some(Reason::NoLease),
                    duration_ms: 12 + i as i64,
                    commands_sent: 3,
                },
            )
            .unwrap();
            assert_eq!(id, i as i64 + 1);
        }
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].request_id, "r2");
        assert_eq!(all[0].outcome, Outcome::Blocked);
        assert_eq!(all[0].reason, Some(Reason::NoLease));
        assert_eq!(all[0].global_epoch, "ab:1");
        assert_eq!(all[2].session_epoch, "ab:0");
        assert_eq!(all[2].params["text"]["sha256_8"], "deadbeef");
        assert_eq!(all[2].tab.as_deref(), Some("t_0000abcd"));
        assert_eq!(all[2].url.as_deref(), Some("https://example.com/path"));
        assert_eq!(all[2].commands_sent, 3);
        let s1 = list(&conn, Some("s1"), 1).unwrap();
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].request_id, "r1");
        assert!(list(&conn, Some("nobody"), 50).unwrap().is_empty());
    }

    #[test]
    fn events_leases_prune_and_clear() {
        let conn = conn();
        event(&conn, "connected", Actor::User, None, Some("Chrome/144")).unwrap();
        event(&conn, "grant_on", Actor::User, Some("s1"), None).unwrap();
        let rows = events(&conn, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "grant_on");
        assert_eq!(rows[0].actor, "user");
        assert_eq!(rows[0].session_id.as_deref(), Some("s1"));
        assert_eq!(rows[1].detail.as_deref(), Some("Chrome/144"));

        let tab = TabId("t_00000001".into());
        lease_opened(
            &conn,
            0x77,
            &tab,
            "T1",
            "s1",
            None,
            "https://a.b/c?secret=1#frag",
        )
        .unwrap();
        let url: String = conn
            .query_row(
                "SELECT url FROM browser_leases WHERE tab_id = 't_00000001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://a.b/c");
        assert_eq!(open_lease_count(&conn).unwrap(), 1);
        lease_transferred(&conn, &tab, "s2").unwrap();
        let owner: String = conn
            .query_row(
                "SELECT session_id FROM browser_leases WHERE tab_id = 't_00000001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "s2");
        // A previous run's open lease is closed out at startup, ours is kept.
        lease_opened(
            &conn,
            0x11,
            &TabId("t_00000002".into()),
            "T2",
            "s1",
            Some(&tab),
            "about:blank",
        )
        .unwrap();
        assert_eq!(close_stale_leases(&conn, 0x77).unwrap(), 1);
        assert_eq!(open_lease_count(&conn).unwrap(), 1);
        lease_closed(&conn, &tab).unwrap();
        assert_eq!(open_lease_count(&conn).unwrap(), 0);

        // Prune: age the first audit/event rows past retention.
        record(
            &conn,
            &AuditRecord {
                session_id: "s1",
                request_id: "old",
                global_epoch: Epoch { run: 1, seq: 1 },
                session_epoch: Epoch { run: 1, seq: 1 },
                action: "click",
                tab: None,
                url: None,
                params: &json!({}),
                outcome: Outcome::Done,
                reason: None,
                duration_ms: 1,
                commands_sent: 1,
            },
        )
        .unwrap();
        let now = now_ms();
        conn.execute(
            "UPDATE browser_audit SET ts = ?1",
            params![now - RETENTION_MS - 1000],
        )
        .unwrap();
        conn.execute(
            "UPDATE browser_events SET ts = ?1 WHERE kind = 'connected'",
            params![now - RETENTION_MS - 1000],
        )
        .unwrap();
        let removed = prune(&conn, now).unwrap();
        assert_eq!(removed, 2);
        assert!(list(&conn, None, 10).unwrap().is_empty());
        assert_eq!(events(&conn, 10).unwrap().len(), 1);
        clear(&conn).unwrap();
        assert!(events(&conn, 10).unwrap().is_empty());
        assert_eq!(open_lease_count(&conn).unwrap(), 0);
    }

    #[test]
    fn settings_and_run_counter_persist() {
        let conn = conn();
        assert!(load_settings(&conn).unwrap().is_none());
        let mut settings = BrowserSettings {
            separate_window: true,
            upload_roots: vec!["/data".into()],
            ..BrowserSettings::default()
        };
        save_settings(&conn, &settings).unwrap();
        assert_eq!(load_settings(&conn).unwrap().unwrap(), settings);
        settings.limits.tabs_total = 3;
        save_settings(&conn, &settings).unwrap();
        assert_eq!(load_settings(&conn).unwrap().unwrap().limits.tabs_total, 3);
        assert_eq!(next_run_id(&conn).unwrap(), 1);
        assert_eq!(next_run_id(&conn).unwrap(), 2);
        clear(&conn).unwrap();
        // Clearing the audit keeps settings and the run counter.
        assert_eq!(next_run_id(&conn).unwrap(), 3);
        assert!(load_settings(&conn).unwrap().is_some());
    }

    #[test]
    fn redaction_hides_typed_text_and_query_strings_but_keeps_refs_and_paths() {
        let typed = redact_params(
            ActionKind::Type,
            &json!({ "tab": "t_1", "ref": "e7", "text": "hunter2 pass", "submit": true }),
        );
        assert_eq!(typed["tab"], "t_1");
        assert_eq!(typed["ref"], "e7");
        assert_eq!(typed["submit"], true);
        assert_eq!(typed["text"]["len"], 12);
        let digest = typed["text"]["sha256_8"].as_str().unwrap();
        assert_eq!(digest.len(), 8);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(typed.to_string().find("hunter2").is_none());
        let nav = redact_params(
            ActionKind::Navigate,
            &json!({ "tab": "t_1", "url": "https://x.y/login?token=abc#frag" }),
        );
        assert_eq!(nav["url"], "https://x.y/login");
        let upload = redact_params(
            ActionKind::Upload,
            &json!({ "tab": "t_1", "ref": "e1", "paths": ["/repo/a.png"] }),
        );
        assert_eq!(upload["paths"][0], "/repo/a.png");
        let wait = redact_params(
            ActionKind::Wait,
            &json!({ "tab": "t_1", "for": "text", "value": "Welcome" }),
        );
        assert_eq!(wait["value"]["len"], 7);
        let wait_time = redact_params(
            ActionKind::Wait,
            &json!({ "tab": "t_1", "for": "time", "value": 500 }),
        );
        assert_eq!(wait_time["value"], 500);
        let wait_url = redact_params(
            ActionKind::Wait,
            &json!({ "tab": "t_1", "for": "url", "value": "https://x.y/done*" }),
        );
        assert_eq!(wait_url["value"], "https://x.y/done*");
        let dialog = redact_params(
            ActionKind::Dialog,
            &json!({ "tab": "t_1", "action": "accept", "text": "secret" }),
        );
        assert_eq!(dialog["text"]["len"], 6);
        assert_eq!(redact_params(ActionKind::Click, &Value::Null), json!({}));
    }
}
