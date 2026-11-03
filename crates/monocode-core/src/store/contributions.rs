//! Code contributions per session, kept in the native sidecar so the shared
//! desktop schema is untouched. A commit or pull request is recorded once.
use super::Store;
use crate::contributions::{Contribution, Kind};
use rusqlite::{params, Connection};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS contributions(
    session_id INTEGER NOT NULL, kind TEXT NOT NULL, reference TEXT NOT NULL,
    at INTEGER NOT NULL, additions INTEGER NOT NULL DEFAULT 0, deletions INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(kind, reference));
  CREATE INDEX IF NOT EXISTS contributions_by_session ON contributions(session_id, at);";

impl Store {
    fn contributions_db(&self) -> Result<Option<&Connection>, String> {
        let Some(conn) = self.native_db() else {
            return Ok(None);
        };
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Some(conn))
    }

    /// Records a contribution unless its commit or pull request already is.
    /// An amended commit takes over the session's latest commit instead.
    pub fn record_contribution(&self, item: &Contribution, amend: bool) -> Result<(), String> {
        let Some(conn) = self.contributions_db()? else {
            return Ok(());
        };
        if amend {
            let replaced = conn
                .execute(
                    "UPDATE OR IGNORE contributions SET reference=?2, at=?3, additions=?4, deletions=?5
                     WHERE rowid=(SELECT rowid FROM contributions WHERE session_id=?1 AND kind='commit'
                                  ORDER BY at DESC LIMIT 1)",
                    params![item.session, item.reference, item.at as i64, item.additions as i64, item.deletions as i64],
                )
                .map_err(|e| e.to_string())?;
            if replaced > 0 {
                return Ok(());
            }
        }
        conn.execute(
            "INSERT OR IGNORE INTO contributions(session_id,kind,reference,at,additions,deletions)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                item.session,
                item.kind.id(),
                item.reference,
                item.at as i64,
                item.additions as i64,
                item.deletions as i64
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Whether any of a session's saved history contains one of `needles`.
    /// SQLite searches the stored text itself, so a session that never ran a
    /// matching command is ruled out without loading it into memory.
    pub fn session_mentions(&self, session: i64, needles: &[&str]) -> Result<bool, String> {
        if needles.is_empty() {
            return Ok(false);
        }
        let any = |column: &str| {
            (0..needles.len())
                .map(|i| format!("instr({column},?{})>0", i + 2))
                .collect::<Vec<_>>()
                .join(" OR ")
        };
        let mut values: Vec<&dyn rusqlite::ToSql> = Vec::new();
        if self.shared.is_none() {
            values.push(&session);
            values.extend(needles.iter().map(|n| n as &dyn rusqlite::ToSql));
            return self
                .conn
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM native_rows WHERE session_id=?1 AND role='tool' AND ({}))",
                        any("body")
                    ),
                    values.as_slice(),
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string());
        }
        let original = self.persisted_session_id(session)?;
        values.push(&original);
        values.extend(needles.iter().map(|n| n as &dyn rusqlite::ToSql));
        let window: bool = self
            .conn
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND ({}))",
                    any("blocks_json")
                ),
                values.as_slice(),
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if window {
            return Ok(true);
        }
        // Long Mono conversations keep older history block by block.
        let paged: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='mono_blocks')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !paged {
            return Ok(false);
        }
        self.conn
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM mono_blocks WHERE session_id=?1 AND ({}))",
                    any("block_json")
                ),
                values.as_slice(),
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// The contributions these sessions made since `since` (Unix ms), oldest first.
    pub fn contributions(&self, sessions: &[i64], since: u64) -> Result<Vec<Contribution>, String> {
        let Some(conn) = self.contributions_db()? else {
            return Ok(Vec::new());
        };
        if sessions.is_empty() {
            return Ok(Vec::new());
        }
        let ids = sessions
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut query = conn
            .prepare(&format!(
                "SELECT session_id,kind,reference,at,additions,deletions FROM contributions
                 WHERE session_id IN ({ids}) AND at>=?1 ORDER BY at"
            ))
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([since as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let (session, kind, reference, at, additions, deletions) =
                row.map_err(|e| e.to_string())?;
            if let Some(kind) = Kind::parse(&kind) {
                out.push(Contribution {
                    session,
                    kind,
                    reference,
                    at: at.max(0) as u64,
                    additions: additions.max(0) as u64,
                    deletions: deletions.max(0) as u64,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(session: i64, kind: Kind, reference: &str, at: u64) -> Contribution {
        Contribution {
            session,
            kind,
            reference: reference.into(),
            at,
            additions: 3,
            deletions: 1,
        }
    }

    #[test]
    fn sessions_are_searched_for_commands_without_loading_them() {
        let dir = tempfile::tempdir().unwrap();
        let shared = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        let legacy = Store::open(std::path::Path::new(":memory:")).unwrap();
        for mut store in [shared, legacy] {
            let id = store.create("codex", "/tmp").unwrap().id;
            let mut tool = crate::tool::ToolCall::new("t", "Bash");
            tool.input = serde_json::json!({"command": "git commit -m x"}).to_string();
            tool.state = crate::tool::ToolState::Completed;
            store
                .write_rows(
                    id,
                    0,
                    &[
                        crate::transcript::Row {
                            role: "user".into(),
                            text: "Commit it".into(),
                            first: true,
                            sent_at: None,
                        },
                        crate::transcript::Row {
                            role: "tool".into(),
                            text: serde_json::to_string(&tool).unwrap(),
                            first: true,
                            sent_at: None,
                        },
                    ],
                )
                .unwrap();
            assert!(store
                .session_mentions(id, &["gh pr", "git commit"])
                .unwrap());
            assert!(!store.session_mentions(id, &["gh pr"]).unwrap());
        }
    }

    #[test]
    fn contributions_record_once_and_amends_replace_the_latest_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "aaa", 10), false)
            .unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "aaa", 10), false)
            .unwrap();
        store
            .record_contribution(&item(2, Kind::Review, "acme/app#1@c", 20), false)
            .unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "bbb", 30), true)
            .unwrap();
        let all = store.contributions(&[1, 2], 0).unwrap();
        assert_eq!(
            all.iter().map(|c| c.reference.as_str()).collect::<Vec<_>>(),
            ["acme/app#1@c", "bbb"]
        );
        assert_eq!(store.contributions(&[2], 0).unwrap().len(), 1);
        assert!(store.contributions(&[1], 31).unwrap().is_empty());
        // The desktop database itself gains no table.
        drop(store);
        let desktop = Connection::open(dir.path().join("monocode.db")).unwrap();
        let tables: i64 = desktop
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='contributions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
    }
}
