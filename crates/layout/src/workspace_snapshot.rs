//! Port of src/features/workspace/model/workspaceSnapshot.ts: the tabs,
//! panes, session stubs, and terminal docks the app saves on quit and
//! restores on launch.
//!
//! The serde types read and write the JSON the TypeScript stored through
//! `workspace_set_snapshot`, keeping unknown fields in `extra`. The
//! TypeScript sanitized untyped JSON, so `parse_workspace_snapshot` and the
//! `sanitize_*` helpers take a `serde_json::Value` and drop unknown fields
//! the same way.
//!
//! `hydrate_workspace_snapshot` needs two things the TypeScript imported:
//! `newSession`, which takes a `ModelEnv` here, and `markTurnInterrupted`
//! from sessions/model/inFlight.ts, which needs the transcript reducer and
//! so comes in as a closure.

use std::collections::{HashMap, HashSet};

use monocode_core::block::ModelSettings;
use monocode_core::inbox::InboxAskContext;
use monocode_core::models::ModelEnv;
use monocode_core::paths::path_key;
use monocode_core::session::new_session;
use monocode_core::{Extra, HarnessId, RuntimeMode, Session, js as core_js};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js::{is_present, is_true, nonempty_str, number, str_field, trimmed_str};
use crate::layout::{
    CommitTabSource, EditorPane, FilePaneTab, GitFileDiffKind, LayoutNode, PlanTabSource,
    ReleaseNotesTabSource, SessionChangesSource, SplitDir, WorkspaceTab, close_leaf, has_leaf,
    is_agent_tab, is_terminal_tab, leaf, leaf_ids, new_tab,
};
use crate::paths::{normalize_project_path, parse_remote_path, remote_path};
use crate::project_return::{ProjectReturnMemory, reconcile_project_return};
use crate::project_terminal::{DockSide, ProjectTerminalDock, clamp_dock_size, is_dock_side};
use crate::session_ref::SessionRef;

/// `WorkspaceSessionStub`: enough of a session to reopen its tab when the
/// session itself was never persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSessionStub {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbox_ask: Option<InboxAskContext>,
    pub id: String,
    pub cwd: String,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub runtime_mode: RuntimeMode,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_removed: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl WorkspaceSessionStub {
    /// A stub with no optional fields.
    pub fn new(
        id: impl Into<String>,
        cwd: impl Into<String>,
        harness: HarnessId,
        model: impl Into<String>,
        runtime_mode: RuntimeMode,
        title: impl Into<String>,
    ) -> Self {
        Self {
            inbox_ask: None,
            id: id.into(),
            cwd: cwd.into(),
            harness,
            model: model.into(),
            model_settings: ModelSettings::new(),
            runtime_mode,
            title: title.into(),
            provider_session_id: None,
            provider_account_id: None,
            branch: None,
            worktree_cwd: None,
            worktree_removed: None,
            extra: Extra::new(),
        }
    }
}

impl SessionRef for WorkspaceSessionStub {
    fn session_id(&self) -> &str {
        &self.id
    }

    fn session_cwd(&self) -> &str {
        &self.cwd
    }
}

/// One `projectReturnTargets` entry: the pane to show when the user returns
/// to a project. New snapshots write `tabId` with a pane id in it; readers
/// also accept `paneId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReturnTarget {
    pub project_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `WorkspaceSnapshot`: the `workspace_snapshot.snapshot_json` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<WorkspaceTab>,
    #[serde(default)]
    pub sessions: Vec<WorkspaceSessionStub>,
    pub active_tab_id: String,
    pub project_cwd: String,
    #[serde(default)]
    pub project_terminals: Vec<ProjectTerminalDock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_return_targets: Option<Vec<ProjectReturnTarget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_dock_side: Option<DockSide>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `ResumedWorkspace` from src/features/sessions/model/inFlight.ts: what a
/// restored snapshot reopens.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumedWorkspace {
    pub sessions: Vec<Session>,
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    pub project_cwd: String,
    pub project_terminals: Option<Vec<ProjectTerminalDock>>,
    pub project_return_memory: Option<ProjectReturnMemory>,
    pub last_dock_side: Option<DockSide>,
}

/// A stub and whether it came from an Inbox conversation. The flag is
/// separate because a stored `inboxAsk` that does not match
/// `InboxAskContext` still marks the stub as Inbox, as it did in the
/// TypeScript.
struct Stub {
    stub: WorkspaceSessionStub,
    inbox: bool,
}

/// The snapshot before `withProjectReturnTargets`.
struct Draft {
    tabs: Vec<WorkspaceTab>,
    sessions: Vec<Stub>,
    active_tab_id: String,
    project_cwd: String,
    project_terminals: Vec<ProjectTerminalDock>,
    last_dock_side: Option<DockSide>,
}

/// `collectWorkspaceSnapshot`: the snapshot to save. Stores tabs, session
/// stubs, and the focused tab, not transcripts.
pub fn collect_workspace_snapshot(
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    active_tab_id: &str,
    project_cwd: &str,
    memory: &ProjectReturnMemory,
    project_terminals: &[ProjectTerminalDock],
    last_dock_side: Option<DockSide>,
) -> WorkspaceSnapshot {
    collect_workspace_snapshot_keeping(
        tabs,
        sessions,
        active_tab_id,
        project_cwd,
        memory,
        project_terminals,
        last_dock_side,
        &|_| true,
    )
}

/// `collectWorkspaceSnapshot` with `keepTab`. Tabs `keep_tab` leaves out
/// (another worktree's, say) do not reopen, and neither do sessions that
/// only they showed.
#[allow(clippy::too_many_arguments)]
pub fn collect_workspace_snapshot_keeping(
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    active_tab_id: &str,
    project_cwd: &str,
    memory: &ProjectReturnMemory,
    project_terminals: &[ProjectTerminalDock],
    last_dock_side: Option<DockSide>,
    keep_tab: &dyn Fn(&WorkspaceTab) -> bool,
) -> WorkspaceSnapshot {
    let (kept, left_out): (Vec<WorkspaceTab>, Vec<WorkspaceTab>) =
        tabs.iter().cloned().partition(|tab| keep_tab(tab));
    let kept_ids: HashSet<String> = kept.iter().flat_map(|tab| leaf_ids(&tab.layout)).collect();
    let dropped_ids: HashSet<String> = left_out
        .iter()
        .flat_map(|tab| leaf_ids(&tab.layout))
        .filter(|id| !kept_ids.contains(id))
        .collect();
    let project_cwd = core_js::trim(project_cwd);
    let draft = without_inbox_sessions(Draft {
        tabs: without_agent_tabs(&kept)
            .iter()
            .filter_map(|tab| sanitize_tab(&to_value(tab)))
            .collect(),
        sessions: sessions
            .iter()
            .filter(|session| !dropped_ids.contains(&session.id))
            .filter_map(session_stub)
            .map(|stub| Stub {
                inbox: stub.inbox_ask.is_some(),
                stub,
            })
            .collect(),
        active_tab_id: active_tab_id.to_string(),
        project_cwd: if project_cwd.is_empty() {
            "~".into()
        } else {
            project_cwd.to_string()
        },
        project_terminals: project_terminals
            .iter()
            .filter_map(|dock| sanitize_project_terminal(&to_value(dock)))
            .collect(),
        last_dock_side,
    });
    with_project_return_targets(draft, memory)
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `withProjectReturnTargets`.
fn with_project_return_targets(draft: Draft, memory: &ProjectReturnMemory) -> WorkspaceSnapshot {
    let sessions: Vec<WorkspaceSessionStub> =
        draft.sessions.into_iter().map(|stub| stub.stub).collect();
    let valid = reconcile_project_return(memory, &draft.tabs, &sessions, "");
    WorkspaceSnapshot {
        project_return_targets: Some(
            valid
                .iter()
                .map(|(project_path, pane_id)| ProjectReturnTarget {
                    project_path: project_path.to_string(),
                    tab_id: Some(pane_id.to_string()),
                    pane_id: None,
                    extra: Extra::new(),
                })
                .collect(),
        ),
        tabs: draft.tabs,
        sessions,
        active_tab_id: draft.active_tab_id,
        project_cwd: draft.project_cwd,
        project_terminals: draft.project_terminals,
        last_dock_side: draft.last_dock_side,
        extra: Extra::new(),
    }
}

/// `parseProjectReturnTargets`: project path key to the remembered pane.
pub fn parse_project_return_targets(raw: Option<&Value>) -> ProjectReturnMemory {
    let mut memory = ProjectReturnMemory::new();
    let Some(entries) = raw.and_then(Value::as_array) else {
        return memory;
    };
    for entry in entries {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        if !entry.contains_key("projectPath") {
            continue;
        }
        let Some(project_path) =
            str_field(entry, "projectPath").filter(|path| !core_js::trim(path).is_empty())
        else {
            continue;
        };

        let remembered = match entry.get("paneId") {
            Some(pane_id) if !pane_id.is_null() => Some(pane_id),
            _ => entry.get("tabId"),
        };
        let Some(remembered) = remembered
            .and_then(Value::as_str)
            .map(core_js::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };

        memory.set(path_key(project_path), remembered);
    }
    memory
}

/// `withoutAgentTabs`. Agent tabs watch a live worker, and a run does not
/// outlive the window that started it. Dropping them in `sanitize_file`
/// would strand an empty pane and cost the whole workspace tab on restore,
/// so the pane is closed here instead.
fn without_agent_tabs(tabs: &[WorkspaceTab]) -> Vec<WorkspaceTab> {
    tabs.iter()
        .filter_map(|tab| {
            if !tab
                .editor_panes
                .iter()
                .any(|pane| pane.files.iter().any(is_agent_tab))
            {
                return Some(tab.clone());
            }
            let mut remaining = Some(tab.clone());
            let mut panes = Vec::new();
            for pane in &tab.editor_panes {
                let files: Vec<FilePaneTab> = pane
                    .files
                    .iter()
                    .filter(|file| !is_agent_tab(file))
                    .cloned()
                    .collect();
                if files.len() == pane.files.len() {
                    panes.push(pane.clone());
                } else if files.is_empty() {
                    remaining = remaining.and_then(|remaining| close_leaf(&remaining, &pane.id));
                } else {
                    let active_file_id = if files.iter().any(|file| file.id == pane.active_file_id)
                    {
                        pane.active_file_id.clone()
                    } else {
                        files[0].id.clone()
                    };
                    panes.push(EditorPane {
                        files,
                        active_file_id,
                        ..pane.clone()
                    });
                }
            }
            remaining.map(|remaining| WorkspaceTab {
                editor_panes: panes,
                ..remaining
            })
        })
        .collect()
}

/// `withoutInboxSessions`. Also removes tabs saved by the earlier,
/// persistent Inbox implementation.
fn without_inbox_sessions(draft: Draft) -> Draft {
    let inbox_ids: Vec<String> = draft
        .sessions
        .iter()
        .filter(|stub| stub.inbox)
        .map(|stub| stub.stub.id.clone())
        .collect();
    if inbox_ids.is_empty() {
        return draft;
    }
    let mut tabs = draft.tabs;
    for id in &inbox_ids {
        tabs = tabs
            .into_iter()
            .filter_map(|tab| {
                if !has_leaf(&tab.layout, id) {
                    return Some(tab);
                }
                close_leaf(&tab, id)
            })
            .collect();
    }
    let active_tab_id = if tabs.iter().any(|tab| tab.id == draft.active_tab_id) {
        draft.active_tab_id
    } else {
        tabs.first().map(|tab| tab.id.clone()).unwrap_or_default()
    };
    Draft {
        tabs,
        sessions: draft
            .sessions
            .into_iter()
            .filter(|stub| !stub.inbox)
            .collect(),
        active_tab_id,
        ..draft
    }
}

/// `parseWorkspaceSnapshot`: sanitize a stored snapshot. `None` when it has
/// no usable tab.
pub fn parse_workspace_snapshot(raw: &Value) -> Option<WorkspaceSnapshot> {
    let value = raw.as_object()?;
    let raw_tabs = value.get("tabs").and_then(Value::as_array)?;
    let active_tab_id = str_field(value, "activeTabId")?;
    let tabs: Vec<WorkspaceTab> = raw_tabs.iter().filter_map(sanitize_tab).collect();
    if tabs.is_empty() {
        return None;
    }
    let sessions = value
        .get("sessions")
        .and_then(Value::as_array)
        .map(|stubs| stubs.iter().filter_map(sanitize_stub).collect())
        .unwrap_or_default();
    let active_tab_id = if tabs.iter().any(|tab| tab.id == active_tab_id) {
        active_tab_id.to_string()
    } else {
        tabs[0].id.clone()
    };
    let project_cwd = trimmed_str(value, "projectCwd").unwrap_or("~").to_string();
    let project_terminals = value
        .get("projectTerminals")
        .and_then(Value::as_array)
        .map(|docks| docks.iter().filter_map(sanitize_project_terminal).collect())
        .unwrap_or_default();
    let last_dock_side = is_dock_side(value.get("lastDockSide"));
    let draft = without_inbox_sessions(Draft {
        tabs,
        sessions,
        active_tab_id,
        project_cwd,
        project_terminals,
        last_dock_side,
    });
    if draft.tabs.is_empty() {
        return None;
    }
    let memory = parse_project_return_targets(value.get("projectReturnTargets"));
    Some(with_project_return_targets(draft, &memory))
}

/// `workspaceSnapshotKey`: a string that changes when the snapshot does.
pub fn workspace_snapshot_key(snapshot: &WorkspaceSnapshot) -> String {
    serde_json::to_string(snapshot).unwrap_or_default()
}

/// `hydrateWorkspaceSnapshot` for a typed snapshot.
pub fn hydrate_workspace_snapshot(
    snapshot: &WorkspaceSnapshot,
    loaded: &HashMap<String, Session>,
    interrupted_ids: &[String],
    env: &ModelEnv<'_>,
    mark_turn_interrupted: impl Fn(Session) -> Session,
) -> Option<ResumedWorkspace> {
    hydrate_workspace_snapshot_value(
        &to_value(snapshot),
        loaded,
        interrupted_ids,
        env,
        mark_turn_interrupted,
    )
}

/// `hydrateWorkspaceSnapshot`: reopen the saved tabs and panes.
/// Transcripts come from `loaded` when the session was persisted; blank
/// tabs fall back to the stub. Sessions in `interrupted_ids` go through
/// `mark_turn_interrupted` and get a tab when they had none.
pub fn hydrate_workspace_snapshot_value(
    snapshot: &Value,
    loaded: &HashMap<String, Session>,
    interrupted_ids: &[String],
    env: &ModelEnv<'_>,
    mark_turn_interrupted: impl Fn(Session) -> Session,
) -> Option<ResumedWorkspace> {
    let parsed = parse_workspace_snapshot(snapshot)?;

    let mut pane_ids: HashSet<&str> = HashSet::new();
    for tab in &parsed.tabs {
        for pane in tab.editor_panes.iter().chain(&tab.terminal_panes) {
            pane_ids.insert(&pane.id);
        }
    }

    let mut stubs: HashMap<&str, &WorkspaceSessionStub> = HashMap::new();
    for stub in &parsed.sessions {
        stubs.insert(&stub.id, stub);
    }
    let interrupted = |id: &str| interrupted_ids.iter().any(|entry| entry == id);
    let mut sessions: Vec<Session> = Vec::new();

    // `take`: the session for `id`, created once from `loaded` or its stub.
    let take = |id: &str, sessions: &mut Vec<Session>| -> Option<usize> {
        if let Some(index) = sessions.iter().position(|session| session.id == id) {
            return Some(index);
        }
        let base = match loaded.get(id) {
            Some(record) => record.clone(),
            None => session_from_stub(stubs.get(id)?, env),
        };
        if base.inbox_ask.is_some() {
            return None;
        }
        let next = if interrupted(id) {
            mark_turn_interrupted(base)
        } else {
            Session {
                busy: Some(false),
                ..base
            }
        };
        sessions.push(next);
        Some(sessions.len() - 1)
    };

    for stub in &parsed.sessions {
        take(&stub.id, &mut sessions);
    }

    let mut tabs: Vec<WorkspaceTab> = Vec::new();
    for tab in &parsed.tabs {
        for id in leaf_ids(&tab.layout) {
            if pane_ids.contains(id.as_str()) || take(&id, &mut sessions).is_some() {
                continue;
            }
            let fallback = WorkspaceSessionStub::new(
                id.clone(),
                parsed.project_cwd.clone(),
                HarnessId::Cursor,
                "",
                RuntimeMode::Supervised,
                "",
            );
            sessions.push(session_from_stub(&fallback, env));
        }
        tabs.push(tab.clone());
    }

    for id in interrupted_ids {
        let Some(index) = take(id, &mut sessions) else {
            continue;
        };
        if sessions[index].inbox_ask.is_some() {
            continue;
        }
        if tabs.iter().any(|tab| has_leaf(&tab.layout, id)) {
            continue;
        }
        tabs.push(new_tab(id));
    }

    if tabs.is_empty() {
        return None;
    }
    let active_tab_id = if tabs.iter().any(|tab| tab.id == parsed.active_tab_id) {
        parsed.active_tab_id.clone()
    } else {
        tabs[0].id.clone()
    };
    let project_cwd = if parsed.project_cwd != "~" {
        parsed.project_cwd.clone()
    } else {
        sessions
            .first()
            .map(|session| session.cwd.clone())
            .unwrap_or_else(|| "~".into())
    };

    let memory = parse_project_return_targets(
        parsed
            .project_return_targets
            .as_ref()
            .map(to_value)
            .as_ref(),
    );
    let project_return_memory = reconcile_project_return(&memory, &tabs, &sessions, &active_tab_id);
    Some(ResumedWorkspace {
        sessions,
        tabs,
        active_tab_id,
        project_cwd,
        project_terminals: Some(parsed.project_terminals),
        project_return_memory: Some(project_return_memory),
        last_dock_side: parsed.last_dock_side,
    })
}

/// A non-empty optional string, the truthiness the TypeScript spread used.
fn present(value: &Option<String>) -> Option<String> {
    value.clone().filter(|value| !value.is_empty())
}

/// `sessionStub`.
fn session_stub(session: &Session) -> Option<WorkspaceSessionStub> {
    if session.id.is_empty() {
        return None;
    }
    Some(WorkspaceSessionStub {
        inbox_ask: session.inbox_ask.clone(),
        model_settings: session.model_settings.clone(),
        provider_session_id: present(&session.provider_session_id),
        provider_account_id: present(&session.provider_account_id),
        branch: present(&session.branch),
        worktree_cwd: present(&session.worktree_cwd),
        worktree_removed: (session.worktree_removed == Some(true)).then_some(true),
        ..WorkspaceSessionStub::new(
            session.id.clone(),
            if session.cwd.is_empty() {
                "~".to_string()
            } else {
                session.cwd.clone()
            },
            session.harness,
            session.model.clone(),
            session.runtime_mode,
            session.title.clone(),
        )
    })
}

/// `sessionFromStub`. A snapshot is a saved choice, not a new conversation:
/// catalog discovery and the current picker preferences must not replace
/// its model.
fn session_from_stub(stub: &WorkspaceSessionStub, env: &ModelEnv<'_>) -> Session {
    let session = new_session(
        env,
        stub.id.clone(),
        stub.harness,
        &stub.cwd,
        Some(&stub.model),
        Some(stub.runtime_mode),
        Some(&stub.model_settings),
    );
    Session {
        model: if stub.model.is_empty() {
            session.model.clone()
        } else {
            stub.model.clone()
        },
        model_settings: stub.model_settings.clone(),
        title: stub.title.clone(),
        inbox_ask: stub.inbox_ask.clone(),
        provider_session_id: present(&stub.provider_session_id),
        provider_account_id: present(&stub.provider_account_id),
        branch: present(&stub.branch),
        worktree_cwd: present(&stub.worktree_cwd),
        worktree_removed: (stub.worktree_removed == Some(true)).then_some(true),
        ..session
    }
}

/// `/^[A-Za-z0-9_-]+$/`.
fn is_account_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `sanitizeStub`.
fn sanitize_stub(raw: &Value) -> Option<Stub> {
    let value = raw.as_object()?;
    let id = nonempty_str(value, "id")?;
    let harness = str_field(value, "harness").and_then(HarnessId::parse)?;
    let runtime_mode = str_field(value, "runtimeMode").and_then(RuntimeMode::parse)?;
    let model_settings: ModelSettings = value
        .get("modelSettings")
        .and_then(Value::as_object)
        .map(|settings| {
            settings
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    // `value.inboxAsk && typeof value.inboxAsk === "object"`.
    let inbox_raw = value
        .get("inboxAsk")
        .filter(|inbox| matches!(inbox, Value::Object(_) | Value::Array(_)));
    let stub = WorkspaceSessionStub {
        inbox_ask: inbox_raw.and_then(|inbox| serde_json::from_value(inbox.clone()).ok()),
        model_settings,
        provider_session_id: nonempty_str(value, "providerSessionId").map(str::to_string),
        provider_account_id: str_field(value, "providerAccountId")
            .filter(|id| is_account_id(id))
            .map(str::to_string),
        branch: trimmed_str(value, "branch").map(str::to_string),
        worktree_cwd: trimmed_str(value, "worktreeCwd").map(str::to_string),
        worktree_removed: is_true(value, "worktreeRemoved").then_some(true),
        ..WorkspaceSessionStub::new(
            id,
            trimmed_str(value, "cwd").unwrap_or("~"),
            harness,
            str_field(value, "model").unwrap_or(""),
            runtime_mode,
            str_field(value, "title").unwrap_or(""),
        )
    };
    Some(Stub {
        stub,
        inbox: inbox_raw.is_some(),
    })
}

/// `sanitizeTab`.
pub fn sanitize_tab(raw: &Value) -> Option<WorkspaceTab> {
    let value = raw.as_object()?;
    let id = nonempty_str(value, "id")?;
    let layout = sanitize_layout(value.get("layout")?)?;
    let (editor_panes, editor_invalid) = sanitize_panes(value.get("editorPanes"));
    let (terminal_panes, terminal_invalid) = sanitize_panes(value.get("terminalPanes"));
    let invalid: HashSet<String> = editor_invalid.into_iter().chain(terminal_invalid).collect();
    let leaves = leaf_ids(&layout);
    if leaves.iter().any(|id| invalid.contains(id)) {
        return None;
    }
    let focused_id = match nonempty_str(value, "focusedId") {
        Some(focused) => focused.to_string(),
        None => leaves.first().cloned().filter(|id| !id.is_empty())?,
    };
    Some(WorkspaceTab {
        editor_panes,
        terminal_panes,
        diff_open: is_true(value, "diffOpen").then_some(true),
        diff_focused: is_true(value, "diffFocused").then_some(true),
        group_id: nonempty_str(value, "groupId").map(str::to_string),
        ..WorkspaceTab::new(id, layout, focused_id)
    })
}

/// `sanitizeLayout`.
pub fn sanitize_layout(raw: &Value) -> Option<LayoutNode> {
    let value = raw.as_object()?;
    if str_field(value, "type") == Some("leaf") {
        return nonempty_str(value, "id").map(leaf);
    }
    if str_field(value, "type") != Some("split") {
        return None;
    }
    let id = nonempty_str(value, "id")?;
    let dir = match str_field(value, "dir") {
        Some("down") => SplitDir::Down,
        Some("right") => SplitDir::Right,
        _ => return None,
    };
    let raw_children = value.get("children").and_then(Value::as_array)?;
    if raw_children.len() < 2 {
        return None;
    }
    let children: Vec<LayoutNode> = raw_children.iter().filter_map(sanitize_layout).collect();
    if children.len() < 2 {
        return None;
    }
    let sizes: Vec<f64> = value
        .get("sizes")
        .and_then(Value::as_array)
        .map(|sizes| {
            sizes
                .iter()
                .filter_map(Value::as_f64)
                .filter(|size| size.is_finite())
                .collect()
        })
        .unwrap_or_default();
    let normalized = if sizes.len() == children.len() {
        sizes
    } else {
        vec![1.0 / children.len() as f64; children.len()]
    };
    Some(LayoutNode::split(id, dir, children, normalized))
}

/// `sanitizePanes`: the valid panes, and the ids of panes that failed.
fn sanitize_panes(raw: Option<&Value>) -> (Vec<EditorPane>, Vec<String>) {
    let Some(entries) = raw.and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    let mut panes = Vec::new();
    let mut invalid_ids = Vec::new();
    for entry in entries {
        if let Some(pane) = sanitize_pane(entry) {
            panes.push(pane);
            continue;
        }
        if let Some(id) = entry
            .as_object()
            .and_then(|entry| nonempty_str(entry, "id"))
        {
            invalid_ids.push(id.to_string());
        }
    }
    (panes, invalid_ids)
}

/// `sanitizePane`.
pub fn sanitize_pane(raw: &Value) -> Option<EditorPane> {
    let value = raw.as_object()?;
    let id = nonempty_str(value, "id")?;
    let files: Vec<FilePaneTab> = value
        .get("files")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(sanitize_file)
        .collect();
    let first = files.first()?.id.clone();
    let active_file_id = str_field(value, "activeFileId")
        .filter(|active| files.iter().any(|file| file.id == *active))
        .map(str::to_string)
        .unwrap_or(first);
    Some(EditorPane::new(id, files, active_file_id))
}

/// `sanitizeFile`.
pub fn sanitize_file(raw: &Value) -> Option<FilePaneTab> {
    let value = raw.as_object()?;
    let id = nonempty_str(value, "id")?;
    let path = nonempty_str(value, "path")?;
    let cwd = nonempty_str(value, "cwd")?;
    let plan = value.get("plan").and_then(sanitize_plan);
    let has_release_notes = value.contains_key("releaseNotes");
    let release_notes = value.get("releaseNotes").and_then(sanitize_release_notes);
    let has_commit = value.contains_key("commit");
    let commit = value.get("commit").and_then(sanitize_commit);
    let has_session_changes = value.contains_key("sessionChanges");
    let session_changes = value
        .get("sessionChanges")
        .and_then(sanitize_session_changes);
    let has_remote_file = value.contains_key("remoteFile");
    let remote_file = value.get("remoteFile").is_some_and(is_remote_file);
    if has_release_notes && release_notes.is_none() {
        return None;
    }
    if has_commit && commit.is_none() {
        return None;
    }
    if has_session_changes && session_changes.is_none() {
        return None;
    }
    if has_remote_file && !remote_file {
        return None;
    }
    let remote_owner = if remote_file {
        str_field(value, "projectCwd").and_then(parse_remote_path)
    } else {
        None
    };
    if remote_file && remote_owner.is_none() {
        return None;
    }
    let plan_present = is_present(value, "plan");
    let terminal = is_true(value, "terminal");
    let review = is_true(value, "review");
    let changes = is_true(value, "changes");
    if remote_file
        && (plan_present
            || release_notes.is_some()
            || commit.is_some()
            || session_changes.is_some()
            || terminal)
    {
        return None;
    }
    if session_changes.is_some()
        && (plan_present || release_notes.is_some() || commit.is_some() || changes || terminal)
    {
        return None;
    }
    if release_notes.is_some()
        && (plan_present
            || review
            || changes
            || session_changes.is_some()
            || terminal
            || commit.is_some())
    {
        return None;
    }
    if commit.is_some()
        && (plan_present || review || changes || session_changes.is_some() || terminal)
    {
        return None;
    }
    let (path, cwd) = match &remote_owner {
        Some(owner) => (
            remote_path(&owner.environment_id, path),
            remote_path(&owner.environment_id, cwd),
        ),
        None => (path.to_string(), cwd.to_string()),
    };
    let change_kind = match str_field(value, "changeKind") {
        Some("staged") => Some(GitFileDiffKind::Staged),
        Some("unstaged") => Some(GitFileDiffKind::Unstaged),
        _ => None,
    };
    Some(FilePaneTab {
        project_cwd: nonempty_str(value, "projectCwd").map(str::to_string),
        plan,
        release_notes,
        commit,
        review: (session_changes.is_some() || review || changes).then_some(true),
        session_changes,
        changes: changes.then_some(true),
        change_kind,
        terminal: terminal.then_some(true),
        preview: is_true(value, "preview").then_some(true),
        ..FilePaneTab::new(id, path, cwd)
    })
}

/// `sanitizeRemoteFile`, reduced to whether it is valid: the parsed value
/// itself was never stored.
fn is_remote_file(raw: &Value) -> bool {
    raw.as_object().is_some_and(|value| {
        nonempty_str(value, "machineId").is_some()
            && nonempty_str(value, "projectId").is_some()
            && nonempty_str(value, "relativePath").is_some()
    })
}

/// `sanitizeSessionChanges`.
fn sanitize_session_changes(raw: &Value) -> Option<SessionChangesSource> {
    let value = raw.as_object()?;
    trimmed_str(value, "sessionId").map(SessionChangesSource::new)
}

/// `sanitizeCommit`.
fn sanitize_commit(raw: &Value) -> Option<CommitTabSource> {
    let value = raw.as_object()?;
    let sha = trimmed_str(value, "sha")?;
    let short_sha = trimmed_str(value, "shortSha")?;
    let subject = str_field(value, "subject")?;
    Some(CommitTabSource::new(sha, short_sha, subject))
}

/// `sanitizeReleaseNotes`.
fn sanitize_release_notes(raw: &Value) -> Option<ReleaseNotesTabSource> {
    let value = raw.as_object()?;
    trimmed_str(value, "version").map(ReleaseNotesTabSource::new)
}

/// `sanitizeProjectTerminal`.
pub fn sanitize_project_terminal(raw: &Value) -> Option<ProjectTerminalDock> {
    let value = raw.as_object()?;
    let project_path =
        str_field(value, "projectPath").filter(|path| !core_js::trim(path).is_empty())?;
    let side = is_dock_side(value.get("side"))?;
    let pane = sanitize_pane(value.get("pane")?)?;
    let files: Vec<FilePaneTab> = pane
        .files
        .iter()
        .filter(|file| is_terminal_tab(file))
        .cloned()
        .collect();
    let first = files.first()?.id.clone();
    let active_file_id = if files.iter().any(|file| file.id == pane.active_file_id) {
        pane.active_file_id.clone()
    } else {
        first
    };
    Some(ProjectTerminalDock {
        project_path: normalize_project_path(project_path),
        pane: EditorPane {
            files,
            active_file_id,
            ..pane
        },
        side,
        size: clamp_dock_size(side, number(value.get("size")), None),
        open: value.get("open") != Some(&Value::Bool(false)),
        extra: Extra::new(),
    })
}

/// `sanitizePlan`.
fn sanitize_plan(raw: &Value) -> Option<PlanTabSource> {
    let value = raw.as_object()?;
    Some(PlanTabSource {
        session_id: nonempty_str(value, "sessionId")?.to_string(),
        block_id: nonempty_str(value, "blockId")?.to_string(),
        title: str_field(value, "title")?.to_string(),
        extra: Extra::new(),
    })
}

/// The `extra` keys a snapshot carries, as `scope.key`, so a caller can see
/// what fell outside the modeled shape.
pub fn unknown_fields(snapshot: &WorkspaceSnapshot) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut note = |scope: &str, extra: &Map<String, Value>| {
        out.extend(extra.keys().map(|key| format!("{scope}.{key}")));
    };
    note("snapshot", &snapshot.extra);
    for tab in &snapshot.tabs {
        note("tab", &tab.extra);
        note_layout(&tab.layout, &mut note);
        for pane in tab.editor_panes.iter().chain(&tab.terminal_panes) {
            note("pane", &pane.extra);
            for file in &pane.files {
                note_file(file, &mut note);
            }
        }
    }
    for stub in &snapshot.sessions {
        note("session", &stub.extra);
    }
    for dock in &snapshot.project_terminals {
        note("dock", &dock.extra);
        note("pane", &dock.pane.extra);
        for file in &dock.pane.files {
            note_file(file, &mut note);
        }
    }
    for target in snapshot.project_return_targets.iter().flatten() {
        note("projectReturnTarget", &target.extra);
    }
    out.sort();
    out.dedup();
    out
}

fn note_layout(node: &LayoutNode, note: &mut impl FnMut(&str, &Map<String, Value>)) {
    match node {
        LayoutNode::Leaf(leaf) => note("leaf", &leaf.extra),
        LayoutNode::Split(split) => {
            note("split", &split.extra);
            for child in &split.children {
                note_layout(child, note);
            }
        }
    }
}

fn note_file(file: &FilePaneTab, note: &mut impl FnMut(&str, &Map<String, Value>)) {
    note("file", &file.extra);
    if let Some(plan) = &file.plan {
        note("plan", &plan.extra);
    }
    if let Some(release_notes) = &file.release_notes {
        note("releaseNotes", &release_notes.extra);
    }
    if let Some(commit) = &file.commit {
        note("commit", &commit.extra);
    }
    if let Some(session_changes) = &file.session_changes {
        note("sessionChanges", &session_changes.extra);
    }
    if let Some(agent) = &file.agent {
        note("agent", &agent.extra);
    }
}

#[cfg(test)]
#[path = "workspace_snapshot_tests.rs"]
mod tests;
