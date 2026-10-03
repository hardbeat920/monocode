//! Port of src/features/source-control/ui/CreateWorktreeDialog.tsx.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_engine::projects::{GitStatus, GitWatch, WatchKind};
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{Theme, UiStyled as _, u};

use crate::git::{GitBranches, Worktree};
use crate::paths::pretty_cwd;
use crate::scm::Scm;
use crate::ui::common::field_input;
use crate::ui::dialogs::{DialogButton, dialog_button};
use crate::ui::searchable_select::{SearchableSelect, SelectEvent, SelectOption};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateWorktreeEvent {
    Created(Worktree),
    Cancel,
}

pub struct CreateWorktreeDialog {
    scm: Scm,
    cwd: String,
    base_cwd: String,
    default_root: Option<String>,
    status: Option<Entity<GitStatus>>,
    _watch: Option<GitWatch>,
    name: Entity<InputState>,
    branch_type: Entity<SearchableSelect>,
    existing_branch: Entity<SearchableSelect>,
    start_from: Entity<SearchableSelect>,
    base: String,
    existing: bool,
    existing_name: String,
    busy: bool,
    error: Option<String>,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CreateWorktreeEvent> for CreateWorktreeDialog {}

/// `localBranches`.
pub fn local_branch_options(branches: Option<&GitBranches>) -> Vec<SelectOption> {
    branches
        .map(|b| b.branches.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter(|branch| branch.remote.is_none())
        .map(|branch| SelectOption::new(branch.name.clone(), branch.name.clone()))
        .collect()
}

/// `baseOptions`: the current commit, then every branch as a ref.
pub fn base_options(branches: Option<&GitBranches>) -> Vec<SelectOption> {
    let current = branches
        .and_then(|b| b.current.as_deref())
        .map(|current| format!(" ({current})"))
        .unwrap_or_default();
    let mut options = vec![
        SelectOption::new("HEAD", format!("Current commit{current}"))
            .keywords("HEAD current commit"),
    ];
    for branch in branches.map(|b| b.branches.as_slice()).unwrap_or(&[]) {
        let r = match &branch.remote {
            Some(remote) => format!("{remote}/{}", branch.name),
            None => branch.name.clone(),
        };
        options.push(SelectOption::new(r.clone(), r));
    }
    options
}

impl CreateWorktreeDialog {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        base_cwd: impl Into<String>,
        default_root: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let base_cwd = base_cwd.into();
        let valid = !base_cwd.is_empty() && base_cwd != "~";
        let status = valid.then(|| scm.status(&base_cwd, cx));
        let watch = status
            .as_ref()
            .map(|status| status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx)));
        let branches = status
            .as_ref()
            .and_then(|status| status.read(cx).branches().cloned());
        let layer = Theme::of(cx).layer.dialog_popover;
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-task"));
        name.update(cx, |state, cx| state.focus(window, cx));
        let branch_type = cx.new(|cx| {
            SearchableSelect::new(
                "Branch type",
                "new",
                vec![
                    SelectOption::new("new", "Create a new branch"),
                    SelectOption::new("existing", "Use an existing local branch"),
                ],
                "Search options…",
                window,
                cx,
            )
            .layer(layer)
        });
        let existing_branch = cx.new(|cx| {
            SearchableSelect::new(
                "Existing branch",
                "",
                local_branch_options(branches.as_ref()),
                "Search local branches…",
                window,
                cx,
            )
            .placeholder("Choose a branch…")
            .empty_label("No matching local branches")
            .layer(layer)
        });
        let start_from = cx.new(|cx| {
            SearchableSelect::new(
                "Start from",
                "HEAD",
                base_options(branches.as_ref()),
                "Search branches and refs…",
                window,
                cx,
            )
            .layer(layer)
        });
        let mut subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &branch_type,
                window,
                |this, _, event: &SelectEvent, window, cx| {
                    let SelectEvent::Change(value) = event;
                    this.existing = value == "existing";
                    this.existing_name.clear();
                    this.name
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    this.existing_branch
                        .update(cx, |select, cx| select.set_value("", cx));
                    if !this.existing {
                        this.name.update(cx, |state, cx| state.focus(window, cx));
                    }
                    cx.notify();
                },
            ),
            cx.subscribe(&existing_branch, |this, _, event: &SelectEvent, cx| {
                let SelectEvent::Change(value) = event;
                this.existing_name = value.clone();
                cx.notify();
            }),
            cx.subscribe(&start_from, |this, _, event: &SelectEvent, cx| {
                let SelectEvent::Change(value) = event;
                this.base = value.clone();
                cx.notify();
            }),
        ];
        if let Some(status) = &status {
            subscriptions.push(cx.observe(status, |this, status, cx| {
                let branches = status.read(cx).branches().cloned();
                this.existing_branch.update(cx, |select, cx| {
                    select.set_options(local_branch_options(branches.as_ref()), cx)
                });
                this.start_from.update(cx, |select, cx| {
                    select.set_options(base_options(branches.as_ref()), cx)
                });
            }));
        }
        Self {
            scm,
            cwd,
            base_cwd,
            default_root,
            status,
            _watch: watch,
            name,
            branch_type,
            existing_branch,
            start_from,
            base: "HEAD".into(),
            existing: false,
            existing_name: String::new(),
            busy: false,
            error: None,
            task: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn branch_type(&self) -> &Entity<SearchableSelect> {
        &self.branch_type
    }

    pub fn existing_branch(&self) -> &Entity<SearchableSelect> {
        &self.existing_branch
    }

    pub fn start_from(&self) -> &Entity<SearchableSelect> {
        &self.start_from
    }

    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn branch_name(&self, cx: &gpui::App) -> String {
        if self.existing {
            self.existing_name.trim().to_string()
        } else {
            self.name.read(cx).value().trim().to_string()
        }
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.branch_name(cx);
        if self.busy || name.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.sync_selects(cx);
        let (base_cwd, base, existing) = (self.base_cwd.clone(), self.base.clone(), self.existing);
        let call = self.scm.run(cx, move |git| {
            git.git_worktree_create(&base_cwd, &name, &base, existing)
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.sync_selects(cx);
                match result {
                    Ok(tree) => {
                        this.scm.notify_git_changed(cx);
                        cx.emit(CreateWorktreeEvent::Created(tree));
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(CreateWorktreeEvent::Cancel);
        }
    }

    fn sync_selects(&mut self, cx: &mut Context<Self>) {
        let busy = self.busy;
        for select in [&self.branch_type, &self.existing_branch, &self.start_from] {
            select.update(cx, |select, cx| select.set_disabled(busy, cx));
        }
        self.name
            .update(cx, |state, cx| state.set_disabled(busy, cx));
    }
}

impl Render for CreateWorktreeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let busy = self.busy;
        let label = |text: &'static str| div().child(text);
        let group = |title: &'static str, body: gpui::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(u(6.))
                .text_px(12.)
                .text_color(theme.content(0.70))
                .child(label(title))
                .child(body)
        };
        let mut form = div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .p(u(16.))
            .child(div().text_px(12.).text_color(theme.content(0.55)).child(format!(
                "An independent working copy of {}. Existing uncommitted changes stay in their current working copy.",
                pretty_cwd(&self.cwd)
            )))
            .child(group("Branch", self.branch_type.clone().into_any_element()));
        let name_body = if self.existing {
            self.existing_branch.clone().into_any_element()
        } else {
            field_input(&self.name, theme.colors.background_base, window, cx)
                .when(busy, |el| el.opacity(0.5))
                .into_any_element()
        };
        form = form.child(group(
            if self.existing {
                "Existing branch"
            } else {
                "New branch name"
            },
            name_body,
        ));
        if !self.existing {
            form = form.child(group(
                "Start from",
                self.start_from.clone().into_any_element(),
            ));
        }
        if let Some(root) = &self.default_root {
            form = form.child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.40))
                    .child(format!("Created in {}", pretty_cwd(root))),
            );
        }
        if let Some(error) = &self.error {
            form = form.child(
                div()
                    .text_px(12.)
                    .text_color(theme.colors.danger)
                    .child(error.clone()),
            );
        }
        let this = cx.entity().downgrade();
        let cancel = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| this.cancel(cx));
            }
        };
        let submit = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| this.submit(cx));
            }
        };
        let empty = self.branch_name(cx).is_empty();
        form = form.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(dialog_button(
                    "worktree-cancel",
                    "Cancel",
                    DialogButton::Ghost,
                    busy,
                    false,
                    cancel,
                    cx,
                ))
                .child(dialog_button(
                    "worktree-create",
                    "Create worktree",
                    DialogButton::Primary,
                    busy || empty,
                    busy,
                    submit,
                    cx,
                )),
        );
        let close = this.clone();
        modal("create-worktree-dialog", "Create worktree")
            .size(ModalSize::Sm)
            .on_close(move |_, cx| {
                let _ = close.update(cx, |this, cx| this.cancel(cx));
            })
            .child(form)
    }
}

impl CreateWorktreeDialog {
    /// The status entity the branch lists come from.
    pub fn status(&self) -> Option<&Entity<GitStatus>> {
        self.status.as_ref()
    }
}
