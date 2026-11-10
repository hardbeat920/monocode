//! Durable composer messages. Delivery receipts live beside the transcript so a
//! crash between provider acceptance and the UI update cannot resend a message.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<u64>,
    /// Snapshot transport context without exposing it as the editable message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_prompt: Option<String>,
    pub attachments: Vec<PathBuf>,
    pub mode: Option<String>,
    /// Insertion point for a saved draft, independent of provider history.
    pub at: usize,
    pub error: Option<String>,
    pub steer: bool,
}
impl Message {
    pub fn label(&self) -> String {
        if self.text.trim().is_empty() {
            format!(
                "{} attachment{}",
                self.attachments.len(),
                if self.attachments.len() == 1 { "" } else { "s" }
            )
        } else {
            self.text.trim().to_owned()
        }
    }
    /// Editing ordinary text keeps snapshotted MCP context while its selected
    /// tokens remain present, including after restoring a saved queue.
    pub fn replace_text(&mut self, text: String, prepared: String) {
        let context = self
            .prepared_prompt
            .as_deref()
            .and_then(|prepared| prepared.strip_suffix(self.text.as_str()))
            .filter(|prefix| prefix.starts_with("MCP context:"))
            .map(str::to_owned);
        let tokens = |text: &str| {
            text.lines()
                .filter(|line| !line.trim_start().starts_with('>'))
                .flat_map(str::split_whitespace)
                .map(|word| word.trim_matches(|c: char| "()[]{}.!?;,".contains(c)))
                .filter(|word| word.starts_with("@mcp/"))
                .map(str::to_owned)
                .collect::<std::collections::HashSet<_>>()
        };
        let previous = tokens(&self.text);
        self.prepared_prompt = if prepared != text {
            Some(prepared)
        } else if !previous.is_empty() && previous == tokens(&text) {
            context.map(|prefix| format!("{prefix}{text}"))
        } else {
            None
        };
        self.text = text;
    }
    pub fn can_steer(&self) -> bool {
        self.mode.is_none()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub revision: u64,
    pub messages: Vec<Message>,
    pub paused: bool,
    pub draft: Option<Message>,
}

impl State {
    /// Snapshot picked files just once, as React persists queued attachment
    /// bytes. A later rename, edit or deletion of the original must not change
    /// the message the user already saved.
    pub fn retain_attachments(
        &mut self,
        session: i64,
        directory: Option<&std::path::Path>,
    ) -> Result<(), String> {
        use std::hash::{Hash, Hasher};
        let Some(directory) = directory else {
            return Ok(());
        };
        for message in self.messages.iter_mut().chain(self.draft.iter_mut()) {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            message.id.hash(&mut hash);
            let root = directory
                .join("composer-messages")
                .join(session.to_string())
                .join(format!("{:016x}", hash.finish()));
            for (index, source) in message.attachments.iter_mut().enumerate() {
                let name = source.file_name().ok_or("An attachment has no file name")?;
                let parent = root.join(index.to_string());
                let target = parent.join(name);
                if *source == target {
                    continue;
                }
                if !target.exists() {
                    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
                    let temporary = parent.join(format!(".copy-{}", crate::mono::new_id()));
                    if let Err(error) = std::fs::copy(&*source, &temporary)
                        .and_then(|_| std::fs::rename(&temporary, &target))
                    {
                        let _ = std::fs::remove_file(&temporary);
                        return Err(format!("Could not retain {}: {error}", source.display()));
                    }
                }
                *source = target;
            }
        }
        Ok(())
    }
}

impl crate::store::Store {
    pub fn follow_ups(&self, session: i64) -> Result<State, String> {
        let mut state: State = if self.uses_monocode_schema() && session >= 0 {
            self.shared_follow_ups(session)?
        } else {
            self.setting(&format!("follow-up:{session}"))?
                .map(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
                .transpose()?
                .unwrap_or_default()
        };
        let mut remaining = Vec::new();
        for message in state.messages {
            if self
                .setting(&format!("mono-delivered:{session}:{}", message.id))?
                .is_none()
            {
                remaining.push(message);
            }
        }
        state.messages = remaining;
        if let Some(draft) = &state.draft {
            if self
                .setting(&format!("mono-delivered:{session}:{}", draft.id))?
                .is_some()
            {
                state.draft = None;
            }
        }
        // Recovery is always explicit, as in React's deserializeSession.
        state.paused = !state.messages.is_empty();
        Ok(state)
    }
    pub fn save_follow_ups(&self, session: i64, state: &State) -> Result<(), String> {
        if self.uses_monocode_schema() && session >= 0 {
            return self.shared_save_follow_ups(session, state);
        }
        let key = format!("follow-up:{session}");
        let previous = self
            .setting(&key)?
            .and_then(|text| serde_json::from_str::<State>(&text).ok());
        if previous.is_some_and(|previous| previous.revision > state.revision) {
            return Ok(());
        }
        self.save_setting(
            &key,
            &serde_json::to_string(state).map_err(|e| e.to_string())?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::Store, transcript::Transcript};
    #[test]
    fn prepared_context_is_durable_without_changing_the_visible_message() {
        let mut message = Message {
            text: "Use @mcp/docs".into(),
            prepared_prompt: Some("MCP context: Use docs.\n\nUse @mcp/docs".into()),
            ..Default::default()
        };
        let restored: Message =
            serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
        assert_eq!(restored.label(), "Use @mcp/docs");
        assert!(restored.prepared_prompt.is_some());
        message.replace_text(
            "Ask @mcp/docs instead".into(),
            "Ask @mcp/docs instead".into(),
        );
        assert_eq!(
            message.prepared_prompt.as_deref(),
            Some("MCP context: Use docs.\n\nAsk @mcp/docs instead")
        );
        message.replace_text("Ask without a server".into(), "Ask without a server".into());
        assert!(message.prepared_prompt.is_none());
    }
    #[test]
    fn prepared_context_never_serializes_into_agent_settings() {
        let settings = crate::agent::Settings {
            prepared_prompt: Some("MCP context: transport only".into()),
            ..Default::default()
        };
        assert!(!serde_json::to_string(&settings)
            .unwrap()
            .contains("transport only"));
    }
    #[test]
    fn recovery_preserves_attachments_and_edits_but_never_replays_delivered_messages() {
        let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/tmp").unwrap();
        let message = Message {
            id: "first".into(),
            text: "edited 🙂".into(),
            attachments: vec!["/tmp/photo.png".into()],
            ..Default::default()
        };
        let mut state = State {
            revision: 2,
            messages: vec![message.clone()],
            draft: Some(Message {
                id: "draft".into(),
                text: "later".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        store.save_follow_ups(session.id, &state).unwrap();
        let recovered = store.follow_ups(session.id).unwrap();
        assert!(recovered.paused);
        assert_eq!(recovered.messages, vec![message]);
        state.revision = 1;
        state.messages.clear();
        store.save_follow_ups(session.id, &state).unwrap();
        assert_eq!(store.follow_ups(session.id).unwrap().messages.len(), 1);
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.append("delivered");
        store
            .write_rows_with_message(session.id, 0, &transcript.rows, Some("first"))
            .unwrap();
        assert!(store.follow_ups(session.id).unwrap().messages.is_empty());
        assert!(store.follow_ups(session.id).unwrap().draft.is_some());
    }

    #[test]
    fn retained_files_survive_original_edits_and_deletion_without_changing_their_names() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("design.txt");
        std::fs::write(&file, "original content").unwrap();
        let message = Message {
            id: "message".into(),
            attachments: vec![file.clone()],
            ..Default::default()
        };
        let mut state = State {
            messages: vec![message.clone()],
            ..Default::default()
        };
        let data = temp.path().join("data");
        state.retain_attachments(1, Some(&data)).unwrap();
        let retained = state.messages[0].attachments[0].clone();
        assert_eq!(retained.file_name(), file.file_name());
        std::fs::write(&file, "changed").unwrap();
        // A newer UI snapshot may still contain the original path.
        state.messages[0] = message;
        state.retain_attachments(1, Some(&data)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&retained).unwrap(),
            "original content"
        );
        std::fs::remove_file(&file).unwrap();
        state.retain_attachments(1, Some(&data)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&retained).unwrap(),
            "original content"
        );
    }
}
