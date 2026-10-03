//! Port of src/features/sessions/model/addChatToWorkspace.ts: open a new
//! chat for an "Add to chat" request when no session pane can take it.

use monocode_core::{RuntimeMode, Session};
use monocode_layout::workspace_tab_groups::{
    focused_workspace_tab_cwd, open_add_to_chat_session_pane,
};
use monocode_layout::{WorkspaceTab, focused_file_tab, leaf_ids, new_tab};

use super::chat_context::{ChatContextItem, composer_seed_for_add_to_chat};
use super::session_factory::SessionFactory;

/// `AddChatToWorkspaceResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddChatToWorkspaceResult {
    /// The sessions, including the new chat.
    pub sessions: Vec<Session>,
    /// The tabs, including the one hosting the new chat.
    pub tabs: Vec<WorkspaceTab>,
    /// The tab to activate.
    pub active_tab_id: String,
    /// The new chat.
    pub session_id: String,
}

/// The input of `applyAddToChatRequest`.
pub struct AddToChatRequest<'a> {
    pub sessions: &'a [Session],
    pub tabs: &'a [WorkspaceTab],
    pub active_tab_id: Option<&'a str>,
    pub project_cwd: &'a str,
    /// The cwd fallback for the normal path (App's `sessionDefaults?.cwd`).
    pub fallback_cwd: Option<&'a str>,
    pub default_runtime_mode: Option<RuntimeMode>,
    pub item: &'a ChatContextItem,
}

/// `applyAddToChatRequest`: handle an add-to-chat request end to end.
///
/// Normally the new chat splits into the active (or first) tab beside a
/// file-only pane. A mounted session pane owns add-to-chat instead, so this
/// returns `None` when the target tab already shows a session.
///
/// With no tabs at all (issue #311), it builds one session seeded with the
/// context chip and opens it as the replacement tab. That tab already hosts
/// the session, so there is no second split (PR #325 review). The cwd is
/// always the project directory, and the last known session donates the
/// provider, model, and settings, so exactly one conversation is created.
pub fn apply_add_to_chat_request(
    request: AddToChatRequest<'_>,
    factory: &dyn SessionFactory,
) -> Option<AddChatToWorkspaceResult> {
    let AddToChatRequest {
        sessions,
        tabs,
        active_tab_id,
        project_cwd,
        fallback_cwd,
        default_runtime_mode,
        item,
    } = request;
    let composer_seed = composer_seed_for_add_to_chat(item);

    let mut current_sessions: Vec<Session> = sessions.to_vec();
    let mut current_tabs: Vec<WorkspaceTab> = tabs.to_vec();
    let found = current_tabs
        .iter()
        .find(|entry| Some(entry.id.as_str()) == active_tab_id)
        .or(current_tabs.first())
        .cloned();
    let mut created_session: Option<Session> = None;

    let tab = match found {
        Some(tab) => tab,
        None => {
            // Seed one replacement chat from the last known session's
            // provider, model, and settings, but never its cwd.
            let donor = sessions.last();
            let created = Session {
                composer_seed: Some(composer_seed.clone()),
                ..factory.new_session_like(donor, project_cwd)
            };
            let tab = new_tab(&created.id);
            current_sessions.push(created.clone());
            current_tabs.push(tab.clone());
            created_session = Some(created);
            tab
        }
    };

    // The fallback tab wraps the seeded session, so its only leaf is mounted
    // by construction and this guard must not reject it.
    if created_session.is_none() {
        let mounted = leaf_ids(&tab.layout)
            .iter()
            .any(|id| current_sessions.iter().any(|session| &session.id == id));
        if mounted {
            return None;
        }
    }

    let cwd = focused_workspace_tab_cwd(&tab, &current_sessions)
        .or_else(|| fallback_cwd.map(str::to_string))
        .unwrap_or_else(|| project_cwd.to_string());
    let file = focused_file_tab(&tab);
    let session = match &created_session {
        Some(created) => created.clone(),
        None => {
            let mut session = factory.new_default_session(&cwd, default_runtime_mode);
            if let Some(file) = file
                && file.project_cwd.is_some()
            {
                session.worktree_cwd = Some(file.cwd.clone());
            }
            session.composer_seed = Some(composer_seed);
            session
        }
    };

    let next_tab = if created_session.is_some() {
        // The fallback tab already hosts the new session; splitting it
        // beside itself would duplicate the pane (PR #325 review).
        tab.clone()
    } else {
        // A mounted session pane owns the normal add-to-chat path.
        open_add_to_chat_session_pane(&tab, sessions, &session.id)?
    };

    let session_id = session.id.clone();
    let (sessions, tabs) = if created_session.is_some() {
        (current_sessions, current_tabs)
    } else {
        current_sessions.push(session);
        let tabs = current_tabs
            .into_iter()
            .map(|entry| {
                if entry.id == tab.id {
                    next_tab.clone()
                } else {
                    entry
                }
            })
            .collect();
        (current_sessions, tabs)
    };
    Some(AddChatToWorkspaceResult {
        sessions,
        tabs,
        active_tab_id: tab.id,
        session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::session_factory::ModelEnvSessions;
    use monocode_core::HarnessId;
    use monocode_layout::{EditorPane, new_file_tab};

    fn item() -> ChatContextItem {
        ChatContextItem::Code {
            path: "src/value.ts".into(),
            start_line: 3,
            end_line: 5,
        }
    }

    fn session(id: &str, cwd: &str) -> Session {
        Session::blank(id, HarnessId::Cursor, "", cwd)
    }

    fn donor(id: &str, harness: HarnessId, model: &str, mode: RuntimeMode) -> Session {
        Session {
            runtime_mode: mode,
            ..Session::blank(id, harness, model, "/other/project")
        }
    }

    fn file_only_tab(id: &str, cwd: &str) -> WorkspaceTab {
        let file = new_file_tab(&format!("{cwd}/readme.md"), cwd, false, None, None);
        WorkspaceTab {
            id: id.into(),
            editor_panes: vec![EditorPane::new("pane", vec![file.clone()], file.id.clone())],
            ..new_tab("unused")
        }
    }

    fn apply(
        sessions: &[Session],
        tabs: &[WorkspaceTab],
        active_tab_id: Option<&str>,
    ) -> Option<AddChatToWorkspaceResult> {
        let item = item();
        apply_add_to_chat_request(
            AddToChatRequest {
                sessions,
                tabs,
                active_tab_id,
                project_cwd: "/current/project",
                fallback_cwd: None,
                default_runtime_mode: None,
                item: &item,
            },
            &ModelEnvSessions::default(),
        )
    }

    fn new_chat(result: &AddChatToWorkspaceResult) -> &Session {
        result
            .sessions
            .iter()
            .find(|session| session.id == result.session_id)
            .unwrap()
    }

    #[test]
    fn creates_exactly_one_seeded_session_hosted_by_exactly_one_pane() {
        let donor = donor("s1", HarnessId::Claude, "claude:opus-5", RuntimeMode::Auto);
        let result = apply(&[donor], &[], None).unwrap();
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.session_id, new_chat(&result).id);
        assert_eq!(
            leaf_ids(&result.tabs[0].layout),
            vec![result.session_id.clone()]
        );
        assert_eq!(result.tabs[0].focused_id, result.session_id);
        assert_eq!(result.active_tab_id, result.tabs[0].id);
    }

    #[test]
    fn seeds_the_composer_with_the_context_chip() {
        let result = apply(&[], &[], None).unwrap();
        assert_eq!(
            new_chat(&result).composer_seed.as_deref(),
            Some(composer_seed_for_add_to_chat(&item()).as_str())
        );
    }

    #[test]
    fn keeps_the_donor_sessions_harness_model_and_runtime_mode() {
        let donor = donor("s1", HarnessId::Claude, "claude:opus-5", RuntimeMode::Auto);
        let result = apply(&[donor], &[], None).unwrap();
        let chat = new_chat(&result);
        assert_eq!(chat.harness, HarnessId::Claude);
        assert_eq!(chat.model, "claude:opus-5");
        assert_eq!(chat.runtime_mode, RuntimeMode::Auto);
    }

    #[test]
    fn uses_the_project_cwd_never_another_projects_session_cwd() {
        let result = apply(&[session("s1", "/other/project")], &[], None).unwrap();
        assert_eq!(new_chat(&result).cwd, "/current/project");
    }

    #[test]
    fn donates_settings_from_the_last_known_session_not_the_first() {
        let first = donor(
            "s1",
            HarnessId::Codex,
            "codex:gpt-5",
            RuntimeMode::FullAccess,
        );
        let last = donor("s2", HarnessId::Claude, "claude:opus-5", RuntimeMode::Auto);
        let result = apply(&[first, last], &[], None).unwrap();
        let chat = new_chat(&result);
        assert_eq!(chat.harness, HarnessId::Claude);
        assert_eq!(chat.model, "claude:opus-5");
        assert_eq!(chat.runtime_mode, RuntimeMode::Auto);
    }

    #[test]
    fn falls_back_to_claude_defaults_with_an_empty_workspace() {
        let result = apply(&[], &[], None).unwrap();
        assert_eq!(new_chat(&result).harness, HarnessId::Claude);
        assert_eq!(new_chat(&result).cwd, "/current/project");
    }

    #[test]
    fn splits_the_new_chat_beside_the_file_pane() {
        let tab = file_only_tab("tab1", "/current/project");
        let result = apply(&[], &[tab], Some("tab1")).unwrap();
        assert_eq!(
            leaf_ids(&result.tabs[0].layout),
            vec!["unused".to_string(), result.session_id.clone()]
        );
        assert_eq!(result.tabs[0].focused_id, result.session_id);
        assert_eq!(result.sessions.len(), 1);
    }

    #[test]
    fn bails_when_the_target_tab_already_shows_a_mounted_session() {
        let mounted = session("s1", "/current/project");
        let tab = new_tab("s1");
        let id = tab.id.clone();
        assert_eq!(apply(&[mounted], &[tab], Some(&id)), None);
    }
}
