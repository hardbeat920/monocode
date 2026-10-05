//! Port of src/app/model/quickLaunchSession.ts: hand a quick launch to the
//! window's workspace before the receiver acknowledges it. The delivery id
//! becomes the session id, so a retried delivery finds the session it
//! already made instead of starting another one.

use std::rc::Rc;

use gpui::AsyncApp;
use monocode_core::block::{BlockRole, TurnIntent};
use monocode_layout::new_tab;

use super::host::{LaunchHost, SessionPlacement};
use super::launch_delivery::LaunchError;
use super::quick_composer::{QuickIntent, QuickLaunchRequest, apply_quick_workspace};
use crate::runtime::Engine;
use crate::submit::SubmitOptions;

/// `acceptQuickLaunch`: create the session (in a new tab, or split beside
/// `placement`), then save the draft or submit the first turn. Resolves
/// once the workspace accepted the turn, not when the agent finishes.
pub async fn accept_quick_launch(
    launch: QuickLaunchRequest,
    delivery_id: String,
    host: Rc<dyn LaunchHost>,
    placement: Option<SessionPlacement>,
    cx: &mut AsyncApp,
) -> Result<(), LaunchError> {
    // Read image previews back here; the panel sends paths.
    let prepare = cx
        .update(|cx| host.prepare_attachments(launch.attachments.clone().unwrap_or_default(), cx));
    let attachments = prepare.await;
    let sessions = cx.update(|cx| Engine::sessions(cx));
    let existing = cx.update(|cx| sessions.read(cx).get(&delivery_id).cloned());
    if let Some(previous) = existing.as_ref().and_then(|existing| {
        existing
            .blocks
            .iter()
            .find(|block| block.app_request_id.as_deref() == Some(delivery_id.as_str()))
    }) {
        if previous.text != launch.prompt || (!launch.is_draft() && previous.is_draft()) {
            return Err(LaunchError::Failed(
                "Request ID was already used for another session launch".into(),
            ));
        }
        return Ok(());
    }
    if existing.as_ref().is_some_and(|existing| {
        existing.quick_launch_accepted == Some(true)
            || existing
                .blocks
                .iter()
                .any(|block| block.role == BlockRole::User && !block.is_draft())
    }) {
        return Ok(());
    }

    let is_new = existing.is_none();
    let mut session = match existing {
        Some(existing) => existing,
        None => cx.update(|cx| {
            apply_quick_workspace(
                host.new_session(
                    launch.harness,
                    &launch.cwd,
                    launch.model.as_deref(),
                    launch.runtime_mode,
                    None,
                    cx,
                ),
                &launch,
            )
        }),
    };
    session.id = delivery_id.clone();
    if let Some(settings) = &launch.model_settings {
        session.model_settings = cx
            .update(|cx| host.merge_model_settings(session.harness, &session.model, settings, cx));
    }

    if is_new {
        let tab = new_tab(&session.id);
        // Resolve the target before adding the session, so a closed pane
        // cannot leave an orphaned launch behind.
        let tab_id = match &placement {
            Some(placement) => cx
                .update(|cx| {
                    host.place_session(&session.id, placement, &launch.cwd, launch.reveal, cx)
                })
                .map_err(LaunchError::Failed)?,
            None => tab.id.clone(),
        };
        if tab_id.is_empty() {
            return Err(LaunchError::Failed("The target pane is unavailable".into()));
        }
        let added = session.clone();
        cx.update(|cx| {
            sessions.update(cx, |sessions, cx| sessions.upsert(added, cx));
            if placement.is_none() {
                host.append_tab(tab, &launch.cwd, cx);
            }
            if launch.reveal {
                // The title bar filters tabs by project. Select it first.
                host.set_project_cwd(&launch.cwd, cx);
                host.remember_project(&launch.cwd, cx);
                host.reveal_tab(&tab_id, &launch.cwd, cx);
            }
        });
    }

    if launch.is_draft() {
        let saved = cx.update(|cx| {
            host.save_draft(&session.id, &launch.prompt, attachments, &delivery_id, cx)
        });
        if !saved {
            return Err(LaunchError::Failed(
                "The workspace could not save the session draft yet.".into(),
            ));
        }
    } else {
        let options = SubmitOptions {
            intent: launch.intent.map(|intent| match intent {
                QuickIntent::Plan => TurnIntent::Plan,
                QuickIntent::Orchestrate => TurnIntent::Orchestrate,
            }),
            ..SubmitOptions::default()
        };
        let acceptance =
            cx.update(|cx| host.submit(&session.id, &launch.prompt, attachments, options, cx));
        match acceptance.resolve().await {
            Ok(true) => {}
            Ok(false) => {
                return Err(LaunchError::Failed(
                    "The workspace could not accept the queued session yet.".into(),
                ));
            }
            Err(error) if error.project_not_found => {
                return Err(LaunchError::ProjectNotFound(error.message));
            }
            Err(error) => return Err(LaunchError::Failed(error.message)),
        }
    }
    cx.update(|cx| {
        sessions.update(cx, |sessions, cx| {
            sessions.update(&delivery_id, cx, |session| {
                session.quick_launch_accepted = Some(true);
            });
        });
    });
    Ok(())
}
