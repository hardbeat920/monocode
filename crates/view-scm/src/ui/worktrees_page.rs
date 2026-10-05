//! Port of src/features/source-control/ui/WorktreesPage.tsx: the settings
//! section that lists a project's linked worktrees, creates them, reveals
//! them, and deletes them with or without their sessions.
//!
//! The project picker is a [`SearchableSelect`] over the folder names; the
//! React page used the projects feature's `SearchableProjectPicker`.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use monocode_engine::projects::{GitStatus, GitWatch, WatchKind};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::Worktree;
use crate::model::worktrees::{
    LiveSession, delete_blocked_reason, plural, project_choices, short_head, worktree_session_ids,
    worktree_status_label,
};
use crate::paths::{is_equal_or_inside, path_key, pretty_cwd, project_name};
use crate::scm::Scm;
use crate::ui::common::{palette, spin_icon, with_alpha};
use crate::ui::dialogs::create_worktree::{CreateWorktreeDialog, CreateWorktreeEvent};
use crate::ui::dialogs::delete_worktree::{
    DeleteWorktreeDialog, DeleteWorktreeEvent, RemoveHandler, RemoveRequest,
};
use crate::ui::searchable_select::{SearchableSelect, SelectEvent, SelectOption, SelectVariant};

/// `RemoveWorktree(cwd, path, force, keepSessions?)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeRemoveCall {
    pub cwd: String,
    pub path: String,
    pub force: bool,
    pub keep_sessions: Option<bool>,
}

pub type RemoveWorktree = Rc<dyn Fn(WorktreeRemoveCall, &mut App) -> Task<Result<(), String>>>;
/// `onDeleteSessions`: whether every session was deleted.
pub type DeleteSessions = Rc<dyn Fn(Vec<String>, &mut App) -> Task<Result<bool, String>>>;

pub struct WorktreesPage {
    scm: Scm,
    cwd: String,
    recents: Vec<String>,
    archived: Vec<String>,
    live_sessions: Vec<LiveSession>,
    on_remove: RemoveWorktree,
    on_check_remove: Option<RemoveWorktree>,
    on_delete_sessions: Option<DeleteSessions>,
    project: String,
    project_select: Entity<SearchableSelect>,
    status: Option<Entity<GitStatus>>,
    _watch: Option<GitWatch>,
    error: Option<String>,
    creating: Option<Entity<CreateWorktreeDialog>>,
    deleting: Option<(Worktree, Entity<DeleteWorktreeDialog>)>,
    refreshing_after_failure: bool,
    status_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl WorktreesPage {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        on_remove: RemoveWorktree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let choices = project_choices(&cwd, &[], &[]);
        let project = if cwd == "~" {
            choices.first().cloned().unwrap_or_default()
        } else {
            cwd.clone()
        };
        let project_select = cx.new(|cx| {
            SearchableSelect::new(
                "Switch project",
                project.clone(),
                project_options(&choices),
                "Search projects...",
                window,
                cx,
            )
            .variant(SelectVariant::Pill)
        });
        let subscriptions =
            vec![
                cx.subscribe(&project_select, |this, _, event: &SelectEvent, cx| {
                    let SelectEvent::Change(path) = event;
                    this.select_project(path.clone(), cx);
                }),
            ];
        let mut this = Self {
            scm,
            cwd,
            recents: Vec::new(),
            archived: Vec::new(),
            live_sessions: Vec::new(),
            on_remove,
            on_check_remove: None,
            on_delete_sessions: None,
            project,
            project_select,
            status: None,
            _watch: None,
            error: None,
            creating: None,
            deleting: None,
            refreshing_after_failure: false,
            status_subscription: None,
            _subscriptions: subscriptions,
        };
        this.watch_project(cx);
        this
    }

    /// Recent and archived project folders for the picker.
    pub fn set_projects(
        &mut self,
        recents: Vec<String>,
        archived: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.recents = recents;
        self.archived = archived;
        let options = project_options(&project_choices(&self.cwd, &self.recents, &self.archived));
        self.project_select
            .update(cx, |select, cx| select.set_options(options, cx));
        cx.notify();
    }

    pub fn set_live_sessions(&mut self, sessions: Vec<LiveSession>, cx: &mut Context<Self>) {
        self.live_sessions = sessions;
        cx.notify();
    }

    /// `onCheckRemove`. Defaults to the git preflight.
    pub fn set_check_remove(&mut self, check: Option<RemoveWorktree>) {
        self.on_check_remove = check;
    }

    pub fn set_delete_sessions(&mut self, delete: Option<DeleteSessions>) {
        self.on_delete_sessions = delete;
    }

    // Reading, for tests.

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn deleting(&self) -> Option<&Entity<DeleteWorktreeDialog>> {
        self.deleting.as_ref().map(|(_, dialog)| dialog)
    }

    pub fn creating(&self) -> Option<&Entity<CreateWorktreeDialog>> {
        self.creating.as_ref()
    }

    pub fn project_select(&self) -> &Entity<SearchableSelect> {
        &self.project_select
    }

    fn snapshot(&self, cx: &App) -> (Option<crate::git::Worktrees>, Option<String>) {
        match &self.status {
            Some(status) => {
                let snapshot = status.read(cx).worktrees();
                (snapshot.data.clone(), snapshot.error.clone())
            }
            None => (None, None),
        }
    }

    /// The linked worktrees shown as rows.
    pub fn worktrees(&self, cx: &App) -> Vec<Worktree> {
        self.snapshot(cx)
            .0
            .map(|data| {
                data.worktrees
                    .into_iter()
                    .filter(|tree| !tree.is_main)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Why a row's delete button is off, if it is.
    pub fn delete_disabled(&self, tree: &Worktree, cx: &App) -> bool {
        delete_blocked_reason(tree).is_some()
            || self.refreshing_after_failure
            || self.snapshot(cx).1.is_some()
    }

    pub fn refresh_title(&self, cx: &App) -> String {
        match self.snapshot(cx).1 {
            Some(error) => format!("Refresh failed: {error}. Click to retry."),
            None => "Refresh worktrees".into(),
        }
    }

    // Actions.

    fn watch_project(&mut self, cx: &mut Context<Self>) {
        self._watch = None;
        self.status_subscription = None;
        let valid = !self.project.is_empty() && self.project != "~";
        self.status = valid.then(|| self.scm.status(&self.project, cx));
        if let Some(status) = &self.status {
            self._watch =
                Some(status.update(cx, |status, cx| status.watch(WatchKind::Worktrees, cx)));
            self.status_subscription = Some(cx.observe(status, |_, _, cx| cx.notify()));
        }
    }

    pub fn select_project(&mut self, path: String, cx: &mut Context<Self>) {
        self.project = path.clone();
        self.error = None;
        self.deleting = None;
        self.project_select
            .update(cx, |select, cx| select.set_value(path, cx));
        self.watch_project(cx);
        cx.notify();
    }

    /// `refresh`: resolves to whether the list loaded.
    pub fn refresh(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        match &self.status {
            Some(status) => {
                let load = status.update(cx, |status, cx| status.refresh_worktrees(cx));
                cx.spawn(async move |_, _| load.await)
            }
            None => Task::ready(false),
        }
    }

    pub fn reveal(&mut self, tree: &Worktree, cx: &mut Context<Self>) {
        let path = tree.path.clone();
        let call = self.scm.run(cx, move |git| git.reveal_path(&path));
        cx.spawn(async move |this, cx| {
            if let Err(error) = call.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.snapshot(cx).0.map(|data| data.default_root);
        let project = self.project.clone();
        let dialog = cx.new(|cx| {
            CreateWorktreeDialog::new(self.scm.clone(), project.clone(), project, root, window, cx)
        });
        self._subscriptions.push(cx.subscribe(
            &dialog,
            |this, _, event: &CreateWorktreeEvent, cx| {
                this.creating = None;
                if matches!(event, CreateWorktreeEvent::Created(_)) {
                    this.refresh(cx).detach();
                }
                cx.notify();
            },
        ));
        self.creating = Some(dialog);
        cx.notify();
    }

    pub fn start_delete(&mut self, tree: Worktree, cx: &mut Context<Self>) {
        if self.delete_disabled(&tree, cx) {
            return;
        }
        self.error = None;
        let count = worktree_session_ids(&tree, &self.live_sessions).len();
        let page = cx.entity().downgrade();
        let on_remove: RemoveHandler = Rc::new(
            move |request: RemoveRequest, _: &mut Window, cx: &mut App| match page
                .update(cx, |page, cx| page.remove(request, cx))
            {
                Ok(task) => task,
                Err(error) => Task::ready(Err(error.to_string())),
            },
        );
        let dialog = cx.new(|_| {
            DeleteWorktreeDialog::new(self.project.clone(), tree.clone(), count, on_remove)
        });
        self._subscriptions.push(cx.subscribe(
            &dialog,
            |this, _, event: &DeleteWorktreeEvent, cx| {
                match event {
                    DeleteWorktreeEvent::Deleted => {
                        if let Some((tree, _)) = this.deleting.take()
                            && is_equal_or_inside(&this.project, &tree.path)
                        {
                            let main = this.snapshot(cx).0.and_then(|data| {
                                data.worktrees.into_iter().find(|tree| tree.is_main)
                            });
                            if let Some(main) = main {
                                this.select_project(main.path, cx);
                            }
                        }
                        this.deleting = None;
                        this.refresh(cx).detach();
                    }
                    DeleteWorktreeEvent::Close => this.deleting = None,
                }
                cx.notify();
            },
        ));
        self.deleting = Some((tree, dialog));
        cx.notify();
    }

    /// The dialog's `onRemove`: check the predictable blockers, delete the
    /// sessions when asked, then remove the working copy.
    fn remove(
        &mut self,
        request: RemoveRequest,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some((tree, _)) = &self.deleting else {
            return Task::ready(Err("No worktree selected".into()));
        };
        let session_ids = worktree_session_ids(tree, &self.live_sessions);
        let check = match &self.on_check_remove {
            Some(check) => check(
                WorktreeRemoveCall {
                    cwd: request.cwd.clone(),
                    path: request.path.clone(),
                    force: request.force,
                    keep_sessions: None,
                },
                cx,
            ),
            None => {
                let (cwd, path, force) = (request.cwd.clone(), request.path.clone(), request.force);
                self.scm.run(cx, move |git| {
                    git.git_worktree_check_remove(&cwd, &path, force)
                })
            }
        };
        let on_remove = self.on_remove.clone();
        let on_delete_sessions = self.on_delete_sessions.clone();
        cx.spawn(async move |this, cx| {
            // Check predictable blockers before any conversation is destroyed.
            check.await?;
            let mut sessions_deleted = false;
            let result: Result<(), String> = async {
                if !session_ids.is_empty() && request.delete_sessions {
                    let Some(delete) = on_delete_sessions else {
                        return Err("Sessions still use this worktree and could not be deleted."
                            .to_string());
                    };
                    let task = cx.update(|cx| delete(session_ids.clone(), cx));
                    if !task.await? {
                        return Err(
                            "Some sessions could not be deleted, so the worktree was kept."
                                .to_string(),
                        );
                    }
                    sessions_deleted = true;
                }
                let task = cx.update(|cx| {
                    on_remove(
                        WorktreeRemoveCall {
                            cwd: request.cwd.clone(),
                            path: request.path.clone(),
                            force: request.force,
                            keep_sessions: Some(!request.delete_sessions),
                        },
                        cx,
                    )
                });
                task.await
            }
            .await;
            let Err(error) = result else {
                return Ok(());
            };
            let failure = if sessions_deleted {
                format!("The sessions were deleted, but the worktree was kept. {error}")
            } else {
                error
            };
            // A partial deletion invalidates the dialog's saved session ids.
            // Require a fresh listing before another destructive attempt.
            let refresh = this.update(cx, |this, cx| {
                this.deleting = None;
                this.error = Some(failure.clone());
                this.refreshing_after_failure = true;
                cx.notify();
                this.refresh(cx)
            });
            if let Ok(refresh) = refresh {
                refresh.await;
            }
            let _ = this.update(cx, |this, cx| {
                this.refreshing_after_failure = false;
                cx.notify();
            });
            Err(failure)
        })
    }
}

fn project_options(choices: &[String]) -> Vec<SelectOption> {
    choices
        .iter()
        .map(|path| SelectOption::new(path.clone(), project_name(path)).keywords(path.clone()))
        .collect()
}

impl Render for WorktreesPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let (data, load_error) = self.snapshot(cx);
        let small_button = |id: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(u(6.))
                .rounded(u(6.))
                .px(u(8.))
                .text_px(11.)
        };
        let mut create = small_button("create-worktree")
            .h(u(30.))
            .child(icon(IconName::Plus).size(u(12.)).text_color(c.content))
            .child("Create worktree");
        if data.is_none() {
            create = create.opacity(0.4);
        } else {
            create = create
                .hover(|s| s.bg(theme.content(0.12)))
                .on_click(cx.listener(|this, _, window, cx| this.start_create(window, cx)));
        }
        let refresh_ink = if load_error.is_some() {
            c.danger
        } else {
            theme.content(0.65)
        };
        let mut refresh = small_button("refresh-worktrees")
            .h(u(28.))
            .flex_none()
            .bg(theme.content(0.08))
            .text_color(refresh_ink)
            .tooltip(tooltip(self.refresh_title(cx)))
            .child(
                icon(IconName::RefreshCw)
                    .size(u(14.))
                    .text_color(refresh_ink),
            )
            .child("Refresh");
        if self.project.is_empty() {
            refresh = refresh.opacity(0.4);
        } else {
            refresh = refresh
                .hover(|s| s.bg(theme.content(0.12)))
                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx).detach()));
        }
        let mut page = div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .text_color(c.content)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(u(4.))
                    .child(self.project_select.clone())
                    .child(create),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .child(div().min_w_0().flex_1().text_px(12.).text_color(theme.content(0.50)).child(
                        "Sessions can share a worktree. Deleting one keeps its sessions by default and discards uncommitted changes. Its branch and commits are kept.",
                    ))
                    .child(refresh),
            );
        if let Some(error) = &self.error {
            page = page.child(div().text_px(12.).text_color(c.danger).child(error.clone()));
        }
        let worktrees = self.worktrees(cx);
        if self.project.is_empty() {
            page = page.child(
                div()
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child("Add a project to manage its worktrees."),
            );
        } else if data.is_none() && load_error.is_some() {
            page = page.child(
                div()
                    .text_px(12.)
                    .text_color(c.danger)
                    .child(load_error.clone().unwrap_or_default()),
            );
        } else if data.is_none() {
            page = page.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child(spin_icon(
                        "worktrees-page-loading",
                        16.,
                        theme.content(0.50),
                    ))
                    .child("Loading worktrees…"),
            );
        } else if worktrees.is_empty() {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(12.))
                    .border_1()
                    .border_color(c.stroke)
                    .px(u(16.))
                    .py(u(32.))
                    .child(
                        icon(IconName::FolderTree)
                            .size(u(20.))
                            .text_color(theme.content(0.35)),
                    )
                    .child(div().text_px(13.).medium().child("No additional worktrees"))
                    .child(div().text_px(12.).text_color(theme.content(0.50)).child(
                        "Create a worktree to work on another branch in a separate folder.",
                    )),
            );
        } else {
            let mut list = div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(u(12.))
                .border_1()
                .border_color(c.stroke);
            for (index, tree) in worktrees.iter().enumerate() {
                let count = worktree_session_ids(tree, &self.live_sessions).len();
                let blocked = delete_blocked_reason(tree);
                let mut facts = div()
                    .mt(u(8.))
                    .flex()
                    .flex_wrap()
                    .gap_x(u(12.))
                    .text_px(11.)
                    .text_color(theme.content(0.55))
                    .child(
                        div().child(format!("{count} session{} in this worktree", plural(count))),
                    )
                    .child(
                        div()
                            .when(tree.dirty == Some(true), |el| el.text_color(c.warning))
                            .child(worktree_status_label(tree)),
                    );
                if let Some(unpushed) = tree.unpushed.filter(|n| *n != 0) {
                    facts = facts.child(div().child(format!(
                        "{unpushed} unpublished commit{}",
                        if unpushed == 1 { "" } else { "s" }
                    )));
                }
                if tree.locked {
                    facts = facts.child(div().child("Locked"));
                }
                let mut reveal = div()
                    .id(SharedString::from(format!("reveal-{}", tree.path)))
                    .rounded(u(6.))
                    .p(u(6.))
                    .tooltip(tooltip("Reveal folder"))
                    .child(
                        icon(IconName::FolderOpen)
                            .size(u(16.))
                            .text_color(theme.content(0.40)),
                    );
                if tree.missing {
                    reveal = reveal.opacity(0.3);
                } else {
                    let target = tree.clone();
                    reveal = reveal
                        .hover(|s| s.bg(theme.content(0.08)))
                        .on_click(cx.listener(move |this, _, _, cx| this.reveal(&target, cx)));
                }
                let disabled = self.delete_disabled(tree, cx);
                let mut delete = div()
                    .id(SharedString::from(format!("delete-{}", tree.path)))
                    .rounded(u(6.))
                    .p(u(6.))
                    .tooltip(tooltip(blocked.unwrap_or("Delete worktree")))
                    .child(
                        icon(IconName::Trash2)
                            .size(u(16.))
                            .text_color(theme.content(0.40)),
                    );
                if disabled {
                    delete = delete.opacity(0.25);
                } else {
                    let target = tree.clone();
                    delete =
                        delete
                            .hover(|s| s.bg(with_alpha(palette::red_500(), 0.10)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_delete(target.clone(), cx)
                            }));
                }
                list = list.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(u(12.))
                        .p(u(16.))
                        .when(index > 0, |el| el.border_t_1().border_color(c.stroke))
                        .child(
                            div().mt(u(2.)).child(
                                icon(IconName::FolderTree)
                                    .size(u(16.))
                                    .text_color(theme.content(0.45)),
                            ),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_center()
                                        .gap(u(8.))
                                        .child(
                                            div()
                                                .text_px(13.)
                                                .medium()
                                                .child(project_name(&tree.path)),
                                        )
                                        .when(
                                            path_key(&tree.path) == path_key(&self.project),
                                            |el| {
                                                el.child(
                                                    div()
                                                        .text_px(10.)
                                                        .text_color(theme.content(0.40))
                                                        .child("Selected project folder"),
                                                )
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .mt(u(4.))
                                        .text_px(11.)
                                        .text_color(theme.content(0.40))
                                        .child(pretty_cwd(&tree.path)),
                                )
                                .child(
                                    div()
                                        .mt(u(8.))
                                        .flex()
                                        .items_center()
                                        .gap(u(6.))
                                        .text_px(11.)
                                        .text_color(theme.content(0.55))
                                        .child(
                                            icon(IconName::GitBranch)
                                                .size(u(12.))
                                                .text_color(theme.content(0.55)),
                                        )
                                        .child(div().min_w_0().child(match &tree.branch {
                                            Some(branch) => format!("Current branch: {branch}"),
                                            None => {
                                                format!("Detached at {}", short_head(&tree.head))
                                            }
                                        })),
                                )
                                .child(facts),
                        )
                        .child(reveal)
                        .child(delete),
                );
            }
            page = page.child(list);
        }
        if let Some(data) = &data {
            page = page.child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.40))
                    .child(format!(
                        "New worktrees are created in {}.",
                        pretty_cwd(&data.default_root)
                    )),
            );
        }
        if let Some(dialog) = &self.creating {
            page = page.child(dialog.clone());
        }
        if let Some((_, dialog)) = &self.deleting {
            page = page.child(dialog.clone());
        }
        page
    }
}
