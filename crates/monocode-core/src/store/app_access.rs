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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{completion_report, read_call, turn_end, TurnEnd};
    use std::path::Path;

    /// The report reads projected rows, where a plan or an unstructured task
    /// list shows as assistant prose; `sessions.read` reads the canonical
    /// blocks. Both must pick the same message, so the read call returns it.
    #[test]
    fn report_and_read_call_agree_on_projected_plan_and_task_messages() {
        let long = format!("{}PLAN END", "step\n".repeat(4000));
        for (trailing, text) in [
            (
                json!({"id":"p","role":"plan","text":"# Plan\n\n- Fix it"}),
                "# Plan\n\n- Fix it",
            ),
            (
                json!({"id":"t","role":"tasks","text":"1. Fix it"}),
                "1. Fix it",
            ),
            (json!({"id":"p","role":"plan","text":long}), long.as_str()),
        ] {
            let store = Store::open_monocode(Path::new(":memory:")).unwrap();
            let session = store.create("codex", "/work/repo").unwrap();
            store.provider_id(session.id, "thread").unwrap();
            let blocks = vec![
                json!({"id":"u","role":"user","text":"Plan the fix"}),
                json!({"id":"a","role":"assistant","text":"THE ANSWER"}),
                trailing,
            ];
            store.shared_store_blocks(session.id, &blocks).unwrap();
            let rows = store.load(session.id).unwrap().rows;
            let call = read_call(session.id, Some("/work/repo")).unwrap();
            let report = completion_report(&rows, TurnEnd::Unknown, Some(&call), 12000);
            let expected = crate::response::normalized(text);
            let mut input = call.clone();
            let mut read = String::new();
            loop {
                let page = store
                    .app_conversation(session.id, &input, false, false)
                    .unwrap();
                let message = &page["turns"][0]["assistant"];
                read.push_str(message["text"].as_str().unwrap());
                match message.get("nextCharOffset") {
                    Some(next) => input["charOffset"] = next.clone(),
                    None => break,
                }
            }
            assert_eq!(read, expected, "sessions.read returns the reported message");
            assert!(expected.ends_with(report.rsplit("\n\n").next().unwrap()));
            if expected.chars().count() <= 12000 {
                assert_eq!(report, expected);
            }
        }
    }

    /// A native stop leaves no row. The receipt the runtime closes records it,
    /// so the report still says the turn was stopped after a restart.
    #[test]
    fn a_stopped_turn_still_reports_unfinished_after_reopening_the_store() {
        let dir = std::env::temp_dir().join(format!(
            "monocode-stopped-{}-{}",
            std::process::id(),
            crate::time::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let progress = "I will investigate this now";
        for shared in [false, true] {
            let path = dir.join(format!("store-{shared}.sqlite"));
            let open = |path: &Path| {
                if shared {
                    Store::open_monocode(path).unwrap()
                } else {
                    Store::open(path).unwrap()
                }
            };
            let id = {
                let mut store = open(&path);
                let session = store.create("codex", "/work/repo").unwrap();
                let mut transcript = crate::transcript::Transcript::default();
                for (role, text) in [("user", "Investigate"), ("assistant", progress)] {
                    transcript.begin(role);
                    transcript.append(text);
                }
                store.write_rows(session.id, 0, &transcript.rows).unwrap();
                let mut settings = store
                    .agent_settings(session.id)
                    .unwrap()
                    .unwrap_or_default();
                settings.turns.insert(
                    1,
                    crate::agent::TurnInfo {
                        started_at: Some(1),
                        duration_known: true,
                        stopped: true,
                        ..Default::default()
                    },
                );
                store.save_agent_settings(session.id, &settings).unwrap();
                session.id
            };
            let store = open(&path);
            let rows = store.load(id).unwrap().rows;
            let turns = store.agent_settings(id).unwrap().unwrap_or_default().turns;
            assert_eq!(
                turn_end(&rows, &turns),
                TurnEnd::Stopped,
                "shared: {shared}"
            );
            assert_eq!(
                completion_report(&rows, turn_end(&rows, &turns), None, 12000),
                format!("(its latest turn did not finish: it was stopped. This is the last message it wrote, not a final answer:)\n\n{progress}")
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
