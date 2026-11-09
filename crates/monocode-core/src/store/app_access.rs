use super::{remote::RemoteSession, Session, Store};
use crate::remote::Connections;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

impl Store {
    /// A launch is recoverable as an unsent draft if the app exits before its
    /// provider accepts it. Queue delivery uses this same stable message ID.
    /// `draft` records that the launch saves the prompt rather than sending it,
    /// so a remote launch resumed after a restart does the same.
    pub fn prepare_app_prompt(
        &self,
        id: i64,
        request: &str,
        prompt: &str,
        draft: bool,
    ) -> Result<(), String> {
        let mut state = self.follow_ups(id)?;
        let message_id = format!("{APP_PROMPT}{}", crate::mono::files::content_hash(request));
        if state.draft.is_none() && !state.messages.iter().any(|m| m.id == message_id) {
            state.revision += 1;
            state.draft = Some(crate::follow_up::Message {
                id: message_id,
                text: prompt.into(),
                ..Default::default()
            });
            self.save_follow_ups(id, &state)?;
            if draft {
                self.save_setting(&format!("{APP_LAUNCH_DRAFT}{id}"), "1")?;
            }
        }
        Ok(())
    }
    /// A remote app launch's saved prompt that the host does not have yet, and
    /// whether to send it (`true`) or save it as a draft.
    ///
    /// The local prompt is the launch's only durable record until the remote
    /// runtime journals its commands. Once the outbox holds them, the host has
    /// the session, or the host rejected them, the local copy is dropped here.
    pub fn pending_remote_app_launch(&self, id: i64) -> Result<Option<(String, bool)>, String> {
        let Some(remote) = self.remote_session(id)? else {
            return Ok(None);
        };
        let state = self.follow_ups(id)?;
        let Some(draft) = state
            .draft
            .as_ref()
            .filter(|draft| draft.id.starts_with(APP_PROMPT))
        else {
            return Ok(None);
        };
        if remote.session_id.is_some() || !remote.outbox.is_empty() || remote.rejected.is_some() {
            let text = draft.text.clone();
            self.finish_remote_app_launch(id, &text)?;
            return Ok(None);
        }
        let send = self.setting(&format!("{APP_LAUNCH_DRAFT}{id}"))?.is_none();
        Ok(Some((draft.text.clone(), send)))
    }
    /// Drops a remote app launch's local prompt once `text`, its first turn,
    /// is journaled in the session's remote outbox.
    pub fn finish_remote_app_launch(&self, id: i64, text: &str) -> Result<(), String> {
        let mut state = self.follow_ups(id)?;
        if !state
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id.starts_with(APP_PROMPT) && draft.text == text)
        {
            return Ok(());
        }
        state.revision += 1;
        state.draft = None;
        self.save_follow_ups(id, &state)?;
        self.delete_settings(&[format!("{APP_LAUNCH_DRAFT}{id}")], None)
    }
    /// Reserve a request's child durably before its UI/provider is installed.
    /// Shared stores use a deterministic desktop ID; legacy stores reserve their
    /// numeric ID and mapping in the same SQLite transaction.
    pub fn create_app_session(
        &self,
        caller: i64,
        request: &str,
        fingerprint: &str,
        provider: &str,
        project: &str,
    ) -> Result<(Session, bool), String> {
        let key = format!(
            "operator-app-launch:{caller}:{}",
            crate::mono::files::content_hash(request)
        );
        let previous = self
            .setting(&key)?
            .map(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
            .transpose()?;
        if previous
            .as_ref()
            .is_some_and(|v| v["fingerprint"] != fingerprint)
        {
            return Err("Request ID was already used with different input".into());
        }
        if let Some(id) = previous.as_ref().and_then(|v| v["sessionId"].as_i64()) {
            return self
                .sessions()?
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| (s, false))
                .ok_or("The session created by this request was removed".into());
        }
        if self.shared.is_some() {
            self.save_setting(&key, &json!({"fingerprint":fingerprint}).to_string())?;
            let original = format!(
                "operator-{}",
                crate::mono::files::content_hash(&format!("{caller}:{request}"))
            );
            let result = self.shared_create_named(provider, project, &original)?;
            self.save_setting(
                &key,
                &json!({"fingerprint":fingerprint,"sessionId":result.0.id}).to_string(),
            )?;
            return Ok(result);
        }
        let transaction = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let session = self.create(provider, project)?;
        self.save_setting(
            &key,
            &json!({"fingerprint":fingerprint,"sessionId":session.id}).to_string(),
        )?;
        transaction.commit().map_err(|e| e.to_string())?;
        Ok((session, true))
    }

    /// Reads what `sessions.read` needs from the store. A remote session's
    /// blocks live on its host; [`AppConversation::page`] fetches them, so the
    /// caller can release the store lock before that network request.
    pub fn app_conversation(&self, id: i64) -> Result<AppConversation, String> {
        let source = if let Some(remote) = self.remote_session(id)? {
            // A remote handle has no row in the local sessions table.
            Source::Remote(Box::new(remote))
        } else if self.shared.is_some() {
            Source::Blocks(self.shared_blocks(id)?)
        } else {
            let mut blocks: Vec<Value> = Vec::new();
            for (i, row) in self.load(id)?.rows.into_iter().enumerate() {
                if !row.first && !blocks.is_empty() {
                    let block = blocks.last_mut().unwrap();
                    let text = block["text"].as_str().unwrap_or("").to_owned() + &row.text;
                    block["text"] = json!(text);
                } else {
                    blocks.push(json!({"id":format!("row-{i}"),"role":row.role,"text":row.text}));
                }
            }
            Source::Blocks(blocks)
        };
        let title = self
            .sessions()?
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.title);
        Ok(AppConversation { id, title, source })
    }
}

/// Follow-up draft IDs of prompts saved by `sessions.start`.
const APP_PROMPT: &str = "app-prompt-";
/// Marks a launch whose prompt is saved as a draft instead of sent.
pub(super) const APP_LAUNCH_DRAFT: &str = "app-launch-draft:";

/// How long `sessions.read` waits for a remote session's host.
pub const REMOTE_READ_TIMEOUT: Duration = Duration::from_secs(20);

/// A session read taken from the store, not yet paged.
pub struct AppConversation {
    id: i64,
    title: Option<String>,
    source: Source,
}

enum Source {
    Blocks(Vec<Value>),
    Remote(Box<RemoteSession>),
}

impl AppConversation {
    /// Pages the conversation, fetching a remote session's blocks from its
    /// host through `client` within `timeout`. Call without the store lock.
    pub fn page(
        self,
        client: &Arc<Connections>,
        timeout: Duration,
        input: &Value,
        busy: bool,
        has_draft: bool,
    ) -> Result<Value, String> {
        let blocks = match self.source {
            Source::Blocks(blocks) => blocks,
            // Not sent yet: the host has nothing to return.
            Source::Remote(remote) if remote.session_id.is_none() => Vec::new(),
            Source::Remote(remote) => {
                let (sender, receiver) = std::sync::mpsc::channel();
                let client = client.clone();
                // The request itself may take up to its own 30 s timeout, plus
                // a tunnel start; let it finish in the background past ours.
                std::thread::spawn(move || {
                    let _ = sender.send(remote.host_snapshot(&client));
                });
                match receiver.recv_timeout(timeout) {
                    Ok(snapshot) => snapshot?
                        .and_then(|snapshot| snapshot["session"]["blocks"].as_array().cloned())
                        .unwrap_or_default(),
                    Err(_) => {
                        return Err(format!(
                            "The machine did not return this session within {} seconds. \
                             Check its connection in Settings → Connections, then read it again.",
                            timeout.as_secs()
                        ))
                    }
                }
            }
        };
        let mut page = crate::app_access::conversation_page(&blocks, input)?;
        page["sessionId"] = json!(self.id);
        page["busy"] = json!(busy);
        page["hasDraft"] = json!(has_draft || blocks.iter().any(|b| b["draft"] == true));
        if let Some(title) = self.title {
            page["title"] = json!(title);
        }
        Ok(page)
    }
}
