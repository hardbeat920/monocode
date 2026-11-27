//! Bounded hover-card metadata. Never read transcripts or discover agent state.
use super::Store;
use rusqlite::OptionalExtension;

#[derive(Clone, Debug, Default)]
pub struct SessionPreviewData {
    pub model: Option<String>,
    pub worktree_cwd: Option<String>,
}

impl Store {
    pub fn session_preview(&self, id: i64) -> Result<SessionPreviewData, String> {
        let result = if self.shared.is_some() {
            // Old desktop schemas may not have worktree metadata. Read just
            // these two scalar columns, never blocks_json/model_settings.
            let cwd = self.shared_field("worktree_cwd", "NULL");
            self.conn
                .query_row(
                    &format!(
                        "SELECT substr(model,1,512),substr({cwd},1,4096) FROM sessions WHERE id=?1"
                    ),
                    [self.persisted_session_id(id)?],
                    |row| {
                        Ok(SessionPreviewData {
                            model: row.get(0)?,
                            worktree_cwd: row.get(1)?,
                        })
                    },
                )
                .optional()
        } else {
            self.conn.query_row(
                "SELECT CASE WHEN json_valid(value) THEN substr(json_extract(value,'$.model'),1,512) END,
                 CASE WHEN json_valid(value) THEN substr(json_extract(value,'$.worktree_cwd'),1,4096) END
                 FROM native_settings WHERE key='agent:'||?1",
                [id],
                |row| Ok(SessionPreviewData { model: row.get(0)?, worktree_cwd: row.get(1)? }),
            ).optional()
        };
        result
            .map(|value| value.unwrap_or_default())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn desktop_preview_never_decodes_blocks_or_model_controls() {
        let store = Store::open_monocode(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let original = store.persisted_session_id(session.id).unwrap();
        store.conn.execute("UPDATE sessions SET model='codex:gpt-6-sol',model_settings='broken JSON',blocks_json='broken JSON',worktree_cwd='/linked' WHERE id=?1", [original]).unwrap();
        let preview = store.session_preview(session.id).unwrap();
        assert_eq!(preview.model.as_deref(), Some("codex:gpt-6-sol"));
        assert_eq!(preview.worktree_cwd.as_deref(), Some("/linked"));
    }

    #[test]
    fn preview_reads_only_bounded_settings_without_history() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        store.conn.execute("INSERT INTO native_settings(key,value) VALUES('agent:'||?1,?2)",
            rusqlite::params![session.id, serde_json::json!({"model":"m".repeat(1000),"worktree_cwd":"/worktree","unknown":"ignored"}).to_string()]).unwrap();
        // The preview must still work when full history cannot be read.
        store.conn.execute_batch("DROP TABLE native_rows").unwrap();
        let preview = store.session_preview(session.id).unwrap();
        assert_eq!(preview.model.unwrap().len(), 512);
        assert_eq!(preview.worktree_cwd.as_deref(), Some("/worktree"));
    }

    #[test]
    fn absent_or_invalid_saved_settings_use_safe_defaults() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        assert!(store.session_preview(session.id).unwrap().model.is_none());
        store
            .conn
            .execute(
                "INSERT INTO native_settings(key,value) VALUES('agent:'||?1,'malformed JSON')",
                [session.id],
            )
            .unwrap();
        assert!(store.session_preview(session.id).unwrap().model.is_none());
    }
}
