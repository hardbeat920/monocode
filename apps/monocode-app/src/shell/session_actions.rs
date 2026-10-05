//! Conversation removal shared by title menus and focused-session shortcuts.

use gpui::{App, Entity};
use monocode_core::Session;
use monocode_engine::history::{History, session_removal::SessionRemovalMode};
use monocode_engine::remote::RemoteGlobal;
use monocode_engine::runtime::Engine;
use monocode_layout::paths::{is_remote_project_path, same_project_path};
use serde_json::json;

pub(super) fn remove_sessions(
    sessions: Vec<Session>,
    history: Option<Entity<History>>,
    mode: SessionRemovalMode,
    cx: &mut App,
) {
    if sessions.is_empty() {
        return;
    }
    if sessions
        .iter()
        .all(|session| !is_remote_project_path(&session.cwd))
    {
        if let Some(history) = history {
            let ids = sessions.iter().map(|session| session.id.clone()).collect();
            history.update(cx, |history, cx| match mode {
                SessionRemovalMode::Archive => history.archive_sessions(ids, true, cx).detach(),
                SessionRemovalMode::Delete if sessions.len() == 1 => {
                    history.delete_session(&sessions[0].id, cx).detach()
                }
                SessionRemovalMode::Delete => history.delete_sessions(ids, cx).detach(),
            });
        }
        return;
    }
    let confirm = (mode == SessionRemovalMode::Delete).then(|| {
        monocode_app::bridge::dialogs::confirm(
            &format!(
                "Delete {} conversations? This cannot be undone.",
                sessions.len()
            ),
            "Delete",
            cx,
        )
    });
    cx.spawn(async move |cx| {
        if let Some(confirm) = confirm
            && !confirm.await
        {
            return;
        }
        for session in sessions {
            if !is_remote_project_path(&session.cwd) {
                if let Some(history) = &history {
                    let step = history.update(cx, |history, cx| {
                        history.remove_session(&session.id, mode, true, cx)
                    });
                    if !step.await {
                        break;
                    }
                }
                continue;
            }
            let target = cx.update(|cx| remote_target(&session.id, &session.cwd, cx));
            let target = match target {
                Ok(target) => target,
                Err(detail) => {
                    cx.update(|cx| monocode_app::bridge::dialogs::alert(&detail, true, cx));
                    break;
                }
            };
            if let Some(RemoteTarget {
                machine,
                project,
                host_id,
                client,
            }) = target
            {
                let mut params = json!({ "projectId": project, "sessionId": host_id });
                if mode == SessionRemovalMode::Archive {
                    params["archived"] = true.into();
                }
                if let Err(error) = client
                    .request(
                        &machine,
                        if mode == SessionRemovalMode::Delete {
                            "sessions.delete"
                        } else {
                            "sessions.update"
                        },
                        params,
                    )
                    .await
                {
                    cx.update(|cx| {
                        monocode_app::bridge::dialogs::alert(
                            &format!("Could not {} this conversation.\n\n{error}", mode.verb()),
                            true,
                            cx,
                        )
                    });
                    break;
                }
                cx.update(|cx| forget_remote_bindings(&session.cwd, &host_id, cx));
            } else {
                cx.update(|cx| {
                    crate::slots::forget_session_in_windows(&session.id, cx);
                    RemoteGlobal::forget_tab(&session.id, cx);
                });
            }
        }
        cx.update(|cx| {
            if let Some(remote) = RemoteGlobal::try_global(cx) {
                remote.connections.clone().update(cx, |connections, cx| {
                    connections.refresh_remote_project_sessions(cx)
                });
            }
        });
    })
    .detach();
}

struct RemoteTarget {
    machine: String,
    project: String,
    host_id: String,
    client: monocode_engine::remote::RemoteClient,
}

fn remote_target(shell_id: &str, cwd: &str, cx: &App) -> Result<Option<RemoteTarget>, String> {
    let Some(remote) = RemoteGlobal::try_global(cx) else {
        return Err("Connect this project's machine to change its sessions.".into());
    };
    let connections = remote.connections.read(cx);
    let Some(host_id) = connections.remote_session_for(shell_id) else {
        return Ok(None);
    };
    let project = connections
        .remote_project_for(cwd)
        .ok_or("This remote project is unavailable.")?;
    let machine = connections
        .project_sessions(cwd)
        .machine
        .ok_or("Connect this project's machine to change its sessions.")?;
    Ok(Some(RemoteTarget {
        machine: machine.id,
        project: project.project_id,
        host_id,
        client: remote.client.clone(),
    }))
}

fn forget_remote_bindings(cwd: &str, host_id: &str, cx: &mut App) {
    let Some(remote) = RemoteGlobal::try_global(cx) else {
        return;
    };
    let connections = remote.connections.clone();
    let ids = Engine::sessions(cx)
        .read(cx)
        .all()
        .iter()
        .filter(|session| {
            same_project_path(&session.cwd, cwd)
                && connections
                    .read(cx)
                    .remote_session_for(&session.id)
                    .as_deref()
                    == Some(host_id)
        })
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        crate::slots::forget_session_in_windows(&id, cx);
        RemoteGlobal::forget_tab(&id, cx);
    }
}
