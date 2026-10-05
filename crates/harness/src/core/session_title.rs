//! Port of src/features/sessions/model/sessionTitle.ts: the title prompt and
//! the parser for what the model returns.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use monocode_core::js;

use super::json_text::{extract_json_object, first_line, js_string, limit_section};

const MESSAGE_LIMIT: usize = 8_000;
const TITLE_LIMIT: usize = 50;

const THREAD_TITLE_PROMPT: &str = r#"Generate a title that will help the user recognize this coding session weeks later.
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

/// `GeneratedWorkItemHint["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkItemKind {
    #[serde(rename = "issue")]
    Issue,
    #[serde(rename = "pr")]
    Pr,
}

/// `GeneratedWorkItemHint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedWorkItemHint {
    pub kind: WorkItemKind,
    pub number: i64,
}

/// `GeneratedSessionTitle`. `workItem` is always present, as `null` when unset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedSessionTitle {
    pub title: String,
    pub work_item: Option<GeneratedWorkItemHint>,
}

/// `shouldGenerateSessionTitle`.
pub fn should_generate_session_title(
    is_first_turn: bool,
    placeholder_title: bool,
    refresh_title: bool,
) -> bool {
    refresh_title || (is_first_turn && placeholder_title)
}

/// `buildThreadTitlePrompt`.
pub fn build_thread_title_prompt(message: &str) -> String {
    format!(
        "{THREAD_TITLE_PROMPT}\n\nUser message:\n{}",
        limit_section(message, MESSAGE_LIMIT)
    )
}

fn is_quote(c: char) -> bool {
    matches!(c, '\'' | '"' | '`')
}

/// Collapse each run of JavaScript whitespace to one space.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if js::is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `sanitizeThreadTitle`.
pub fn sanitize_thread_title(raw: &str) -> String {
    let line = js::trim(first_line(js::trim(raw)));
    let unquoted = line.trim_start_matches(is_quote).trim_end_matches(is_quote);
    let normalized = collapse_spaces(js::trim(unquoted));
    if normalized.is_empty() {
        return String::new();
    }
    if js::len(&normalized) <= TITLE_LIMIT {
        return normalized;
    }
    format!(
        "{}...",
        js::trim_end(js::slice_prefix(&normalized, TITLE_LIMIT - 3))
    )
}

/// `referencedNumber`: `(^|\D)<number>(?=\D|$)`.
fn referenced_number(message: &str, number: i64) -> bool {
    let needle = number.to_string();
    let bytes = message.as_bytes();
    let mut from = 0;
    while let Some(found) = message[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
        let after_ok = end == bytes.len() || !bytes[end].is_ascii_digit();
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn valid_work_item(candidate: Option<&Value>, message: &str) -> Option<GeneratedWorkItemHint> {
    let Some(Value::Object(item)) = candidate else {
        return None;
    };
    let kind = match item.get("kind") {
        Some(Value::String(kind)) if kind == "issue" => WorkItemKind::Issue,
        Some(Value::String(kind)) if kind == "pr" => WorkItemKind::Pr,
        _ => return None,
    };
    let number = item.get("number")?.as_f64()?;
    if number.fract() != 0.0 || number.abs() > MAX_SAFE_INTEGER || number <= 0.0 {
        return None;
    }
    let number = number as i64;
    referenced_number(message, number).then_some(GeneratedWorkItemHint { kind, number })
}

/// `parseGeneratedSessionTitle`.
pub fn parse_generated_session_title(raw: &str, message: &str) -> Option<GeneratedSessionTitle> {
    if let Some(json) = extract_json_object(raw)
        && let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(&json)
        && let Some(title) = parsed.get("title")
    {
        let title = sanitize_thread_title(&js_string(title));
        if !title.is_empty() {
            let work_item = valid_work_item(parsed.get("workItem"), message);
            return Some(GeneratedSessionTitle { title, work_item });
        }
    }

    // Fall through to a bare-title parse when the model skipped JSON.
    let fallback = sanitize_thread_title(raw);
    if fallback.is_empty() || fallback.contains(['{', '}']) {
        return None;
    }
    let words = fallback.split(' ').filter(|word| !word.is_empty()).count();
    if !(2..=10).contains(&words) {
        return None;
    }
    Some(GeneratedSessionTitle {
        title: fallback,
        work_item: None,
    })
}

/// `parseGeneratedThreadTitle`: the title alone.
pub fn parse_generated_thread_title(raw: &str) -> Option<String> {
    parse_generated_session_title(raw, "").map(|parsed| parsed.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_the_title_pass_for_one_optional_work_item() {
        assert!(build_thread_title_prompt("Fix PR #42").contains("title and workItem"));
    }

    #[test]
    fn accepts_a_referenced_pr_number() {
        assert_eq!(
            parse_generated_session_title(
                r#"{"title":"Fix session links","workItem":{"kind":"pr","number":42}}"#,
                "Please fix PR #42",
            ),
            Some(GeneratedSessionTitle {
                title: "Fix session links".into(),
                work_item: Some(GeneratedWorkItemHint {
                    kind: WorkItemKind::Pr,
                    number: 42
                }),
            })
        );
    }

    #[test]
    fn drops_a_model_invented_number_without_losing_the_title() {
        assert_eq!(
            parse_generated_session_title(
                r#"{"title":"Fix session links","workItem":{"kind":"issue","number":99}}"#,
                "Please fix the session links",
            ),
            Some(GeneratedSessionTitle {
                title: "Fix session links".into(),
                work_item: None
            })
        );
    }

    #[test]
    fn keeps_compatibility_with_a_bare_generated_title() {
        assert_eq!(
            parse_generated_session_title("Fix session links", "anything"),
            Some(GeneratedSessionTitle {
                title: "Fix session links".into(),
                work_item: None
            })
        );
    }

    #[test]
    fn can_refresh_a_generated_title_for_an_event_added_to_an_existing_session() {
        assert!(should_generate_session_title(false, false, true));
        assert!(!should_generate_session_title(false, false, false));
        assert!(should_generate_session_title(true, true, false));
    }

    #[test]
    fn sanitizes_quotes_whitespace_and_length() {
        assert_eq!(
            sanitize_thread_title("  \"Fix   the\tbug\"\nsecond"),
            "Fix the bug"
        );
        let long = "word ".repeat(20);
        let title = sanitize_thread_title(&long);
        assert!(title.ends_with("..."));
        assert_eq!(js::len(&title), 50);
        assert_eq!(parse_generated_thread_title("one"), None);
        assert_eq!(parse_generated_thread_title("{broken"), None);
    }

    #[test]
    fn matches_numbers_only_on_digit_boundaries() {
        assert!(referenced_number("PR #42", 42));
        assert!(referenced_number("42", 42));
        assert!(!referenced_number("PR #420", 42));
        assert!(!referenced_number("1423", 42));
        assert!(referenced_number("1420 and 42.", 42));
    }

    #[test]
    fn serializes_a_missing_work_item_as_null() {
        let title = GeneratedSessionTitle {
            title: "T".into(),
            work_item: None,
        };
        assert_eq!(
            serde_json::to_value(&title).unwrap(),
            serde_json::json!({ "title": "T", "workItem": null })
        );
    }
}
