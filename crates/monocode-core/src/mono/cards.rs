use serde_json::{json, Value};
fn text(v: &Value, key: &str, max: usize) -> Result<String, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.chars().count() <= max)
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .ok_or_else(|| format!("Provide {key} under {max} characters"))
}
pub fn parse(v: &Value) -> Result<Value, String> {
    let kind = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Card type must be pr, session, choices or habit")?;
    let allowed = match kind {
        "pr" => vec!["repo", "number", "note"],
        "session" => vec!["sessionId", "note"],
        "choices" => vec!["options"],
        "habit" => vec!["name", "instructions", "schedule"],
        _ => return Err("Card type must be pr, session, choices or habit".into()),
    };
    if v.as_object()
        .ok_or("Provide a card object")?
        .keys()
        .any(|k| k != "type" && !allowed.contains(&k.as_str()))
    {
        return Err(format!("Unexpected {kind} card field"));
    }
    let mut result = match kind {
        "pr" => {
            let number = v
                .get("number")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
                .ok_or("Provide a positive PR number")?;
            let repo = v.get("repo").map(|_| text(v, "repo", 140)).transpose()?;
            if repo.as_ref().is_some_and(|r| {
                r.split('/').count() != 2
                    || !r
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
                    || r.starts_with('/')
                    || r.ends_with('/')
            }) {
                return Err("repo must be owner/name".into());
            }
            json!({"type":"pr","number":number,"repo":repo})
        }
        "session" => {
            let session = v
                .get("sessionId")
                .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                .filter(|n| *n > 0)
                .ok_or("Provide sessionId")?;
            json!({"type":"session","sessionId":session})
        }
        "choices" => {
            let options = v
                .get("options")
                .and_then(Value::as_array)
                .filter(|o| !o.is_empty() && o.len() <= 4)
                .ok_or("Choose 1–4 replies")?;
            let options = options
                .iter()
                .map(|o| {
                    o.as_str()
                        .filter(|s| !s.trim().is_empty() && s.chars().count() <= 160)
                        .map(str::to_owned)
                        .ok_or("Replies must contain 1–160 characters")
                })
                .collect::<Result<Vec<_>, _>>()?;
            json!({"type":"choices","options":options})
        }
        "habit" => {
            let schedule: super::habits::Schedule =
                serde_json::from_value(v.get("schedule").cloned().ok_or("Provide schedule")?)
                    .map_err(|e| e.to_string())?;
            schedule.validate()?;
            json!({"type":"habit","name":text(v,"name",80)?,"instructions":text(v,"instructions",4000)?,"schedule":schedule})
        }
        _ => unreachable!(),
    };
    if v.get("note").is_some() {
        result["note"] = json!(text(v, "note", 280)?);
    }
    Ok(result)
}
pub fn pull_request(cwd: &str, repo: Option<&str>, number: u64) -> Result<Value, String> {
    let binary = crate::scm::shell::resolve_gui_binary("gh").ok_or("GitHub CLI not found")?;
    let mut cmd = std::process::Command::new(binary);
    cmd.current_dir(cwd).args([
        "pr",
        "view",
        &number.to_string(),
        "--json",
        "title,state,url,isDraft,additions,deletions",
    ]);
    if let Some(repo) = repo {
        cmd.args(["--repo", repo]);
    }
    cmd.env("GH_PROMPT_DISABLED", "1");
    crate::scm::shell::apply_gui_env(&mut cmd);
    crate::process::configure_child(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1000)
            .collect());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cards_validate_fields_and_do_not_accept_arbitrary_links_or_commands() {
        assert!(parse(&json!({"type":"pr","repo":"owner/repo;echo x","number":1})).is_err());
        assert!(parse(&json!({"type":"choices","options":["a"],"command":"rm"})).is_err());
        assert!(parse(&json!({"type":"choices","options":["a","b","c","d","e"]})).is_err());
        assert_eq!(
            parse(&json!({"type":"session","sessionId":"12"})).unwrap()["sessionId"],
            12
        );
        assert!(parse(&json!({"type":"habit","name":"Report","instructions":"Report builds","schedule":{"kind":"daily","time":"09:00"}})).is_ok());
    }
}
