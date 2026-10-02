//! Port of src/features/sessions/data/sessionHistory.ts: the history rows a
//! project sidebar shows, merged with the live sessions in memory.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use monocode_core::session::{session_display_title, session_draft_block, session_needs_input};
use monocode_core::{HarnessId, Session};

use super::reducer::now_ms;
use super::session_store::{
    OrchestrationSummary, OrchestrationSummaryTask, SessionSummary, should_persist_session,
};
use super::util::fuzzy::fuzzy_match;
use super::util::project_path::same_project_path;
use monocode_core::Extra;

/// `SessionGitHint`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionGitHint {
    pub repo: Option<String>,
    pub branch: Option<String>,
}

/// One task of a loaded orchestration run, as history needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRunTask {
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    pub model: String,
    pub status: String,
}

/// The parts of an `OrchestrationRun` that history reads. The orchestration
/// package builds these from its runs.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRun {
    pub lead_id: String,
    pub status: String,
    pub tasks: Vec<LiveRunTask>,
}

/// `summarizeOrchestration` from orchestrationSummary.ts.
pub fn summarize_orchestration(run: &LiveRun, sessions: &[Session]) -> OrchestrationSummary {
    let by_id: HashMap<&str, &Session> = sessions.iter().map(|s| (s.id.as_str(), s)).collect();
    OrchestrationSummary {
        status: run.status.clone(),
        live: Some(true),
        tasks: run
            .tasks
            .iter()
            .map(|task| OrchestrationSummaryTask {
                session_id: task.session_id.clone(),
                title: task.title.clone(),
                harness: task.harness,
                model: task.model.clone(),
                status: task.status.clone(),
                needs_input: Some(
                    by_id
                        .get(task.session_id.as_str())
                        .is_some_and(|session| session_needs_input(session)),
                ),
                extra: Extra::new(),
            })
            .collect(),
        extra: Extra::new(),
    }
}

/// `compareSessionSummaries`: pinned first, then newest, then by id.
pub fn compare_session_summaries(a: &SessionSummary, b: &SessionSummary) -> Ordering {
    let pinned = |row: &SessionSummary| row.pinned == Some(true);
    pinned(b)
        .cmp(&pinned(a))
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        // TODO(port): `localeCompare` sorts by locale; this compares code points.
        .then_with(|| a.id.cmp(&b.id))
}

/// `mergeHistorySummary`: put `summary` first, keeping the flags and
/// ownership fields the incoming row omits, then sort.
pub fn merge_history_summary(
    current: &[SessionSummary],
    summary: SessionSummary,
) -> Vec<SessionSummary> {
    let previous = current.iter().find(|entry| entry.id == summary.id);
    let mut next = summary;
    if let Some(previous) = previous {
        next.archived = next.archived.or(previous.archived);
        next.pinned = next.pinned.or(previous.pinned);
        if next.orchestration.is_none() {
            next.orchestration = previous.orchestration.clone();
        }
        if next.orchestration_lead_id.is_none() {
            next.orchestration_lead_id = previous.orchestration_lead_id.clone();
        }
        if next.automation_id.is_none() {
            next.automation_id = previous.automation_id.clone();
        }
    }
    let mut rows = Vec::with_capacity(current.len() + 1);
    let id = next.id.clone();
    rows.push(next);
    rows.extend(current.iter().filter(|entry| entry.id != id).cloned());
    rows.sort_by(compare_session_summaries);
    rows
}

/// `replaceProjectHistory`: swap in one project's fresh rows and keep every
/// other project's cached rows.
pub fn replace_project_history(
    current: &[SessionSummary],
    cwd: &str,
    rows: Vec<SessionSummary>,
) -> Vec<SessionSummary> {
    let mut next: Vec<SessionSummary> = current
        .iter()
        .filter(|entry| !same_project_path(&entry.cwd, cwd))
        .cloned()
        .collect();
    next.extend(rows);
    next
}

/// `mergeProjectHistorySummary`: `mergeHistorySummary` scoped to the
/// summary's project. A session that changed project leaves its old one.
pub fn merge_project_history_summary(
    current: &[SessionSummary],
    summary: SessionSummary,
) -> Vec<SessionSummary> {
    let mut mine = Vec::new();
    let mut others = Vec::new();
    for entry in current {
        if same_project_path(&entry.cwd, &summary.cwd) {
            mine.push(entry.clone());
        } else if entry.id != summary.id {
            others.push(entry.clone());
        }
    }
    others.extend(merge_history_summary(&mine, summary));
    others
}

/// `filterSessionsByArchive`.
pub fn filter_sessions_by_archive(
    rows: &[SessionSummary],
    show_archived: bool,
) -> Vec<SessionSummary> {
    rows.iter()
        .filter(|row| (row.archived == Some(true)) == show_archived)
        .cloned()
        .collect()
}

/// `filterSessionsByQuery`: fuzzy match on title, model, harness, and git.
pub fn filter_sessions_by_query(rows: &[SessionSummary], query: &str) -> Vec<SessionSummary> {
    let needle = monocode_core::js::trim(query);
    if needle.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| session_search_hit(row, needle))
        .cloned()
        .collect()
}

fn session_search_hit(row: &SessionSummary, query: &str) -> bool {
    let title = session_display_title(&row.title, row.harness);
    let git = [row.repo.as_deref(), row.branch.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    [
        title.as_str(),
        row.title.as_str(),
        row.model.as_str(),
        row.harness.as_str(),
        git.as_str(),
    ]
    .iter()
    .any(|field| !field.is_empty() && fuzzy_match(query, field).is_some())
}

/// `summaryFromSession`: the row a live session would have.
pub fn summary_from_session(session: &Session, git: Option<&SessionGitHint>) -> SessionSummary {
    let branch = session
        .branch
        .clone()
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            git.and_then(|git| git.branch.clone())
                .filter(|b| !b.is_empty())
        });
    SessionSummary {
        orchestration_lead_id: session.orchestration_lead_id.clone(),
        orchestration: None,
        id: session.id.clone(),
        cwd: session.cwd.clone(),
        harness: session.harness,
        model: session.model.clone(),
        runtime_mode: session.runtime_mode,
        title: session.title.clone(),
        provider_session_id: session.provider_session_id.clone(),
        branch: if session.worktree_removed == Some(true) {
            None
        } else {
            branch
        },
        worktree_cwd: session.worktree_cwd.clone(),
        worktree_removed: session.worktree_removed,
        repo: git
            .and_then(|git| git.repo.clone())
            .filter(|repo| !repo.is_empty()),
        additions: None,
        deletions: None,
        created_at: 0,
        updated_at: now_ms(),
        archived: None,
        pinned: None,
        draft: Some(session_draft_block(&session.blocks).is_some()),
        linked_work_item: session.linked_work_item.clone(),
        automation_id: session.automation_id.clone().filter(|id| !id.is_empty()),
    }
}

/// `projectGitHint`: prefer the project's persisted origin name, then the
/// overlay or folder name. The overlay's branch wins.
pub fn project_git_hint(
    rows: &[SessionSummary],
    overlay: Option<&SessionGitHint>,
) -> SessionGitHint {
    let repo = rows
        .iter()
        .find_map(|row| row.repo.clone().filter(|repo| !repo.is_empty()))
        .or_else(|| overlay.and_then(|overlay| overlay.repo.clone()))
        .filter(|repo| !repo.is_empty());
    let branch = overlay
        .and_then(|overlay| overlay.branch.clone())
        .or_else(|| {
            rows.iter()
                .find_map(|row| row.branch.clone().filter(|b| !b.is_empty()))
        })
        .filter(|branch| !branch.is_empty());
    SessionGitHint { repo, branch }
}

/// `projectName` from paths.ts: the folder name for display.
fn project_name(cwd: &str) -> String {
    if cwd.is_empty() || cwd == "~" {
        return "~".into();
    }
    let slashed = monocode_core::paths::slash(cwd);
    let trimmed = slashed.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return trimmed.to_string();
    }
    trimmed
        .split('/')
        .rfind(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

fn git_overlay_for_cwd(cwd: &str, git: Option<&SessionGitHint>) -> SessionGitHint {
    if git.is_some_and(|git| git.repo.as_ref().is_some_and(|repo| !repo.is_empty())) {
        return git.cloned().unwrap_or_default();
    }
    if cwd.is_empty() || cwd == "~" {
        return git.cloned().unwrap_or_default();
    }
    let name = project_name(cwd);
    if name.is_empty() || name == "~" {
        return git.cloned().unwrap_or_default();
    }
    SessionGitHint {
        repo: Some(name),
        branch: git.and_then(|git| git.branch.clone()),
    }
}

/// `historyWithLiveSessions`: the project's stored rows plus live sessions
/// that are not saved yet, with orchestration workers grouped under their
/// lead.
pub fn history_with_live_sessions(
    history: &[SessionSummary],
    sessions: &[Session],
    cwd: &str,
    git: Option<&SessionGitHint>,
    runs: &[LiveRun],
) -> Vec<SessionSummary> {
    let mut worker_ids: HashSet<&str> = sessions
        .iter()
        .filter(|session| session.orchestration_lead_id.is_some())
        .map(|session| session.id.as_str())
        .collect();
    for row in history {
        if let Some(orchestration) = row.orchestration.as_ref() {
            worker_ids.extend(
                orchestration
                    .tasks
                    .iter()
                    .map(|task| task.session_id.as_str()),
            );
        }
    }
    for run in runs {
        worker_ids.extend(run.tasks.iter().map(|task| task.session_id.as_str()));
    }
    let inbox_ids: HashSet<&str> = sessions
        .iter()
        .filter(|session| session.inbox_ask.is_some())
        .map(|session| session.id.as_str())
        .collect();
    let mut rows: Vec<SessionSummary> = history
        .iter()
        .filter(|entry| {
            !inbox_ids.contains(entry.id.as_str())
                && entry.orchestration_lead_id.is_none()
                && !worker_ids.contains(entry.id.as_str())
                && same_project_path(&entry.cwd, cwd)
        })
        .cloned()
        .collect();
    let hint = project_git_hint(&rows, Some(&git_overlay_for_cwd(cwd, git)));
    for session in sessions {
        if session.inbox_ask.is_some() || worker_ids.contains(session.id.as_str()) {
            continue;
        }
        if !same_project_path(&session.cwd, cwd) {
            continue;
        }
        let live = session.is_busy() || session_needs_input(session);
        if !should_persist_session(session) && !live {
            continue;
        }
        if let Some(stored) = rows.iter_mut().find(|row| row.id == session.id) {
            let draft = session_draft_block(&session.blocks).is_some();
            let automation_id = session
                .automation_id
                .clone()
                .filter(|id| !id.is_empty())
                .or_else(|| stored.automation_id.clone());
            if (stored.draft == Some(true)) != draft || stored.automation_id != automation_id {
                stored.draft = draft.then_some(true);
                if automation_id.is_some() {
                    stored.automation_id = automation_id;
                }
            }
            continue;
        }
        let session_hint = SessionGitHint {
            repo: hint.repo.clone(),
            branch: session
                .branch
                .clone()
                .filter(|branch| !branch.is_empty())
                .or_else(|| hint.branch.clone()),
        };
        rows = merge_history_summary(&rows, summary_from_session(session, Some(&session_hint)));
    }
    let by_lead: HashMap<&str, &LiveRun> =
        runs.iter().map(|run| (run.lead_id.as_str(), run)).collect();
    let mut rows: Vec<SessionSummary> = rows
        .into_iter()
        .map(|mut row| {
            if let Some(run) = by_lead.get(row.id.as_str()) {
                row.orchestration = Some(summarize_orchestration(run, sessions));
            }
            row
        })
        .collect();
    rows.sort_by(compare_session_summaries);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::{Block, BlockRole};
    use monocode_core::harness::RuntimeMode;

    fn summary(id: &str, cwd: &str, updated_at: i64) -> SessionSummary {
        SessionSummary {
            model: "gpt-5".into(),
            runtime_mode: RuntimeMode::Supervised,
            title: format!("cursor · {id}"),
            created_at: updated_at,
            updated_at,
            additions: Some(0),
            deletions: Some(0),
            ..SessionSummary::new(id, cwd, HarnessId::Cursor)
        }
    }

    fn ids(rows: &[SessionSummary]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    fn chat(id: &str, cwd: &str) -> Session {
        let mut session = Session::blank(id, HarnessId::Cursor, "cursor:auto", cwd);
        session.blocks = vec![Block::new("u1", BlockRole::User, "hello")];
        session
    }

    fn run() -> LiveRun {
        LiveRun {
            lead_id: "lead".into(),
            status: "active".into(),
            tasks: ["worker-a", "worker-b"]
                .iter()
                .map(|id| LiveRunTask {
                    session_id: id.to_string(),
                    title: id.to_string(),
                    harness: HarnessId::Codex,
                    model: "codex:test".into(),
                    status: "running".into(),
                })
                .collect(),
        }
    }

    const PROJECT_A: &str = "/tmp/project-a";

    #[test]
    fn groups_live_and_already_saved_workers_under_their_lead_before_adoption_effects_run() {
        let sessions: Vec<Session> = ["lead", "worker-a", "worker-b"]
            .iter()
            .map(|id| Session {
                busy: Some(*id != "lead"),
                ..chat(id, PROJECT_A)
            })
            .collect();
        let rows = history_with_live_sessions(
            &[
                summary("lead", PROJECT_A, 1),
                summary("worker-a", PROJECT_A, 1),
                summary("unrelated", PROJECT_A, 1),
            ],
            &sessions,
            PROJECT_A,
            None,
            &[run()],
        );
        let mut found = ids(&rows);
        found.sort_unstable();
        assert_eq!(found, vec!["lead", "unrelated"]);
        let orchestration = rows
            .iter()
            .find(|r| r.id == "lead")
            .unwrap()
            .orchestration
            .as_ref()
            .unwrap();
        assert_eq!(orchestration.live, Some(true));
        assert_eq!(orchestration.status, "active");
        let task_ids: Vec<_> = orchestration
            .tasks
            .iter()
            .map(|t| t.session_id.as_str())
            .collect();
        assert_eq!(task_ids, vec!["worker-a", "worker-b"]);
    }

    #[test]
    fn keeps_ownership_after_restart_cache_merges_and_replacement_runs() {
        let saved = summarize_orchestration(&run(), &[]);
        let history = vec![
            SessionSummary {
                orchestration: Some(OrchestrationSummary {
                    status: "paused".into(),
                    live: None,
                    ..saved
                }),
                ..summary("lead", PROJECT_A, 1)
            },
            summary("worker-a", PROJECT_A, 1),
            SessionSummary {
                orchestration_lead_id: Some("lead".into()),
                ..summary("older-worker", PROJECT_A, 1)
            },
        ];
        let merged = merge_history_summary(&history, summary("lead", PROJECT_A, 2));
        let rows = history_with_live_sessions(&merged, &[], PROJECT_A, None, &[]);
        assert_eq!(ids(&rows), vec!["lead"]);
        assert_eq!(rows[0].orchestration.as_ref().unwrap().tasks.len(), 2);
        let replacement = history_with_live_sessions(
            &merged,
            &[],
            PROJECT_A,
            None,
            &[LiveRun {
                tasks: Vec::new(),
                ..run()
            }],
        );
        assert_eq!(ids(&replacement), vec!["lead"]);
        assert!(
            replacement[0]
                .orchestration
                .as_ref()
                .unwrap()
                .tasks
                .is_empty()
        );
    }

    #[test]
    fn does_not_inject_an_internal_worker_without_a_loaded_run() {
        let worker = Session {
            orchestration_lead_id: Some("lead".into()),
            busy: Some(true),
            ..Session::blank("worker", HarnessId::Codex, "m", PROJECT_A)
        };
        assert!(history_with_live_sessions(&[], &[worker], PROJECT_A, None, &[]).is_empty());
    }

    #[test]
    fn drops_persisted_sessions_from_other_projects() {
        let history = vec![
            summary("a1", PROJECT_A, 1),
            summary("b1", "/tmp/project-b", 1),
        ];
        let rows = history_with_live_sessions(&history, &[], PROJECT_A, None, &[]);
        assert_eq!(ids(&rows), vec!["a1"]);
    }

    #[test]
    fn does_not_inject_live_sessions_from_other_projects() {
        let session = Session {
            busy: Some(true),
            ..chat("s", "/tmp/project-b")
        };
        assert!(history_with_live_sessions(&[], &[session], PROJECT_A, None, &[]).is_empty());
    }

    #[test]
    fn includes_live_sessions_for_the_active_project() {
        let session = Session {
            busy: Some(true),
            ..chat("s", PROJECT_A)
        };
        let rows = history_with_live_sessions(&[], &[session], PROJECT_A, None, &[]);
        assert_eq!(ids(&rows), vec!["s"]);
        assert_eq!(rows[0].repo.as_deref(), Some("project-a"));
    }

    #[test]
    fn marks_drafts_appended_to_started_threads_and_clears_stale_draft_status_when_sent() {
        let mut session = chat("draft-session", PROJECT_A);
        session.blocks = vec![
            Block::new("sent", BlockRole::User, "Start here"),
            Block::new("reply", BlockRole::Assistant, "Done"),
            Block {
                draft: Some(true),
                ..Block::new("draft", BlockRole::User, "Explore this")
            },
        ];
        let draft_rows =
            history_with_live_sessions(&[], std::slice::from_ref(&session), PROJECT_A, None, &[]);
        assert_eq!(draft_rows[0].draft, Some(true));
        session.blocks[2] = Block::new("follow-up", BlockRole::User, "Explore this");
        session.busy = Some(true);
        let stored = SessionSummary {
            draft: Some(true),
            ..draft_rows[0].clone()
        };
        let sent_rows = history_with_live_sessions(&[stored], &[session], PROJECT_A, None, &[]);
        assert_eq!(sent_rows[0].draft, None);
    }

    #[test]
    fn overlays_an_automation_origin_onto_an_already_saved_session() {
        let session = Session {
            automation_id: Some("automation-1".into()),
            busy: Some(true),
            ..chat("auto-session", PROJECT_A)
        };
        let rows = history_with_live_sessions(
            &[summary("auto-session", PROJECT_A, 1)],
            &[session],
            PROJECT_A,
            None,
            &[],
        );
        assert_eq!(rows[0].automation_id.as_deref(), Some("automation-1"));
    }

    fn hint(repo: &str, branch: &str) -> SessionGitHint {
        SessionGitHint {
            repo: Some(repo.into()),
            branch: Some(branch.into()),
        }
    }

    #[test]
    fn stamps_composer_git_onto_a_live_session_that_is_not_persisted_yet() {
        let session = Session {
            busy: Some(true),
            ..chat("s", "/tmp/monocode")
        };
        let rows = history_with_live_sessions(
            &[],
            &[session],
            "/tmp/monocode",
            Some(&hint("monocode", "main")),
            &[],
        );
        assert_eq!(rows[0].id, "s");
        assert_eq!(rows[0].repo.as_deref(), Some("monocode"));
        assert_eq!(rows[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn copies_origin_repo_from_sibling_history_and_prefers_the_live_branch() {
        let history = vec![SessionSummary {
            repo: Some("monocode".into()),
            branch: Some("main".into()),
            ..summary("a1", "/tmp/agent-terminal", 1)
        }];
        let session = Session {
            busy: Some(true),
            ..chat("s", "/tmp/agent-terminal")
        };
        let rows = history_with_live_sessions(
            &history,
            &[session],
            "/tmp/agent-terminal",
            Some(&hint("agent-terminal", "fix-gutter")),
            &[],
        );
        let live = rows.iter().find(|row| row.id == "s").unwrap();
        assert_eq!(live.repo.as_deref(), Some("monocode"));
        assert_eq!(live.branch.as_deref(), Some("fix-gutter"));
    }

    #[test]
    fn keeps_a_sessions_own_branch_instead_of_the_project_overlay() {
        let session = Session {
            busy: Some(true),
            branch: Some("feat/picker".into()),
            ..chat("s", "/tmp/agent-terminal")
        };
        let rows = history_with_live_sessions(
            &[],
            &[session],
            "/tmp/agent-terminal",
            Some(&hint("monocode", "main")),
            &[],
        );
        assert_eq!(rows[0].repo.as_deref(), Some("monocode"));
        assert_eq!(rows[0].branch.as_deref(), Some("feat/picker"));
    }

    #[test]
    fn matches_project_paths_with_trailing_slashes() {
        let rows = history_with_live_sessions(
            &[summary("a1", "/tmp/project-a/", 1)],
            &[],
            PROJECT_A,
            None,
            &[],
        );
        assert_eq!(ids(&rows), vec!["a1"]);
    }

    #[test]
    fn hides_archived_sessions_by_default() {
        let rows = vec![
            summary("a1", PROJECT_A, 1),
            SessionSummary {
                archived: Some(true),
                ..summary("a2", PROJECT_A, 1)
            },
        ];
        assert_eq!(ids(&filter_sessions_by_archive(&rows, false)), vec!["a1"]);
        assert_eq!(ids(&filter_sessions_by_archive(&rows, true)), vec!["a2"]);
    }

    #[test]
    fn filters_by_query() {
        let rows = vec![summary("a1", PROJECT_A, 1), summary("a2", PROJECT_A, 1)];
        assert_eq!(
            ids(&filter_sessions_by_query(&rows, "  ")),
            vec!["a1", "a2"]
        );
        let titled = vec![
            SessionSummary {
                title: "cursor · Fix sidebar search".into(),
                ..summary("a1", PROJECT_A, 1)
            },
            SessionSummary {
                title: "cursor · Archive sessions".into(),
                ..summary("a2", PROJECT_A, 1)
            },
        ];
        assert_eq!(
            ids(&filter_sessions_by_query(&titled, "sidebar")),
            vec!["a1"]
        );
        let labelled = vec![
            SessionSummary {
                branch: Some("main".into()),
                ..summary("a1", PROJECT_A, 1)
            },
            SessionSummary {
                model: "opus".into(),
                branch: Some("fix-gutter".into()),
                ..summary("a2", PROJECT_A, 1)
            },
        ];
        assert_eq!(
            ids(&filter_sessions_by_query(&labelled, "opus")),
            vec!["a2"]
        );
        assert_eq!(
            ids(&filter_sessions_by_query(&labelled, "gutter")),
            vec!["a2"]
        );
    }

    #[test]
    fn replaces_one_projects_rows() {
        let current = vec![
            summary("a1", PROJECT_A, 3),
            summary("b1", "/tmp/project-b", 2),
        ];
        let next = replace_project_history(&current, PROJECT_A, vec![summary("a2", PROJECT_A, 5)]);
        let mut found = ids(&next);
        found.sort_unstable();
        assert_eq!(found, vec!["a2", "b1"]);
        assert_eq!(
            ids(&replace_project_history(&current, PROJECT_A, Vec::new())),
            vec!["b1"]
        );
    }

    #[test]
    fn merges_into_its_own_project_and_moves_between_projects() {
        let current = vec![
            summary("a1", PROJECT_A, 1),
            summary("b1", "/tmp/project-b", 2),
        ];
        let next = merge_project_history_summary(&current, summary("a1", PROJECT_A, 9));
        assert_eq!(
            next.iter().find(|row| row.id == "a1").unwrap().updated_at,
            9
        );
        assert_eq!(next.len(), 2);
        let moved = merge_project_history_summary(&current, summary("a1", "/tmp/project-b", 9));
        let mut found = ids(&moved);
        found.sort_unstable();
        assert_eq!(found, vec!["a1", "b1"]);
        assert_eq!(
            moved.iter().find(|row| row.id == "a1").unwrap().cwd,
            "/tmp/project-b"
        );
    }

    #[test]
    fn keeps_pinned_sessions_above_newer_unpinned_ones() {
        let current = vec![
            summary("new", PROJECT_A, 20),
            SessionSummary {
                pinned: Some(true),
                ..summary("pin", PROJECT_A, 5)
            },
        ];
        assert_eq!(
            ids(&merge_history_summary(
                &current,
                summary("new", PROJECT_A, 30)
            )),
            vec!["pin", "new"]
        );
    }

    #[test]
    fn preserves_automation_origin_and_pin_when_an_incoming_summary_omits_them() {
        let auto = vec![SessionSummary {
            automation_id: Some("automation-1".into()),
            ..summary("auto", PROJECT_A, 5)
        }];
        let next = merge_history_summary(&auto, summary("auto", PROJECT_A, 9));
        assert_eq!(next[0].automation_id.as_deref(), Some("automation-1"));
        let pinned = vec![SessionSummary {
            pinned: Some(true),
            ..summary("pin", PROJECT_A, 5)
        }];
        let next = merge_history_summary(&pinned, summary("pin", PROJECT_A, 9));
        assert_eq!((next[0].pinned, next[0].updated_at), (Some(true), 9));
    }

    #[test]
    fn returns_an_unpinned_session_to_recency_order() {
        let current = vec![
            SessionSummary {
                pinned: Some(true),
                ..summary("pin", PROJECT_A, 5)
            },
            summary("new", PROJECT_A, 20),
        ];
        let next = merge_history_summary(
            &current,
            SessionSummary {
                pinned: Some(false),
                ..summary("pin", PROJECT_A, 5)
            },
        );
        assert_eq!(ids(&next), vec!["new", "pin"]);
        assert_eq!(next[1].pinned, Some(false));
    }

    #[test]
    fn sorts_pinned_history_to_the_top_even_without_a_live_inject() {
        let history = vec![
            summary("new", PROJECT_A, 20),
            SessionSummary {
                pinned: Some(true),
                ..summary("pin", PROJECT_A, 5)
            },
        ];
        assert_eq!(
            ids(&history_with_live_sessions(
                &history,
                &[],
                PROJECT_A,
                None,
                &[]
            )),
            vec!["pin", "new"]
        );
    }
}
