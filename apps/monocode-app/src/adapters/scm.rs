//! Source control views over the app's shared git status registry.
use gpui::{AnyView, App, AppContext as _, Global, Task, Window};
use monocode_app::boot::AppServices;
use monocode_engine::{projects::ProjectsGlobal, runtime::Engine, workspace::Files};
use monocode_harness::core::task::{AbortSignal, SharedSpawner};
use monocode_view_files::{ExternalSurface, SurfaceRequest};
use monocode_view_scm::hooks::PrContent;
use monocode_view_scm::ui::diffs::{CommitDiff, SessionChangesDiff, WorkingTreeDiff};
use monocode_view_scm::{Scm, ScmHooks};
use monocode_view_settings::settings::SlotContext;
use std::{future::Future, rc::Rc, sync::Arc};

struct AppScm(Scm);
impl Global for AppScm {}

pub fn app_scm(cx: &mut App) -> Scm {
    if let Some(scm) = cx.try_global::<AppScm>() {
        return scm.0.clone();
    }
    let hooks = app_hooks();
    let scm = if let Some(projects) = ProjectsGlobal::try_global(cx) {
        Scm::new(
            Arc::new(crate::adapters::git::AppGit::new(cx)),
            projects.git.clone(),
            hooks,
            monocode_core::appearance::ChangesView::parse(
                projects
                    .kv
                    .get_item(monocode_core::appearance::CHANGES_VIEW_KEY)
                    .as_deref(),
            ),
            cx,
        )
    } else {
        Scm::local(hooks, cx)
    };
    cx.set_global(AppScm(scm.clone()));
    scm
}

fn app_hooks() -> ScmHooks {
    ScmHooks {
        generate_commit_message: Some(Rc::new(|request, cx| {
            let Some(services) = AppServices::try_global(cx) else {
                return Task::ready(Err("The provider services are unavailable.".into()));
            };
            let registry = services.registry.clone();
            let availability = services.availability.clone();
            let spawner = registry.spawner().clone();
            let signal = AbortSignal::new();
            let provider_signal = signal.clone();
            generation_task(
                spawner,
                Some(signal),
                async move {
                    monocode_harness::generate_commit_message(
                        &registry,
                        &request.cwd,
                        request.text_harness,
                        Some(provider_signal),
                        |id| availability.is_harness_available(id),
                    )
                    .await
                    .map_err(|error| error.to_string())
                },
                cx,
            )
        })),
        generate_pr_content: Some(Rc::new(|request, cx| {
            let Some(services) = AppServices::try_global(cx) else {
                return Task::ready(Err("The provider services are unavailable.".into()));
            };
            let registry = services.registry.clone();
            let availability = services.availability.clone();
            let spawner = registry.spawner().clone();
            generation_task(
                spawner,
                None,
                async move {
                    monocode_harness::generate_pr_content(
                        &registry,
                        &request.cwd,
                        request.text_harness,
                        |id| availability.is_harness_available(id),
                    )
                    .await
                    .map(|content| {
                        content.map(|content| PrContent {
                            title: content.title,
                            body: content.body,
                            base: content.base,
                            head: content.head,
                        })
                    })
                    .map_err(|error| error.to_string())
                },
                cx,
            )
        })),
        files_changed: Some(Rc::new(|paths, cx| {
            Files::invalidate_watched_files(paths, cx)
        })),
        git_changed: Some(Rc::new(Files::notify_git_changed)),
        add_to_chat: Some(Rc::new(|item, cx| {
            let item = serde_json::to_value(item)
                .ok()
                .and_then(|value| serde_json::from_value(value).ok());
            if let Some(item) = item {
                monocode_engine::submit::Submit::global(cx)
                    .update(cx, |submit, cx| submit.request_add_to_chat(item, cx));
            }
        })),
        pr_activity: Some(Rc::new(|cwd, number, cx| {
            use monocode_engine::inbox::{
                inbox::Inbox,
                inbox_self_activity::InboxSelfActivityTarget,
                types::{InboxKind, InboxProvider},
            };
            if let Some(inbox) = Inbox::try_global(cx) {
                inbox
                    .read(cx)
                    .client()
                    .record_inbox_self_activity(InboxSelfActivityTarget {
                        kind: Some(InboxKind::Pr),
                        number: Some(number),
                        project_path: Some(cwd.to_owned()),
                        ..InboxSelfActivityTarget::new(InboxProvider::Github)
                    });
            }
        })),
        save_changes_view: Some(Rc::new(|view, cx| {
            if let Some(projects) = ProjectsGlobal::try_global(cx) {
                projects
                    .kv
                    .set_item(monocode_core::appearance::CHANGES_VIEW_KEY, view.as_str());
            }
        })),
        ..Default::default()
    }
}

struct CancelGeneration(Option<AbortSignal>);

impl Drop for CancelGeneration {
    fn drop(&mut self) {
        if let Some(signal) = &self.0 {
            signal.abort();
        }
    }
}

fn generation_task<T: Send + 'static>(
    spawner: SharedSpawner,
    signal: Option<AbortSignal>,
    work: impl Future<Output = Result<T, String>> + Send + 'static,
    cx: &App,
) -> Task<Result<T, String>> {
    let (send, receive) = futures::channel::oneshot::channel();
    // Keep the provider future running long enough to handle cancellation and
    // close its child. Cancelling the view's task aborts the provider signal.
    spawner.spawn(Box::pin(async move {
        let _ = send.send(work.await);
    }));
    let cancel = CancelGeneration(signal);
    cx.foreground_executor().spawn(async move {
        let _cancel = cancel;
        receive
            .await
            .unwrap_or_else(|_| Err("The provider stopped before returning generated text.".into()))
    })
}

pub fn diff_surface(
    request: &SurfaceRequest<'_>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyView> {
    let file = request.file;
    let focus = (file.path != file.cwd).then(|| file.path.clone());
    let scm = app_scm(cx);
    match request.kind {
        ExternalSurface::WorkingTreeDiff => {
            let kind = file.change_kind.map(|kind| match kind {
                monocode_layout::GitFileDiffKind::Staged => {
                    monocode_view_scm::GitFileDiffKind::Staged
                }
                monocode_layout::GitFileDiffKind::Unstaged => {
                    monocode_view_scm::GitFileDiffKind::Unstaged
                }
            });
            Some(
                cx.new(|cx| WorkingTreeDiff::new(scm, file.cwd.clone(), focus, kind, window, cx))
                    .into(),
            )
        }
        ExternalSurface::Commit => {
            let sha = file.commit.as_ref()?.sha.clone();
            Some(
                cx.new(|cx| CommitDiff::new(scm, file.cwd.clone(), sha, cx))
                    .into(),
            )
        }
        ExternalSurface::SessionChanges => {
            let session_id = file.session_changes.as_ref()?.session_id.clone();
            let checkpoints = Engine::checkpoints(cx);
            let review = Engine::global(cx).review.clone();
            Some(
                cx.new(|cx| {
                    SessionChangesDiff::new(
                        file.cwd.clone(),
                        session_id,
                        focus,
                        checkpoints,
                        Some(review),
                        cx,
                    )
                })
                .into(),
            )
        }
        _ => None,
    }
}

pub fn worktrees_slot(ctx: &SlotContext, window: &mut Window, cx: &mut App) -> AnyView {
    let scm = app_scm(cx);
    let cwd = ctx.cwd.clone();
    let remove = Rc::new(
        |call: monocode_view_scm::ui::worktrees_page::WorktreeRemoveCall, cx: &mut App| {
            monocode_engine::projects::actions::on_remove_worktree(
                &call.cwd,
                &call.path,
                call.force,
                call.keep_sessions.unwrap_or(false),
                cx,
            )
        },
    );
    cx.new(|cx| {
        monocode_view_scm::ui::worktrees_page::WorktreesPage::new(scm, cwd, remove, window, cx)
    })
    .into()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn cancelled_commit_generation_allows_provider_cleanup(cx: &mut TestAppContext) {
        let executor = cx.executor();
        let spawner: SharedSpawner =
            Arc::new(move |future: futures::future::BoxFuture<'static, ()>| {
                executor.spawn(future).detach()
            });
        let signal = AbortSignal::new();
        let provider_signal = signal.clone();
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let provider_cleanup = cleaned_up.clone();
        let task = cx.update(|cx| {
            generation_task(
                spawner,
                Some(signal.clone()),
                async move {
                    provider_signal.aborted().await;
                    provider_cleanup.store(true, Ordering::SeqCst);
                    Err::<String, _>("Generation cancelled".to_owned())
                },
                cx,
            )
        });
        cx.run_until_parked();
        assert!(!signal.is_aborted());
        assert!(!cleaned_up.load(Ordering::SeqCst));

        drop(task);
        cx.run_until_parked();
        assert!(signal.is_aborted());
        assert!(cleaned_up.load(Ordering::SeqCst));
    }
}
