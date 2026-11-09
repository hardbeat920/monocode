use super::{remote::RemoteSession, Session, Store};
use crate::remote::Connections;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

impl Store {
    /// A launch is recoverable as an unsent draft if the app exits before its
    /// provider accepts it. Queue delivery uses this same stable message ID.
    pub fn prepare_app_prompt(&self, id: i64, request: &str, prompt: &str) -> Result<(), String> {
        let mut state = self.follow_ups(id)?;
        let message_id = format!("app-prompt-{}", crate::mono::files::content_hash(request));
        if state.draft.is_none() && !state.messages.iter().any(|m| m.id == message_id) {
            state.revision += 1;
            state.draft = Some(crate::follow_up::Message {
                id: message_id,
                text: prompt.into(),
                ..Default::default()
            });
            self.save_follow_ups(id, &state)?;
        }
        Ok(())
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
