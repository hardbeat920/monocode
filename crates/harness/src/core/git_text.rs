//! Port of src/features/source-control/model/gitText.ts: prompts and parsers
//! for generated commit messages, pull request text, and branch names.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use monocode_core::js;

use super::json_text::{first_line, limit_section, parse_json_object, string_field};

/// `CommitMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommitMessage {
    pub subject: String,
    pub body: String,
}

/// `PrContent`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PrContent {
    pub title: String,
    pub body: String,
}

/// The input of `buildCommitMessagePrompt`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitMessagePromptInput {
    pub branch: Option<String>,
    pub staged_summary: String,
    pub staged_patch: String,
    pub include_branch: bool,
}

/// `buildCommitMessagePrompt`.
pub fn build_commit_message_prompt(input: &CommitMessagePromptInput) -> String {
    let wants_branch = input.include_branch;
    let mut lines: Vec<String> = vec![
        "You write concise git commit messages.".into(),
        if wants_branch {
            "Return a JSON object with keys: subject, body, branch.".into()
        } else {
            "Return a JSON object with keys: subject, body.".into()
        },
        "Do not call tools. Reply with JSON only.".into(),
        "Rules:".into(),
        "- subject must be imperative, <= 72 chars, and no trailing period".into(),
        "- body can be empty string or short bullet points".into(),
    ];
    if wants_branch {
        lines.push("- branch must be a short semantic git branch fragment for this change".into());
    }
    lines.extend([
        "- capture the primary user-visible or developer-visible change".into(),
        String::new(),
        format!(
            "Branch: {}",
            input.branch.as_deref().unwrap_or("(detached)")
        ),
        String::new(),
        "Staged files:".into(),
        limit_section(&input.staged_summary, 6_000),
        String::new(),
        "Staged patch:".into(),
        limit_section(&input.staged_patch, 40_000),
    ]);
    lines.join("\n")
}

/// The input of `buildPrContentPrompt`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrContentPromptInput {
    pub base_branch: String,
    pub head_branch: String,
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

/// `buildPrContentPrompt`.
pub fn build_pr_content_prompt(input: &PrContentPromptInput) -> String {
    [
        "You write source control change request content.".to_string(),
        "Return a JSON object with keys: title, body.".into(),
        "Do not call tools. Reply with JSON only.".into(),
        "Rules:".into(),
        "- title should be concise and specific".into(),
        "- body must be markdown and include headings '## Summary' and '## Testing'".into(),
        "- under Summary, provide short bullet points".into(),
        "- under Testing, include bullet points with concrete checks or 'Not run' where appropriate"
            .into(),
        String::new(),
        format!("Base branch: {}", input.base_branch),
        format!("Head branch: {}", input.head_branch),
        String::new(),
        "Commits:".into(),
        limit_section(&input.commit_summary, 12_000),
        String::new(),
        "Diff stat:".into(),
        limit_section(&input.diff_summary, 12_000),
        String::new(),
        "Diff patch:".into(),
        limit_section(&input.diff_patch, 40_000),
    ]
    .join("\n")
}

/// `buildBranchNamePrompt`.
pub fn build_branch_name_prompt(message: &str) -> String {
    [
        "You generate concise git branch names.".to_string(),
        "Return a JSON object with key: branch.".into(),
        "Do not call tools. Reply with JSON only.".into(),
        "Rules:".into(),
        "- Branch should describe the requested work from the user message.".into(),
        "- Keep it short and specific (2-6 words).".into(),
        "- Use plain words only, no issue prefixes and no punctuation-heavy text.".into(),
        String::new(),
        "User message:".into(),
        limit_section(message, 8_000),
    ]
    .join("\n")
}

/// `parseCommitMessage`.
pub fn parse_commit_message(raw: &str) -> Option<CommitMessage> {
    let rec = parse_json_object(raw)?;
    let subject = [
        string_field(&rec, "subject"),
        string_field(&rec, "title"),
        string_field(&rec, "message"),
    ]
    .into_iter()
    .find(|value| !value.is_empty())
    .unwrap_or_default();
    let subject = sanitize_commit_subject(&subject);
    if subject.is_empty() {
        return None;
    }
    Some(CommitMessage {
        subject,
        body: commit_body(&rec),
    })
}

fn commit_body(rec: &Map<String, Value>) -> String {
    match rec.get("body") {
        Some(Value::String(value)) => js::trim(value).to_string(),
        Some(Value::Array(items)) => js::trim(
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

/// `formatCommitMessage`.
pub fn format_commit_message(message: &CommitMessage) -> String {
    if message.body.is_empty() {
        message.subject.clone()
    } else {
        format!("{}\n\n{}", message.subject, message.body)
    }
}

/// `parsePrContent`.
pub fn parse_pr_content(raw: &str) -> Option<PrContent> {
    let rec = parse_json_object(raw)?;
    let title = sanitize_pr_title(&string_field(&rec, "title"));
    let body = js::trim(&string_field(&rec, "body")).to_string();
    if title.is_empty() {
        return None;
    }
    Some(PrContent { title, body })
}

/// `parseBranchName`.
pub fn parse_branch_name(raw: &str) -> Option<String> {
    let branch = match parse_json_object(raw) {
        Some(rec) => sanitize_branch_fragment(&string_field(&rec, "branch")),
        None => sanitize_branch_fragment(raw),
    };
    (!branch.is_empty()).then_some(branch)
}

/// `sanitizeCommitSubject`.
pub fn sanitize_commit_subject(raw: &str) -> String {
    let single_line = js::trim(first_line(js::trim(raw)));
    let without_trailing_period = js::trim(single_line.trim_end_matches('.'));
    if without_trailing_period.is_empty() {
        return String::new();
    }
    if js::len(without_trailing_period) <= 72 {
        return without_trailing_period.to_string();
    }
    js::trim_end(js::slice_prefix(without_trailing_period, 72)).to_string()
}

/// `sanitizePrTitle`.
pub fn sanitize_pr_title(raw: &str) -> String {
    js::trim(first_line(js::trim(raw))).to_string()
}

static EDGE_PUNCT_OR_SPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[./\s_-]+|[./\s_-]+$").unwrap());
static NOT_BRANCH_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9/_-]+").unwrap());
static SLASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/+").unwrap());
static DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-+").unwrap());
static EDGE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[./_-]+|[./_-]+$").unwrap());
static TRAILING_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[./_-]+$").unwrap());

/// `sanitizeBranchFragment`.
pub fn sanitize_branch_fragment(raw: &str) -> String {
    let lowered = js::trim(raw).to_lowercase().replace(['\'', '"', '`'], "");
    let normalized = EDGE_PUNCT_OR_SPACE.replace_all(&lowered, "");
    let fragment = NOT_BRANCH_CHARS.replace_all(&normalized, "-");
    let fragment = SLASHES.replace_all(&fragment, "/");
    let fragment = DASHES.replace_all(&fragment, "-");
    let fragment = EDGE_PUNCT.replace_all(&fragment, "");
    let fragment = js::slice_prefix(&fragment, 64).to_string();
    TRAILING_PUNCT.replace_all(&fragment, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_commit_message_from_noisy_output() {
        let parsed = parse_commit_message(
            "Sure {not json} {\"title\":\"Add thing.\",\"body\":[\"- a\",3,\"- b\"]}",
        )
        .unwrap();
        assert_eq!(parsed.subject, "Add thing");
        assert_eq!(parsed.body, "- a\n- b");
        assert_eq!(format_commit_message(&parsed), "Add thing\n\n- a\n- b");
        assert_eq!(parse_commit_message("{\"subject\":\"...\"}"), None);
    }

    #[test]
    fn caps_the_subject_at_72_units() {
        let long = "a".repeat(80);
        assert_eq!(sanitize_commit_subject(&long).len(), 72);
        assert_eq!(sanitize_commit_subject("Fix it.\nbody"), "Fix it");
    }

    #[test]
    fn parses_pr_content() {
        assert_eq!(
            parse_pr_content("{\"title\":\" Title\\nmore\",\"body\":\" ## Summary \"}"),
            Some(PrContent {
                title: "Title".into(),
                body: "## Summary".into()
            })
        );
        assert_eq!(parse_pr_content("{\"body\":\"x\"}"), None);
    }

    #[test]
    fn sanitizes_branch_names() {
        assert_eq!(
            sanitize_branch_fragment("  Fix the 'Login' Bug!  "),
            "fix-the-login-bug"
        );
        assert_eq!(sanitize_branch_fragment("feat//Thing__"), "feat/thing");
        assert_eq!(
            parse_branch_name("{\"branch\":\"Add API docs\"}").as_deref(),
            Some("add-api-docs")
        );
        assert_eq!(parse_branch_name("plain words"), Some("plain-words".into()));
        assert_eq!(parse_branch_name("..."), None);
    }

    #[test]
    fn builds_the_prompts() {
        let prompt = build_commit_message_prompt(&CommitMessagePromptInput {
            branch: None,
            staged_summary: "M a.rs".into(),
            staged_patch: "diff".into(),
            include_branch: true,
        });
        assert!(prompt.contains("keys: subject, body, branch."));
        assert!(prompt.contains("Branch: (detached)"));
        assert!(prompt.contains("- branch must be"));
        let pr = build_pr_content_prompt(&PrContentPromptInput {
            base_branch: "main".into(),
            head_branch: "feat".into(),
            ..Default::default()
        });
        assert!(pr.contains("Base branch: main\nHead branch: feat"));
        assert!(build_branch_name_prompt("hello").ends_with("User message:\nhello"));
    }
}
