//! Desktop work references belong to conversation blocks, not the Mono roster.
//! Keep projection separate from persistence so opening a Mono never rewrites it.
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn session_handle(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().map(crate::store::desktop_session_handle))
}

pub(crate) fn document_cards(block: &Value) -> Vec<Value> {
    block["artifactCards"].as_array().into_iter().flatten().filter_map(|card| {
        let id = card["id"].as_str()?;
        let kind = card["kind"].as_str()?;
        if id.is_empty() || !matches!(kind, "document" | "html") { return None; }
        Some(json!({"id":format!("artifact:{id}"),"type":"document","documentId":id,
            "kind":kind,"title":card["title"].as_str().unwrap_or("Document"),"summary":card["summary"]}))
    }).collect()
}

/// A visual block's document, keyed by its visual id so republishing replaces it.
pub(crate) fn visual_document(block: &Value) -> Option<super::Document> {
    if block["role"] != "visual" {
        return None;
    }
    let visual: crate::visual::VisualDocument =
        serde_json::from_str(block["text"].as_str()?).ok()?;
    let at = block["sentAt"].as_u64().unwrap_or_default();
    Some(super::Document {
        id: format!("visual:{}", visual.id),
        kind: "visual".into(),
        title: visual.title.clone(),
        summary: String::new(),
        body: block["text"].as_str()?.into(),
        created_at: at,
        updated_at: at,
    })
}

fn launch(value: &Value) -> Option<Value> {
    let id = value["sessionId"].as_str().or(value["id"].as_str())?;
    if id.is_empty()
        || id.len() > 512
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return None;
    }
    let cwd = value["cwd"].as_str()?.trim();
    let model = value["model"].as_str()?.trim();
    let harness = value["harness"].as_str()?;
    // Match desktop HARNESSES, including providers not yet executable natively.
    if cwd.is_empty()
        || model.is_empty()
        || ![
            "codex",
            "claude",
            "cursor",
            "opencode",
            "pi",
            "omp",
            "fx",
            "hermes",
            "antigravity",
            "devin",
            "grok",
        ]
        .contains(&harness)
    {
        return None;
    }
    let title = value["title"].as_str()?.trim();
    Some(
        json!({"sessionId":id,"cwd":cwd,"title":if title.is_empty(){"Agent session".into()}else{title.chars().take(300).collect::<String>()},"harness":harness,"model":model}),
    )
}

/// Conservative shell tokenization: a CLI mention in echo/output is not a call.
fn words(command: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for ch in command.chars() {
        if escape {
            word.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escape = true;
            continue;
        }
        if quote == Some(ch) {
            quote = None;
            continue;
        }
        if (ch == '\'' || ch == '"') && quote.is_none() {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
            continue;
        }
        if quote != Some('\'') && matches!(ch, '$' | '`') {
            return None;
        }
        if quote.is_none()
            && matches!(
                ch,
                ';' | '&' | '|' | '<' | '>' | '(' | ')' | '#' | '\n' | '\r'
            )
        {
            return None;
        }
        word.push(ch);
    }
    if quote.is_some() || escape {
        return None;
    }
    if !word.is_empty() {
        result.push(word);
    }
    Some(result)
}
fn action(block: &Value) -> Option<String> {
    if block["role"] != "tool"
        || !matches!(
            block["tool"]["status"].as_str(),
            Some("completed" | "success")
        )
    {
        return None;
    }
    let meta = &block["tool"];
    let command = if meta["preview"]["kind"] == "shell" {
        meta["preview"]["title"].as_str()
    } else {
        None
    }
    .or(meta["title"].as_str())
    .or(block["text"].as_str())?;
    command_action(command)
}
/// Recognize the app CLI itself, including the desktop shell wrapper.
pub fn command_action(command: &str) -> Option<String> {
    let mut parts = words(command.trim())?;
    if parts.len() == 3
        && parts[1].starts_with('-')
        && parts[1].contains('c')
        && parts[0]
            .rsplit('/')
            .next()
            .is_some_and(|s| ["sh", "bash", "zsh"].contains(&s))
    {
        parts = words(&parts[2])?;
    }
    if parts
        .first()?
        .rsplit(['/', '\\'])
        .next()?
        .trim_end_matches(".exe")
        != "monocode"
        || parts.get(1)? != "app"
    {
        return None;
    }
    for flags in parts[3.min(parts.len())..].chunks(2) {
        if flags.len() != 2 || !["--json", "--input", "--request-id"].contains(&flags[0].as_str()) {
            return None;
        }
    }
    parts.get(2).cloned()
}

/// One receipt list per source block. Listings supply details for accepted sends
/// but never create launch cards by themselves (monoSpawnedSessions.ts).
pub(crate) fn block_launches(blocks: &[Value]) -> Vec<Vec<Value>> {
    let mut result = Vec::new();
    let mut known = HashMap::new();
    let mut sent = Vec::new();
    for block in blocks {
        let mut launches: Vec<Value> = block["monoSpawnedSessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(launch)
            .collect();
        let receipt: Option<Value> = block["tool"]["detail"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok());
        if let Some(receipt) = receipt.filter(|r| r["ok"] == true) {
            let value = &receipt["result"];
            match action(block).as_deref() {
                Some("sessions.start")
                    if value["id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("app-")) =>
                {
                    let mut value = value.clone();
                    if value["title"].is_null() {
                        value["title"] = json!("Agent session");
                    }
                    if let Some(value) = launch(&value) {
                        launches.push(value);
                    }
                }
                Some("sessions.list") => {
                    for value in value["sessions"].as_array().into_iter().flatten() {
                        let mut value = value.clone();
                        value["cwd"] = receipt["result"]["cwd"].clone();
                        if let Some(value) = launch(&value) {
                            known.insert(value["sessionId"].as_str().unwrap().to_owned(), value);
                        }
                    }
                }
                Some("sessions.send") if value["submitted"] == true => {
                    if let Some(id) = value["sessionId"].as_str() {
                        sent.push((result.len(), id.to_owned()));
                    }
                }
                _ => {}
            }
        }
        for value in &launches {
            known.insert(
                value["sessionId"].as_str().unwrap().to_owned(),
                value.clone(),
            );
        }
        result.push(launches);
    }
    for (index, id) in sent {
        if let Some(value) = known.get(&id) {
            result[index].push(value.clone());
        }
    }
    result
}

impl super::Mono {
    /// Refresh references without discarding native-created documents or launch
    /// monitoring state. Removed desktop references disappear on the next read.
    pub fn refresh_work(&mut self, store: &crate::store::Store) -> Result<bool, String> {
        let Some(session) = self.session_id else {
            return Ok(false);
        };
        let Some((documents, launches)) = store.mono_work(session)? else {
            return Ok(false);
        };
        self.apply_work(documents, launches)
    }
    pub fn apply_work(
        &mut self,
        documents: Vec<super::Document>,
        mut launches: Vec<super::Launch>,
    ) -> Result<bool, String> {
        let before = serde_json::to_string(&(self.documents.clone(), self.launches.clone()))
            .map_err(|e| e.to_string())?;
        for launch in &mut launches {
            if let Some(previous) = self
                .launches
                .iter()
                .find(|old| old.session_id == launch.session_id)
            {
                launch.origin_turn = previous.origin_turn;
                launch.notify_on_complete = previous.notify_on_complete;
                launch.completed = previous.completed;
                launch.dismissed = previous.dismissed;
            }
        }
        self.documents.retain(|d| {
            !self.desktop_document_ids.contains(&d.id)
                && !documents.iter().any(|new| new.id == d.id)
        });
        self.launches.retain(|l| {
            !self.desktop_launch_ids.contains(&l.session_id)
                && !launches.iter().any(|new| new.session_id == l.session_id)
        });
        self.desktop_document_ids = documents.iter().map(|d| d.id.clone()).collect();
        self.desktop_launch_ids = launches.iter().map(|l| l.session_id).collect();
        self.documents.extend(documents);
        self.launches.extend(launches);
        Ok(before
            != serde_json::to_string(&(self.documents.clone(), self.launches.clone()))
                .map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn details(id: &str) -> Value {
        json!({"sessionId":id,"cwd":"/repo","title":"Review","harness":"codex","model":"gpt-6"})
    }
    fn receipt(command: &str, result: Value) -> Value {
        json!({"role":"tool","text":command,"tool":{"status":"completed","detail":json!({"ok":true,"result":result}).to_string()}})
    }
    #[test]
    fn desktop_receipts_recover_launches_and_accepted_followups_without_linking_listings() {
        let start = receipt("/bin/zsh -lc 'monocode app sessions.start'", {
            let mut v = details("app-child");
            v["id"] = json!("app-child");
            v
        });
        let listing = receipt(
            "monocode app sessions.list",
            json!({"cwd":"/repo","sessions":[{"id":"existing","title":"Continue","harness":"claude","model":"opus"}]}),
        );
        let send = receipt(
            "monocode app sessions.send",
            json!({"submitted":true,"sessionId":"existing"}),
        );
        let results = block_launches(&[listing.clone(), start.clone(), send.clone()]);
        assert!(results[0].is_empty());
        assert_eq!(results[1][0]["sessionId"], "app-child");
        assert_eq!(results[2][0]["sessionId"], "existing");
        let mut failed = start.clone();
        failed["tool"]["status"] = json!("error");
        let mut truncated = start.clone();
        truncated["tool"]["detail"] = json!("{\"ok\":true,");
        let echo = receipt(
            "echo monocode app sessions.start",
            json!({"id":"app-child"}),
        );
        assert!(block_launches(&[failed, truncated, echo, listing])
            .into_iter()
            .flatten()
            .next()
            .is_none());
        assert!(block_launches(&[send])[0].is_empty());
    }
    #[test]
    fn reconcile_work_retains_native_documents_monitoring_and_desktop_removals() {
        let mut mono = super::super::Mono::default();
        let document = super::super::Document {
            id: "desktop".into(),
            kind: "html".into(),
            title: "Visual".into(),
            summary: "Summary".into(),
            body: "<h1>Saved</h1>".into(),
            created_at: 1,
            updated_at: 2,
        };
        let mut native = document.clone();
        native.id = "native".into();
        mono.documents.push(native);
        mono.apply_work(vec![document], Vec::new()).unwrap();
        assert_eq!(mono.documents.len(), 2);
        mono.apply_work(Vec::new(), Vec::new()).unwrap();
        assert_eq!(mono.documents.len(), 1);
        assert_eq!(mono.documents[0].id, "native");
    }
}
