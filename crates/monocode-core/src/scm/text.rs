//! Commit-message and PR prompts, ported from gitText.ts and jsonText.ts.
//! Lengths count UTF-16 code units, like the JavaScript originals.
use serde_json::{Map, Value};

use super::RangeContext;

type Record = Map<String, Value>;

const GIT_TEXT_KEYS: [&str; 5] = ["subject", "title", "message", "body", "branch"];

/// Prompt asking a model for `{subject, body}` describing the staged change.
pub fn commit_message_prompt(
    branch: Option<&str>,
    staged_summary: &str,
    staged_patch: &str,
) -> String {
    [
        "You write concise git commit messages.",
        "Return a JSON object with keys: subject, body.",
        "Do not call tools. Reply with JSON only.",
        "Rules:",
        "- subject must be imperative, <= 72 chars, and no trailing period",
        "- body can be empty string or short bullet points",
        "- capture the primary user-visible or developer-visible change",
        "",
        &format!("Branch: {}", branch.unwrap_or("(detached)")),
        "",
        "Staged files:",
        &limit_section(staged_summary, 6_000),
        "",
        "Staged patch:",
        &limit_section(staged_patch, 40_000),
    ]
    .join("\n")
}

/// Prompt asking a model for `{title, body}` describing a branch range.
pub fn pr_content_prompt(range: &RangeContext) -> String {
    [
        "You write source control change request content.",
        "Return a JSON object with keys: title, body.",
        "Do not call tools. Reply with JSON only.",
        "Rules:",
        "- title should be concise and specific",
        "- body must be markdown and include headings '## Summary' and '## Testing'",
        "- under Summary, provide short bullet points",
        "- under Testing, include bullet points with concrete checks or 'Not run' where appropriate",
        "",
        &format!("Base branch: {}", range.base),
        &format!("Head branch: {}", range.head),
        "",
        "Commits:",
        &limit_section(&range.commit_summary, 12_000),
        "",
        "Diff stat:",
        &limit_section(&range.diff_summary, 12_000),
        "",
        "Diff patch:",
        &limit_section(&range.diff_patch, 40_000),
    ]
    .join("\n")
}

/// The model's commit message formatted as `subject\n\nbody`, or the subject alone.
pub fn parse_commit_message(raw: &str) -> Option<String> {
    message_from(&parse_json_object(raw)?)
}

/// A commit message, subject then body, from a record's text keys.
fn message_from(rec: &Record) -> Option<String> {
    let subject = [
        string_field(rec, "subject"),
        string_field(rec, "title"),
        string_field(rec, "message"),
    ]
    .into_iter()
    .find(|value| !value.is_empty())
    .unwrap_or("");
    let subject = sanitize_commit_subject(subject);
    if subject.is_empty() {
        return None;
    }
    let body = commit_body(rec);
    Some(if body.is_empty() {
        subject
    } else {
        format!("{subject}\n\n{body}")
    })
}

/// One commit of a plan: its message and the changed files it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedCommit {
    pub message: String,
    pub paths: Vec<String>,
}

/// Prompt asking a model to split every changed file into commits that each
/// make one change, as `{commits: [{subject, body, paths}]}`.
pub fn commit_plan_prompt(branch: Option<&str>, files_summary: &str, patch: &str) -> String {
    [
        "You split a working copy's uncommitted changes into a few focused git commits.",
        "Return a JSON object with one key, commits: an array of objects with keys subject, body, paths.",
        "Do not call tools. Reply with JSON only.",
        "Rules:",
        "- every changed file below appears in exactly one commit's paths, written exactly as listed",
        "- group files that belong to one change; put renames, generated files and lockfiles with the change that caused them",
        "- order commits so each builds on the ones before it",
        "- use as few commits as keep each one coherent; one commit is fine for a small change",
        "- subject must be imperative, <= 72 chars, and no trailing period",
        "- body can be empty string or short bullet points",
        "",
        &format!("Branch: {}", branch.unwrap_or("(detached)")),
        "",
        "Changed files (path | status | lines added, removed):",
        &limit_section(files_summary, 12_000),
        "",
        "Patch:",
        &limit_section(patch, 30_000),
    ]
    .join("\n")
}

/// The model's plan, reconciled with the files that really changed: unknown and
/// repeated paths are dropped, empty commits go, and files the model left out
/// gather in a last commit, so every changed file lands in exactly one. `None`
/// without a usable commit.
pub fn parse_commit_plan(raw: &str, known: &[String]) -> Option<Vec<PlannedCommit>> {
    let commits = plan_commits(raw)?;
    let mut left: Vec<&String> = known.iter().collect();
    let mut plan = Vec::new();
    for commit in &commits {
        let Value::Object(rec) = commit else { continue };
        let Some(message) = message_from(rec) else {
            continue;
        };
        let paths: Vec<String> = match rec.get("paths") {
            Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
        .into_iter()
        .filter_map(|path| {
            let at = left.iter().position(|known| known.as_str() == path)?;
            Some(left.remove(at).clone())
        })
        .collect();
        if !paths.is_empty() {
            plan.push(PlannedCommit { message, paths });
        }
    }
    if plan.is_empty() {
        return None;
    }
    if !left.is_empty() {
        plan.push(PlannedCommit {
            message: "Update remaining files".into(),
            paths: left.into_iter().cloned().collect(),
        });
    }
    Some(plan)
}

/// The first JSON object in `raw` that has a `commits` array, past any
/// preamble that mentions braces.
fn plan_commits(raw: &str) -> Option<Vec<Value>> {
    let trimmed = js_trim(raw);
    let mut from = 0;
    while let Some(offset) = trimmed[from..].find('{') {
        let start = from + offset;
        let json = extract_json_object(&trimmed[start..])?;
        from = start + 1;
        if let Ok(Value::Object(mut rec)) = serde_json::from_str::<Value>(json) {
            if let Some(Value::Array(commits)) = rec.remove("commits") {
                return Some(commits);
            }
        }
    }
    None
}

/// The model's PR `(title, body)`; `None` without a usable title.
pub fn parse_pr_content(raw: &str) -> Option<(String, String)> {
    let rec = parse_json_object(raw)?;
    let title = sanitize_pr_title(string_field(&rec, "title"));
    let body = js_trim(string_field(&rec, "body")).to_string();
    if title.is_empty() {
        return None;
    }
    Some((title, body))
}

fn commit_body(rec: &Record) -> String {
    match rec.get("body") {
        Some(Value::String(value)) => js_trim(value).to_string(),
        Some(Value::Array(items)) => js_trim(
            &items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .to_string(),
        _ => String::new(),
    }
}

fn sanitize_commit_subject(raw: &str) -> String {
    let single_line = first_line(js_trim(raw));
    let without_period = js_trim(single_line.trim_end_matches('.'));
    if without_period.is_empty() {
        return String::new();
    }
    if js_len(without_period) <= 72 {
        return without_period.to_string();
    }
    js_slice(without_period, 72)
        .trim_end_matches(js_space)
        .to_string()
}

fn sanitize_pr_title(raw: &str) -> String {
    first_line(js_trim(raw)).to_string()
}

/// First line of `/\r?\n/`, trimmed.
fn first_line(text: &str) -> &str {
    let line = text.split('\n').next().unwrap_or("");
    js_trim(line.strip_suffix('\r').unwrap_or(line))
}

pub(crate) fn limit_section(value: &str, max_chars: usize) -> String {
    if js_len(value) <= max_chars {
        return value.to_string();
    }
    format!("{}\n\n[truncated]", js_slice(value, max_chars))
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let trimmed = js_trim(raw);
    let start = trimmed.find('{')?;
    let mut depth = 0i64;
    let mut in_string = false;
    let mut escaping = false;
    for (index, ch) in trimmed[start..].char_indices() {
        if in_string {
            if escaping {
                escaping = false;
            } else if ch == '\\' {
                escaping = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&trimmed[start..start + index + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// First JSON object with a git text key, else the first object at all.
pub(crate) fn parse_json_object(raw: &str) -> Option<Record> {
    let trimmed = js_trim(raw);
    let mut search_from = 0;
    let mut fallback: Option<Record> = None;
    while search_from < trimmed.len() {
        let Some(offset) = trimmed[search_from..].find('{') else {
            break;
        };
        let start = search_from + offset;
        let Some(json) = extract_json_object(&trimmed[start..]) else {
            break;
        };
        search_from = start + 1;
        // Models often mention `{` in preamble; keep scanning for real JSON.
        let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        if GIT_TEXT_KEYS
            .iter()
            .any(|key| matches!(rec.get(*key), Some(Value::String(_))))
        {
            return Some(rec);
        }
        if fallback.is_none() {
            fallback = Some(rec);
        }
    }
    fallback
}

pub(crate) fn string_field<'a>(rec: &'a Record, key: &str) -> &'a str {
    rec.get(key).and_then(Value::as_str).unwrap_or("")
}

/// JavaScript `\s`: Unicode white space plus the BOM.
pub(crate) fn js_space(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{feff}'
}

pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

pub(crate) fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Longest prefix of at most `units` UTF-16 code units, without splitting a pair.
pub(crate) fn js_slice(text: &str, units: usize) -> &str {
    let mut count = 0;
    for (index, ch) in text.char_indices() {
        count += ch.len_utf16();
        if count > units {
            return &text[..index];
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_section_truncates_with_marker() {
        assert_eq!(limit_section("abc", 3), "abc");
        assert_eq!(limit_section("abcdef", 3), "abc\n\n[truncated]");
        assert_eq!(limit_section("😀😀", 2), "😀\n\n[truncated]");
    }

    #[test]
    fn commit_prompt_includes_branch_and_sections() {
        let prompt = commit_message_prompt(None, "a.txt | 1 +", "PATCH");
        assert!(prompt.starts_with("You write concise git commit messages.\n"));
        assert!(prompt.contains("Return a JSON object with keys: subject, body.\n"));
        assert!(prompt.contains("\nBranch: (detached)\n\nStaged files:\na.txt | 1 +\n"));
        assert!(prompt.ends_with("\nStaged patch:\nPATCH"));
        assert!(commit_message_prompt(Some("feature"), "", "").contains("Branch: feature"));
    }

    #[test]
    fn pr_prompt_includes_range() {
        let prompt = pr_content_prompt(&RangeContext {
            base: "main".into(),
            head: "feature".into(),
            commit_summary: "Add thing".into(),
            diff_summary: "1 file".into(),
            diff_patch: "PATCH".into(),
        });
        assert!(prompt.contains("Base branch: main\nHead branch: feature\n"));
        assert!(prompt.contains("Commits:\nAdd thing\n\nDiff stat:\n1 file\n"));
        assert!(prompt.ends_with("Diff patch:\nPATCH"));
    }

    #[test]
    fn parses_commit_message_from_noisy_output() {
        assert_eq!(
            parse_commit_message(r#"{"subject":"Update selected files","body":""}"#).as_deref(),
            Some("Update selected files")
        );
        assert_eq!(
            parse_commit_message(
                "Sure {not json} here:\n```json\n{\"subject\": \"Fix the {brace} parser.\", \"body\": \"- one\\n- two \"}\n```"
            )
            .as_deref(),
            Some("Fix the {brace} parser\n\n- one\n- two")
        );
        assert_eq!(
            parse_commit_message(r#"{"title":"Use title\nsecond line","body":["- a", 3, "- b"]}"#)
                .as_deref(),
            Some("Use title\n\n- a\n- b")
        );
        assert_eq!(
            parse_commit_message(r#"{"message":"..."}"#),
            None,
            "a subject of only periods is empty"
        );
        assert_eq!(parse_commit_message("no json"), None);
        assert_eq!(parse_commit_message(r#"{"other":1}"#), None);
    }

    #[test]
    fn commit_subject_is_capped_at_72() {
        let long = "a".repeat(71) + " b";
        let message = parse_commit_message(&format!(r#"{{"subject":"{long}c"}}"#)).unwrap();
        assert_eq!(message, "a".repeat(71));
    }

    #[test]
    fn json_object_prefers_git_text_keys() {
        let rec = parse_json_object(r#"{"x":1} then {"branch":"b"}"#).unwrap();
        assert_eq!(string_field(&rec, "branch"), "b");
        let rec = parse_json_object(r#"{"x":1} {"y":2}"#).unwrap();
        assert!(rec.contains_key("x"));
    }

    #[test]
    fn parses_pr_content() {
        assert_eq!(
            parse_pr_content(r###"{"title":" Add X \nmore","body":"## Summary\n- x\n"}"###),
            Some(("Add X".into(), "## Summary\n- x".into()))
        );
        assert_eq!(parse_pr_content(r#"{"body":"x"}"#), None);
    }

    #[test]
    fn commit_plan_covers_every_changed_file_once() {
        let known: Vec<String> = ["a.rs", "b.rs", "c.rs", "d.rs"].map(String::from).into();
        let raw = r#"Here {you go}:
{"commits":[
 {"subject":"Add a","body":"- one","paths":["a.rs","ghost.rs","b.rs"]},
 {"subject":"Add b again","paths":["b.rs"]},
 {"subject":"","paths":["c.rs"]},
 {"subject":"Nothing","paths":["ghost.rs"]}
]}"#;
        let plan = parse_commit_plan(raw, &known).unwrap();
        assert_eq!(
            plan,
            vec![
                PlannedCommit {
                    message: "Add a\n\n- one".into(),
                    paths: vec!["a.rs".into(), "b.rs".into()],
                },
                PlannedCommit {
                    message: "Update remaining files".into(),
                    paths: vec!["c.rs".into(), "d.rs".into()],
                },
            ]
        );
    }

    #[test]
    fn commit_plan_needs_a_usable_commit() {
        let known = vec!["a.rs".to_string()];
        assert_eq!(parse_commit_plan("no json", &known), None);
        assert_eq!(parse_commit_plan(r#"{"commits":[]}"#, &known), None);
        assert_eq!(
            parse_commit_plan(r#"{"commits":[{"subject":"x","paths":["z"]}]}"#, &known),
            None
        );
    }
}
