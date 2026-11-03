use super::{Session, Store};
use serde_json::{json, Value};

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

    pub fn app_conversation(
        &self,
        id: i64,
        input: &Value,
        busy: bool,
        has_draft: bool,
    ) -> Result<Value, String> {
        let blocks = if self.shared.is_some() {
            self.shared_blocks(id)?
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
            blocks
        };
        let mut page = crate::app_access::conversation_page(&blocks, input)?;
        page["sessionId"] = json!(id);
        page["busy"] = json!(busy);
        page["hasDraft"] = json!(has_draft || blocks.iter().any(|b| b["draft"] == true));
        if let Some(session) = self.sessions()?.into_iter().find(|s| s.id == id) {
            page["title"] = json!(session.title);
        }
        Ok(page)
    }
}
