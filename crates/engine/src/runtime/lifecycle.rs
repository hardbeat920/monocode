//! Port of src/app/model/appLifecycle.ts plus the close, unload, and quit
//! handlers in App.tsx (lines 1332-1363 and 1751-1800): boot restore, the
//! quit state, and what a quit or a window close saves first.
//!
//! The TypeScript ran one copy per webview and a Rust coordinator summed the
//! windows' answers. One native process holds every window, so the
//! coordinator's poll, decision, and ready messages become return values:
//! `in_flight_count` is the poll reply, `ask_quit_confirmation` is the
//! decision, and `commit_quit` resolves to "persisted".

use std::collections::{HashMap, HashSet};

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AppContext, Context, EventEmitter, Task};
use monocode_core::Session;
use monocode_core::platform::Platform;
use serde_json::Value;

use super::engine::Engine;
use super::hooks::EngineHooks;
use super::in_flight::{
    ResumedWorkspace, has_in_flight_sessions, in_flight_refs, is_in_flight_session,
    mark_turn_interrupted, quit_while_busy_message, was_turn_interrupted, workspace_from_resumed,
};
use super::session_store::{InFlightRef, SessionSummary, should_persist_session};
use super::sessions::get_stored_session;
use super::util::project_path::{normalize_project_path, same_project_path};
use super::window_transfer::PendingWindowTransfer;

/// `BootWorkspace`.
#[derive(Debug, Clone, PartialEq)]
pub struct BootWorkspace {
    /// The window transfer this window opened with, as JSON.
    pub window_transfer: Option<Value>,
    pub resumed: Option<ResumedWorkspace>,
    /// Sidebar rows listed before first paint, so the rail is not empty.
    pub history: Vec<SessionSummary>,
    pub history_cwd: Option<String>,
}

/// How `persist_quit_state` treats failed writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitMode {
    /// The process ends, so a failed write is work that never comes back:
    /// report it so the caller can call the quit off.
    Quit,
    /// A reload or a tray close: best effort is enough.
    Unload,
}

/// Lifecycle changes views react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    QuitStarted,
    QuitAborted,
}

type SharedResume = Shared<Task<Option<ResumedWorkspace>>>;
type SharedBoot = Shared<Task<BootWorkspace>>;

/// Boot restore and quit state (`quitting`, `quitDialogOpen`,
/// `bootingResumed`, and whether a live workspace is attached).
pub struct Lifecycle {
    quitting: bool,
    quit_dialog_open: bool,
    booting_resumed: Option<ResumedWorkspace>,
    resumed: Option<SharedResume>,
    boot: Option<SharedBoot>,
    live: bool,
    transfer: PendingWindowTransfer,
    _quit: Option<gpui::Subscription>,
}

impl EventEmitter<LifecycleEvent> for Lifecycle {}

impl Lifecycle {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // The pagehide and beforeunload handler: an app quit that did not go
        // through the quit flow still saves this window.
        let quit = cx.on_app_quit(|this: &mut Self, cx| this.handle_unload(cx));
        Self {
            quitting: false,
            quit_dialog_open: false,
            booting_resumed: None,
            resumed: None,
            boot: None,
            live: false,
            transfer: PendingWindowTransfer::default(),
            _quit: Some(quit),
        }
    }

    /// `isAppQuitting`.
    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// `setQuitWorkspace`: the workspace is live, so quits read the open
    /// sessions and tabs instead of the boot restore.
    pub fn attach_workspace(&mut self) {
        self.live = true;
        self.booting_resumed = None;
    }

    /// The release function `setQuitWorkspace` returned.
    pub fn detach_workspace(&mut self) {
        self.live = false;
    }

    pub fn workspace_attached(&self) -> bool {
        self.live
    }

    /// Hand a window transfer to the next window that boots.
    pub fn put_window_transfer(&mut self, payload: Value) {
        self.transfer.put(payload);
    }

    /// `loadWindowTransfer`: take the pending transfer, once.
    pub fn take_window_transfer(&mut self) -> Option<Value> {
        self.transfer.take()
    }

    /// `handleQuitRequested` and `commitQuit`: save this workspace for a
    /// quit the user already confirmed. Resolves to whether everything
    /// saved; on failure the quit is off.
    pub fn handle_quit_requested(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        if self.live {
            let sessions = Engine::sessions(cx);
            sessions.update(cx, |sessions, cx| sessions.flush(cx));
            self.quitting = true;
            cx.emit(LifecycleEvent::QuitStarted);
            let snapshot = sessions.read(cx).all().to_vec();
            let persist = persist_quit_state(snapshot, QuitMode::Quit, cx);
            return cx.spawn(async move |this, cx| {
                let saved = persist.await.is_ok();
                if !saved {
                    this.update(cx, |this, cx| this.abort_quit(cx)).ok();
                }
                saved
            });
        }
        let boot = self.load_boot_workspace(None, cx);
        cx.spawn(async move |this, cx| {
            let boot = boot.await;
            let pending = this
                .update(cx, |this, cx| {
                    this.quitting = true;
                    cx.emit(LifecycleEvent::QuitStarted);
                    boot.resumed
                        .clone()
                        .or_else(|| this.booting_resumed.clone())
                })
                .ok()
                .flatten();
            let Some(pending) = pending else {
                return true;
            };
            let persist = cx.update(|cx| persist_booting_resume(pending, cx));
            let saved = persist.await.is_ok();
            if !saved {
                this.update(cx, |this, cx| this.abort_quit(cx)).ok();
            }
            saved
        })
    }

    /// `commitQuit`: the same as `handle_quit_requested`. The result is the
    /// `persisted` flag the coordinator received.
    pub fn commit_quit(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        self.handle_quit_requested(cx)
    }

    /// `reportQuitPoll`: every running turn in this workspace, not just the
    /// resumable ones. An Inbox Ask still counts as work nobody agreed to
    /// throw away.
    pub fn in_flight_count(&mut self, cx: &mut Context<Self>) -> usize {
        if !self.live {
            return 0;
        }
        let sessions = Engine::sessions(cx);
        sessions.update(cx, |sessions, cx| sessions.flush(cx));
        sessions
            .read(cx)
            .all()
            .iter()
            .filter(|session| is_in_flight_session(session))
            .count()
    }

    /// `askQuitConfirmation`: the one quit dialog. Resolves to the user's
    /// decision; a second request while it is open is a refusal.
    pub fn ask_quit_confirmation(
        &mut self,
        in_flight: usize,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if self.quit_dialog_open {
            return Task::ready(false);
        }
        self.quit_dialog_open = true;
        let ask =
            Engine::hooks(cx)
                .workspace
                .confirm(&quit_while_busy_message(in_flight), "Quit", cx);
        cx.spawn(async move |this, cx| {
            let confirmed = ask.await;
            this.update(cx, |this, _| this.quit_dialog_open = false)
                .ok();
            confirmed
        })
    }

    /// `abortQuit`: a quit stopped part way, so this workspace goes back to
    /// saving on unload.
    pub fn abort_quit(&mut self, cx: &mut Context<Self>) {
        self.quitting = false;
        cx.emit(LifecycleEvent::QuitAborted);
    }

    /// `closeBusyWindow`: confirm, then stop this window's work and close it
    /// without quitting the app.
    pub fn close_busy_window(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if !self.live {
            return Task::ready(());
        }
        let sessions = Engine::sessions(cx);
        sessions.update(cx, |sessions, cx| sessions.flush(cx));
        self.confirm_and_close_window(cx)
    }

    /// `confirmAndCloseWindow`.
    fn confirm_and_close_window(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.quit_dialog_open {
            return Task::ready(());
        }
        self.quit_dialog_open = true;
        let hooks = Engine::hooks(cx);
        let sessions = Engine::sessions(cx).read(cx).all().to_vec();
        let refs = in_flight_refs(&sessions, &hooks.workspace.tab_session_ids(cx));
        let ask = if refs.is_empty() {
            Task::ready(true)
        } else {
            hooks.workspace.confirm(
                "Close this window and stop its running chats? Other windows will stay open.",
                "Close window",
                cx,
            )
        };
        cx.spawn(async move |this, cx| {
            if ask.await {
                let started = this
                    .update(cx, |this, cx| {
                        this.quitting = true;
                        cx.emit(LifecycleEvent::QuitStarted);
                    })
                    .is_ok();
                if started {
                    let persist =
                        cx.update(|cx| persist_quit_state(sessions.clone(), QuitMode::Quit, cx));
                    if persist.await.is_ok() {
                        let reap = cx.update(|cx| reap_window_runtime(&sessions, false, cx));
                        reap.await;
                        cx.update(|cx| Engine::hooks(cx).workspace.close_window(cx));
                    } else {
                        this.update(cx, |this, cx| this.abort_quit(cx)).ok();
                    }
                }
            }
            this.update(cx, |this, _| this.quit_dialog_open = false)
                .ok();
        })
    }

    /// The window close button (`onCloseRequested`). Busy chats keep
    /// running behind a hidden window on macOS or with close-to-tray; an
    /// idle window saves and closes or hides.
    pub fn handle_close_requested(
        &mut self,
        close_to_tray: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let hooks = Engine::hooks(cx);
        let sessions_entity = Engine::sessions(cx);
        let busy = has_in_flight_sessions(sessions_entity.read(cx).all());
        if busy {
            sessions_entity.update(cx, |sessions, cx| sessions.flush(cx));
            if !close_to_tray && !Platform::current().is_mac() {
                return self.close_busy_window(cx);
            }
            // Not `persist_quit_state`: that marks the live turns interrupted.
            let sessions = sessions_entity.read(cx).all().to_vec();
            let persist = persist_live_transcripts(&sessions, cx);
            hooks.workspace.hide_window(cx);
            return cx.spawn(async move |_, _| persist.await);
        }
        let sessions = sessions_entity.read(cx).all().to_vec();
        let persist = persist_quit_state(sessions, QuitMode::Unload, cx);
        cx.spawn(async move |_, cx| {
            let _ = persist.await;
            cx.update(|cx| {
                if close_to_tray {
                    hooks.workspace.hide_window(cx);
                } else {
                    hooks.workspace.close_window(cx);
                }
            });
        })
    }

    /// The pagehide and beforeunload handler: save the workspace as an
    /// unload, then drop this window's children. Skipped during a quit,
    /// which saved already.
    pub fn handle_unload(&mut self, cx: &mut Context<Self>) -> Task<()> {
        if self.quitting || Engine::try_global(cx).is_none() {
            return Task::ready(());
        }
        let sessions = Engine::sessions(cx).read(cx).all().to_vec();
        let persist = persist_quit_state(sessions, QuitMode::Unload, cx);
        cx.spawn(async move |_, cx| {
            let _ = persist.await;
            let reap = cx.update(|cx| {
                let sessions = Engine::sessions(cx).read(cx).all().to_vec();
                reap_window_runtime(&sessions, true, cx)
            });
            reap.await;
        })
    }

    /// `loadResumedWorkspace`: restore runs once; callers share it.
    pub fn load_resumed_workspace(&mut self, cx: &mut Context<Self>) -> SharedResume {
        if let Some(resumed) = self.resumed.as_ref() {
            return resumed.clone();
        }
        let load = load_resumed_workspace_once(cx);
        let task = cx
            .spawn(async move |this, cx| {
                let (workspace, interrupted) = load.await;
                this.update(cx, |this, _| this.booting_resumed = workspace.clone())
                    .ok();
                if let Some(workspace) = workspace.as_ref() {
                    let saves = cx.update(|cx| save_interrupted(workspace, &interrupted, cx));
                    futures::future::join_all(saves).await;
                }
                workspace
            })
            .shared();
        self.resumed = Some(task.clone());
        task
    }

    /// `loadBootWorkspace`: the window transfer or the restored workspace,
    /// plus the sidebar rows for its project. `hinted_cwd` is the last
    /// project path (`lastProjectPath`). Runs once; callers share it.
    pub fn load_boot_workspace(
        &mut self,
        hinted_cwd: Option<String>,
        cx: &mut Context<Self>,
    ) -> SharedBoot {
        if let Some(boot) = self.boot.as_ref() {
            return boot.clone();
        }
        let history_hint = list_project_history(hinted_cwd.as_deref(), cx);
        let transfer = self.take_window_transfer();
        let task = if let Some(transfer) = transfer {
            let cwd = transfer
                .get("projectCwd")
                .and_then(Value::as_str)
                .map(str::to_string);
            let listed = history_for_cwd(cwd.as_deref(), hinted_cwd.as_deref(), history_hint, cx);
            cx.spawn(async move |_, _| {
                let listed = listed.await;
                BootWorkspace {
                    window_transfer: Some(transfer),
                    resumed: None,
                    history: listed
                        .as_ref()
                        .map(|(_, rows)| rows.clone())
                        .unwrap_or_default(),
                    history_cwd: listed.map(|(cwd, _)| cwd),
                }
            })
        } else {
            let resumed = self.load_resumed_workspace(cx);
            cx.spawn(async move |_, cx| {
                let (resumed, hinted) = futures::future::join(resumed, history_hint).await;
                let cwd = resumed
                    .as_ref()
                    .map(|workspace| workspace.project_cwd.clone())
                    .or_else(|| hinted_cwd.clone());
                let listed = cx
                    .update(|cx| {
                        history_for_cwd(
                            cwd.as_deref(),
                            hinted_cwd.as_deref(),
                            Task::ready(hinted),
                            cx,
                        )
                    })
                    .await;
                BootWorkspace {
                    window_transfer: None,
                    resumed,
                    history: listed
                        .as_ref()
                        .map(|(_, rows)| rows.clone())
                        .unwrap_or_default(),
                    history_cwd: listed.map(|(cwd, _)| cwd),
                }
            })
        };
        let shared = task.shared();
        self.boot = Some(shared.clone());
        shared
    }

    /// `confirmReload`: ask before a reload discards unsaved files.
    pub fn confirm_reload(has_unsaved_files: bool, cx: &mut App) -> Task<bool> {
        if !has_unsaved_files {
            return Task::ready(true);
        }
        Engine::hooks(cx).workspace.confirm(
            "Reload MonoCode and discard unsaved changes?",
            "Reload",
            cx,
        )
    }
}

/// Saves for the restored sessions whose turn a quit interrupted. Idle
/// transcripts already came from disk; rewriting every open chat here
/// serialized the entire workspace before first paint.
fn save_interrupted(
    workspace: &ResumedWorkspace,
    interrupted: &HashSet<String>,
    cx: &mut App,
) -> Vec<Task<()>> {
    let writer = Engine::writer(cx);
    workspace
        .sessions
        .iter()
        .filter(|session| interrupted.contains(&session.id) && should_persist_session(session))
        .map(|session| {
            let upsert = writer.upsert_session(session);
            cx.background_spawn(async move {
                let _ = upsert.await;
            })
        })
        .collect()
}

/// `loadResumedWorkspaceOnce` without the saves: read the snapshot and the
/// quit list, load every session they name, and rebuild the workspace.
/// Also returns the ids the quit list marked interrupted.
fn load_resumed_workspace_once(cx: &mut App) -> Task<(Option<ResumedWorkspace>, HashSet<String>)> {
    let writer = Engine::writer(cx);
    let snapshot = writer.load_workspace_snapshot();
    let in_flight = writer.list_in_flight_sessions();
    let hooks = Engine::hooks(cx);
    cx.spawn(async move |cx| {
        let (snapshot, refs) = futures::future::join(snapshot, in_flight).await;
        let snapshot = snapshot.ok().flatten();
        let refs: Vec<InFlightRef> = refs.unwrap_or_default();
        let interrupted: HashSet<String> =
            refs.iter().map(|entry| entry.session_id.clone()).collect();
        let mut ids: Vec<String> = Vec::new();
        if let Some(snapshot) = snapshot.as_ref() {
            ids.extend(hooks.workspace.snapshot_session_ids(snapshot));
        }
        ids.extend(refs.iter().map(|entry| entry.session_id.clone()));
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(id.clone()));
        let loads: Vec<Task<Option<Session>>> =
            cx.update(|cx| ids.iter().map(|id| get_stored_session(id, cx)).collect());
        let loaded: HashMap<String, Session> = futures::future::join_all(loads)
            .await
            .into_iter()
            .flatten()
            .map(|session| (session.id.clone(), session))
            .collect();
        let mut workspace = snapshot.as_ref().and_then(|snapshot| {
            hooks
                .workspace
                .hydrate_snapshot(snapshot, &loaded, &interrupted)
        });
        if workspace.is_none() && !refs.is_empty() {
            let sessions = refs
                .iter()
                .filter_map(|entry| loaded.get(&entry.session_id))
                .map(mark_turn_interrupted)
                .collect();
            workspace =
                workspace_from_resumed(sessions, |ids| hooks.workspace.layout_for_sessions(ids));
        }
        (workspace, interrupted)
    })
}

fn list_project_history(
    cwd: Option<&str>,
    cx: &App,
) -> Task<Option<(String, Vec<SessionSummary>)>> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty() && *cwd != "~") else {
        return Task::ready(None);
    };
    let list = Engine::writer(cx).list_sessions_by_project(cwd);
    let key = normalize_project_path(cwd);
    cx.background_spawn(async move { list.await.ok().map(|rows| (key, rows)) })
}

fn history_for_cwd(
    cwd: Option<&str>,
    hinted_cwd: Option<&str>,
    hinted: Task<Option<(String, Vec<SessionSummary>)>>,
    cx: &App,
) -> Task<Option<(String, Vec<SessionSummary>)>> {
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty() && *cwd != "~") else {
        return Task::ready(None);
    };
    if hinted_cwd.is_some_and(|hinted| same_project_path(cwd, hinted)) {
        return hinted;
    }
    list_project_history(Some(cwd), cx)
}

/// `persistLiveTranscripts`: save every open transcript as it stands.
pub fn persist_live_transcripts(sessions: &[Session], cx: &App) -> Task<()> {
    let writer = Engine::writer(cx);
    let saves: Vec<_> = sessions
        .iter()
        .filter(|session| should_persist_session(session))
        .map(|session| writer.upsert_session(session))
        .collect();
    cx.background_spawn(async move {
        futures::future::join_all(saves).await;
    })
}

/// `persistQuitState`: save every transcript (interrupted turns marked),
/// the workspace snapshot, and the quit list. A quit reports the first
/// failed write; an unload ignores failures.
pub fn persist_quit_state(
    sessions: Vec<Session>,
    mode: QuitMode,
    cx: &App,
) -> Task<Result<(), String>> {
    let hooks: EngineHooks = Engine::hooks(cx);
    let writer = Engine::writer(cx);
    let refs = in_flight_refs(&sessions, &hooks.workspace.tab_session_ids(cx));
    let interrupted: HashSet<&str> = refs.iter().map(|entry| entry.session_id.as_str()).collect();
    let saves: Vec<_> = sessions
        .iter()
        .filter(|session| should_persist_session(session))
        .map(|session| {
            if interrupted.contains(session.id.as_str()) {
                writer.upsert_session(&mark_turn_interrupted(session))
            } else {
                writer.upsert_session(session)
            }
        })
        .collect();
    let snapshot = hooks.workspace.collect_snapshot(&sessions, cx);
    cx.background_spawn(async move {
        let check = |result: Result<(), String>| match mode {
            QuitMode::Quit => result,
            QuitMode::Unload => Ok(()),
        };
        for result in futures::future::join_all(saves).await {
            check(result.map(|_| ()))?;
        }
        if let Some(snapshot) = snapshot {
            check(writer.save_workspace_snapshot(snapshot).await)?;
        }
        // A reload must not wipe a restored snapshot: those chats are idle in
        // this process until Continue runs.
        if mode == QuitMode::Quit || !refs.is_empty() {
            check(writer.replace_in_flight_sessions(refs).await)?;
        }
        Ok(())
    })
}

/// `persistBootingResume`: a quit before the workspace attached saves the
/// restored workspace as it was loaded.
pub fn persist_booting_resume(workspace: ResumedWorkspace, cx: &App) -> Task<Result<(), String>> {
    let hooks = Engine::hooks(cx);
    let writer = Engine::writer(cx);
    let saves: Vec<_> = workspace
        .sessions
        .iter()
        .filter(|session| should_persist_session(session))
        .map(|session| writer.upsert_session(session))
        .collect();
    let snapshot = hooks.workspace.collect_resumed_snapshot(&workspace);
    let refs: Vec<InFlightRef> = workspace
        .sessions
        .iter()
        .filter(|session| was_turn_interrupted(session))
        .map(|session| InFlightRef {
            session_id: session.id.clone(),
            cwd: session.cwd.clone(),
        })
        .collect();
    cx.background_spawn(async move {
        futures::future::join_all(saves).await;
        if let Some(snapshot) = snapshot {
            let _ = writer.save_workspace_snapshot(snapshot).await;
        }
        let _ = writer.replace_in_flight_sessions(refs).await;
        Ok(())
    })
}

/// `reapWindowRuntime`: drop every session child and terminal of this
/// window, and with `include_all_children` the catalog probes, title
/// generators, and usage scrapers too.
pub fn reap_window_runtime(
    sessions: &[Session],
    include_all_children: bool,
    cx: &mut App,
) -> Task<()> {
    let hooks = Engine::hooks(cx);
    let mut forgets = Vec::new();
    for session in sessions {
        for harness in hooks.harness.session_child_harnesses(session) {
            forgets.push(hooks.harness.forget_session(harness, &session.id, cx));
        }
    }
    let terminals = hooks.workspace.kill_terminals(cx);
    let children = include_all_children.then(|| hooks.harness.kill_all_children(cx));
    cx.background_spawn(async move {
        futures::future::join_all(forgets).await;
        terminals.await;
        if let Some(children) = children {
            children.await;
        }
    })
}
