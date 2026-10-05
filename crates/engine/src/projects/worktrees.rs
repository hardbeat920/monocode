//! Port of src/features/source-control/model/worktrees.ts: working copies
//! (git worktrees), the branch names MonoCode gives them, and how sessions
//! bind to them.
//!
//! The git calls (`listWorktrees`, `createWorktree`, and the rest) are
//! methods on `ProjectsGlobal`, because they run on the background executor
//! and announce git changes.

use std::time::{SystemTime, UNIX_EPOCH};

use monocode_core::models::ModelEnv;
use monocode_core::paths::{path_key, slash};
use monocode_core::session::{MessageQueueStatus, new_session, session_work_cwd};
use monocode_core::{Session, js};
use monocode_layout::layout::{FilePaneTab, is_filesystem_tab};
use monocode_layout::project_return::is_blank_session;

pub use super::backend::{Worktree, WorktreeRemoval, Worktrees};
use super::hooks::ProjectsHooks;
use crate::runtime::session_store::SessionSummary;

/// `NO_BRANCH_LABEL`.
pub const NO_BRANCH_LABEL: &str = "No branch selected";

/// `trimSlash`.
fn trim_slash(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

/// `isEqualOrInside` from src/shared/lib/paths.ts.
pub fn is_equal_or_inside(path: &str, root: &str) -> bool {
    let key = path_key(&trim_slash(path));
    let base_key = path_key(&trim_slash(root));
    key == base_key || key.starts_with(&format!("{base_key}/"))
}

/// `Date.now().toString(36)`.
fn now_base36() -> String {
    let mut value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    if value == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(digits[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The letters and digits of `id`, lowercased, at most `max` of them.
fn branch_token(id: &str, max: usize) -> String {
    id.chars()
        .filter(char::is_ascii_alphanumeric)
        .take(max)
        .collect::<String>()
        .to_lowercase()
}

/// `temporaryWorktreeBranchName`: `mc/` and eight characters of a new id.
pub fn temporary_worktree_branch_name(id: Option<&str>) -> String {
    let generated;
    let id = match id {
        Some(id) => id,
        None => {
            generated = uuid::Uuid::new_v4().to_string();
            &generated
        }
    };
    let token = branch_token(id, 8);
    if token.is_empty() {
        format!("mc/{}", now_base36())
    } else {
        format!("mc/{token}")
    }
}

/// `orchestrationWorktreeBranchName`: `mc/orch-` and twelve characters of
/// the run id.
pub fn orchestration_worktree_branch_name(id: &str) -> String {
    let token = branch_token(id, 12);
    if token.is_empty() {
        format!("mc/orch-{}", now_base36())
    } else {
        format!("mc/orch-{token}")
    }
}

/// `namedWorktreeBranch`: a user's fragment under `mc/`, without a
/// repeated `mc/` or `monocode/` prefix. `None` when nothing is left.
pub fn named_worktree_branch(fragment: &str) -> Option<String> {
    let mut clean = js::trim(fragment);
    for prefix in ["mc/", "monocode/"] {
        if let Some(rest) = clean.strip_prefix(prefix) {
            clean = rest.trim_start_matches('/');
            break;
        }
    }
    let clean = clean.trim_matches('/');
    (!clean.is_empty()).then(|| format!("mc/{clean}"))
}

/// `assertWorktreeFilesClosed`: fail while a file or terminal is open in the
/// worktree.
pub fn assert_worktree_files_closed(path: &str, files: &[FilePaneTab]) -> Result<(), String> {
    if files.iter().any(|file| {
        is_equal_or_inside(&file.cwd, path)
            || (is_filesystem_tab(file) && is_equal_or_inside(&file.path, path))
    }) {
        return Err("Close the files and terminals open in this worktree first.".into());
    }
    Ok(())
}

/// `worktreeSessionIds`: the saved sessions in a working copy, corrected by
/// the open ones. An open session's live context wins over its saved row.
pub fn worktree_session_ids(tree: &Worktree, sessions: &[Session]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for id in &tree.session_ids {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for session in sessions {
        ids.retain(|id| *id != session.id);
        let work_cwd = session
            .worktree_cwd
            .as_deref()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or(&session.cwd);
        if session.worktree_removed != Some(true) && is_equal_or_inside(work_cwd, &tree.path) {
            ids.push(session.id.clone());
        }
    }
    ids
}

/// `detachSessionWorktree` for an open session: keep the transcript and the
/// project, and require a new working copy before the next turn.
pub fn detach_session_worktree(session: &Session, project_cwd: &str, path: &str) -> Session {
    let mut next = session.clone();
    next.cwd = if is_equal_or_inside(&session.cwd, path) {
        project_cwd.to_string()
    } else {
        session.cwd.clone()
    };
    next.worktree_cwd = Some(
        session
            .worktree_cwd
            .clone()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or_else(|| session.cwd.clone()),
    );
    next.worktree_removed = Some(true);
    next.branch = None;
    next.provider_session_id = None;
    next.context = None;
    next.pending_switch = None;
    next.pending_question = None;
    next.busy = Some(false);
    next.queue_status = Some(MessageQueueStatus::Paused);
    next
}

/// `detachSessionWorktree` for a history row. Rows have no live turn state,
/// so only the working copy fields change.
pub fn detach_summary_worktree(
    summary: &SessionSummary,
    project_cwd: &str,
    path: &str,
) -> SessionSummary {
    let mut next = summary.clone();
    next.cwd = if is_equal_or_inside(&summary.cwd, path) {
        project_cwd.to_string()
    } else {
        summary.cwd.clone()
    };
    next.worktree_cwd = Some(
        summary
            .worktree_cwd
            .clone()
            .filter(|cwd| !cwd.is_empty())
            .unwrap_or_else(|| summary.cwd.clone()),
    );
    next.worktree_removed = Some(true);
    next.branch = None;
    next.provider_session_id = None;
    next
}

/// `sessionInWorktree`: an existing working copy stays bound; a removed one
/// is replaced in place with a handoff; a session with a conversation opens
/// a new session in the target; a blank session moves.
pub fn session_in_worktree(
    env: &ModelEnv<'_>,
    hooks: &dyn ProjectsHooks,
    session: &Session,
    tree: &Worktree,
) -> Session {
    let removed = session.worktree_removed == Some(true);
    if !removed && path_key(session_work_cwd(session)) == path_key(&tree.path) {
        return session.clone();
    }
    let blank = is_blank_session(Some(session));
    let mut target = if removed && !blank {
        let text = format!(
            "The previous working copy was deleted. Continue this conversation in {}. Recheck the files before making changes.\n\n{}",
            tree.path,
            hooks.build_deterministic_handoff(session)
        );
        hooks.append_ready_handoff(session, session.harness, session.harness, &text)
    } else if blank {
        session.clone()
    } else {
        let mut fresh = new_session(
            env,
            uuid::Uuid::new_v4().to_string(),
            session.harness,
            &session.cwd,
            Some(&session.model),
            Some(session.runtime_mode),
            Some(&session.model_settings),
        );
        fresh.provider_account_id = session.provider_account_id.clone();
        fresh
    };
    target.worktree_removed = None;
    target.worktree_cwd = if path_key(&tree.path) == path_key(&session.cwd) {
        None
    } else {
        Some(tree.path.clone())
    };
    target.branch = tree.branch.clone();
    target.provider_session_id = None;
    target.context = None;
    target.pending_switch = None;
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::TestHooks;
    use monocode_core::HarnessId;
    use monocode_core::block::{Block, BlockRole};
    use monocode_core::context_usage::ContextUsage;
    use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelPrefs};
    use monocode_core::project_providers::ProjectProviders;
    use monocode_layout::layout::{new_file_tab, new_terminal_file};

    struct Env {
        catalog: ModelCatalog,
        prefs: ModelPrefs,
        availability: HarnessAvailability,
        projects: ProjectProviders,
    }

    impl Env {
        fn new() -> Self {
            Self {
                catalog: ModelCatalog::new(),
                prefs: ModelPrefs::default(),
                availability: HarnessAvailability::default(),
                projects: ProjectProviders::default(),
            }
        }

        fn env(&self) -> ModelEnv<'_> {
            ModelEnv {
                catalog: &self.catalog,
                prefs: &self.prefs,
                availability: &self.availability,
                projects: &self.projects,
            }
        }

        fn session(&self, cwd: &str) -> Session {
            new_session(
                &self.env(),
                uuid::Uuid::new_v4().to_string(),
                HarnessId::Codex,
                cwd,
                None,
                None,
                None,
            )
        }
    }

    fn tree() -> Worktree {
        Worktree {
            head: "abc".into(),
            dirty: Some(false),
            unpushed: Some(0),
            ..Worktree::new("/repo-worktrees/feature", Some("feature"))
        }
    }

    fn context() -> Option<ContextUsage> {
        serde_json::from_value(serde_json::json!({ "used": 12 })).ok()
    }

    fn user(text: &str) -> Vec<Block> {
        vec![Block::new("u", BlockRole::User, text)]
    }

    // worktree deletion preflight

    #[test]
    fn blocks_open_files_and_terminals_including_nested_working_folders() {
        let path = tree().path;
        for file in [
            new_file_tab(&format!("{path}/file.ts"), &path, false, None, None),
            new_file_tab(&format!("{path}/file.ts"), "/repo", false, None, None),
            new_terminal_file(&path, None, None),
            new_terminal_file(&format!("{path}/src"), None, None),
        ] {
            let error = assert_worktree_files_closed(&path, &[file]).unwrap_err();
            assert!(error.contains("Close the files and terminals"));
        }
    }

    #[test]
    fn allows_unrelated_files_and_similarly_named_sibling_worktrees() {
        let path = tree().path;
        assert!(
            assert_worktree_files_closed(
                &path,
                &[
                    new_file_tab("/repo/file.ts", "/repo", false, None, None),
                    new_terminal_file(&format!("{path}-other"), None, None),
                ],
            )
            .is_ok()
        );
    }

    // working-copy context

    #[test]
    fn lets_an_empty_session_select_a_worktree_and_return_to_main_in_place() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let mut session = env.session("/repo");
        session.provider_session_id = Some("old-provider".into());
        session.context = context();
        session.composer_seed = Some("An unsent draft".into());
        session.blocks = vec![Block::new("s", BlockRole::System, "Ready")];

        let selected = session_in_worktree(&env.env(), &*hooks, &session, &tree());
        assert_eq!(selected.cwd, "/repo");
        assert_eq!(selected.id, session.id);
        assert_eq!(selected.blocks, session.blocks);
        assert_eq!(selected.composer_seed.as_deref(), Some("An unsent draft"));
        assert_eq!(session_work_cwd(&selected), tree().path);
        assert_eq!(selected.provider_session_id, None);
        assert_eq!(selected.context, None);

        let main = session_in_worktree(
            &env.env(),
            &*hooks,
            &selected,
            &Worktree {
                is_main: true,
                ..Worktree::new("/repo", Some("main"))
            },
        );
        assert_eq!(main.worktree_cwd, None);
        assert_eq!(main.id, session.id);
        assert_eq!(session_work_cwd(&main), "/repo");
    }

    #[test]
    fn starts_a_separate_session_on_main_without_changing_the_worktree_conversation() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let mut source = env.session("/repo");
        source.worktree_cwd = Some(tree().path);
        source.branch = tree().branch;
        source.provider_session_id = Some("existing-agent-thread".into());
        source.provider_account_id = Some("work-account".into());
        source.context = context();
        source.title = "Build feature".into();
        source.blocks = user("Build feature");
        source.composer_seed = Some("Keep this draft here".into());
        let before = source.clone();

        let selected = session_in_worktree(
            &env.env(),
            &*hooks,
            &source,
            &Worktree {
                is_main: true,
                ..Worktree::new("/repo", Some("main"))
            },
        );
        assert_eq!(source, before);
        assert_ne!(selected.id, source.id);
        assert_eq!(selected.cwd, source.cwd);
        assert_eq!(selected.worktree_cwd, None);
        assert_eq!(session_work_cwd(&selected), "/repo");
        assert_eq!(selected.branch.as_deref(), Some("main"));
        assert!(selected.blocks.is_empty());
        assert_eq!(selected.provider_session_id, None);
        assert_eq!(selected.context, None);
        assert_eq!(selected.composer_seed, None);
        assert_ne!(selected.title, source.title);
        assert_eq!(selected.harness, source.harness);
        assert_eq!(selected.model, source.model);
        assert_eq!(selected.model_settings, source.model_settings);
        assert_eq!(selected.runtime_mode, source.runtime_mode);
        assert_eq!(selected.provider_account_id, source.provider_account_id);
    }

    #[test]
    fn binds_a_session_after_the_first_user_message_even_without_a_provider_thread() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let mut source = env.session("/repo");
        // Attachment-only messages count too.
        source.blocks = user("");
        let selected = session_in_worktree(&env.env(), &*hooks, &source, &tree());
        assert_ne!(selected.id, source.id);
        assert!(selected.blocks.is_empty());
        assert_eq!(session_work_cwd(&selected), tree().path);
        assert_eq!(session_work_cwd(&source), "/repo");
        assert_eq!(source.blocks.len(), 1);
    }

    #[test]
    fn opens_a_new_session_when_selecting_another_linked_worktree() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let mut source = env.session("/repo");
        source.worktree_cwd = Some(tree().path);
        source.blocks = user("Build feature");
        let other = Worktree {
            path: "/repo-worktrees/other".into(),
            branch: Some("other".into()),
            ..tree()
        };
        let selected = session_in_worktree(&env.env(), &*hooks, &source, &other);
        assert_ne!(selected.id, source.id);
        assert_eq!(selected.cwd, "/repo");
        assert_eq!(selected.worktree_cwd.as_deref(), Some(other.path.as_str()));
        assert_eq!(source.worktree_cwd, Some(tree().path));
    }

    #[test]
    fn leaves_the_current_working_copy_and_provider_context_unchanged_when_reselected() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let mut source = env.session("/repo");
        source.worktree_cwd = Some(tree().path);
        source.provider_session_id = Some("existing-agent-thread".into());
        source.context = context();
        source.blocks = user("Build feature");
        assert_eq!(
            session_in_worktree(&env.env(), &*hooks, &source, &tree()),
            source
        );
    }

    #[test]
    fn counts_shared_archived_and_unsaved_sessions_without_double_counting() {
        let env = Env::new();
        let saved_tree = Worktree {
            session_ids: vec!["saved".into(), "archived".into(), "moved".into()],
            ..tree()
        };
        let with = |id: &str, cwd: &str, worktree: Option<&str>| Session {
            id: id.into(),
            worktree_cwd: worktree.map(str::to_string),
            ..env.session(cwd)
        };
        let sessions = [
            with("saved", "/repo", Some(&tree().path)),
            with("blank", "/repo", Some(&tree().path)),
            with("moved", "/repo", None),
            with("opened-as-project", &tree().path, None),
        ];
        let mut ids = worktree_session_ids(&saved_tree, &sessions);
        ids.sort();
        assert_eq!(ids, ["archived", "blank", "opened-as-project", "saved"]);
    }

    // sessions kept after worktree deletion

    fn kept_source(env: &Env) -> Session {
        let mut source = env.session("/repo");
        source.worktree_cwd = Some(tree().path);
        source.branch = Some("feature".into());
        source.provider_session_id = Some("old-agent-thread".into());
        source.title = "Build feature".into();
        source.blocks = user("Build feature");
        source
    }

    #[test]
    fn keeps_the_transcript_and_clears_the_selected_branch_and_provider() {
        let env = Env::new();
        let source = kept_source(&env);
        let kept = detach_session_worktree(&source, "/repo", &tree().path);
        assert_eq!(kept.id, source.id);
        assert_eq!(kept.blocks, source.blocks);
        assert_eq!(kept.title, source.title);
        assert_eq!(kept.worktree_removed, Some(true));
        assert_eq!(kept.branch, None);
        assert_eq!(kept.provider_session_id, None);
        let saved = Worktree {
            session_ids: vec![source.id.clone()],
            ..tree()
        };
        assert!(worktree_session_ids(&saved, &[kept]).is_empty());
    }

    #[test]
    fn moves_a_directly_opened_worktrees_project_identity_to_the_surviving_repository() {
        let env = Env::new();
        let mut direct = kept_source(&env);
        direct.cwd = format!("{}/src", tree().path);
        direct.worktree_cwd = None;
        let kept = detach_session_worktree(&direct, "/repo", &tree().path);
        assert_eq!(kept.cwd, "/repo");
        assert_eq!(kept.worktree_cwd, Some(format!("{}/src", tree().path)));
    }

    #[test]
    fn continues_the_same_conversation_after_deletion() {
        let env = Env::new();
        let hooks = TestHooks::new();
        let source = kept_source(&env);
        for path in [
            "/repo".to_string(),
            tree().path,
            "/repo-worktrees/other".into(),
        ] {
            let kept = detach_session_worktree(&source, "/repo", &tree().path);
            let selected = session_in_worktree(
                &env.env(),
                &*hooks,
                &kept,
                &Worktree {
                    path: path.clone(),
                    ..tree()
                },
            );
            assert_eq!(selected.id, source.id);
            assert_eq!(selected.title, source.title);
            assert_eq!(selected.blocks[0], source.blocks[0]);
            let last = selected.blocks.last().unwrap();
            assert_eq!(
                last.handoff.as_ref().and_then(|handoff| handoff.pending),
                Some(true)
            );
            assert!(last.text.contains("Build feature"));
            assert!(
                last.text
                    .contains(&format!("Continue this conversation in {path}."))
            );
            assert_eq!(session_work_cwd(&selected), path);
            assert_eq!(selected.worktree_removed, None);
            assert_eq!(selected.provider_session_id, None);
        }
    }

    #[test]
    fn detaches_history_rows() {
        let mut row = SessionSummary::new("s1", "/repo", HarnessId::Codex);
        row.worktree_cwd = Some(tree().path);
        row.branch = Some("feature".into());
        row.provider_session_id = Some("thread".into());
        let kept = detach_summary_worktree(&row, "/repo", &tree().path);
        assert_eq!(kept.cwd, "/repo");
        assert_eq!(kept.worktree_cwd, Some(tree().path));
        assert_eq!(kept.worktree_removed, Some(true));
        assert_eq!(kept.branch, None);
        assert_eq!(kept.provider_session_id, None);
    }

    // branch names

    #[test]
    fn names_temporary_and_orchestration_branches() {
        assert_eq!(
            temporary_worktree_branch_name(Some("AB-12_cd/ef9-xyz")),
            "mc/ab12cdef"
        );
        assert!(temporary_worktree_branch_name(Some("---")).starts_with("mc/"));
        assert_eq!(temporary_worktree_branch_name(None).len(), "mc/".len() + 8);
        assert_eq!(
            orchestration_worktree_branch_name("Run-1234-5678-90ab-cdef"),
            "mc/orch-run123456789"
        );
        assert!(orchestration_worktree_branch_name("").starts_with("mc/orch-"));
    }

    #[test]
    fn names_user_branches_under_mc() {
        assert_eq!(
            named_worktree_branch(" feature/x ").as_deref(),
            Some("mc/feature/x")
        );
        assert_eq!(named_worktree_branch("mc//fix").as_deref(), Some("mc/fix"));
        assert_eq!(
            named_worktree_branch("monocode/fix/").as_deref(),
            Some("mc/fix")
        );
        assert_eq!(named_worktree_branch("/lead/").as_deref(), Some("mc/lead"));
        assert_eq!(named_worktree_branch("mc/"), None);
        assert_eq!(named_worktree_branch("  "), None);
        assert_eq!(named_worktree_branch("mcx/y").as_deref(), Some("mc/mcx/y"));
    }

    #[test]
    fn compares_paths_inside_roots() {
        assert!(is_equal_or_inside("/repo/", "/repo"));
        assert!(is_equal_or_inside("/repo/src", "/repo"));
        assert!(!is_equal_or_inside("/repo-other", "/repo"));
        assert!(is_equal_or_inside("C:\\Repo\\src", "c:/repo"));
    }
}
