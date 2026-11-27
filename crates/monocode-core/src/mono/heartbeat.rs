//! Inactivity heartbeats: an app-owned wake-up that lets a Mono look for
//! useful follow-ups and propose them. The app gathers short headlines of what
//! changed; the Mono opens details itself only when something looks worth it.
use super::files;
use crate::{store::Session, transcript::Row};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_MINUTES: u32 = 30;
pub const INTERVALS: [u32; 4] = [15, 30, 60, 120];
/// Running habits use the same runner; this id never names a stored habit.
pub const RUN_ID: &str = "heartbeat";
/// A session is worth raising once it has been left alone this long.
pub const IDLE_MS: u64 = 60 * 60 * 1000;
/// Older sessions, notes and inbox items are history, not follow-ups.
const LOOKBACK_MS: u64 = 3 * 24 * 60 * 60 * 1000;
const MAX_SESSIONS: usize = 5;
const MAX_NOTES: usize = 3;
const MAX_INBOX: usize = 5;
const MAX_RECENT: usize = 5;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    /// When the last heartbeat ran or was skipped; the cursor for what changed.
    pub last_run_at: u64,
    /// What it raised lately, so it does not propose the same thing again.
    pub recent: Vec<String>,
    /// Model runs in a row that found nothing; the interval backs off.
    pub quiet_streak: u32,
}
pub fn load(root: &Path, id: &str) -> Result<State, String> {
    let file = files::read(root, id, None, "heartbeat.json")?;
    Ok(file
        .text
        .as_deref()
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default())
}
pub fn update<T>(root: &Path, id: &str, change: impl Fn(&mut State) -> T) -> Result<T, String> {
    for _ in 0..5 {
        let file = files::read(root, id, None, "heartbeat.json")?;
        let mut state: State = file
            .text
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();
        let result = change(&mut state);
        let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())? + "\n";
        match files::save(root, id, None, "heartbeat.json", &text, Some(&file.hash)) {
            Ok(_) => return Ok(result),
            Err(e) if e == files::CONFLICT => continue,
            Err(e) => return Err(e),
        }
    }
    Err("Heartbeat state kept changing; try again".into())
}
impl State {
    pub fn remember(&mut self, report: &str) {
        self.recent.insert(0, clip(report, 300));
        self.recent.truncate(MAX_RECENT);
    }
}

/// Quiet runs back off: after three in a row each one doubles the wait, up to
/// four hours (or the chosen interval, if longer). Any activity resets it.
pub fn interval_ms(minutes: u32, quiet_streak: u32) -> u64 {
    let base = minutes as u64 * 60_000;
    if quiet_streak < 3 {
        return base;
    }
    (base << (quiet_streak - 2).min(4)).min(base.max(4 * 60 * 60_000))
}
/// Due once the Mono has been left alone for its interval. Heartbeats count as
/// activity, so a quiet Mono wakes once per interval rather than every tick.
pub fn due(minutes: u32, state: &State, last_activity: u64, now: u64) -> bool {
    let streak = if last_activity > state.last_run_at {
        0
    } else {
        state.quiet_streak
    };
    minutes > 0
        && state.last_run_at > 0
        && now.saturating_sub(state.last_run_at.max(last_activity)) >= interval_ms(minutes, streak)
}

/// Deciding whether anything is worth raising is triage, not work: a small
/// model at medium effort, enough judgment to tell noise from a real follow-up.
/// Proposals the user accepts run in the Mono's own chat, on its own model. The first candidate the live catalog offers wins; with
/// none, the heartbeat keeps the Mono's model.
pub fn model(
    provider: &str,
    current: Option<&str>,
    available: &[crate::catalog::Model],
) -> Option<(String, Option<String>)> {
    let candidates: &[&str] = match provider {
        "claude" => &["haiku", "claude-haiku-5-5"],
        "codex" => &["gpt-6-luna", "gpt-5.6-luna"],
        "pi" | "omp" => &["openai-codex/gpt-6-luna", "openai-codex/gpt-5.6-luna"],
        "cursor" => &["composer-2.5", "default"],
        "fx" => &["anthropic/claude-haiku-5.5", "openai/gpt-5.4-mini"],
        // Stay on the route (and account) the Mono already uses.
        "hermes" => &[
            "anthropic:claude-haiku-5-5",
            "openrouter:anthropic/claude-haiku-5.5",
        ],
        _ => &[],
    };
    let route = current.and_then(|m| m.split_once(':')).map(|(r, _)| r);
    candidates
        .iter()
        .filter(|id| {
            provider != "hermes" || route.is_some_and(|r| id.starts_with(&format!("{r}:")))
        })
        .find_map(|id| available.iter().find(|m| m.id == *id))
        .map(|m| {
            let effort = ["medium", "low", "minimal"]
                .iter()
                .find(|e| m.efforts.iter().any(|x| x == *e))
                .map(|e| e.to_string());
            (m.id.clone(), effort)
        })
}

/// The last two exchanges, shortened: enough to know what the user and the
/// Mono were in the middle of, without resending the chat.
pub fn brief(rows: &[Row]) -> String {
    let mut exchanges: Vec<(String, String)> = Vec::new();
    for row in rows {
        if row.role == "user" {
            if row.first || exchanges.is_empty() {
                exchanges.push((row.text.clone(), String::new()));
            } else if let Some(last) = exchanges.last_mut() {
                last.0.push_str(&row.text);
            }
        } else if row.role == "assistant" {
            if let Some(last) = exchanges.last_mut() {
                if row.first {
                    last.1 = row.text.clone();
                } else {
                    last.1.push_str(&row.text);
                }
            }
        }
    }
    exchanges
        .iter()
        .rev()
        .take(2)
        .rev()
        .map(|(user, agent)| {
            let mut text = format!("User: {}", clip(user, 400));
            if !agent.trim().is_empty() {
                text.push_str(&format!("\nYou: {}", clip(agent, 800)));
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The soul's standing instructions, without the default placeholder.
pub fn standing_instructions(soul: &str) -> Option<String> {
    let mut lines = soul.lines().skip_while(|l| {
        !l.trim_start_matches('#')
            .trim()
            .eq_ignore_ascii_case("standing instructions")
            || !l.starts_with('#')
    });
    lines.next()?;
    let body = lines
        .take_while(|l| !l.starts_with("## ") && !l.starts_with("# "))
        .filter(|l| {
            let t = l.trim().trim_start_matches(['-', '*']).trim();
            !t.is_empty() && !(t.starts_with('(') && t.ends_with(')'))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!body.trim().is_empty()).then(|| clip(&body, 4000))
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionLead {
    pub id: i64,
    pub title: String,
    pub idle_ms: u64,
    pub user: String,
    pub agent: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NoteLead {
    pub id: String,
    pub title: String,
    pub updated_at: u64,
    pub preview: String,
}
#[derive(Clone, Debug, Default)]
pub struct Digest {
    pub sessions: Vec<SessionLead>,
    pub more_sessions: usize,
    pub notes: Vec<NoteLead>,
    pub more_notes: usize,
    pub inbox: Vec<String>,
    pub more_inbox: usize,
}
impl Digest {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.notes.is_empty() && self.inbox.is_empty()
    }
}

fn in_projects(cwd: &str, projects: &[String]) -> bool {
    let cwd = crate::files::path_key(cwd);
    projects.iter().any(|p| {
        let p = crate::files::path_key(p);
        cwd == p || cwd.starts_with(&format!("{}/", p.trim_end_matches('/')))
    })
}

/// Sessions in the Mono's projects that crossed the idle threshold since the
/// last heartbeat, so each left-alone session is raised once.
pub fn idle_sessions<'a>(
    sessions: &'a [Session],
    projects: &[String],
    skip: impl Fn(i64) -> bool,
    since: u64,
    now: u64,
) -> Vec<&'a Session> {
    let floor = since.max(now.saturating_sub(LOOKBACK_MS));
    let mut found: Vec<_> = sessions
        .iter()
        .filter(|s| {
            let idle_from = s.updated_at + IDLE_MS;
            !s.archived
                && s.has_messages
                && s.updated_at > 0
                && !skip(s.id)
                && in_projects(&s.cwd, projects)
                && idle_from <= now
                && idle_from > floor
        })
        .collect();
    found.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    found
}

/// The final user message and the end of the agent's reply: enough to notice
/// an unanswered plan or question without loading the conversation.
pub fn last_exchange(rows: &[Row]) -> (String, String) {
    let Some(prompt) = rows.iter().rposition(|r| r.role == "user" && r.first) else {
        return (String::new(), String::new());
    };
    let user = rows[prompt..]
        .iter()
        .take_while(|r| r.role == "user")
        .map(|r| r.text.as_str())
        .collect::<String>();
    let reply_start = rows[prompt..]
        .iter()
        .rposition(|r| r.role == "assistant" && r.first)
        .map(|i| prompt + i);
    let agent = reply_start
        .map(|at| {
            rows[at..]
                .iter()
                .take_while(|r| r.role == "assistant")
                .map(|r| r.text.as_str())
                .collect::<String>()
        })
        .unwrap_or_default();
    (clip(&user, 200), tail(&agent, 300))
}

/// Notes saved from the Mono's projects. Notes with no project are the user's
/// own; the Mono can still list them, but a heartbeat does not raise them.
pub fn changed_notes(
    notes: &[crate::notes::Note],
    projects: &[String],
    since: u64,
    now: u64,
) -> Vec<NoteLead> {
    let floor = since.max(now.saturating_sub(LOOKBACK_MS)) as i64;
    let mut found: Vec<_> = notes
        .iter()
        .filter(|n| {
            n.updated_at > floor
                && n.source_cwd
                    .as_deref()
                    .is_some_and(|cwd| in_projects(cwd, projects))
        })
        .map(|n| NoteLead {
            id: n.id.clone(),
            title: if n.title.trim().is_empty() {
                "Untitled".into()
            } else {
                clip(&n.title, 120)
            },
            updated_at: n.updated_at.max(0) as u64,
            preview: clip(
                n.body.lines().find(|l| !l.trim().is_empty()).unwrap_or(""),
                160,
            ),
        })
        .collect();
    found.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
    found
}

/// One line per changed inbox item; a large PR still costs one line. Only items
/// tied to an assigned checkout: tracker issues (Linear, Jira) have none.
pub fn changed_inbox(
    items: &[crate::inbox::Item],
    projects: &[String],
    since: u64,
    now: u64,
) -> Vec<String> {
    let floor = since.max(now.saturating_sub(LOOKBACK_MS)) as i64;
    let mut found: Vec<_> = items
        .iter()
        .filter(|i| {
            i.updated_ms() > floor
                && i.status() != "Closed"
                && i.status() != "Merged"
                && in_projects(&i.project_path, projects)
        })
        .collect();
    found.sort_by_key(|i| std::cmp::Reverse(i.updated_ms()));
    found
        .into_iter()
        .map(|i| {
            let mut line = format!(
                "{} {} {} \"{}\" · {}",
                crate::inbox::provider_label(&i.provider),
                if i.kind.is_empty() { "item" } else { &i.kind },
                i.reference(),
                clip(&i.title, 120),
                i.status()
            );
            if !i.repo.is_empty() {
                line.push_str(&format!(" · {}", i.repo));
            }
            if !i.attention_reason.is_empty() {
                line.push_str(&format!(" · {}", clip(&i.attention_reason, 80)));
            }
            line.push_str(&format!(
                " · updated {}",
                age(now.saturating_sub(i.updated_ms() as u64))
            ));
            line
        })
        .collect()
}

pub fn cap<T>(mut items: Vec<T>, max: usize) -> (Vec<T>, usize) {
    let more = items.len().saturating_sub(max);
    items.truncate(max);
    (items, more)
}
pub fn digest(sessions: Vec<SessionLead>, notes: Vec<NoteLead>, inbox: Vec<String>) -> Digest {
    let (sessions, more_sessions) = cap(sessions, MAX_SESSIONS);
    let (notes, more_notes) = cap(notes, MAX_NOTES);
    let (inbox, more_inbox) = cap(inbox, MAX_INBOX);
    Digest {
        sessions,
        more_sessions,
        notes,
        more_notes,
        inbox,
        more_inbox,
    }
}

const GUIDANCE: &str = "[Heartbeat] An inactivity wake-up from MonoCode, not a user message. You run in a side session; the user sees nothing unless you report.

Do one round:
1. Reread your soul's standing instructions. Do the next round of any recurring check they describe, within the access you were given.
2. Review your memory and the recent chat below for open commitments or follow-ups you promised.
3. Scan what changed. Open at most 3 promising items with sessions.read, notes.read or the GitHub CLI before judging them; never load everything.

The headlines below are only filtered to your assigned projects, not to your role. Your soul decides what kind of work is yours: skip anything outside it. For example, if you are not meant to review or work on pull requests, ignore PRs entirely. Most headlines will not concern you; that is expected.

A heartbeat does not authorize new projects, code changes, starting sessions, sending messages, commenting, publishing or destructive actions. Propose those instead: write a short, specific message to the user and add a choices card with the replies they can pick. At most 2 proposals. Never invent work to fill the interval, and never repeat something you raised recently unless it changed. If nothing is worth raising, reply exactly NOTHING_TO_REPORT.";

pub fn message(
    standing: Option<&str>,
    digest: &Digest,
    recent: &[String],
    brief: &str,
    since: u64,
    now: u64,
) -> String {
    let mut out = String::from(GUIDANCE);
    if let Some(standing) = standing {
        out.push_str("\n\nStanding instructions (from your soul):\n");
        out.push_str(standing);
    }
    if !brief.trim().is_empty() {
        out.push_str("\n\nRecent chat with the user:\n");
        out.push_str(brief.trim());
    }
    if !recent.is_empty() {
        out.push_str("\n\nYou raised these lately:\n");
        for item in recent {
            out.push_str(&format!("- {}\n", item.replace('\n', " ")));
        }
    }
    out.push_str(&format!(
        "\n\nSince your last heartbeat ({} ago):",
        age(now.saturating_sub(since))
    ));
    if digest.is_empty() {
        out.push_str("\nNothing new in sessions, notes or the inbox.");
    }
    if !digest.sessions.is_empty() {
        out.push_str("\n\nSessions left idle:");
        for s in &digest.sessions {
            out.push_str(&format!(
                "\n- \"{}\" (sessionId {}) · idle {}",
                s.title,
                s.id,
                age(s.idle_ms)
            ));
            if !s.user.is_empty() {
                out.push_str(&format!("\n  user: \"{}\"", s.user));
            }
            if !s.agent.is_empty() {
                out.push_str(&format!("\n  agent: \"…{}\"", s.agent));
            }
        }
        more(&mut out, digest.more_sessions);
    }
    if !digest.inbox.is_empty() {
        out.push_str("\n\nInbox:");
        for line in &digest.inbox {
            out.push_str(&format!("\n- {line}"));
        }
        more(&mut out, digest.more_inbox);
    }
    if !digest.notes.is_empty() {
        out.push_str("\n\nNotes:");
        for n in &digest.notes {
            out.push_str(&format!(
                "\n- \"{}\" (id {}) · edited {} ago",
                n.title,
                n.id,
                age(now.saturating_sub(n.updated_at))
            ));
            if !n.preview.is_empty() {
                out.push_str(&format!(" · \"{}\"", n.preview));
            }
        }
        more(&mut out, digest.more_notes);
    }
    out
}
fn more(out: &mut String, n: usize) {
    if n > 0 {
        out.push_str(&format!("\n- +{n} more"));
    }
}

fn clip(text: &str, n: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= n {
        return text;
    }
    text.chars().take(n).collect::<String>() + "…"
}
fn tail(text: &str, n: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(n)).collect()
}
fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    if minutes < 60 {
        format!("{}m", minutes.max(1))
    } else if minutes < 48 * 60 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / (24 * 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: i64, cwd: &str, updated_at: u64) -> Session {
        Session {
            id,
            title: format!("S{id}"),
            provider: "codex".into(),
            cwd: cwd.into(),
            provider_id: None,
            has_messages: true,
            updated_at,
            archived: false,
            pinned: false,
            work_item: None,
        }
    }
    fn row(role: &str, text: &str, first: bool) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first,
            sent_at: None,
        }
    }

    #[test]
    fn standing_instructions_skip_the_default_placeholder() {
        assert_eq!(standing_instructions(super::super::DEFAULT_SOUL), None);
        let soul = "# Soul\n\n## Standing instructions\n- Keep CI green\n\n## Voice\n- Brief\n";
        assert_eq!(
            standing_instructions(soul).as_deref(),
            Some("- Keep CI green")
        );
    }

    #[test]
    fn due_waits_for_a_full_quiet_interval_and_never_fires_unseeded() {
        let min = 60_000;
        let at = |last_run_at, quiet_streak| State {
            last_run_at,
            quiet_streak,
            ..Default::default()
        };
        assert!(!due(30, &at(0, 0), 0, 100 * min));
        assert!(!due(30, &at(10 * min, 0), 0, 39 * min));
        assert!(due(30, &at(10 * min, 0), 0, 40 * min));
        assert!(!due(30, &at(10 * min, 0), 20 * min, 40 * min));
        assert!(!due(0, &at(10 * min, 0), 0, 400 * min));
        // Three quiet runs double the wait; user activity resets it.
        assert!(!due(30, &at(10 * min, 3), 0, 40 * min));
        assert!(due(30, &at(10 * min, 3), 0, 70 * min));
        assert!(due(30, &at(10 * min, 3), 11 * min, 41 * min));
    }

    #[test]
    fn quiet_backoff_doubles_up_to_four_hours() {
        let min = 60_000;
        assert_eq!(interval_ms(30, 2), 30 * min);
        assert_eq!(interval_ms(30, 3), 60 * min);
        assert_eq!(interval_ms(30, 4), 120 * min);
        assert_eq!(interval_ms(30, 9), 240 * min);
        assert_eq!(interval_ms(300, 9), 300 * min);
    }

    #[test]
    fn heartbeat_model_prefers_cheap_catalog_entries_and_falls_back() {
        let m = |id: &str, efforts: &[&str]| crate::catalog::Model {
            id: id.into(),
            name: id.into(),
            alias_target: None,
            efforts: efforts.iter().map(|e| e.to_string()).collect(),
            default_effort: None,
            fast: false,
            thinking: false,
            default: false,
            tiers: Vec::new(),
            controls: Vec::new(),
        };
        let codex = [
            m("gpt-6.1-sol", &["low", "high"]),
            m("gpt-5.6-luna", &["low", "medium"]),
        ];
        assert_eq!(
            model("codex", Some("gpt-6.1-sol"), &codex),
            Some(("gpt-5.6-luna".into(), Some("medium".into())))
        );
        let cursor = [m("default", &[]), m("composer-2.5", &[])];
        assert_eq!(
            model("cursor", None, &cursor),
            Some(("composer-2.5".into(), None))
        );
        assert_eq!(model("claude", Some("opus"), &[m("opus", &["low"])]), None);
        assert_eq!(
            model("claude", Some("opus"), &[m("haiku", &["low", "high"])]),
            Some(("haiku".into(), Some("low".into()))),
            "without medium, the lightest offered effort"
        );
        assert_eq!(
            model("grok", Some("grok-4.7"), &[m("grok-4.7-build-fast", &[])]),
            None
        );
        let hermes = [
            m("anthropic:claude-haiku-5-5", &[]),
            m("openrouter:anthropic/claude-haiku-5.5", &[]),
        ];
        assert_eq!(
            model(
                "hermes",
                Some("openrouter:anthropic/claude-opus-5.5"),
                &hermes
            )
            .map(|m| m.0),
            Some("openrouter:anthropic/claude-haiku-5.5".into())
        );
        assert_eq!(model("hermes", None, &hermes), None);
    }

    #[test]
    fn brief_keeps_only_the_last_two_exchanges_shortened() {
        let rows = vec![
            row("user", "old question", true),
            row("assistant", "old answer", true),
            row("user", "second", true),
            row("assistant", "reply two", true),
            row("user", &"long ".repeat(200), true),
            row("assistant", "final", true),
        ];
        let text = brief(&rows);
        assert!(!text.contains("old question"));
        assert!(text.starts_with("User: second\nYou: reply two"));
        assert!(text.len() < 700);
    }

    #[test]
    fn idle_sessions_are_raised_once_and_only_in_assigned_projects() {
        let hour = IDLE_MS;
        let now = 10 * hour;
        let all = vec![
            session(1, "/repo", now - hour - 1),      // just crossed idle
            session(2, "/repo", now - 3 * hour),      // crossed before the last beat
            session(3, "/elsewhere", now - hour - 1), // other project
            session(4, "/repo/wt", now - hour - 5),   // worktree under the project
            session(5, "/repo", now - 10),            // still active
        ];
        let since = now - 30 * 60_000;
        let ids: Vec<_> = idle_sessions(&all, &["/repo".into()], |_| false, since, now)
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec![1, 4]);
        assert!(
            idle_sessions(&all, &["/repo".into()], |id| id == 1 || id == 4, since, now).is_empty()
        );
    }

    #[test]
    fn last_exchange_keeps_the_ask_and_the_end_of_the_reply() {
        let rows = vec![
            row("user", "first", true),
            row("assistant", "old", true),
            row("user", "lets do option B", true),
            row("tool", "{}", true),
            row("assistant", "Plan: 1) add flag ", true),
            row("assistant", "2) tests. Want me to start?", false),
        ];
        let (user, agent) = last_exchange(&rows);
        assert_eq!(user, "lets do option B");
        assert!(agent.ends_with("Want me to start?"));
    }

    #[test]
    fn notes_and_inbox_are_scoped_to_assigned_projects() {
        let note = |id: &str, cwd: Option<&str>| crate::notes::Note {
            id: id.into(),
            title: id.into(),
            body: "body".into(),
            slug: id.into(),
            tags: Vec::new(),
            source_session_id: None,
            source_cwd: cwd.map(Into::into),
            slug_pending: false,
            created_at: 100,
            updated_at: 100,
        };
        let notes = vec![
            note("mine", Some("/repo")),
            note("other", Some("/x")),
            note("global", None),
        ];
        let ids: Vec<_> = changed_notes(&notes, &["/repo".into()], 0, 200)
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, vec!["mine"]);

        let item = |provider: &str, path: &str| crate::inbox::Item {
            provider: provider.into(),
            title: format!("{provider} item"),
            project_path: path.into(),
            updated_at: "2026-10-29T10:00:00Z".into(),
            ..Default::default()
        };
        let items = vec![
            item("github", "/repo"),
            item("github", "/x"),
            item("linear", ""),
        ];
        let now = crate::inbox::timestamp_ms("2026-10-29T11:00:00Z") as u64;
        let lines = changed_inbox(&items, &["/repo".into()], 0, now);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("github item"));
    }

    #[test]
    fn message_lists_headlines_and_recent_proposals() {
        let d = digest(
            vec![SessionLead {
                id: 7,
                title: "Hermes effort".into(),
                idle_ms: 3 * IDLE_MS,
                user: "do B".into(),
                agent: "Want me to start?".into(),
            }],
            Vec::new(),
            vec!["GitHub pr #12 \"Fix\" · Open".into(); 7],
        );
        let text = message(
            Some("- Keep CI green"),
            &d,
            &["Proposed fixing clippy".into()],
            "",
            0,
            30 * 60_000,
        );
        assert!(text.contains("NOTHING_TO_REPORT"));
        assert!(text.contains("ignore PRs entirely"));
        assert!(text.contains("- Keep CI green"));
        assert!(text.contains("(sessionId 7) · idle 3h"));
        assert!(text.contains("+2 more"));
        assert!(text.contains("Proposed fixing clippy"));
    }
}
