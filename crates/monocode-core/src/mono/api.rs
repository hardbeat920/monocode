use serde_json::Value;
/// Reject misspelled fields and ambiguous types before any app mutation.
pub fn validate(action: &str, input: &Value) -> Result<(), String> {
    let fields=match action{
        "models.list"|"soul.read"|"habits.list"=>"",
        "sessions.list"|"folders.list"|"worktrees.list"=>"project",
        "sessions.read"=>"sessionId project before limit maxChars",
        "sessions.start"=>"prompt project harness model modelSettings effort runtimeMode title draft reveal workspaceMode worktreeBase worktreeCwd placement besideSessionId notifyOnComplete",
        "sessions.send"=>"sessionId prompt project notifyOnComplete",
        "sessions.draft"=>"sessionId prompt project",
        "sessions.stop"|"sessions.archive"|"sessions.delete"=>"sessionId project",
        "worktrees.create"=>"branch base existing project",
        "folders.move"=>"sessionId folderId newFolderName project",
        "notes.list"=>"limit offset","notes.read"=>"id","notes.write"=>"id title body tags",
        "artifacts.list"=>"kind limit offset","artifacts.read"=>"id","artifacts.write"=>"id kind title body summary",
        "soul.update"=>"text expectedHash","memory.read"=>"topic","memory.search"=>"query since",
        "memory.add"=>"fact topic until","memory.replace"=>"find fact topic until","memory.remove"=>"find topic",
        "habits.add"=>"name instructions schedule","habits.update"=>"id name instructions schedule enabled","habits.run"|"habits.remove"=>"id",
        "chat.card"=>"type repo number note sessionId options name instructions schedule",
        _=>return Err("Unknown app action. Run app help".into()),
    };
    for (key, value) in input.as_object().ok_or("Input must be an object")? {
        if !fields.split_whitespace().any(|f| f == key) {
            return Err(format!("{action} does not take {key}"));
        }
        if ["draft", "reveal", "notifyOnComplete", "existing", "enabled"].contains(&key.as_str())
            && !value.is_boolean()
        {
            return Err(format!("{key} must be true or false"));
        }
        if ["limit", "offset", "maxChars", "number"].contains(&key.as_str())
            && value.as_u64().is_none()
        {
            return Err(format!("{key} must be a non-negative integer"));
        }
        if key == "before" && !value.is_string() && value.as_u64().is_none() {
            return Err("before must be a turn ID".into());
        }
        if matches!(key.as_str(), "id" | "sessionId" | "besideSessionId")
            && !value.is_string()
            && value.as_u64().is_none()
        {
            return Err(format!("{key} must be an ID"));
        }
        if ![
            "draft",
            "reveal",
            "notifyOnComplete",
            "existing",
            "enabled",
            "before",
            "limit",
            "offset",
            "maxChars",
            "number",
            "id",
            "sessionId",
            "besideSessionId",
            "modelSettings",
            "schedule",
            "tags",
            "options",
        ]
        .contains(&key.as_str())
            && !value.is_string()
        {
            return Err(format!("{key} must be text"));
        }
    }
    if input.get("modelSettings").is_some_and(|value| {
        value
            .as_object()
            .is_none_or(|values| values.values().any(|v| !v.is_string()))
    }) {
        return Err("modelSettings must be an object of strings".into());
    }
    if input["draft"] == true && input["notifyOnComplete"] == true {
        return Err("An unsent draft cannot send a completion notification".into());
    }
    if input.get("runtimeMode").is_some_and(|v| {
        !matches!(
            v.as_str(),
            Some("read-only" | "supervised" | "auto-accept-edits" | "auto" | "full-access")
        )
    }) {
        return Err("Unknown runtimeMode".into());
    }
    if input
        .get("workspaceMode")
        .is_some_and(|v| !matches!(v.as_str(), Some("current" | "checkout" | "worktree")))
    {
        return Err("workspaceMode must be current or worktree".into());
    }
    if input
        .get("placement")
        .is_some_and(|v| !matches!(v.as_str(), Some("tab" | "right" | "down")))
    {
        return Err("placement must be tab, right or down".into());
    }
    if input.get("besideSessionId").is_some()
        && !matches!(input["placement"].as_str(), Some("right" | "down"))
    {
        return Err("besideSessionId requires placement right or down".into());
    }
    if input.get("tags").is_some_and(|v| {
        v.as_array().is_none_or(|a| {
            a.len() > crate::notes::MAX_TAGS
                || a.iter().any(|v| {
                    v.as_str()
                        .is_none_or(|s| s.len() > crate::notes::MAX_TAG_LENGTH)
                })
        })
    }) {
        return Err("tags must be an array of up to 20 short strings".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_before_mutation() {
        assert!(validate("sessions.start", &json!({"prompt":"x","draft":"false"})).is_err());
        assert!(validate("sessions.delete", &json!({"sessionId":1,"force":true})).is_err());
        assert!(validate("sessions.start", &json!({"runtimeMode":"unsafe"})).is_err());
        assert!(validate(
            "sessions.start",
            &json!({"prompt":"x","project":"/code","draft":true})
        )
        .is_ok());
    }
}
