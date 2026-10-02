//! Calls from the runtime into the other engine packages.
//!
//! The TypeScript app had call cycles between submit, the orchestrator, the
//! message queue, and remote. Here each package fills in the hook trait it
//! owns, and every method has a no-op default, so `runtime` builds and runs
//! with no other package present.
//!
//! Rules for implementations:
//! - The runtime calls most hooks while it holds the `Sessions` entity, so a
//!   hook must not read or update `Sessions`. The runtime passes what it knows
//!   as arguments instead. Methods documented as "called outside `Sessions`"
//!   may touch it.
//! - Hooks that return a `Task` run their work themselves and return
//!   `Task::ready(..)` when there is nothing to do.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{App, Task};
use monocode_core::{HarnessEvent, HarnessId, ModelSettings, Session};
use serde_json::Value;

use super::in_flight::ResumedWorkspace;

/// Every hook the runtime calls, one trait object per owning package.
#[derive(Clone)]
pub struct EngineHooks {
    pub harness: Rc<dyn HarnessHooks>,
    pub submit: Rc<dyn SubmitHooks>,
    pub attention: Rc<dyn AttentionHooks>,
    pub side_threads: Rc<dyn SideThreadHooks>,
    pub orchestration: Rc<dyn OrchestrationHooks>,
    pub workspace: Rc<dyn WorkspaceHooks>,
    pub remote: Rc<dyn RemoteHooks>,
}

impl Default for EngineHooks {
    fn default() -> Self {
        let noop = Rc::new(NoopHooks);
        Self {
            harness: noop.clone(),
            submit: noop.clone(),
            attention: noop.clone(),
            side_threads: noop.clone(),
            orchestration: noop.clone(),
            workspace: noop.clone(),
            remote: noop,
        }
    }
}

/// The default for every hook trait: does nothing.
pub struct NoopHooks;

impl HarnessHooks for NoopHooks {}
impl SubmitHooks for NoopHooks {}
impl AttentionHooks for NoopHooks {}
impl SideThreadHooks for NoopHooks {}
impl OrchestrationHooks for NoopHooks {}
impl WorkspaceHooks for NoopHooks {}
impl RemoteHooks for NoopHooks {}

/// A session after the provider-specific repairs that run when a stored
/// session loads (Cursor subagents, OMP interjections).
pub struct RecoveredSession {
    pub session: Session,
    /// The repair changed the transcript and the store should get the new copy.
    pub persist: bool,
}

/// The harness registry (`integrations/harness/core/registry.ts` and
/// `child.ts`). Filled in by the package that owns the harness bridge.
pub trait HarnessHooks {
    /// `isLiveHarness`: the harness keeps a child process between turns.
    fn is_live_harness(&self, _harness: HarnessId) -> bool {
        false
    }

    /// `bindHarnessSession`: attach a stored provider thread to a session id
    /// so the next turn resumes it. The session carries the provider session
    /// id, account, work cwd, and blocks.
    fn bind_session(&self, _session: &Session, _cx: &mut App) {}

    /// `forgetHarnessSession`: drop the child for this session.
    fn forget_session(&self, _harness: HarnessId, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `cancelHarnessTurn`.
    fn cancel_turn(&self, _harness: HarnessId, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `killAllChildren`: catalog probes, title generators, and usage
    /// scrapers that are not session children.
    fn kill_all_children(&self, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `sessionChildHarnesses` from handoff.ts: every harness that may still
    /// hold a child for this session. The default ports it as written.
    fn session_child_harnesses(&self, session: &Session) -> Vec<HarnessId> {
        super::reducer::session_child_harnesses(session)
    }

    /// Provider repairs that run after a stored session loads:
    /// `recoverCursorSubagents` and the OMP interjection backfill.
    fn recover_loaded_session(&self, session: Session, _cx: &mut App) -> Task<RecoveredSession> {
        Task::ready(RecoveredSession {
            session,
            persist: false,
        })
    }

    /// `probeHarnessAvailability` at boot.
    fn probe_availability(&self, _cx: &mut App) {}

    /// `refreshHarnessCatalogs` for the harnesses already in the window.
    fn refresh_catalogs(&self, _harnesses: Vec<HarnessId>, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `resolveModel` plus `mergeModelSettings` against the live catalog.
    /// `None` keeps the session's model as it is.
    fn resolve_model(&self, _session: &Session, _cx: &App) -> Option<(String, ModelSettings)> {
        None
    }
}

/// The submit pipeline and message queue.
pub trait SubmitHooks {
    /// Harness events landed for these sessions. Called outside `Sessions`,
    /// after the flush, so the queue can dispatch follow-ups.
    fn sessions_flushed(&self, _session_ids: &[String], _cx: &mut App) {}
}

/// Notifications, the dock badge, and the done markers.
pub trait AttentionHooks {
    /// `syncDockBadge` with the session list after events applied.
    fn sync_dock_badge(&self, _sessions: &[Session], _cx: &mut App) {}

    /// Sessions that finished while unfocused and are still unseen.
    fn unseen_finished_ids(&self, _cx: &App) -> HashSet<String> {
        HashSet::new()
    }

    /// The "live agents" setting: keep unseen finished chats attached.
    fn live_agents_enabled(&self, _cx: &App) -> bool {
        true
    }
}

/// By-the-way side threads.
pub trait SideThreadHooks {
    /// Sessions changed. Abort side-thread requests whose session is no
    /// longer open.
    fn sessions_closed(&self, _live_ids: &HashSet<String>, _cx: &mut App) {}

    /// The engine is shutting down: abort every request and
    /// `stopHarnessTextPrompts`.
    fn stop_all(&self, _cx: &mut App) {}
}

/// The orchestrator.
pub trait OrchestrationHooks {
    /// `orchestrator.stopForSession` before a session is removed.
    fn stop_for_session(&self, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// Lead ids of runs that are active or paused. Their workers stay
    /// attached while idle.
    fn running_lead_ids(&self, _cx: &App) -> HashSet<String> {
        HashSet::new()
    }
}

/// Tabs, panes, windows, terminals, and file watchers.
pub trait WorkspaceHooks {
    /// The window is hidden or minimized (`document.hidden`).
    fn window_hidden(&self, _cx: &App) -> bool {
        false
    }

    /// The user can see this session now: it is a pane of the active tab or
    /// the agent behind its active file, or the open Inbox Ask, and no full
    /// page covers the workspace. Output for a foreground session flushes on
    /// the next frame instead of the background cadence.
    fn is_foreground(&self, _session_id: &str, _cx: &App) -> bool {
        true
    }

    /// Session ids of every tab's panes, in tab order and then pane order
    /// (`leafIds` over all tabs). The runtime uses it for visibility and for
    /// the order of the in-flight snapshot.
    fn tab_session_ids(&self, _cx: &App) -> Vec<String> {
        Vec::new()
    }

    /// `collectWorkspaceSnapshot` for the live workspace with these sessions.
    /// `None` skips the snapshot write.
    fn collect_snapshot(&self, _sessions: &[Session], _cx: &App) -> Option<Value> {
        None
    }

    /// `collectWorkspaceSnapshot` for a restored workspace the window has not
    /// adopted yet.
    fn collect_resumed_snapshot(&self, _workspace: &ResumedWorkspace) -> Option<Value> {
        None
    }

    /// `workspaceSnapshotKey`: two snapshots with the same key are the same save.
    fn snapshot_key(&self, snapshot: &Value) -> String {
        snapshot.to_string()
    }

    /// Every session id a stored snapshot references: its session stubs and
    /// every tab's panes. Empty when the snapshot does not parse.
    fn snapshot_session_ids(&self, _snapshot: &Value) -> Vec<String> {
        Vec::new()
    }

    /// `parseWorkspaceSnapshot` plus `hydrateWorkspaceSnapshot`.
    fn hydrate_snapshot(
        &self,
        _snapshot: &Value,
        _loaded: &HashMap<String, Session>,
        _interrupted: &HashSet<String>,
    ) -> Option<ResumedWorkspace> {
        None
    }

    /// The layout part of `workspaceFromResumed`: one new tab per session, in
    /// the snapshot's JSON shape, with the first tab active.
    fn layout_for_sessions(&self, _session_ids: &[String]) -> Value {
        Value::Null
    }

    /// Ask the user to confirm (`ask` from the dialog plugin).
    fn confirm(&self, _message: &str, _ok_label: &str, _cx: &mut App) -> Task<bool> {
        Task::ready(true)
    }

    /// `hide_window`.
    fn hide_window(&self, _cx: &mut App) {}

    /// `destroy_window`: close this window without asking again.
    fn close_window(&self, _cx: &mut App) {}

    /// Kill the PTY behind every terminal file in the tabs and project docks.
    fn kill_terminals(&self, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `resolveWorkspacePath`: an agent-reported path made absolute against
    /// the session's work cwd.
    fn resolve_workspace_path(&self, _path: &str, _cwd: &str) -> Option<String> {
        None
    }

    /// `nudgeWatchedFiles`: re-check these open files (all when `None`).
    fn nudge_watched_files(&self, _paths: Option<&[String]>, _cx: &mut App) {}

    /// `invalidateWatchedFiles`: reload these open files even when the mtime
    /// looks unchanged (all when `None`).
    fn invalidate_watched_files(&self, _paths: Option<&[String]>, _cx: &mut App) {}

    /// `notifyGitChanged`.
    fn notify_git_changed(&self, _cx: &mut App) {}

    /// `nudgeWorkspace`: `invalidateProjectFiles(cwd)` and `notifyDirsChanged`.
    fn nudge_workspace(&self, _cwd: Option<&str>, _cx: &mut App) {}
}

/// Remote hosts and the headless host mode.
pub trait RemoteHooks {
    /// Events applied to this session. The headless host forwards them to
    /// connected desktops. Called outside `Sessions`.
    fn session_events(&self, _session_id: &str, _events: &[HarnessEvent], _cx: &mut App) {}
}
