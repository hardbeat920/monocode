//! Generated sidebar titles, following sessions/model/sessionTitle.ts.
use crate::{
    agent::Settings,
    provider::{self, Kind, Request},
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

/// Tauri's sessionDisplayTitle; persistence retains its canonical provider prefix.
pub fn display_title<'a>(title: &'a str, provider: &str) -> &'a str {
    let prefix = format!("{provider} · ");
    if let Some(title) = title.strip_prefix(&prefix) {
        return title;
    }
    if title == provider
        || title == Kind::from_id(provider).label()
        || title == format!("New {} session", Kind::from_id(provider).label())
    {
        return "New session";
    }
    title
}

const PROMPT: &str = r#"Generate a title that will help the user recognize this coding session weeks later.
Also identify one GitHub issue or pull request only when the user explicitly refers to it by number or URL.
Return JSON with exactly two keys: title and workItem.
workItem must be null or an object with exactly two keys: kind ("issue" or "pr") and number (a positive integer copied from the user message).
Never invent a work item number. If the reference is ambiguous or has no number, return null.
Do not call tools. Reply with JSON only.

Before answering, silently reduce the request to:
- Subject: What system, feature, or problem is this really about?
- Outcome: What does the user ultimately want to understand or change?
- Incidental instructions: What only describes how the agent should do the work?

Title the subject and outcome. Discard incidental instructions.

Editorial rules:
- 3-8 words, fewer than 40 characters.
- Use a compact noun phrase or clear action phrase.
- Capture the umbrella goal when the request lists several symptoms or steps.
- Name the product change, not the mock, plan, report, branch, or PR used to produce it.
- Models, subagents, tools, and output formats do not belong in the title unless they are themselves the topic.
- Do not claim the work is complete.
- Do not copy and truncate the user's message.
- Avoid quotes, labels, filler, and trailing punctuation."#;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItemHint {
    pub kind: String,
    pub number: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedTitle {
    pub title: String,
    pub work_item: Option<WorkItemHint>,
}

pub fn supports(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Codex
            | Kind::Claude
            | Kind::Cursor
            | Kind::Grok
            | Kind::OpenCode
            | Kind::Pi
            | Kind::Omp
    )
}
pub fn prompt(message: &str) -> String {
    format!(
        "{PROMPT}\n\nUser message:\n{}",
        crate::response::limit(message, 8_000)
    )
}
pub fn sanitize(raw: &str) -> String {
    let normalized = raw
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['\'', '"', '`'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.chars().count() <= 50 {
        normalized
    } else {
        format!(
            "{}...",
            normalized.chars().take(47).collect::<String>().trim_end()
        )
    }
}
pub fn parse(raw: &str, message: &str) -> Option<GeneratedTitle> {
    if let Some(start) = raw.find('{') {
        if let Some(Ok(value)) = serde_json::Deserializer::from_str(&raw[start..])
            .into_iter::<serde_json::Value>()
            .next()
        {
            if let Some(title) = value["title"]
                .as_str()
                .map(sanitize)
                .filter(|t| !t.is_empty())
            {
                let work_item = serde_json::from_value::<WorkItemHint>(value["workItem"].clone())
                    .ok()
                    .filter(|hint| {
                        matches!(hint.kind.as_str(), "issue" | "pr")
                            && hint.number > 0
                            && hint.number <= 9_007_199_254_740_991
                            && message
                                .split(|c: char| !c.is_ascii_digit())
                                .any(|n| n == hint.number.to_string())
                    });
                return Some(GeneratedTitle { title, work_item });
            }
        }
    }
    let title = sanitize(raw);
    if title.contains(['{', '}']) || !(2..=10).contains(&title.split_whitespace().count()) {
        return None;
    }
    Some(GeneratedTitle {
        title,
        work_item: None,
    })
}
/// Runs on a worker, outside the user's provider thread and native transcript.
pub fn generate(
    kind: Kind,
    cwd: &Path,
    message: &str,
    cancel: Arc<AtomicBool>,
) -> Option<GeneratedTitle> {
    generate_for(kind, None, cwd, message, Default::default(), cancel)
}
/// `model` overrides the provider's cheap default.
pub fn generate_for(
    kind: Kind,
    model: Option<String>,
    cwd: &Path,
    message: &str,
    account: crate::accounts::Profile,
    cancel: Arc<AtomicBool>,
) -> Option<GeneratedTitle> {
    if !supports(kind) {
        return None;
    }
    let binary = provider::resolve(kind)?;
    let model = model.or_else(|| match kind {
        Kind::Codex => Some("gpt-5.6-luna".into()),
        Kind::Claude => Some("claude-haiku-4-5".into()),
        _ => None,
    });
    let output = provider::run_text(
        Request {
            kind,
            binary,
            cwd: cwd.into(),
            prompt: prompt(message),
            provider_id: None,
            model,
            settings: Settings {
                account,
                ..Default::default()
            },
        },
        cancel,
    )
    .ok()?;
    parse(&output, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_metadata_rejects_invented_numbers_and_accepts_wrapped_json() {
        let raw = "```json\n{\"title\":\"Fix session links\",\"workItem\":{\"kind\":\"pr\",\"number\":42}}\n```";
        let title = parse(raw, "Fix PR #42").unwrap();
        assert_eq!(title.title, "Fix session links");
        assert_eq!(title.work_item.unwrap().number, 42);
        assert!(parse(raw, "Fix PR #142").unwrap().work_item.is_none());
        assert!(parse(raw, "Fix session links").unwrap().work_item.is_none());
        assert_eq!(
            parse("Fix session links", "").unwrap().title,
            "Fix session links"
        );
        assert!(parse("{broken title}", "").is_none());
        assert!(parse("single", "").is_none());
    }
    #[test]
    fn title_and_prompt_limits_preserve_unicode() {
        assert_eq!(
            sanitize("  `Fix   session links`\nignored"),
            "Fix session links"
        );
        assert_eq!(sanitize(&"🙂".repeat(70)).chars().count(), 50);
        assert!(prompt(&"🙂".repeat(9_000)).chars().count() < PROMPT.len() + 8_050);
    }
    #[test]
    fn saved_manual_names_and_deleted_sessions_reject_late_results() {
        let db = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let session = db.create("codex", "/tmp").unwrap();
        assert!(db
            .replace_title(session.id, &session.title, "Initial prompt")
            .unwrap());
        db.title(session.id, "Manual name").unwrap();
        assert!(!db
            .replace_title(session.id, "Initial prompt", "Generated name")
            .unwrap());
        assert!(!db
            .replace_title(99_999, "Initial prompt", "Generated name")
            .unwrap());
        assert_eq!(db.sessions().unwrap()[0].title, "Manual name");
    }
}
