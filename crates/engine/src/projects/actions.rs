//! The project flows from src/app/App.tsx: switching a session's folder,
//! branch, or working copy (lines 4913-5170), deleting a worktree (lines
//! 4283-4429), opening, restoring, and removing projects (lines 5172-5418),
//! following a renamed folder (lines 5438-5502), and the git calls in
//! worktrees.ts. `onManageWorktrees` (line 10445) opens Settings, which the
//! view does.
//!
//! These are free functions over `&mut App`, not `Projects` methods,
//! because they call hooks that may update `Projects` and `Sessions`.
//! Neither entity is held while a hook runs.

use std::collections::HashSet;

use gpui::{App, AppContext, Context, Entity, Task};
use monocode_core::paths::path_key;
use monocode_core::session::{
    MessageQueueStatus, WorkspaceMode, new_default_session, new_session,
    retarget_session_to_project, session_work_cwd,
};
use monocode_core::{HarnessId, Session};
use monocode_layout::layout::{WorkspaceTab, new_tab};
use monocode_layout::project_return::is_blank_session;
use monocode_layout::workspace_tab_groups::filter_tabs_for_project;

use super::backend::{Worktree, WorktreeRemoval, Worktrees};
use super::hooks::{ProjectsHooks, SessionFolderTarget};
use super::project_open_run::{ProjectOpenStep, plan_project_open_run};
use super::recents::{
    is_remote_project_path, looks_like_project, normalize_project_path, same_project_path,
};
use super::worktrees::{
    assert_worktree_files_closed, detach_session_worktree, detach_summary_worktree,
    is_equal_or_inside, session_in_worktree, worktree_session_ids,
};
use super::{ProjectsGlobal, notify_git_changed, notify_review_changed, project_data};
use crate::runtime::engine::Engine;
use crate::runtime::reducer::stop_streaming;
use crate::runtime::session_store::should_persist_session;
use crate::runtime::sessions::{Sessions, bind_resumed_sessions};

// Small helpers over the runtime.

fn sessions_entity(cx: &App) -> Option<Entity<Sessions>> {
    Engine::try_global(cx).map(|engine| engine.sessions.clone())
}

fn open_sessions(cx: &App) -> Vec<Session> {
    sessions_entity(cx)
        .map(|sessions| sessions.read(cx).all().to_vec())
        .unwrap_or_default()
}

fn find_session(session_id: &str, cx: &App) -> Option<Session> {
    sessions_entity(cx).and_then(|sessions| sessions.read(cx).get(session_id).cloned())
}

fn update_sessions<R>(
    cx: &mut App,
    update: impl FnOnce(&mut Sessions, &mut Context<Sessions>) -> R,
) -> Option<R> {
    let sessions = sessions_entity(cx)?;
    Some(sessions.update(cx, update))
}

fn hooks(cx: &App) -> std::rc::Rc<dyn ProjectsHooks> {
    ProjectsGlobal::hooks(cx)
}

/// `setRecents(rememberProject(path))`.
pub fn remember_project(path: &str, cx: &mut App) {
    if let Some(global) = ProjectsGlobal::try_global(cx) {
        let projects = global.projects.clone();
        projects.update(cx, |projects, cx| projects.remember_project(path, cx));
    }
}

fn backend(cx: &App) -> Option<std::sync::Arc<dyn super::ProjectsBackend>> {
    ProjectsGlobal::try_global(cx).map(|global| global.backend.clone())
}

/// `listWorktrees`.
pub fn list_worktrees(cwd: &str, cx: &App) -> Task<Result<Worktrees, String>> {
    let Some(backend) = backend(cx) else {
        return Task::ready(Err("Projects are not ready.".into()));
    };
    let cwd = cwd.to_string();
    cx.background_spawn(async move { backend.git_worktrees(&cwd) })
}

/// Run a blocking git call, then announce the change (`notifyGitChanged`).
fn git_then_notify<T: Send + 'static>(
    cx: &mut App,
    call: impl FnOnce(&dyn super::ProjectsBackend) -> Result<T, String> + Send + 'static,
) -> Task<Result<T, String>> {
    let Some(backend) = backend(cx) else {
        return Task::ready(Err("Projects are not ready.".into()));
    };
    let run = cx.background_spawn(async move { call(backend.as_ref()) });
    cx.spawn(async move |cx| {
        let value = run.await?;
        cx.update(notify_git_changed);
        Ok(value)
    })
}

/// `createWorktree`.
pub fn create_worktree(
    cwd: &str,
    branch: &str,
    base: &str,
    existing: bool,
    cx: &mut App,
) -> Task<Result<Worktree, String>> {
    let (cwd, branch, base) = (cwd.to_string(), branch.to_string(), base.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_worktree_create(&cwd, &branch, &base, existing)
    })
}

/// `createOrchestrationWorktree`.
pub fn create_orchestration_worktree(
    cwd: &str,
    branch: &str,
    cx: &mut App,
) -> Task<Result<Worktree, String>> {
    let (cwd, branch) = (cwd.to_string(), branch.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_orchestration_worktree_create(&cwd, &branch)
    })
}

/// `renameWorktreeBranch`.
pub fn rename_worktree_branch(
    cwd: &str,
    path: &str,
    branch: &str,
    cx: &mut App,
) -> Task<Result<Worktree, String>> {
    let (cwd, path, branch) = (cwd.to_string(), path.to_string(), branch.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_worktree_rename_branch(&cwd, &path, &branch)
    })
}

/// `removeWorktree`.
pub fn remove_worktree(
    cwd: &str,
    path: &str,
    force: bool,
    keep_sessions: bool,
    cx: &mut App,
) -> Task<Result<WorktreeRemoval, String>> {
    let (cwd, path) = (cwd.to_string(), path.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_worktree_remove(&cwd, &path, force, keep_sessions)
    })
}

/// `removeOrchestrationWorktree`.
pub fn remove_orchestration_worktree(
    cwd: &str,
    path: &str,
    cx: &mut App,
) -> Task<Result<WorktreeRemoval, String>> {
    let (cwd, path) = (cwd.to_string(), path.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_orchestration_worktree_remove(&cwd, &path)
    })
}

/// `removeOrchestrationBranch`.
pub fn remove_orchestration_branch(
    cwd: &str,
    branch: &str,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let (cwd, branch) = (cwd.to_string(), branch.to_string());
    git_then_notify(cx, move |backend| {
        backend.git_orchestration_branch_remove(&cwd, &branch)
    })
}

/// `checkWorktreeRemoval`: the read-only preflight. The removal itself
/// checks again for new blockers.
pub fn check_worktree_removal(
    cwd: &str,
    path: &str,
    force: bool,
    cx: &App,
) -> Task<Result<(), String>> {
    let Some(backend) = backend(cx) else {
        return Task::ready(Err("Projects are not ready.".into()));
    };
    let (cwd, path) = (cwd.to_string(), path.to_string());
    cx.background_spawn(async move { backend.git_worktree_check_remove(&cwd, &path, force) })
}

// Folder, branch, and working copy.

/// `onCwdChange`: the composer's folder picker moved a session to another
/// folder. A conversation stays with its project and opens a new tab
/// instead; a blank session moves and adopts the project's defaults.
pub fn on_cwd_change(session_id: &str, cwd: &str, cx: &mut App) {
    let hooks = hooks(cx);
    let normalized = normalize_project_path(cwd);
    let current = find_session(session_id, cx);
    let previous = current.as_ref().map(|session| session.cwd.clone());
    let inputs = ProjectsGlobal::model_inputs(cx);

    if let (Some(current), Some(previous)) = (&current, &previous)
        && !previous.is_empty()
        && looks_like_project(previous)
        && !same_project_path(previous, &normalized)
        && !is_blank_session(Some(current))
    {
        hooks.set_project_cwd(&normalized, cx);
        remember_project(&normalized, cx);
        let session = new_session(
            &inputs.env(),
            uuid::Uuid::new_v4().to_string(),
            current.harness,
            &normalized,
            Some(&current.model),
            Some(current.runtime_mode),
            Some(&current.model_settings),
        );
        let tab = new_tab(&session.id);
        let tab_id = tab.id.clone();
        update_sessions(cx, |sessions, cx| sessions.insert(session, cx));
        hooks.append_tab(tab, Some(&normalized), cx);
        hooks.set_active_tab(&tab_id, cx);
        hooks.set_composer_focused(true, cx);
        return;
    }

    if let Some(previous) = previous.as_ref().filter(|previous| {
        !previous.is_empty() && !same_project_path(previous, &normalized) && *previous != "~"
    }) && let Some(engine) = Engine::try_global(cx)
    {
        engine.checkpoints.keep(session_id, previous, None).detach();
    }
    hooks.set_project_cwd(&normalized, cx);
    remember_project(&normalized, cx);
    update_sessions(cx, |sessions, cx| {
        sessions.update(session_id, cx, |session| {
            // A blank session moving into a project adopts its provider
            // defaults; a conversation keeps its own provider.
            let mut next = if is_blank_session(Some(session)) {
                retarget_session_to_project(&inputs.env(), session, &normalized)
            } else {
                session.clone()
            };
            next.cwd = normalized.clone();
            next.branch = None;
            next.worktree_cwd = None;
            next.worktree_removed = None;
            next.workspace_mode = None;
            next.worktree_base = None;
            *session = next;
        });
    });
    // A group only holds tabs of one project, so the tab may have to leave.
    hooks.session_project_changed(session_id, &normalized, cx);
    notify_review_changed(session_id, cx);
}

/// `onBranchChange`: the branch picker checked out another branch. The
/// provider thread no longer matches the files, so it is dropped.
pub fn on_branch_change(session_id: &str, cx: &mut App) {
    notify_git_changed(cx);
    let Some(current) = find_session(session_id, cx) else {
        return;
    };
    Engine::hooks(cx)
        .harness
        .forget_session(current.harness, session_id, cx)
        .detach();
    update_sessions(cx, |sessions, cx| {
        sessions.update(session_id, cx, |session| {
            session.branch = None;
            session.provider_session_id = None;
            session.context = None;
        });
        sessions.persist(session_id, cx);
    });
    notify_review_changed(session_id, cx);
}

/// A session that may still change where it works: blank, or a worktree
/// session that has not made its working copy yet and is idle.
fn workspace_mode_editable(session: &Session) -> bool {
    is_blank_session(Some(session))
        || (session.workspace_mode.is_some()
            && session.worktree_cwd.as_deref().is_none_or(str::is_empty)
            && !session.is_busy())
}

/// `onWorkspaceModeChange`: run the next turn in the current folder or in
/// a new worktree from `base`.
pub fn on_workspace_mode_change(
    session_id: &str,
    mode: WorkspaceMode,
    base: Option<&str>,
    cx: &mut App,
) {
    let base = base.filter(|base| !base.is_empty()).map(str::to_string);
    update_sessions(cx, |sessions, cx| {
        let editable = sessions
            .get(session_id)
            .is_some_and(workspace_mode_editable);
        if !editable {
            return;
        }
        sessions.update(session_id, cx, |session| {
            if mode == WorkspaceMode::Worktree {
                let base = base.clone().or_else(|| {
                    session
                        .worktree_base
                        .clone()
                        .filter(|base| !base.is_empty())
                });
                if let Some(base) = base {
                    session.workspace_mode = Some(WorkspaceMode::Worktree);
                    session.worktree_base = Some(base);
                }
            } else {
                session.workspace_mode = None;
                session.worktree_base = None;
            }
        });
    });
}

/// `onWorktreeBaseChange`.
pub fn on_worktree_base_change(session_id: &str, base: &str, cx: &mut App) {
    update_sessions(cx, |sessions, cx| {
        let editable = sessions.get(session_id).is_some_and(|session| {
            workspace_mode_editable(session)
                && session.workspace_mode == Some(WorkspaceMode::Worktree)
        });
        if editable {
            sessions.update(session_id, cx, |session| {
                session.worktree_base = Some(base.to_string());
            });
        }
    });
}

const WAIT_TO_SWITCH: &str = "Wait for this session to finish before changing working copies.";
const SESSION_CHANGED: &str = "The session changed. Try selecting the working copy again.";

/// `onWorktreeChange`: move a session to another working copy. A session
/// with a conversation keeps its files and opens a new session in the
/// target instead.
pub fn on_worktree_change(
    session_id: &str,
    tree: Worktree,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let Some(sessions) = sessions_entity(cx) else {
        return Task::ready(Err(WAIT_TO_SWITCH.into()));
    };
    let current = {
        let state = sessions.read(cx);
        state.get(session_id).cloned().filter(|current| {
            !current.is_busy()
                && !state.is_removing(session_id)
                && state.switching_worktree(session_id).is_none()
        })
    };
    let Some(current) = current else {
        return Task::ready(Err(WAIT_TO_SWITCH.into()));
    };
    let removed = current.worktree_removed == Some(true);
    if !removed && path_key(session_work_cwd(&current)) == path_key(&tree.path) {
        return Task::ready(Ok(()));
    }
    let removing = ProjectsGlobal::try_global(cx)
        .map(|global| global.projects.read(cx).removing_worktree_paths.clone())
        .unwrap_or_default();
    if removing
        .iter()
        .any(|path| is_equal_or_inside(&tree.path, path))
    {
        return Task::ready(Err(
            "This worktree is being deleted. Select another working copy.".into(),
        ));
    }
    if !removed
        && current
            .queued_messages
            .as_ref()
            .is_some_and(|queued| !queued.is_empty())
    {
        return Task::ready(Err(
            "Clear queued messages before changing working copies.".into()
        ));
    }
    if hooks(cx).orchestration_running(session_id, cx) {
        return Task::ready(Err(
            "Stop this orchestration run before changing working copies.".into(),
        ));
    }
    sessions.update(cx, |sessions, _| {
        sessions.begin_worktree_switch(session_id, &tree.path)
    });
    let id = session_id.to_string();
    let list = list_worktrees(&current.cwd, cx);
    cx.spawn(async move |cx| {
        let result = switch_worktree(&id, &current, &tree, list, cx).await;
        cx.update(|cx| {
            update_sessions(cx, |sessions, _| sessions.end_worktree_switch(&id));
        });
        result
    })
}

async fn switch_worktree(
    id: &str,
    current: &Session,
    tree: &Worktree,
    list: Task<Result<Worktrees, String>>,
    cx: &mut gpui::AsyncApp,
) -> Result<(), String> {
    let listed = list.await?;
    let target = listed
        .worktrees
        .into_iter()
        .find(|entry| path_key(&entry.path) == path_key(&tree.path) && !entry.missing)
        .ok_or("This worktree is no longer available. Refresh the picker.")?;
    let same_place = |session: &Session| {
        session.cwd == current.cwd && session_work_cwd(session) == session_work_cwd(current)
    };
    let source = cx
        .update(|cx| find_session(id, cx))
        .filter(|source| !source.is_busy() && same_place(source))
        .ok_or(SESSION_CHANGED)?;
    let selected = cx.update(|cx| {
        let inputs = ProjectsGlobal::model_inputs(cx);
        session_in_worktree(&inputs.env(), hooks(cx).as_ref(), &source, &target)
    });
    if selected.id != id {
        // Leave the original conversation, checkpoints, and live provider
        // context attached to the files they describe.
        cx.update(|cx| {
            let hooks = hooks(cx);
            let tab = new_tab(&selected.id);
            let tab_id = tab.id.clone();
            let cwd = selected.cwd.clone();
            update_sessions(cx, |sessions, cx| sessions.insert(selected, cx));
            hooks.append_tab(tab, Some(&cwd), cx);
            hooks.set_active_tab(&tab_id, cx);
            hooks.set_composer_focused(true, cx);
        });
        return Ok(());
    }
    let (flush, forgets) = cx.update(|cx| {
        let engine = Engine::try_global(cx).map(|engine| engine.checkpoints.clone());
        let flush = engine.map(|checkpoints| checkpoints.flush_session_checkpoint(id));
        (flush, Engine::hooks(cx))
    });
    if let Some(flush) = flush {
        flush.await;
    }
    for harness in forgets.harness.session_child_harnesses(&source) {
        let forget = cx.update(|cx| forgets.harness.forget_session(harness, id, cx));
        forget.await;
    }
    let latest = cx
        .update(|cx| find_session(id, cx))
        .filter(|latest| {
            (latest.worktree_removed == Some(true) || is_blank_session(Some(latest)))
                && same_place(latest)
        })
        .ok_or(SESSION_CHANGED)?;
    let next = cx.update(|cx| {
        let inputs = ProjectsGlobal::model_inputs(cx);
        session_in_worktree(&inputs.env(), hooks(cx).as_ref(), &latest, &target)
    });
    if latest.worktree_removed == Some(true) {
        let keep = cx.update(|cx| {
            Engine::try_global(cx).map(|engine| engine.checkpoints.keep(id, &target.path, None))
        });
        if let Some(keep) = keep {
            keep.await?;
        }
    }
    if should_persist_session(&next) {
        let upsert = cx
            .update(|cx| Engine::try_global(cx).map(|engine| engine.writer.upsert_session(&next)));
        if let Some(upsert) = upsert {
            upsert.await?;
        }
    }
    cx.update(|cx| {
        let cwd = next.cwd.clone();
        update_sessions(cx, |sessions, cx| {
            sessions.invalidate_loaded(id);
            sessions.update(id, cx, |session| *session = next);
        });
        notify_git_changed(cx);
        notify_review_changed(id, cx);
        hooks(cx).refresh_history(&cwd, cx);
    });
    Ok(())
}

// Worktree deletion.

/// `checkOpenWorktreeFiles`.
fn check_open_worktree_files(path: &str, cx: &App) -> Result<(), String> {
    assert_worktree_files_closed(path, &hooks(cx).open_files(cx))
}

/// `onCheckWorktreeRemoval`: the preflight the delete dialog runs.
pub fn on_check_worktree_removal(
    cwd: &str,
    path: &str,
    force: bool,
    cx: &mut App,
) -> Task<Result<(), String>> {
    if let Err(error) = check_open_worktree_files(path, cx) {
        return Task::ready(Err(error));
    }
    let check = check_worktree_removal(cwd, path, force, cx);
    let path = path.to_string();
    cx.spawn(async move |cx| {
        check.await?;
        // Re-read the open files after the native check.
        cx.update(|cx| check_open_worktree_files(&path, cx))
    })
}

/// `onRemoveWorktree`: delete a working copy. With `keep_sessions`, the
/// sessions in it stay and must pick a new working copy before their next
/// turn; without it, any session in it blocks the removal.
pub fn on_remove_worktree(
    cwd: &str,
    path: &str,
    force: bool,
    keep_sessions: bool,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let Some(global) = ProjectsGlobal::try_global(cx) else {
        return Task::ready(Err("Projects are not ready.".into()));
    };
    let projects = global.projects.clone();
    let started = projects.update(cx, |projects, _| {
        projects.removing_worktree_paths.insert(path.to_string())
    });
    if !started {
        return Task::ready(Err("This worktree is already being deleted.".into()));
    }
    let (cwd, path) = (cwd.to_string(), path.to_string());
    cx.spawn(async move |cx| {
        let mut locked: Vec<String> = Vec::new();
        let mut forgotten: HashSet<String> = HashSet::new();
        let result = remove_worktree_flow(
            &cwd,
            &path,
            force,
            keep_sessions,
            &mut locked,
            &mut forgotten,
            cx,
        )
        .await;
        cx.update(|cx| {
            if result.is_err() {
                rebind_kept_sessions(&forgotten, cx);
            }
            projects.update(cx, |projects, _| {
                projects.removing_worktree_paths.remove(&path);
            });
            update_sessions(cx, |sessions, _| {
                for id in &locked {
                    sessions.end_removal(id);
                }
            });
        });
        result
    })
}

/// Removal may fail after idle agent processes were stopped. Rebind their
/// saved threads so the unchanged working copy can still resume.
fn rebind_kept_sessions(forgotten: &HashSet<String>, cx: &mut App) {
    let kept: Vec<Session> = open_sessions(cx)
        .into_iter()
        .filter(|session| forgotten.contains(&session.id) && session.worktree_removed != Some(true))
        .collect();
    let engine_hooks = Engine::hooks(cx);
    bind_resumed_sessions(&kept, &engine_hooks, cx);
    for session in &kept {
        let Some(pending) = session.pending_switch.as_ref() else {
            continue;
        };
        let Some(provider) = pending
            .from_provider_session_id
            .clone()
            .filter(|provider| !provider.is_empty())
        else {
            continue;
        };
        // `bindHarnessSession(pending.from, ...)`: the thread the pending
        // switch leaves, in the same working copy.
        let mut from = session.clone();
        from.harness = pending.from;
        from.provider_session_id = Some(provider);
        from.provider_account_id = pending.from_provider_account_id.clone();
        engine_hooks.harness.bind_session(&from, cx);
    }
}

async fn remove_worktree_flow(
    cwd: &str,
    path: &str,
    force: bool,
    keep_sessions: bool,
    locked: &mut Vec<String>,
    forgotten: &mut HashSet<String>,
    cx: &mut gpui::AsyncApp,
) -> Result<(), String> {
    let selecting = cx.update(|cx| {
        sessions_entity(cx).is_some_and(|sessions| {
            let state = sessions.read(cx);
            state.all().iter().any(|session| {
                state
                    .switching_worktree(&session.id)
                    .is_some_and(|target| is_equal_or_inside(target, path))
            })
        })
    });
    if selecting {
        return Err(
            "A session is selecting this worktree. Try deleting it again once selection finishes."
                .into(),
        );
    }
    let check = cx.update(|cx| on_check_worktree_removal(cwd, path, force, cx));
    check.await?;
    let list = cx.update(|cx| list_worktrees(cwd, cx));
    let listed = list.await?;
    let tree = listed
        .worktrees
        .into_iter()
        .find(|entry| path_key(&entry.path) == path_key(path))
        .ok_or("This worktree is no longer available.")?;
    let ids = cx.update(|cx| worktree_session_ids(&tree, &open_sessions(cx)));
    if !keep_sessions && !ids.is_empty() {
        return Err("Move or delete the sessions using this worktree first.".into());
    }
    let busy = cx.update(|cx| {
        sessions_entity(cx).is_some_and(|sessions| {
            let state = sessions.read(cx);
            ids.iter()
                .any(|id| state.is_removing(id) || state.switching_worktree(id).is_some())
        })
    });
    if busy {
        return Err(
            "Wait for these sessions to finish changing before deleting the worktree.".into(),
        );
    }
    cx.update(|cx| {
        update_sessions(cx, |sessions, _| {
            for id in &ids {
                sessions.begin_removal(id);
                locked.push(id.clone());
                sessions.invalidate_loaded(id);
            }
        })
    });

    for id in &ids {
        let stop =
            cx.update(|cx| update_sessions(cx, |sessions, cx| sessions.stop_for_removal(id, cx)));
        if let Some(stop) = stop {
            stop.await;
        }
        let Some(session) = cx.update(|cx| find_session(id, cx)) else {
            continue;
        };
        let (flush, engine_hooks) = cx.update(|cx| {
            let flush = Engine::try_global(cx)
                .map(|engine| engine.checkpoints.flush_session_checkpoint(id));
            (flush, Engine::hooks(cx))
        });
        if let Some(flush) = flush {
            flush.await;
        }
        forgotten.insert(id.clone());
        for harness in engine_hooks.harness.session_child_harnesses(&session) {
            let forget = cx.update(|cx| engine_hooks.harness.forget_session(harness, id, cx));
            forget.await;
        }
        let Some(latest) = cx.update(|cx| find_session(id, cx)) else {
            continue;
        };
        let now = cx.update(|cx| ProjectsGlobal::now(cx));
        let mut stopped = stop_streaming(&latest, now);
        stopped.busy = Some(false);
        stopped.queue_status = Some(MessageQueueStatus::Paused);
        stopped.pending_question = None;
        let upsert = cx.update(|cx| {
            let saved = stopped.clone();
            update_sessions(cx, |sessions, cx| {
                sessions.update(id, cx, |session| *session = saved);
            });
            if should_persist_session(&stopped) {
                Engine::try_global(cx).map(|engine| engine.writer.upsert_session(&stopped))
            } else {
                None
            }
        });
        if let Some(upsert) = upsert {
            upsert.await?;
        }
    }
    let flush =
        cx.update(|cx| Engine::try_global(cx).map(|engine| engine.writer.flush_session_writes()));
    if let Some(flush) = flush {
        flush.await;
    }
    cx.update(|cx| check_open_worktree_files(path, cx))?;
    let remove = cx.update(|cx| remove_worktree(cwd, path, force, keep_sessions, cx));
    let removed = remove.await?;

    cx.update(|cx| {
        let hooks = hooks(cx);
        let mut affected: Vec<String> = ids.clone();
        for id in &removed.session_ids {
            if !affected.contains(id) {
                affected.push(id.clone());
            }
        }
        if is_equal_or_inside(&hooks.project_cwd(cx), path) {
            hooks.set_project_cwd(&removed.project_cwd, cx);
            remember_project(&removed.project_cwd, cx);
        }
        let project_cwd = removed.project_cwd.clone();
        update_sessions(cx, |sessions, cx| {
            for id in &affected {
                sessions.invalidate_loaded(id);
                sessions.forget_persisted(id);
            }
            sessions.update_all(cx, |session| {
                affected
                    .contains(&session.id)
                    .then(|| detach_session_worktree(session, &project_cwd, path))
            });
        });
        hooks.patch_summaries(
            &|entry| {
                affected
                    .contains(&entry.id)
                    .then(|| detach_summary_worktree(entry, &project_cwd, path))
            },
            cx,
        );
        for id in &affected {
            notify_review_changed(id, cx);
        }
    });
    Ok(())
}

// Opening projects.

/// `openProjects`: open a run of folders from one plan, in selection order.
/// The folder chosen last ends up focused.
pub fn open_projects(paths: &[String], cx: &mut App) {
    let hooks = hooks(cx);
    let inputs = ProjectsGlobal::model_inputs(cx);
    let memory = hooks.project_return_memory(cx);
    let tabs = hooks.tabs(cx);
    let sessions = open_sessions(cx);
    let active_tab_id = hooks.active_tab_id(cx);
    let steps = plan_project_open_run(
        &inputs.env(),
        &memory,
        &tabs,
        &sessions,
        &active_tab_id,
        paths,
    );
    let Some(last) = steps.last().cloned() else {
        return;
    };

    // After the early return: a dismissed picker hands back no folders, and
    // closing these first would shut whatever the user had open.
    hooks.close_pages(cx);

    // At most one folder takes the blank session, with the retargeting rules
    // `on_cwd_change` owns.
    if let Some(ProjectOpenStep::ReuseBlank { path, session_id }) = steps
        .iter()
        .find(|step| matches!(step, ProjectOpenStep::ReuseBlank { .. }))
    {
        on_cwd_change(session_id, path, cx);
    }

    let created: Vec<(Session, WorkspaceTab, Option<String>, String)> = steps
        .iter()
        .filter_map(|step| match step {
            ProjectOpenStep::Create {
                path,
                session,
                tab,
                beside_tab_id,
            } => Some((
                (**session).clone(),
                (**tab).clone(),
                beside_tab_id.clone(),
                path.clone(),
            )),
            _ => None,
        })
        .collect();
    if !created.is_empty() {
        update_sessions(cx, |sessions, cx| {
            for (session, ..) in &created {
                sessions.insert(session.clone(), cx);
            }
        });
        // Each tab sits beside the one before it in the run.
        for (_, tab, beside, path) in created {
            hooks.insert_tab_beside(tab, beside.as_deref(), Some(&path), cx);
        }
    }

    match &last {
        ProjectOpenStep::Create { path, tab, .. } => {
            hooks.set_project_cwd(path, cx);
            hooks.set_active_tab(&tab.id, cx);
            hooks.set_composer_focused(true, cx);
        }
        ProjectOpenStep::Activate {
            path,
            tab_id,
            pane_id,
        } => {
            hooks.set_project_cwd(path, cx);
            hooks.activate_tab(tab_id, pane_id.as_deref(), cx);
        }
        ProjectOpenStep::Keep { path } => hooks.set_project_cwd(path, cx),
        // `on_cwd_change` already moved to it.
        ProjectOpenStep::ReuseBlank { .. } => {}
    }
    // Every project opened is remembered, the one chosen last most recently.
    for step in &steps {
        remember_project(step.path(), cx);
    }
}

/// `onSelectProject`.
pub fn on_select_project(path: &str, cx: &mut App) {
    open_projects(&[path.to_string()], cx);
}

/// `onRestoreProject`: bring an archived project back to the rail.
pub fn on_restore_project(path: &str, cx: &mut App) {
    remember_project(path, cx);
    on_select_project(path, cx);
}

/// `onPlaceSessionInFolder`: file a session in a session folder of its
/// project. A remote project files the remote session id.
pub fn on_place_session_in_folder(session_id: &str, target: &SessionFolderTarget, cx: &mut App) {
    let Some(source) = find_session(session_id, cx) else {
        return;
    };
    if !looks_like_project(&source.cwd) {
        return;
    }
    let hooks = hooks(cx);
    let id = if is_remote_project_path(&source.cwd) {
        hooks
            .remote_session_for(session_id, cx)
            .unwrap_or_else(|| session_id.to_string())
    } else {
        session_id.to_string()
    };
    hooks.place_session_in_folder(&source.cwd, &id, target, cx);
}

/// `onRemoveProject`: Archive (`purge_data` false) files the project away
/// and saves its idle chats; Delete (`purge_data` true) forgets it and
/// deletes its chats and settings.
pub fn on_remove_project(path: &str, purge_data: bool, cx: &mut App) {
    let hooks = hooks(cx);
    let normalized = normalize_project_path(path);
    let was_current = same_project_path(&hooks.project_cwd(cx), &normalized);
    let Some(global) = ProjectsGlobal::try_global(cx) else {
        return;
    };
    let projects = global.projects.clone();
    let remaining = projects.update(cx, |projects, cx| {
        if purge_data {
            projects.forget_project(&normalized, cx);
        } else {
            projects.archive_project(&normalized, cx);
        }
        projects.recents().to_vec()
    });
    if purge_data {
        projects.update(cx, |projects, _| projects.forget_location(&normalized));
        hooks.project_sidebar_tab_removed(&normalized, cx);
    }

    let tabs = hooks.tabs(cx);
    let sessions = open_sessions(cx);
    let project_tabs = filter_tabs_for_project(&tabs, &sessions, &normalized);
    let project_tab_ids: HashSet<String> = project_tabs.iter().map(|tab| tab.id.clone()).collect();
    let project_sessions: Vec<Session> = sessions
        .iter()
        .filter(|session| same_project_path(&session.cwd, &normalized))
        .cloned()
        .collect();
    let project_session_ids: HashSet<String> = project_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();

    let engine_hooks = Engine::hooks(cx);
    if purge_data {
        update_sessions(cx, |sessions, _| {
            let cached: Vec<String> = sessions
                .loaded_cache()
                .ids()
                .into_iter()
                .map(str::to_string)
                .collect();
            for id in cached {
                sessions.invalidate_loaded(&id);
            }
        });
        for session in &project_sessions {
            let children: Vec<HarnessId> = engine_hooks.harness.session_child_harnesses(session);
            if session.is_busy() {
                update_sessions(cx, |sessions, _| sessions.bump_turn_gen(&session.id));
                for harness in &children {
                    engine_hooks
                        .harness
                        .cancel_turn(*harness, &session.id, cx)
                        .detach();
                }
            }
            for harness in &children {
                engine_hooks
                    .harness
                    .forget_session(*harness, &session.id, cx)
                    .detach();
            }
            update_sessions(cx, |sessions, _| sessions.forget_persisted(&session.id));
        }
        project_data::remove_project_data(&normalized, cx).detach();
    } else {
        for session in &project_sessions {
            if session.is_busy() {
                continue;
            }
            update_sessions(cx, |sessions, cx| {
                if should_persist_session(session) {
                    sessions.remember_loaded(session.clone());
                }
                sessions.persist_session(session.clone(), cx);
            });
            for harness in engine_hooks.harness.session_child_harnesses(session) {
                engine_hooks
                    .harness
                    .forget_session(harness, &session.id, cx)
                    .detach();
            }
        }
    }

    let mut next_tabs: Vec<WorkspaceTab> = tabs
        .iter()
        .filter(|tab| !project_tab_ids.contains(&tab.id))
        .cloned()
        .collect();
    let keep = |session: &Session| {
        !project_session_ids.contains(&session.id) || (!purge_data && session.is_busy())
    };
    let fallback_mode = sessions
        .iter()
        .find(|session| keep(session))
        .map(|session| session.runtime_mode);
    let mut next_active_tab_id = hooks.active_tab_id(cx);
    let mut replacement = None;
    if next_tabs.is_empty() {
        let inputs = ProjectsGlobal::model_inputs(cx);
        let session = new_default_session(
            &inputs.env(),
            uuid::Uuid::new_v4().to_string(),
            "~",
            fallback_mode,
        );
        let tab = new_tab(&session.id);
        next_active_tab_id = tab.id.clone();
        next_tabs = vec![tab];
        replacement = Some(session);
    } else if project_tab_ids.contains(&next_active_tab_id) {
        next_active_tab_id = next_tabs[0].id.clone();
    }

    update_sessions(cx, |sessions, cx| {
        sessions.retain(cx, keep);
        if let Some(session) = replacement {
            sessions.insert(session, cx);
        }
    });
    hooks.set_tabs(next_tabs, &next_active_tab_id, cx);
    let dirty: Vec<String> = project_tabs
        .iter()
        .flat_map(|tab| tab.editor_panes.iter().chain(&tab.terminal_panes))
        .flat_map(|pane| pane.files.iter().map(|file| file.id.clone()))
        .collect();
    hooks.forget_dirty_files(&dirty, cx);
    hooks.remove_project_terminals(&normalized, cx);

    if was_current {
        match remaining.iter().find(|item| looks_like_project(&item.path)) {
            Some(next) => {
                let next = next.path.clone();
                on_select_project(&next, cx);
                hooks.set_project_cwd(&next, cx);
            }
            None => {
                hooks.set_project_cwd("~", cx);
                hooks.set_composer_focused(true, cx);
            }
        }
    }
}

// A renamed folder.

/// `synchronizeProjectLocation` through the shared lookup in `Projects`.
pub fn synchronize_project_location(cwd: &str, cx: &mut App) -> Option<super::LocationSync> {
    let projects = ProjectsGlobal::try_global(cx)?.projects.clone();
    Some(projects.update(cx, |projects, cx| projects.synchronize_location(cwd, cx)))
}

/// `applyProjectLocationChange`: a project folder moved on disk. Its saved
/// chats, open sessions, history rows, docks, file tabs, settings, and rail
/// entry follow it.
pub fn apply_project_location_change(
    from: &str,
    to: &str,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let rebase =
        Engine::try_global(cx).map(|engine| engine.writer.rebase_project_sessions(from, to));
    let (from, to) = (from.to_string(), to.to_string());
    cx.spawn(async move |cx| {
        if let Some(rebase) = rebase {
            rebase.await?;
        }
        cx.update(|cx| {
            let hooks = hooks(cx);
            hooks.rebase_ci_repairs(&from, &to, cx);
            update_sessions(cx, |sessions, cx| {
                sessions.update_all(cx, |session| {
                    same_project_path(&session.cwd, &from).then(|| Session {
                        cwd: to.clone(),
                        ..session.clone()
                    })
                });
                let cached: Vec<Session> = sessions
                    .loaded_cache()
                    .ids()
                    .into_iter()
                    .filter_map(|id| sessions.loaded_cache().get(id).cloned())
                    .filter(|session| same_project_path(&session.cwd, &from))
                    .collect();
                for session in cached {
                    sessions.remember_loaded(Session {
                        cwd: to.clone(),
                        ..session
                    });
                }
            });
            hooks.patch_summaries(
                &|entry| {
                    same_project_path(&entry.cwd, &from).then(|| {
                        let mut next = entry.clone();
                        next.cwd = to.clone();
                        next
                    })
                },
                cx,
            );
            hooks.rebase_loaded_project(
                &normalize_project_path(&from),
                &normalize_project_path(&to),
                cx,
            );
            if same_project_path(&hooks.project_cwd(cx), &from) {
                hooks.set_project_cwd(&to, cx);
            }
            hooks.project_location_changed(&from, &to, cx);
            project_data::rebase_project_data(&from, &to, cx);
            hooks.project_sidebar_tab_moved(&from, &to, cx);
            let projects = ProjectsGlobal::projects(cx);
            projects.update(cx, |projects, cx| {
                projects.replace_project_path(&from, &to, cx)
            });
        });
        Ok(())
    })
}
