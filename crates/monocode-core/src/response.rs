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
/// The final assistant message, including its virtualized continuation rows.
pub fn final_prose(rows: &[Row]) -> String {
    let start = rows
        .iter()
        .rposition(|row| row.first && row.role == "assistant")
        .unwrap_or(rows.len());
    prose(&rows[start..])
}
/// Keeps the end of `text`, where a session's answer is, within `count` chars.
pub fn limit_tail(text: &str, count: usize, marker: &str) -> String {
    let total = text.chars().count();
    if total <= count {
        return text.into();
    }
    let keep = count.saturating_sub(marker.chars().count() + 2);
    let tail: String = text.chars().skip(total - keep).collect();
    format!("{marker}\n\n{tail}")
}
/// A delegated session's completion report: the final assistant message of its
/// latest turn only. Interim progress messages and earlier turns' reports, which
/// the Mono has already received, are left out. Over budget, the end is kept.
pub fn completion_report(session: i64, rows: &[Row], count: usize) -> String {
    let turn = rows.iter().rposition(Row::is_prompt).map_or(0, |i| i + 1);
    let text = final_prose(&rows[turn..]);
    if text.is_empty() {
        return "(no written reply on its latest turn)".into();
    }
    limit_tail(
        &text,
        count,
        &format!(
            "[earlier text truncated; read the full message with sessions.read {{\"sessionId\":{session},\"limit\":1,\"maxChars\":6000}} and pass nextCharOffset as charOffset for the rest]"
        ),
    )
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
    fn final_prose_keeps_the_whole_last_message_without_opening_commentary() {
        let mut transcript = crate::transcript::Transcript::default();
        transcript.begin("mono-system");
        transcript.append("Run the scheduled check");
        assert_eq!(final_prose(&transcript.rows), "");
        transcript.begin("assistant");
        transcript.append("I'll check for updates.");
        transcript.begin("tool");
        transcript.append("{}");
        let report = format!("**Results**\r\n\r\n{}", "A useful finding.\n".repeat(200));
        transcript.begin("assistant");
        transcript.append(&report);
        transcript.begin("mono-document");
        transcript.append("{}");
        assert_eq!(
            final_prose(&transcript.rows),
            report.replace("\r\n", "\n").trim()
        );

        transcript.begin("assistant");
        transcript.append("`NOTHING_TO_REPORT`");
        assert!(
            crate::mono::habits::report(&final_prose(&transcript.rows)).is_none(),
            "a quiet final message must not post the opening commentary"
        );
        transcript.begin("assistant");
        assert_eq!(final_prose(&transcript.rows), "");
    }

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn completion_report_is_the_latest_turns_final_message() {
        let mut rows = vec![
            row("user", "Investigate the crash"),
            row("assistant", "Now reading main.rs..."),
            row("tool", "{}"),
            row("assistant", "Now checking the logs..."),
            row("assistant", "First answer: the crash is a null deref."),
        ];
        assert_eq!(
            completion_report(7, &rows, 12000),
            "First answer: the crash is a null deref."
        );
        rows.push(row("user", "Now fix it"));
        rows.push(row("assistant", "Editing main.rs..."));
        rows.push(row("assistant", "Fixed: added a guard."));
        let report = completion_report(7, &rows, 12000);
        assert_eq!(report, "Fixed: added a guard.");
        assert!(
            !report.contains("First answer"),
            "follow-up must not repeat the first turn"
        );
        rows.push(row("user", "Anything else?"));
        rows.push(row("tool", "{}"));
        assert_eq!(
            completion_report(7, &rows, 12000),
            "(no written reply on its latest turn)",
            "an earlier turn's answer must not stand in for a silent turn"
        );
    }

    #[test]
    fn completion_report_keeps_the_end_when_over_budget() {
        let answer = format!("{}THE CONCLUSION", "detail ".repeat(3000));
        let rows = vec![row("user", "Go"), row("assistant", &answer)];
        let report = completion_report(42, &rows, 12000);
        assert!(report.chars().count() <= 12000);
        assert!(report.ends_with("THE CONCLUSION"));
        assert!(report.starts_with(
            "[earlier text truncated; read the full message with sessions.read {\"sessionId\":42,"
        ));
        assert!(report.contains("nextCharOffset"));
    }

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
