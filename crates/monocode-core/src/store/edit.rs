//! Two-phase edit: capture a revision, then trim only after provider confirmation.
use super::*;
use crate::provider::Kind;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct EditSnapshot {
    pub start: usize,
    pub prompt: String,
    pub attachments: Vec<PathBuf>,
    pub provider_turn_id: Option<String>,
    pub provider_id: String,
    pub account_id: String,
    pub cwd: String,
    pub rows: Vec<Row>,
    pub(super) blocks: Option<Vec<Value>>,
    pub(super) block_start: usize,
    pub(super) provider: String,
}
impl EditSnapshot {
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn matches(&self, rows: &[Row]) -> bool {
        self.rows == rows
    }
}

pub(super) fn codex_turn_start(
    rows: &[Row],
    settings: &crate::agent::Settings,
    id: &str,
) -> Option<usize> {
    let response = settings
        .turns
        .iter()
        .filter(|(_, info)| info.provider_turn_id.as_deref() == Some(id))
        .map(|(&row, _)| row)
        .min()?;
    rows.iter()
        .enumerate()
        .take(response)
        .rev()
        .find(|(_, row)| row.role == "user" && row.first)
        .map(|(row, _)| row)
}

/// Native history previously rendered files as trailing local Markdown links,
/// without retaining the attachment payload. Re-sending that text alone would
/// silently drop images. Refuse ambiguous local-file tails until a manifest is
/// available; rich desktop attachment lists are restored separately.
pub(super) fn untracked_attachment_tail(prompt: &str) -> bool {
    let Some(tail) = prompt.rsplit("\n\n").next() else {
        return false;
    };
    let Some(path) = tail
        .strip_prefix('[')
        .and_then(|tail| tail.split_once("](<"))
        .and_then(|(_, path)| path.strip_suffix(">)"))
    else {
        return false;
    };
    Path::new(path).is_absolute()
        || path.starts_with('/')
        || path.starts_with("\\\\")
        || path.as_bytes().get(1) == Some(&b':')
}

impl Store {
    /// Durable guard for provider context that diverged after a confirmed edit.
    pub fn edit_recovery_reason(&self, id: i64) -> Result<Option<String>, String> {
        Ok(self
            .setting(&format!("edit-recovery:{id}"))?
            .filter(|reason| !reason.is_empty()))
    }
    pub fn mark_edit_recovery_required(&self, id: i64) -> Result<(), String> {
        self.save_setting(&format!("edit-recovery:{id}"),"A previous edit changed provider context before local history could be reconciled. Start a new conversation; the saved history is preserved here.")
    }
    fn clear_edit_recovery(&self, id: i64) -> Result<(), String> {
        self.save_setting(&format!("edit-recovery:{id}"), "")
    }
    /// `expected` is the visible transcript; never recall an unloaded revision.
    pub fn prepare_edit_last_turn(
        &self,
        id: i64,
        expected: &[Row],
        turn_id: Option<&str>,
    ) -> Result<EditSnapshot, String> {
        if let Some(reason) = self.edit_recovery_reason(id)? {
            return Err(reason);
        }
        if self.shared.is_some() {
            return self.shared_prepare_edit(id, expected, turn_id);
        }
        let session = self
            .sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or("Conversation no longer exists")?;
        if !crate::provider::supports_rewind(Kind::from_id(&session.provider)) {
            return Err("This provider does not support editing the last turn".into());
        }
        let rows = self.load(id)?.rows;
        if rows != expected {
            return Err("Conversation changed; reopen it before editing".into());
        }
        self.edit_queue_empty(id)?;
        let latest = rows
            .iter()
            .rposition(|r| r.role == "user" && r.first)
            .ok_or("No user message to edit")?;
        let end = (latest + 1..rows.len())
            .find(|&i| rows[i].first)
            .unwrap_or(rows.len());
        let provider_turn_id = turn_id
            .filter(|id| !id.trim().is_empty())
            .map(str::to_owned);
        if session.provider == "codex" && provider_turn_id.is_none() {
            return Err("Codex did not expose an exact user turn to edit".into());
        }
        let settings = self.agent_settings(id)?.unwrap_or_default();
        let start = if session.provider == "codex" {
            codex_turn_start(&rows, &settings, provider_turn_id.as_deref().unwrap())
                .or_else(|| {
                    (rows
                        .iter()
                        .filter(|row| row.role == "user" && row.first)
                        .count()
                        == 1)
                        .then_some(latest)
                })
                .ok_or("Codex user-turn boundaries are unavailable; history was preserved")?
        } else {
            latest
        };
        let prompt: String = rows[latest..end].iter().map(|r| r.text.as_str()).collect();
        let attachments = rows
            .get(end)
            .map(crate::transcript::attachments::paths)
            .transpose()?
            .unwrap_or_default();
        if attachments.is_empty() && untracked_attachment_tail(&prompt) {
            return Err("This message has attachment links without a saved attachment manifest and cannot be edited safely".into());
        }
        Ok(EditSnapshot {
            start,
            prompt,
            attachments,
            provider_turn_id,
            provider_id: session.provider_id.ok_or("No provider session to edit")?,
            account_id: settings.account.id,
            cwd: settings.worktree_cwd.unwrap_or(session.cwd),
            rows,
            blocks: None,
            block_start: 0,
            provider: session.provider,
        })
    }
    pub(super) fn edit_queue_empty(&self, id: i64) -> Result<(), String> {
        if self.shared.is_some() {
            return self.shared_edit_queue_empty(id);
        }
        let queue = self.follow_ups(id)?;
        if !queue.messages.is_empty() || queue.draft.is_some() {
            return Err(
                "Clear queued messages and saved drafts before editing the last message".into(),
            );
        }
        Ok(())
    }
    /// Checks again immediately before the external RPC, without retaining the
    /// database mutex while waiting on provider I/O.
    pub fn validate_edit_last_turn(&self, id: i64, edit: &EditSnapshot) -> Result<(), String> {
        let session = self
            .sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or("Conversation no longer exists")?;
        if session.provider != edit.provider
            || session.provider_id.as_deref() != Some(edit.provider_id.as_str())
        {
            return Err(
                "Provider session changed; cancel the edit and reopen this conversation".into(),
            );
        }
        let settings = self.agent_settings(id)?.unwrap_or_default();
        if settings.account.id != edit.account_id
            || settings.worktree_cwd.as_deref().unwrap_or(&session.cwd) != edit.cwd
        {
            return Err(
                "The provider account or working directory changed; cancel the edit before sending"
                    .into(),
            );
        }
        if let Some(expected) = &edit.blocks {
            self.shared_validate_edit_rows(id, expected, &edit.rows)?;
        } else if self.load(id)?.rows != edit.rows {
            return Err("Conversation changed; its history was preserved".into());
        }
        self.edit_queue_empty(id)?;
        Ok(())
    }
    /// Call only after `rewind_last_turn` confirms. This is a compare-and-swap,
    /// preserving an independently changed conversation instead of deleting it.
    pub fn finish_edit_last_turn(
        &mut self,
        id: i64,
        edit: &EditSnapshot,
        provider_id: &str,
    ) -> Result<(), String> {
        // This API is called only after provider confirmation. A failed local
        // transaction or crash must leave a durable guard against resuming it.
        self.mark_edit_recovery_required(id)?;
        if self.shared.is_some() {
            self.shared_finish_edit(id, edit, provider_id)?;
            return self.clear_edit_recovery(id);
        }
        let start = i64::try_from(edit.start)
            .map_err(|_| "Conversation row index exceeds SQLite's integer range".to_owned())?;
        let updated_at = crate::time::now_ms().min(i64::MAX as u64) as i64;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        self.validate_edit_last_turn(id, edit)?;
        self.conn
            .execute(
                "DELETE FROM native_rows WHERE session_id=?1 AND row_ix>=?2",
                params![id, start],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE native_sessions SET provider_id=?2,updated_at=?3 WHERE id=?1",
                params![id, provider_id, updated_at],
            )
            .map_err(|e| e.to_string())?;
        if let Some(mut settings) = self.agent_settings(id)? {
            settings.turns.retain(|&row, _| row < edit.start);
            self.save_agent_settings(id, &settings)?;
        }
        self.save_context_usage(id, &Default::default())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.clear_edit_recovery(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, i64, Vec<Row>) {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        store.provider_id(session.id, "thread").unwrap();
        let mut transcript = Transcript::default();
        for (role, text) in [
            ("user", "Earlier"),
            ("assistant", "Kept"),
            ("user", "Edit me"),
            ("assistant", "Replaced"),
        ] {
            transcript.begin(role);
            transcript.append(text);
        }
        store.write_rows(session.id, 0, &transcript.rows).unwrap();
        let mut settings = crate::agent::Settings::default();
        settings.turns.insert(
            3,
            crate::agent::TurnInfo {
                provider_turn_id: Some("turn".into()),
                ..Default::default()
            },
        );
        store.save_agent_settings(session.id, &settings).unwrap();
        (store, session.id, transcript.rows)
    }
    #[test]
    fn preparing_and_cancelling_an_edit_leaves_all_rows_intact() {
        let (store, id, rows) = fixture();
        let edit = store
            .prepare_edit_last_turn(id, &rows, Some("turn"))
            .unwrap();
        assert_eq!(edit.start, 2);
        assert_eq!(edit.prompt, "Edit me");
        assert_eq!(store.load(id).unwrap().rows, rows);
    }
    #[test]
    fn confirmed_edit_truncates_exactly_the_last_turn_and_rebinds_forks() {
        let (mut store, id, rows) = fixture();
        let edit = store
            .prepare_edit_last_turn(id, &rows, Some("turn"))
            .unwrap();
        store.finish_edit_last_turn(id, &edit, "fork").unwrap();
        assert_eq!(store.load(id).unwrap().rows, rows[..2]);
        assert_eq!(
            store.sessions().unwrap()[0].provider_id.as_deref(),
            Some("fork")
        );
    }
    #[test]
    fn late_rows_or_provider_binding_changes_are_never_overwritten() {
        let (mut store, id, rows) = fixture();
        let edit = store
            .prepare_edit_last_turn(id, &rows, Some("turn"))
            .unwrap();
        store.provider_id(id, "another-thread").unwrap();
        assert!(store.finish_edit_last_turn(id, &edit, "fork").is_err());
        assert_eq!(store.load(id).unwrap().rows, rows);
        store.provider_id(id, "thread").unwrap();
        let mut changed = rows.clone();
        changed[3].text = "Another window".into();
        store.write_rows(id, 3, &changed[3..]).unwrap();
        assert!(store.finish_edit_last_turn(id, &edit, "fork").is_err());
        assert_eq!(store.load(id).unwrap().rows, changed);
    }
    #[test]
    fn codex_steered_messages_recall_only_latest_prompt_but_replace_the_complete_provider_turn() {
        let (mut store, id, mut rows) = fixture();
        let mut transcript = Transcript { rows };
        transcript.begin("user");
        transcript.append("Latest steer");
        transcript.begin("assistant");
        transcript.append("After steer");
        rows = transcript.rows;
        store.write_rows(id, 4, &rows[4..]).unwrap();
        let mut settings = store.agent_settings(id).unwrap().unwrap();
        settings.turns.insert(
            5,
            crate::agent::TurnInfo {
                provider_turn_id: Some("turn".into()),
                ..Default::default()
            },
        );
        store.save_agent_settings(id, &settings).unwrap();
        let edit = store
            .prepare_edit_last_turn(id, &rows, Some("turn"))
            .unwrap();
        assert_eq!(edit.prompt, "Latest steer");
        assert_eq!(edit.start, 2);
        store.finish_edit_last_turn(id, &edit, "thread").unwrap();
        assert_eq!(store.load(id).unwrap().rows, rows[..2]);
        assert!(store.agent_settings(id).unwrap().unwrap().turns.is_empty());
    }
    #[test]
    fn native_attachment_links_are_never_recalled_as_text_without_the_original_files() {
        for path in [
            "/tmp/photo.png",
            r"C:\Users\me\photo.png",
            "C:/Users/me/photo.png",
            r"\\server\share\photo.png",
        ] {
            let (mut store, id, mut rows) = fixture();
            rows[2].text = format!("Describe image\n\n[photo.png](<{path}>)");
            store.write_rows(id, 2, &rows[2..]).unwrap();
            assert!(store
                .prepare_edit_last_turn(id, &rows, Some("turn"))
                .unwrap_err()
                .contains("attachment manifest"));
            assert_eq!(store.load(id).unwrap().rows, rows);
        }
        for link in [
            "A [web link](https://example.com)",
            "[photo.png](<https://example.com/photo.png>)",
            "[photo.png](<relative/photo.png>)",
        ] {
            assert!(!untracked_attachment_tail(link));
        }
    }
    #[test]
    fn provider_confirmed_conflict_leaves_a_durable_recovery_guard_and_blocks_recall() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.db");
        let mut store = Store::open(&path).unwrap();
        let session = store.create("pi", "/project").unwrap();
        store.provider_id(session.id, "session").unwrap();
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.append("Prompt");
        store.write_rows(session.id, 0, &transcript.rows).unwrap();
        let edit = store
            .prepare_edit_last_turn(session.id, &transcript.rows, None)
            .unwrap();
        store.provider_id(session.id, "another-session").unwrap();
        assert!(store
            .finish_edit_last_turn(session.id, &edit, "fork")
            .is_err());
        assert!(store.edit_recovery_reason(session.id).unwrap().is_some());
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert!(reopened.edit_recovery_reason(session.id).unwrap().is_some());
        assert!(reopened
            .prepare_edit_last_turn(session.id, &transcript.rows, None)
            .is_err());
        assert_eq!(reopened.load(session.id).unwrap().rows, transcript.rows);
    }
}
