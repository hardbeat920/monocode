//! Code contributions per session, kept in the native sidecar so the shared
//! desktop schema is untouched. A commit or pull request is recorded once,
//! attributed to the first session that records it. Legacy abbreviated IDs
//! lack checkout provenance and are retained even if a full ID is later recorded.
//! Amended commits remain separate when their predecessor cannot be established.
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

    /// Records a contribution once, keeping the first session's attribution.
    pub fn record_contribution(&self, item: &Contribution) -> Result<(), String> {
        let Some(conn) = self.contributions_db()? else {
            return Err("Contribution database is unavailable".into());
        };
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

    /// Complete only the scan that still owns this marker, including across processes.
    pub fn complete_contribution_scan(
        &self,
        session: i64,
        scan_id: &str,
        version: &str,
    ) -> Result<bool, String> {
        let key = format!("{}{session}", crate::contributions::SCANNED);
        let conn = self
            .native_db()
            .ok_or("Contribution database is unavailable")?;
        let table = if self.shared.is_some() {
            "settings"
        } else {
            "native_settings"
        };
        conn.execute(
            &format!("UPDATE {table} SET value=?1 WHERE key=?2 AND value=?3"),
            params![version, key, scan_id],
        )
        .map(|count| count == 1)
        .map_err(|e| e.to_string())
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
    fn scan_markers_are_visible_between_store_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let first = Store::open_monocode(&path).unwrap();
        let second = Store::open_monocode(&path).unwrap();
        let key = format!("{}1", crate::contributions::SCANNED);
        first.save_setting(&key, "done").unwrap();
        assert_eq!(second.setting(&key).unwrap().as_deref(), Some("done"));
        second.save_setting(&key, "0").unwrap();
        assert_eq!(first.setting(&key).unwrap().as_deref(), Some("0"));
        first.save_setting(&key, "scanning").unwrap();
        second.save_setting(&key, "0").unwrap();
        assert!(!first
            .complete_contribution_scan(1, "scanning", "done")
            .unwrap());
        assert_eq!(second.setting(&key).unwrap().as_deref(), Some("0"));
        first.save_setting(&key, "scanning").unwrap();
        assert!(first
            .complete_contribution_scan(1, "scanning", "done")
            .unwrap());
        assert_eq!(second.setting(&key).unwrap().as_deref(), Some("done"));
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
    fn git_amend_on_another_branch_keeps_both_commits_in_any_arrival_order() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| crate::git::git(&repo, args).unwrap();
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "mono@example.com"]);
        git(&["config", "user.name", "Mono"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&["commit", "--allow-empty", "-m", "initial"]);
        git(&["checkout", "-qb", "other"]);
        let other = git(&["commit", "--allow-empty", "-m", "other"]);
        let other_id = git(&["rev-parse", "HEAD"]).trim().to_owned();
        git(&["checkout", "-q", "main"]);
        let amended = git(&["commit", "--amend", "--allow-empty", "-m", "amended"]);
        let amended_id = git(&["rev-parse", "HEAD"]).trim().to_owned();
        let resolve = |command: &str, output: &str| {
            let mut tool = crate::tool::ToolCall::new("call", "Bash");
            tool.input = serde_json::json!({"command": command}).to_string();
            tool.output = output.into();
            tool.state = crate::tool::ToolState::Completed;
            let found = crate::contributions::found(&tool);
            assert_eq!(found.len(), 1);
            let (item, resolved) = crate::contributions::resolve(&found[0], 1, &repo, 0);
            assert!(resolved);
            item
        };
        let items = [
            resolve("git commit -m other", &other),
            resolve("git commit --amend -m amended", &amended),
        ];
        assert_eq!(items[0].reference, other_id);
        assert_eq!(items[1].reference, amended_id);
        for (ix, order) in [[0, 1], [1, 0]].iter().enumerate() {
            let store = Store::open_monocode(&root.path().join(format!("{ix}.db"))).unwrap();
            for i in order {
                store.record_contribution(&items[*i]).unwrap();
            }
            let all = store.contributions(&[1], 0).unwrap();
            assert_eq!(all.len(), 2);
            assert!(all.iter().any(|item| item.reference == other_id));
            assert!(all.iter().any(|item| item.reference == amended_id));
        }
    }

    #[test]
    fn batch_and_out_of_order_amends_preserve_unrelated_and_legacy_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        let mut tool = crate::tool::ToolCall::new("batch", "Bash");
        tool.input =
            serde_json::json!({"command":"git commit -m one && git commit --amend --no-edit"})
                .to_string();
        tool.output = "[main 1111111] one\n[main 2222222] two".into();
        tool.state = crate::tool::ToolState::Completed;
        let commits = crate::contributions::found(&tool);
        store
            .record_contribution(&item(1, Kind::Commit, "3333333", 1))
            .unwrap();
        for found in commits.iter().rev() {
            store
                .record_contribution(&item(1, Kind::Commit, &found.reference, 2))
                .unwrap();
        }
        store
            .record_contribution(&item(1, Kind::Commit, "1111111abcdef", 3))
            .unwrap();
        store
            .record_contribution(&item(2, Kind::Commit, "1111111abcdef", 4))
            .unwrap();
        let all = store.contributions(&[1, 2], 0).unwrap();
        assert_eq!(all.len(), 4);
        assert!(all.iter().any(|c| c.reference == "3333333"));
        assert!(all.iter().any(|c| c.reference == "1111111"));
        assert!(all.iter().any(|c| c.reference == "2222222"));
        assert!(all
            .iter()
            .any(|c| c.reference == "1111111abcdef" && c.session == 1));
        assert!(store.contributions(&[2], 0).unwrap().is_empty());
    }

    #[test]
    fn contributions_record_once_without_replacing_other_commits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "aaa", 10))
            .unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "aaa", 10))
            .unwrap();
        store
            .record_contribution(&item(2, Kind::Review, "acme/app#1@c", 20))
            .unwrap();
        store
            .record_contribution(&item(1, Kind::Commit, "bbb", 30))
            .unwrap();
        let all = store.contributions(&[1, 2], 0).unwrap();
        assert_eq!(
            all.iter().map(|c| c.reference.as_str()).collect::<Vec<_>>(),
            ["aaa", "acme/app#1@c", "bbb"]
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
