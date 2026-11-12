//! Response actions use the same prose and recap rules as transcriptActivity.ts,
//! secondOpinion.ts and handoff.ts. Tool chrome and private reasoning aren't copied.
use crate::{
    tool::{ToolCall, ToolState},
    transcript::Row,
};
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
/// One message's text as reports and `sessions.read` both count it: trimmed,
/// with `\r\n` and `\r` as `\n`, so character offsets agree between them.
pub fn normalized(text: &str) -> String {
    text.trim().replace("\r\n", "\n").replace('\r', "\n")
}
/// Keeps the end of `text`, where a session's answer is, within `count` chars.
/// `marker` leads the kept part only when it fits; the result never exceeds `count`.
pub fn limit_tail(text: &str, count: usize, marker: &str) -> String {
    let total = text.chars().count();
    if total <= count {
        return text.into();
    }
    let room = marker.chars().count() + 2;
    if room >= count {
        return text.chars().skip(total - count).collect();
    }
    let keep = count - room;
    let tail: String = text.chars().skip(total - keep).collect();
    format!("{marker}\n\n{tail}")
}
/// How a delegated session's latest turn ended, as its turn receipt records it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TurnEnd {
    /// No receipt (Tauri, remote and legacy turns): only the rows can tell.
    #[default]
    Unknown,
    /// The receipt was closed without a stop: the turn ran to its end.
    Settled,
    /// The user or the Mono stopped it. Native stops leave no row.
    Stopped,
    /// The receipt was opened but never closed: the app quit or restarted.
    Interrupted,
}
/// The latest turn's outcome from its receipt. A steer's receipt defers to
/// its root turn's, which the runtime closes for the whole provider run.
pub fn turn_end(
    rows: &[Row],
    turns: &std::collections::HashMap<usize, crate::agent::TurnInfo>,
) -> TurnEnd {
    let turn = rows.iter().rposition(Row::is_prompt).map_or(0, |i| i + 1);
    let Some(mut info) = turns.get(&turn) else {
        return TurnEnd::Unknown;
    };
    if let Some(root) = info.joined_turn.and_then(|root| turns.get(&root)) {
        info = root;
    }
    if info.stopped {
        TurnEnd::Stopped
    } else if info.has_duration() {
        TurnEnd::Settled
    } else if info.started_at.is_some() {
        TurnEnd::Interrupted
    } else {
        TurnEnd::Unknown
    }
}
/// The `sessions.read` input that returns `session`'s latest final message.
/// `project` is the Mono project the session belongs to, when `sessions.read`
/// can resolve it. Remote sessions (negative IDs) can't be read that way.
pub fn read_call(session: i64, project: Option<&str>) -> Option<serde_json::Value> {
    let project = project?;
    (session > 0).then(
        || serde_json::json!({"sessionId":session,"project":project,"limit":1,"maxChars":6000}),
    )
}
/// A delegated session's completion report: the last non-blank assistant
/// message of its latest turn. Interim progress messages and earlier turns'
/// reports, which the Mono has already received, are left out. Over budget,
/// the end is kept, and `read` (from [`read_call`]) says how to read the start.
/// A turn that failed, was stopped or was interrupted is reported as
/// unfinished, so a progress line isn't passed off as the answer.
pub fn completion_report(
    rows: &[Row],
    end: TurnEnd,
    read: Option<&serde_json::Value>,
    count: usize,
) -> String {
    let turn = &rows[rows.iter().rposition(Row::is_prompt).map_or(0, |i| i + 1)..];
    // Assistant messages as (start, end) row ranges; continuation rows join theirs.
    let mut messages: Vec<(usize, usize)> = Vec::new();
    for (i, row) in turn.iter().enumerate() {
        if row.role != "assistant" {
            continue;
        }
        match messages.last_mut() {
            Some(last) if last.1 == i && !row.first => last.1 = i + 1,
            _ => messages.push((i, i + 1)),
        }
    }
    let answer = messages.iter().rev().find_map(|&(from, to)| {
        let text = normalized(
            &turn[from..to]
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
        );
        (!text.is_empty()).then_some((to, text))
    });
    let after = &turn[answer.as_ref().map_or(0, |(to, _)| *to)..];
    let error = after
        .iter()
        .rev()
        .find(|r| r.role == "error")
        .map(|r| normalized(&r.text));
    let unfinished = match error.as_deref() {
        Some("Stopped") => Some("it was stopped".to_string()),
        Some(error) => Some(format!(
            "it failed with an error: {}",
            error
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(300)
                .collect::<String>()
        )),
        None => match end {
            TurnEnd::Stopped => Some("it was stopped".into()),
            TurnEnd::Interrupted => Some("the app quit or restarted while it was running".into()),
            // A closed receipt is authoritative: a tool the provider cancelled
            // on its own, or one left Failed by an ACP background handoff, is
            // part of a turn that still finished.
            TurnEnd::Settled => None,
            // Without a receipt, settled turns mark leftover running tools
            // Cancelled (stop) or Failed. Tools update in place, so a stopped
            // tool can sit before the last message: look at the whole turn.
            // One still Running means the app went away mid-turn.
            TurnEnd::Unknown => turn
                .iter()
                .filter(|r| r.role == "tool")
                .filter_map(|r| serde_json::from_str::<ToolCall>(&r.text).ok())
                .find_map(|tool| match tool.state {
                    ToolState::Cancelled => Some("it was stopped".into()),
                    ToolState::Running => Some("it was interrupted during a tool call".into()),
                    _ => None,
                }),
        },
    };
    let Some((_, text)) = answer else {
        return match unfinished {
            Some(how) => format!("(its latest turn did not finish: {how}. It wrote no message.)"),
            None => "(no written reply on its latest turn)".into(),
        };
    };
    let header = unfinished.map(|how| {
        format!(
            "(its latest turn did not finish: {how}. This is the last message it wrote, not a final answer:)\n\n"
        )
    });
    let header_len = header.as_deref().map_or(0, |h| h.chars().count());
    let budget = count.saturating_sub(header_len);
    let total = text.chars().count();
    let marker = |omitted: usize| {
        match read {
        Some(call) => format!(
            "[the first {omitted} of {total} characters were cut. sessions.read {call} returns them from the start; repeat it with charOffset set to the nextCharOffset it returns, stopping at character {omitted}]"
        ),
        None => "[the start of this message was cut; open the session to read it in full]".into(),
    }
    };
    // The cut's length is written in the marker, so settle it against the
    // marker's own length; the digit count changes at most a couple of times.
    let mut text_marker = marker(0);
    for _ in 0..4 {
        let room = text_marker.chars().count() + 2;
        let omitted = total.saturating_sub(budget.saturating_sub(room));
        let next = marker(omitted);
        if next == text_marker {
            break;
        }
        text_marker = next;
    }
    let marker = text_marker;
    format!(
        "{}{}",
        header.unwrap_or_default(),
        limit_tail(&text, budget, &marker)
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

    /// Rows as the runtime writes them: `begin` opens a message, `append`
    /// splits long text into continuation rows.
    fn rows(steps: &[(&str, &str)]) -> Vec<Row> {
        let mut transcript = crate::transcript::Transcript::default();
        for (role, text) in steps {
            transcript.begin(role);
            transcript.append(text);
        }
        transcript.rows
    }
    fn tool(state: ToolState) -> String {
        let mut call = ToolCall::new("t1", "Read");
        call.state = state;
        serde_json::to_string(&call).unwrap()
    }
    fn report(rows: &[Row]) -> String {
        completion_report(rows, TurnEnd::Unknown, None, 12000)
    }

    #[test]
    fn completion_report_is_the_latest_turns_final_message() {
        let done = tool(ToolState::Completed);
        let mut steps = vec![
            ("user", "Investigate the crash"),
            ("assistant", "Now reading main.rs..."),
            ("tool", done.as_str()),
            ("assistant", "Now checking the logs..."),
            ("assistant", "First answer: the crash is a null deref."),
        ];
        assert_eq!(
            report(&rows(&steps)),
            "First answer: the crash is a null deref."
        );
        steps.extend([
            ("user", "Now fix it"),
            ("assistant", "Editing main.rs..."),
            ("assistant", "Fixed: added a guard."),
        ]);
        assert_eq!(report(&rows(&steps)), "Fixed: added a guard.");
        steps.extend([("user", "Anything else?"), ("tool", done.as_str())]);
        assert_eq!(
            report(&rows(&steps)),
            "(no written reply on its latest turn)",
            "an earlier turn's answer must not stand in for a silent turn"
        );
    }

    #[test]
    fn completion_report_skips_blank_messages_after_the_answer() {
        let done = tool(ToolState::Completed);
        // ACP's background handoff opens an empty message; a "\n\n" delta
        // between tool calls opens a whitespace-only one.
        for steps in [
            vec![
                ("user", "Go"),
                ("assistant", "THE ANSWER"),
                ("assistant", ""),
                ("tool", done.as_str()),
            ],
            vec![
                ("user", "Go"),
                ("assistant", "THE ANSWER"),
                ("tool", done.as_str()),
                ("assistant", "\n\n"),
            ],
            vec![
                ("user", "Go"),
                ("assistant", "THE ANSWER"),
                ("assistant", " \r\n\t"),
                ("reasoning", "done"),
            ],
        ] {
            assert_eq!(report(&rows(&steps)), "THE ANSWER", "{steps:?}");
        }
    }

    #[test]
    fn completion_report_says_when_the_turn_did_not_finish() {
        let failed = tool(ToolState::Failed);
        let cancelled = tool(ToolState::Cancelled);
        let running = tool(ToolState::Running);
        let progress = "Now reading main.rs...";
        let cases = [
            // Native: a provider error settles running tools Failed and appends an error row.
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("tool", failed.as_str()),
                    ("error", "Provider rate limited\nretry later"),
                ],
                TurnEnd::Unknown,
                "it failed with an error: Provider rate limited.",
            ),
            // Tauri records a stop through the shared store as a "Stopped" error notice.
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("error", "Stopped"),
                ],
                TurnEnd::Unknown,
                "it was stopped.",
            ),
            // Native stops leave no row; the live view knows, and tools are Cancelled.
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("tool", cancelled.as_str()),
                ],
                TurnEnd::Stopped,
                "it was stopped.",
            ),
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("tool", cancelled.as_str()),
                ],
                TurnEnd::Unknown,
                "it was stopped.",
            ),
            (
                vec![("user", "Go"), ("assistant", progress)],
                TurnEnd::Stopped,
                "it was stopped.",
            ),
            // The app quit mid-turn: the receipt is open and a tool never settled.
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("tool", running.as_str()),
                ],
                TurnEnd::Interrupted,
                "the app quit or restarted while it was running.",
            ),
            (
                vec![
                    ("user", "Go"),
                    ("assistant", progress),
                    ("tool", running.as_str()),
                ],
                TurnEnd::Unknown,
                "it was interrupted during a tool call.",
            ),
        ];
        for (steps, end, how) in cases {
            let report = completion_report(&rows(&steps), end, None, 12000);
            assert_eq!(
                report,
                format!("(its latest turn did not finish: {how} This is the last message it wrote, not a final answer:)\n\n{progress}"),
            );
        }
        assert_eq!(
            report(&rows(&[("user", "Go"), ("error", "boom")])),
            "(its latest turn did not finish: it failed with an error: boom. It wrote no message.)"
        );
        // An earlier turn's failure, or an error the turn recovered from, isn't this answer's.
        let steps = [
            ("user", "Go"),
            ("error", "boom"),
            ("user", "Retry"),
            ("error", "transient"),
            ("assistant", "Done."),
        ];
        assert_eq!(report(&rows(&steps)), "Done.");
    }

    #[test]
    fn turn_receipts_record_how_the_latest_turn_ended() {
        use crate::agent::TurnInfo;
        let rows = rows(&[
            ("user", "Go"),
            ("assistant", "Working"),
            ("user", "Also this"),
        ]);
        let open = TurnInfo {
            started_at: Some(1),
            ..Default::default()
        };
        let settled = TurnInfo {
            started_at: Some(1),
            duration_known: true,
            ..Default::default()
        };
        let stopped = TurnInfo {
            stopped: true,
            ..settled.clone()
        };
        let steer = TurnInfo {
            joined_turn: Some(1),
            ..Default::default()
        };
        let turns = |items: Vec<(usize, TurnInfo)>| items.into_iter().collect();
        let end = |items| turn_end(&rows, &turns(items));
        assert_eq!(end(vec![(3, open.clone())]), TurnEnd::Interrupted);
        assert_eq!(end(vec![(3, settled.clone())]), TurnEnd::Settled);
        assert_eq!(end(vec![(3, stopped.clone())]), TurnEnd::Stopped);
        assert_eq!(
            end(vec![(1, open.clone())]),
            TurnEnd::Unknown,
            "an earlier turn's receipt"
        );
        assert_eq!(
            end(vec![(1, open), (3, steer.clone())]),
            TurnEnd::Interrupted
        );
        assert_eq!(
            end(vec![(1, settled), (3, steer.clone())]),
            TurnEnd::Settled
        );
        assert_eq!(end(vec![(1, stopped), (3, steer)]), TurnEnd::Stopped);
        assert_eq!(
            end(vec![]),
            TurnEnd::Unknown,
            "legacy turns without receipts"
        );
        // The stop survives a round trip through saved settings; unstopped
        // receipts serialize as before.
        let saved = serde_json::to_value(TurnInfo {
            stopped: true,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(saved["stopped"], true);
        assert!(serde_json::from_value::<TurnInfo>(saved).unwrap().stopped);
        assert!(serde_json::to_value(TurnInfo::default())
            .unwrap()
            .get("stopped")
            .is_none());
    }

    /// Tools update their row in place, so a stopped tool can sit before the
    /// turn's last message. Without a receipt the whole turn counts; a closed
    /// receipt is authoritative.
    #[test]
    fn tool_outcomes_anywhere_in_the_turn_mark_it_unfinished() {
        let cancelled = tool(ToolState::Cancelled);
        let running = tool(ToolState::Running);
        let failed = tool(ToolState::Failed);
        let waiting = "Waiting for the delegated results";
        let header = |how: &str| {
            format!("(its latest turn did not finish: {how} This is the last message it wrote, not a final answer:)\n\n{waiting}")
        };
        let before = |state: &str| rows(&[("user", "Go"), ("tool", state), ("assistant", waiting)]);
        assert_eq!(report(&before(&cancelled)), header("it was stopped."));
        assert_eq!(
            report(&before(&running)),
            header("it was interrupted during a tool call.")
        );
        // An ACP background handoff that finished leaves its tool Failed.
        assert_eq!(report(&before(&failed)), waiting);
        assert_eq!(
            completion_report(&before(&cancelled), TurnEnd::Settled, None, 12000),
            waiting,
            "a tool the provider cancelled on its own, in a turn that settled"
        );
        assert_eq!(
            completion_report(&before(&failed), TurnEnd::Stopped, None, 12000),
            header("it was stopped.")
        );
    }

    #[test]
    fn completion_report_keeps_the_end_when_over_budget() {
        let answer = format!("{}THE CONCLUSION", "detail ".repeat(3000));
        let rows = rows(&[("user", "Go"), ("assistant", &answer)]);
        let call = read_call(42, Some("/repo")).unwrap();
        let report = completion_report(&rows, TurnEnd::Unknown, Some(&call), 12000);
        assert_eq!(report.chars().count(), 12000);
        assert!(report.ends_with("THE CONCLUSION"));
        let (marker, tail) = report.split_once("\n\n").unwrap();
        let omitted: usize = marker["[the first ".len()..]
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            omitted + tail.chars().count(),
            21014,
            "the marker names exactly the cut part"
        );
        assert!(marker.ends_with(&format!("stopping at character {omitted}]")));
        assert!(marker.contains(&format!(
            "of 21014 characters were cut. sessions.read {call} returns"
        )));
        // Unfinished turns keep their header within the same budget.
        let unfinished = completion_report(&rows, TurnEnd::Stopped, Some(&call), 12000);
        assert_eq!(unfinished.chars().count(), 12000);
        assert!(unfinished.starts_with("(its latest turn did not finish: it was stopped."));
        assert!(unfinished.ends_with("THE CONCLUSION"));
        // Remote sessions can't be read through sessions.read, so no call is offered.
        assert_eq!(read_call(-5, Some("/repo")), None);
        assert_eq!(read_call(5, None), None);
        let remote = completion_report(&rows, TurnEnd::Unknown, None, 12000);
        assert!(remote.starts_with(
            "[the start of this message was cut; open the session to read it in full]\n\n"
        ));
        assert!(!remote.contains("sessions.read"));
    }

    #[test]
    fn limit_tail_never_exceeds_its_budget() {
        let marker = "[earlier text truncated....]";
        assert_eq!(limit_tail(&"y".repeat(50), 10, marker), "y".repeat(10));
        assert_eq!(limit_tail(&"y".repeat(50), 29, marker).chars().count(), 29);
        assert_eq!(
            limit_tail(&"y".repeat(50), 31, marker),
            format!("{marker}\n\ny")
        );
        assert_eq!(limit_tail("short", 10, marker), "short");
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
