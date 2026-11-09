//! What test hosts share with the real one (`host/engine.ts`).
use serde_json::Value;

/// The commands `host/engine.ts` refuses (`parseCommand`, and the engine's
/// draft lookups) that a desktop could plausibly send here.
pub(crate) fn rejects(command: &Value, snapshot: &Value) -> Option<&'static str> {
    let text = |key: &str| command[key].as_str().is_some_and(|v| !v.trim().is_empty());
    if !text("commandId") {
        return Some("Invalid command ID");
    }
    if command["type"] == "create" {
        // REMOTE_PROVIDERS and RUNTIME_MODES.
        let providers = [
            "codex",
            "claude",
            "cursor",
            "grok",
            "opencode",
            "pi",
            "omp",
            "fx",
            "hermes",
            "antigravity",
            "devin",
        ];
        let modes = ["supervised", "auto-accept-edits", "auto", "full-access"];
        if !command["harness"]
            .as_str()
            .is_some_and(|h| providers.contains(&h))
            || !command["runtimeMode"]
                .as_str()
                .is_some_and(|m| modes.contains(&m))
        {
            return Some("Invalid provider or permission mode");
        }
        if !text("projectId") {
            return Some("Invalid project ID");
        }
        if !text("model") {
            return Some("Invalid model");
        }
        return None;
    }
    if command["sessionId"] != "host-session" {
        return Some("Invalid session ID");
    }
    let has_draft = |id: &Value| {
        snapshot["session"]["blocks"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| &b["id"] == id && b["draft"] == true))
    };
    match command["type"].as_str() {
        Some("send" | "draft") => {
            let attachments = command["attachments"]
                .as_array()
                .is_some_and(|a| !a.is_empty());
            let names_draft = command["type"] == "send" && !command["draftBlockId"].is_null();
            if !command["text"].is_string() || (!text("text") && !attachments && !names_draft) {
                return Some("Invalid prompt");
            }
            if names_draft && !has_draft(&command["draftBlockId"]) {
                return Some("Draft not found");
            }
            None
        }
        Some("removeDraft") if !has_draft(&command["draftBlockId"]) => Some("Draft not found"),
        _ => None,
    }
}
