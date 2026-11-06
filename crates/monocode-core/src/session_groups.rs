//! The sidebar's session grouping: sessionFolders.ts, sessionReminders.ts and
//! sessionWorkItem.ts's URL parsing, without their storage.
use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// sessionFolders.ts's `SessionFolder`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub session_ids: Vec<i64>,
    #[serde(default)]
    pub collapsed: bool,
    /// Index into the tab group palette; missing or 0 is the default wash.
    #[serde(default)]
    pub color_index: Option<usize>,
    /// Desktop folder pickers allow a custom hex color in addition to a palette.
    #[serde(default)]
    pub custom_color: Option<String>,
}

/// A project's folders, and whether its Pinned and Reminders groups are folded.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groups {
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub pinned_collapsed: bool,
    #[serde(default)]
    pub reminders_collapsed: bool,
}

impl Groups {
    pub fn folder_of(&self, session: i64) -> Option<&Folder> {
        self.folders
            .iter()
            .find(|f| f.session_ids.contains(&session))
    }
    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|f| f.id == id)
    }
    fn folder_mut(&mut self, id: &str) -> Option<&mut Folder> {
        self.folders.iter_mut().find(|f| f.id == id)
    }
    fn unique_name(&self, base: &str) -> String {
        let taken = |name: &str| self.folders.iter().any(|f| f.name == name);
        if !taken(base) {
            return base.into();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !taken(name))
            .unwrap()
    }
    fn remove_empty(&mut self) {
        self.folders.retain(|f| !f.session_ids.is_empty());
    }
    /// A new folder, first in the list, takes the sessions from their old ones.
    pub fn create_folder(&mut self, sessions: &[i64], id: String) -> Option<String> {
        let mut ids = Vec::new();
        for &session in sessions {
            if !ids.contains(&session) {
                ids.push(session);
            }
        }
        if ids.is_empty() {
            return None;
        }
        self.remove_sessions(&ids);
        let name = self.unique_name("New folder");
        self.folders.insert(
            0,
            Folder {
                id: id.clone(),
                name,
                session_ids: ids,
                collapsed: false,
                color_index: None,
                custom_color: None,
            },
        );
        Some(id)
    }
    /// Moves the session into the folder and opens it.
    pub fn add_to_folder(&mut self, folder: &str, session: i64) {
        if self.folder(folder).is_none() {
            return;
        }
        if self.folder_of(session).is_none_or(|f| f.id != folder) {
            for f in &mut self.folders {
                f.session_ids.retain(|&id| id != session);
            }
            if let Some(f) = self.folder_mut(folder) {
                f.session_ids.push(session);
            }
            self.remove_empty();
        }
        if let Some(f) = self.folder_mut(folder) {
            f.collapsed = false;
        }
    }
    /// Takes the sessions out of their folders; emptied folders go too.
    pub fn remove_sessions(&mut self, sessions: &[i64]) {
        for f in &mut self.folders {
            f.session_ids.retain(|id| !sessions.contains(id));
        }
        self.remove_empty();
    }
    pub fn rename_folder(&mut self, folder: &str, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            if let Some(f) = self.folder_mut(folder) {
                f.name = name.into();
            }
        }
    }
    /// Ungroup: the folder goes and its sessions return to the list.
    pub fn dissolve(&mut self, folder: &str) {
        self.folders.retain(|f| f.id != folder);
    }
    pub fn toggle_folder(&mut self, folder: &str) {
        if let Some(f) = self.folder_mut(folder) {
            f.collapsed = !f.collapsed;
        }
    }
    pub fn set_color(&mut self, folder: &str, color_index: Option<usize>) {
        if let Some(f) = self.folder_mut(folder) {
            f.color_index = color_index.filter(|&ix| ix > 0);
            f.custom_color = None;
        }
    }
}

/// The sidebar's entries, top to bottom: pinned, reminders, folders, then
/// loose sessions. `visible` is already in the list's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Reminders {
        collapsed: bool,
        sessions: Vec<i64>,
    },
    Folder {
        id: String,
        sessions: Vec<i64>,
    },
    /// Loose pinned sessions, shown as ordinary cards under a Pinned label.
    Pinned {
        sessions: Vec<i64>,
    },
    Session(i64),
}

/// buildSessionList: pins lead, followed by reminders, folders and loose
/// sessions. Reminder membership only changes this view.
pub fn build_list(
    visible: &[i64],
    pinned: impl Fn(i64) -> bool,
    groups: &Groups,
    reminders: &[(i64, u64)],
) -> Vec<Entry> {
    let mut entries = Vec::new();
    let visible_members: HashSet<_> = visible.iter().copied().collect();
    let mut due: Vec<_> = reminders
        .iter()
        .filter(|(id, _)| visible_members.contains(id))
        .copied()
        .collect();
    due.sort_by_key(|&(id, at)| (at, id));
    let reminded: Vec<i64> = due.into_iter().map(|(id, _)| id).collect();
    let reminded_members: HashSet<_> = reminded.iter().copied().collect();
    if !reminded.is_empty() {
        entries.push(Entry::Reminders {
            collapsed: groups.reminders_collapsed,
            sessions: reminded.clone(),
        });
    }
    let mut foldered = HashSet::new();
    for folder in &groups.folders {
        foldered.extend(folder.session_ids.iter().copied());
        let members: HashSet<_> = folder.session_ids.iter().copied().collect();
        let members: Vec<i64> = visible
            .iter()
            .copied()
            .filter(|id| members.contains(id) && !reminded_members.contains(id))
            .collect();
        if !members.is_empty() {
            entries.push(Entry::Folder {
                id: folder.id.clone(),
                sessions: members,
            });
        }
    }
    let loose: Vec<i64> = visible
        .iter()
        .copied()
        .filter(|id| !foldered.contains(id) && !reminded_members.contains(id))
        .collect();
    let (pins, loose): (Vec<_>, Vec<_>) = loose.into_iter().partition(|&id| pinned(id));
    if !pins.is_empty() {
        entries.push(Entry::Pinned { sessions: pins });
    }
    entries.extend(loose.into_iter().map(Entry::Session));
    entries
}

/// sessionListNavigationIds: the sessions as shown, folded groups skipped
/// unless a search has opened them.
pub fn navigation_ids(entries: &[Entry], expand_collapsed: bool) -> Vec<i64> {
    let mut ids = Vec::new();
    for entry in entries {
        match entry {
            Entry::Session(id) => ids.push(*id),
            Entry::Reminders {
                collapsed,
                sessions,
            } => {
                if !collapsed || expand_collapsed {
                    ids.extend(sessions);
                }
            }
            Entry::Folder { sessions, .. } | Entry::Pinned { sessions } => ids.extend(sessions),
        }
    }
    ids
}

pub const REMINDER_PRESETS: [&str; 5] = [
    "reminder:1h",
    "reminder:3h",
    "reminder:evening",
    "reminder:tomorrow",
    "reminder:next-week",
];

/// reminderTime in local wall-clock time; None for a time already past.
pub fn reminder_at(preset: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let nine = NaiveTime::from_hms_opt(9, 0, 0)?;
    let at = match preset {
        "reminder:1h" => return Some(now + Duration::hours(1)),
        "reminder:3h" => return Some(now + Duration::hours(3)),
        "reminder:evening" => now.date().and_time(NaiveTime::from_hms_opt(18, 0, 0)?),
        "reminder:tomorrow" => (now.date() + Duration::days(1)).and_time(nine),
        "reminder:next-week" => {
            // The next Monday, a week out when today is Monday.
            let weekday = now.weekday().num_days_from_sunday() as i64;
            let ahead = match (8 - weekday) % 7 {
                0 => 7,
                days => days,
            };
            (now.date() + Duration::days(ahead)).and_time(nine)
        }
        _ => return None,
    };
    (at > now).then_some(at)
}

/// sessionReminderPresets: id, label and whether it can still be chosen.
pub fn reminder_presets(now: NaiveDateTime) -> Vec<(&'static str, String, bool)> {
    let clock = |hours| {
        reminder_at(hours, now)
            .map(|at| at.format("%-H:%M").to_string())
            .unwrap_or_default()
    };
    vec![
        (
            REMINDER_PRESETS[0],
            format!("In 1 hour ({})", clock(REMINDER_PRESETS[0])),
            true,
        ),
        (
            REMINDER_PRESETS[1],
            format!("In 3 hours ({})", clock(REMINDER_PRESETS[1])),
            true,
        ),
        (
            REMINDER_PRESETS[2],
            "This evening (18:00)".into(),
            reminder_at(REMINDER_PRESETS[2], now).is_some(),
        ),
        (REMINDER_PRESETS[3], "Tomorrow (9:00)".into(), true),
        (REMINDER_PRESETS[4], "Next week (Mon 9:00)".into(), true),
    ]
}

/// formatReminderTime: "Mon, Oct 19, 15:00".
pub fn format_reminder(at: NaiveDateTime) -> String {
    at.format("%a, %b %-d, %-H:%M").to_string()
}

/// session.ts's `LinkedWorkItem`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    /// "issue" or "pr".
    pub kind: String,
    pub repo: String,
    pub number: u64,
    pub url: String,
}

/// parseGithubWorkItemUrl: the first github.com issue or pull request URL.
pub fn parse_github_work_item(text: &str) -> Option<WorkItem> {
    let name = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    let lower = text.to_ascii_lowercase();
    for (start, _) in lower.match_indices("github.com/") {
        let scheme = &lower[..start];
        if !(scheme.ends_with("https://") || scheme.ends_with("http://")) {
            continue;
        }
        let mut parts = text[start + "github.com/".len()..].splitn(4, '/');
        let (Some(owner), Some(repo), Some(kind), Some(rest)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let kind = match kind.to_ascii_lowercase().as_str() {
            "pull" => "pr",
            "issues" => "issue",
            _ => continue,
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        // `\b` after the number: it must not run on into a word.
        let after = rest[digits.len()..].chars().next();
        if !name(owner) || !name(repo) || after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(number) = digits.parse::<u64>().ok().filter(|&n| n > 0) else {
            continue;
        };
        let repo = format!("{owner}/{repo}");
        let url = format!(
            "https://github.com/{repo}/{}/{number}",
            if kind == "pr" { "pull" } else { "issues" }
        );
        return Some(WorkItem {
            kind: kind.into(),
            repo,
            number,
            url,
        });
    }
    None
}

fn github_url(repo: &str, kind: &str, number: u64) -> String {
    let path = if kind == "pr" { "pull" } else { "issues" };
    format!("https://github.com/{repo}/{path}/{number}")
}

fn valid_repo(repo: &str) -> bool {
    repo.split_once('/').is_some_and(|(owner, name)| {
        [owner, name].iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        })
    })
}

/// sessionWorkItem.ts's explicitHint: "PR #12", "pull request 12", "issue 7".
fn explicit_work_item_hint(message: &str) -> Option<(&'static str, u64)> {
    static PATTERNS: std::sync::LazyLock<[(&str, regex::Regex); 2]> =
        std::sync::LazyLock::new(|| {
            [
                ("pr", r"(?i)\b(?:pr|pull\s+request)\s*#?\s*(\d+)\b"),
                ("issue", r"(?i)\bissue\s*#?\s*(\d+)\b"),
            ]
            .map(|(kind, re)| (kind, regex::Regex::new(re).unwrap()))
        });
    PATTERNS.iter().find_map(|(kind, re)| {
        let number = re.captures(message)?[1].parse::<u64>().ok()?;
        (number > 0).then_some((*kind, number))
    })
}

fn references_current_pr(message: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:this|the|current)\s+(?:pr|pull\s+request)\b").unwrap()
    });
    RE.is_match(message)
}

/// resolveLinkedWorkItem: a first message's explicit GitHub context, as one
/// stable identity. A URL wins; then a numbered reference (or the title
/// model's hint) in the working copy's repository; then "this PR", meaning
/// the current branch's pull request. Lookups that fail link nothing.
pub fn resolve_work_item(
    message: &str,
    cwd: &Path,
    generated: Option<&crate::session_title::WorkItemHint>,
) -> Option<WorkItem> {
    if let Some(item) = parse_github_work_item(message) {
        return Some(item);
    }
    let hint = explicit_work_item_hint(message).or_else(|| {
        let hint = generated?;
        let kind = match hint.kind.as_str() {
            "pr" => "pr",
            "issue" => "issue",
            _ => return None,
        };
        (hint.number > 0).then_some((kind, hint.number))
    });
    if let Some((kind, number)) = hint {
        let repo = crate::scm::github_repo(cwd)
            .ok()
            .filter(|r| valid_repo(r))?;
        return Some(WorkItem {
            kind: kind.into(),
            url: github_url(&repo, kind, number),
            repo,
            number,
        });
    }
    if !references_current_pr(message) {
        return None;
    }
    let pr = crate::scm::pr_status(cwd).ok()??;
    let number = u64::try_from(pr.number).ok().filter(|&n| n > 0)?;
    let repo = parse_github_work_item(&pr.url)
        .map(|item| item.repo)
        .or_else(|| crate::scm::github_repo(cwd).ok())
        .filter(|r| valid_repo(r))?;
    let url = if pr.url.is_empty() {
        github_url(&repo, "pr", number)
    } else {
        pr.url
    };
    Some(WorkItem {
        kind: "pr".into(),
        repo,
        number,
        url,
    })
}

/// The pull request a finished tool call opened: `gh pr create` printing the
/// new PR's URL, or a GitHub MCP `create_pull_request` returning it.
pub fn opened_pull_request(tool: &crate::tool::ToolCall) -> Option<WorkItem> {
    static GH_PR_CREATE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?:^|[\s;&|('"])gh\s+pr\s+(?:create|new)\b"#).unwrap()
    });
    if tool.state != crate::tool::ToolState::Completed {
        return None;
    }
    let pull = |text: &str| parse_github_work_item(text).filter(|item| item.kind == "pr");
    if tool.name.ends_with("create_pull_request") {
        let html_url = serde_json::from_str::<serde_json::Value>(&tool.output)
            .ok()
            .and_then(|value| value["html_url"].as_str().and_then(pull));
        return html_url.or_else(|| pull(&tool.output));
    }
    let shown = tool.presentation();
    let command = shown.command.unwrap_or(shown.target);
    if !GH_PR_CREATE.is_match(&command) {
        return None;
    }
    // `gh` prints the new PR's URL last; earlier lines may quote other links.
    tool.output.lines().rev().find_map(pull)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn folders_move_sessions_and_drop_when_emptied() {
        let mut groups = Groups::default();
        let a = groups.create_folder(&[1, 2, 2], "a".into()).unwrap();
        assert_eq!(groups.folders[0].name, "New folder");
        assert_eq!(groups.folders[0].session_ids, vec![1, 2]);
        let b = groups.create_folder(&[2], "b".into()).unwrap();
        assert_eq!(groups.folders[0].name, "New folder 2");
        assert_eq!(groups.folder(&a).unwrap().session_ids, vec![1]);
        groups.toggle_folder(&a);
        groups.add_to_folder(&a, 2);
        assert!(groups.folder(&b).is_none(), "its last session left");
        let folder = groups.folder(&a).unwrap();
        assert_eq!(folder.session_ids, vec![1, 2]);
        assert!(!folder.collapsed, "adding opens the folder");
        groups.rename_folder(&a, "  ");
        groups.rename_folder(&a, " Work ");
        assert_eq!(groups.folder(&a).unwrap().name, "Work");
        groups.remove_sessions(&[1, 2]);
        assert!(groups.folders.is_empty());
    }

    #[test]
    fn list_orders_reminders_folders_then_pins_and_loose_sessions() {
        let mut groups = Groups::default();
        groups.create_folder(&[2, 3], "f".into());
        let visible = [1, 2, 3, 4, 5, 6];
        let list = build_list(
            &visible,
            |id| id == 5 || id == 3,
            &groups,
            &[(6, 20), (3, 10)],
        );
        assert_eq!(
            list,
            vec![
                Entry::Reminders {
                    collapsed: false,
                    sessions: vec![3, 6]
                },
                Entry::Folder {
                    id: "f".into(),
                    sessions: vec![2]
                },
                Entry::Pinned { sessions: vec![5] },
                Entry::Session(1),
                Entry::Session(4),
            ]
        );
        groups.reminders_collapsed = true;
        let list = build_list(&visible, |id| id == 5, &groups, &[(3, 10)]);
        assert_eq!(navigation_ids(&list, false), vec![2, 5, 1, 4, 6]);
        assert_eq!(navigation_ids(&list, true), vec![3, 2, 5, 1, 4, 6]);
    }

    #[test]
    fn reminder_presets_follow_the_local_clock() {
        let monday_noon = at(2026, 10, 19, 12, 5);
        assert_eq!(
            reminder_at("reminder:1h", monday_noon),
            Some(at(2026, 10, 19, 13, 5))
        );
        assert_eq!(
            reminder_at("reminder:evening", monday_noon),
            Some(at(2026, 10, 19, 18, 0))
        );
        assert_eq!(
            reminder_at("reminder:evening", at(2026, 10, 19, 18, 0)),
            None
        );
        assert_eq!(
            reminder_at("reminder:tomorrow", monday_noon),
            Some(at(2026, 10, 20, 9, 0))
        );
        assert_eq!(
            reminder_at("reminder:next-week", monday_noon),
            Some(at(2026, 10, 26, 9, 0))
        );
        assert_eq!(
            reminder_at("reminder:next-week", at(2026, 10, 25, 22, 0)),
            Some(at(2026, 10, 26, 9, 0))
        );
        let presets = reminder_presets(at(2026, 10, 19, 19, 0));
        assert_eq!(presets[0].1, "In 1 hour (20:00)");
        assert!(!presets[2].2, "this evening has passed");
        assert_eq!(format_reminder(monday_noon), "Mon, Oct 19, 12:05");
    }

    #[test]
    fn github_links_parse_issues_and_pull_requests() {
        assert_eq!(
            parse_github_work_item("see https://github.com/acme/app/pull/123/files"),
            Some(WorkItem {
                kind: "pr".into(),
                repo: "acme/app".into(),
                number: 123,
                url: "https://github.com/acme/app/pull/123".into(),
            })
        );
        assert_eq!(
            parse_github_work_item("HTTP://GitHub.com/a/b/Issues/7")
                .unwrap()
                .url,
            "https://github.com/a/b/issues/7"
        );
        for bad in [
            "https://github.com/acme/app/pull/0",
            "https://github.com/acme/app/commit/1",
            "https://github.com/acme/app/pull/12abc",
            "github.com/acme/app/pull/1",
            "https://gitlab.com/acme/app/pull/1",
        ] {
            assert_eq!(parse_github_work_item(bad), None, "{bad}");
        }
    }

    #[test]
    fn first_messages_link_their_explicit_github_context() {
        // A pasted URL needs no repository lookup, whatever the folder is.
        let none = Path::new("/nonexistent/monocode-test");
        let hint = crate::session_title::WorkItemHint {
            kind: "issue".into(),
            number: 9,
        };
        assert_eq!(
            resolve_work_item(
                "hey lets review this pr\n\nhttps://github.com/acme/app/pull/1142 its for the native one",
                none,
                Some(&hint),
            )
            .map(|item| (item.kind, item.repo, item.number)),
            Some(("pr".into(), "acme/app".into(), 1142))
        );
        assert_eq!(resolve_work_item("Fix the sidebar", none, None), None);

        assert_eq!(
            explicit_work_item_hint("look at PR #42 please"),
            Some(("pr", 42))
        );
        assert_eq!(explicit_work_item_hint("pull request 7"), Some(("pr", 7)));
        assert_eq!(explicit_work_item_hint("see Issue#3"), Some(("issue", 3)));
        for text in ["PR #0", "issue tracker", "approach 12", "prs 5"] {
            assert_eq!(explicit_work_item_hint(text), None, "{text}");
        }
        assert!(references_current_pr("review the current PR"));
        assert!(references_current_pr("land this pull request"));
        assert!(!references_current_pr("open a PR for this"));
    }

    #[test]
    fn opened_pull_requests_come_from_gh_and_github_mcp_output() {
        use crate::tool::{ToolCall, ToolState};
        let call = |name: &str, input: serde_json::Value, output: &str| {
            let mut tool = ToolCall::new("t", name);
            tool.input = input.to_string();
            tool.output = output.into();
            tool.state = ToolState::Completed;
            tool
        };
        let created = "Warning: 2 uncommitted changes\n\nCreating pull request for feat/x into main in acme/app\n\nhttps://github.com/acme/app/pull/1187\n";
        let pr = |tool: &ToolCall| opened_pull_request(tool).map(|i| (i.repo, i.number, i.url));
        let expected = Some((
            "acme/app".to_string(),
            1187,
            "https://github.com/acme/app/pull/1187".to_string(),
        ));
        for command in [
            "gh pr create --title Fix --body 'Fixes https://github.com/acme/app/pull/3'",
            "git push -u origin feat/x && gh pr create --fill",
            "/bin/zsh -lc 'gh pr create --fill'",
            "gh pr new --fill",
        ] {
            let bash = call("Bash", serde_json::json!({"command": command}), created);
            assert_eq!(pr(&bash), expected, "{command}");
        }
        let codex = call(
            "commandExecution",
            serde_json::json!({"command": "gh pr create --fill", "cwd": "/repo"}),
            created,
        );
        assert_eq!(pr(&codex), expected);

        let mcp = call(
            "mcp__github__create_pull_request",
            serde_json::json!({"owner": "acme"}),
            r#"{"url":"https://api.github.com/repos/acme/app/pulls/1187","body":"Fixes https://github.com/acme/app/pull/3","html_url":"https://github.com/acme/app/pull/1187"}"#,
        );
        assert_eq!(pr(&mcp), expected);

        let mut failed = call(
            "Bash",
            serde_json::json!({"command": "gh pr create"}),
            created,
        );
        failed.state = ToolState::Failed;
        let mut running = failed.clone();
        running.state = ToolState::Running;
        for tool in [
            failed,
            running,
            call("Bash", serde_json::json!({"command": "gh pr view 1187"}), created),
            call("Bash", serde_json::json!({"command": "echo gh pr creates"}), created),
            call(
                "Bash",
                serde_json::json!({"command": "git push -u origin feat/x"}),
                "remote: Create a pull request for 'feat/x' on GitHub by visiting:\nremote:   https://github.com/acme/app/pull/new/feat/x\n",
            ),
            call(
                "Bash",
                serde_json::json!({"command": "gh pr create --fill"}),
                "https://github.com/acme/app/issues/5\n",
            ),
        ] {
            assert_eq!(pr(&tool), None, "{}", tool.input);
        }
    }
}
