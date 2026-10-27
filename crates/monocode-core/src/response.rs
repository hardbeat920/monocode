//! Response actions use the same prose and recap rules as transcriptActivity.ts,
//! secondOpinion.ts and handoff.ts. Tool chrome and private reasoning aren't copied.
use crate::{tool::ToolCall, transcript::Row};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Handoff,
    SecondOpinion,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    #[serde(default)]
    pub settings: std::collections::HashMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_session: i64,
    pub source_cwd: String,
}
pub use crate::time::now_ms;
pub fn local_clock(epoch_ms: u64) -> String {
    #[cfg(unix)]
    {
        let time = (epoch_ms / 1000) as libc::time_t;
        let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
        // localtime_r writes the caller-owned buffer and doesn't share tm storage.
        if !unsafe { libc::localtime_r(&time, local.as_mut_ptr()) }.is_null() {
            let local = unsafe { local.assume_init() };
            return format!("{:02}:{:02}", local.tm_hour, local.tm_min);
        }
    }
    let minutes = epoch_ms / 60_000;
    format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
}
pub fn limit(text: &str, count: usize) -> String {
    if text.chars().count() <= count {
        text.into()
    } else {
        format!(
            "{}\n\n[truncated]",
            text.chars()
                .take(count.saturating_sub(13))
                .collect::<String>()
        )
    }
}
pub fn prose(rows: &[Row]) -> String {
    let mut messages: Vec<String> = Vec::new();
    let mut previous = false;
    for row in rows {
        if row.role == "assistant" {
            if !previous || row.first {
                messages.push(String::new());
            }
            messages.last_mut().unwrap().push_str(&row.text);
            previous = true;
        } else {
            previous = false;
        }
    }
    messages
        .iter()
        .map(|s| s.trim().replace("\r\n", "\n").replace('\r', "\n"))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub fn request(rows: &[Row]) -> String {
    rows.iter()
        .take_while(|r| r.role == "user")
        .map(|r| r.text.as_str())
        .collect::<String>()
        .trim()
        .into()
}
pub fn edited_files(rows: &[Row]) -> Vec<String> {
    let mut files = Vec::new();
    for row in rows.iter().filter(|r| r.role == "tool") {
        if let Ok(tool) = serde_json::from_str::<ToolCall>(&row.text) {
            let (action, path) = tool.summary();
            if matches!(action.as_str(), "Edit" | "Write")
                && !path.is_empty()
                && !files.contains(&path)
            {
                files.push(path);
            }
        }
    }
    files.truncate(40);
    files
}
pub fn second_opinion(from: &str, rows: &[Row]) -> String {
    let title = crate::provider::Kind::from_id(from).label();
    let user = request(rows);
    let report = prose(rows);
    let files = edited_files(rows);
    let sections=[
        format!("Give a second opinion on work {title} just finished in this same working copy. The files are already on disk."),
        "Review that work: what is wrong, what is missing, and what you would have done differently. Fix anything you agree is broken or incomplete. If you would leave it, say so and stop. Do not redo the task from scratch unless the work is actually wrong. Read the listed files before changing anything.".into(),
        format!("## User request\n{}",if user.is_empty(){"(no user message on this turn)".into()}else{limit(&user,400)}),
        format!("## What {title} reported\n{}",if report.is_empty(){"(no written summary — inspect the files)".into()}else{limit(&report,900)}),
        format!("## Files it edited\n{}",if files.is_empty(){"(none recorded on this turn)".into()}else{files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")}),
    ];
    limit(&sections.join("\n\n"), 1800)
}
pub fn handoff(rows: &[Row]) -> String {
    let mut users = Vec::new();
    let mut user = String::new();
    for row in rows {
        if row.role == "user" {
            if row.first && !user.is_empty() {
                users.push(std::mem::take(&mut user));
            }
            user.push_str(&row.text);
        }
    }
    if !user.is_empty() {
        users.push(user);
    }
    let last = rows
        .iter()
        .rposition(|r| r.role == "assistant" && r.first)
        .unwrap_or(rows.len());
    let report = prose(&rows[last..]);
    let files = edited_files(rows);
    let mut sections = vec![format!(
        "## Session so far\n{}",
        users
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| format!("User: {}", limit(s, 240).replace('\n', " ")))
            .collect::<Vec<_>>()
            .join("\n")
    )];
    if !report.is_empty() {
        sections.push(format!("Assistant: {}", limit(&report, 500)));
    }
    if !files.is_empty() {
        sections.push(format!(
            "## Files edited in this session\n{}",
            files
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    limit(&sections.join("\n\n"), 1800)
}

pub fn outgoing_handoff_prompt(user_request: &str) -> String {
    let request = user_request.trim();
    let request = if request.is_empty() {
        "(no text)"
    } else {
        request
    };
    format!(
        "The user is switching to another coding agent. Their new message will be sent separately — do not repeat it, and do not add a Goal heading.\n\n<user_request>\n{}\n</user_request>\n\nWrite a short recap of this conversation so the next agent can continue. Under 120 words. Plain markdown. No title card. No greeting. Do not paste the whole transcript.\n\nRules:\n- Use only this conversation.\n- Do not run git, do not inspect the working tree, do not read files, do not call tools.\n- Mention files only if this chat edited them.\n- If the chat was a greeting or has no task yet, say that in one sentence. Do not invent work from uncommitted repo files.\n\nInclude only sections that have session-specific content:\n- Session so far (a few bullets, not a transcript)\n- Files edited in this session\n- Suggested next step",
        limit(request, 1500)
    )
}

pub fn choose_handoff_brief(agent: &str, rows: &[Row]) -> String {
    let agent = strip_goal_sections(agent);
    if agent.chars().count() >= 40 {
        limit(&agent, 1800)
    } else {
        strip_goal_sections(&handoff(rows))
    }
}

pub fn wrap_handoff_prompt(from: &str, brief: &str, user_request: &str) -> String {
    let provider = crate::provider::Kind::from_id(from).label();
    let body = strip_goal_sections(brief);
    let lead = format!(
        "You are continuing an existing conversation handed off from {provider}. This is not a new session. Do not say you have no prior context.\n\n{}",
        user_request.trim()
    );
    if body.is_empty() {
        format!("{lead}\n\nContinue from a {provider} session. Do not invent prior work.")
    } else {
        format!(
            "{lead}\n\nPrior conversation from {provider} — this is the thread you are joining, not optional background:\n\n<handoff>\n{body}\n</handoff>"
        )
    }
}

pub fn strip_goal_sections(markdown: &str) -> String {
    let mut skip_section = false;
    let mut removed_goal_line = false;
    markdown
        .trim()
        .lines()
        .filter(|line| {
            let heading = line.trim_start_matches('#');
            let depth = line.len() - heading.len();
            if (1..=6).contains(&depth) && heading.starts_with(char::is_whitespace) {
                let title = heading.trim_start().to_lowercase();
                skip_section = title.strip_prefix("goal").is_some_and(|rest| {
                    rest.chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                });
            }
            if skip_section {
                return false;
            }
            if !removed_goal_line && line.to_lowercase().starts_with("goal:") {
                removed_goal_line = true;
                return false;
            }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_prose_and_transfer_recaps_keep_message_boundaries() {
        let rows = vec![
            Row {
                role: "user".into(),
                text: "Check the footer".into(),
                first: true,
                sent_at: None,
            },
            Row {
                role: "assistant".into(),
                text: "I’ll inspect ".into(),
                first: true,
                sent_at: None,
            },
            Row {
                role: "assistant".into(),
                text: "the footer.".into(),
                first: false,
                sent_at: None,
            },
            Row {
                role: "reasoning".into(),
                text: "Private reasoning".into(),
                first: true,
                sent_at: None,
            },
            Row {
                role: "tool".into(),
                text: "Tool output".into(),
                first: true,
                sent_at: None,
            },
            Row {
                role: "assistant".into(),
                text: "**The footer is ready.**\n\n```rust\nlet ready = true;\n```".into(),
                first: true,
                sent_at: None,
            },
        ];
        assert_eq!(
            prose(&rows),
            "I’ll inspect the footer.\n\n**The footer is ready.**\n\n```rust\nlet ready = true;\n```"
        );
        let recap = handoff(&rows);
        assert!(recap.contains("User: Check the footer"));
        assert!(recap.contains("**The footer is ready.**"));
        assert!(!recap.contains("Private reasoning"));
        let review = second_opinion("codex", &rows);
        assert!(review.contains("## User request\nCheck the footer"));
        assert!(review.contains("## What Codex reported"));
        assert!(!review.contains("Tool output"));
    }
}
