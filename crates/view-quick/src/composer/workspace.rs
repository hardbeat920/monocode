//! Port of src/features/quick-composer/ui/QuickWorkspaceControls.tsx: the
//! working copy and branch triggers above the prompt. Each opens the git
//! popup, a separate panel, so the composer itself never grows a menu.

use gpui::{
    AnyElement, App, Bounds, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div,
};
use monocode_core::session::WorkspaceMode;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::{QuickComposer, QuickComposerEvent};
use crate::model::git_controls::ShowAction;
use crate::model::launch::{QuickGitAnchor, QuickGitKind, QuickGitRequest, QuickGitResult};

impl QuickComposer {
    /// `enabled`: no start in flight and no list open.
    pub(crate) fn controls_enabled(&self) -> bool {
        !self.busy && self.picker.is_none()
    }

    /// `useProjectBranchesState(cwd, enabled && !!cwd)`: load the branches
    /// of the working copy the controls show.
    pub(crate) fn refresh_branches(&mut self, cx: &mut Context<Self>) {
        let cwd = self.workspace().git_cwd();
        if cwd != self.branches.cwd {
            self.branches.branches = None;
            self.branches.settled = false;
            self.branches.cwd = cwd.clone();
        }
        if cwd.is_empty() {
            self.branches.task = None;
            self.branches.settled = true;
            return;
        }
        let task = self.host.branches(&cwd, cx);
        self.branches.task = Some(cx.spawn(async move |this, cx| {
            let branches = task.await;
            this.update(cx, |this, cx| {
                if this.branches.cwd == cwd {
                    this.branches.branches = branches;
                    this.branches.settled = true;
                    this.branches.task = None;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// `base`: the chosen base, else the current branch, else `HEAD`.
    pub(crate) fn worktree_base(&self) -> String {
        self.workspace()
            .base
            .filter(|base| !base.is_empty())
            .or_else(|| {
                self.branches
                    .branches
                    .as_ref()
                    .and_then(|branches| branches.current.clone())
            })
            .unwrap_or_else(|| "HEAD".into())
    }

    /// `disabled`: off while busy or a list is open, and outside a repo.
    pub fn git_controls_disabled(&self) -> bool {
        !self.controls_enabled()
            || self
                .branches
                .branches
                .as_ref()
                .and_then(|branches| branches.current.as_ref())
                .is_none()
    }

    /// `onOpenChange`.
    fn set_git_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.git_open = open;
        if open {
            self.set_picker(None, cx);
            self.slash = None;
        }
        cx.notify();
    }

    /// `cancel`: drop the live popup request.
    pub(crate) fn cancel_git(&mut self, cx: &mut Context<Self>) {
        let had = self.git.open().is_some() || self.git.active().is_some();
        if let Some(id) = self.git.cancel() {
            cx.emit(QuickComposerEvent::CancelGit(id));
        }
        if had || self.git_open {
            self.git_open = false;
            cx.notify();
        }
    }

    /// `beginClick`: the mouse went down on a trigger.
    pub fn git_begin_click(&mut self, kind: QuickGitKind) {
        self.git.begin_click(kind);
    }

    /// `show`: open the popup for `kind`, or close the one it opened.
    pub fn show_git(&mut self, kind: QuickGitKind, cx: &mut Context<Self>) {
        if self.git_controls_disabled() {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        match self.git.show(kind, id) {
            ShowAction::Close { complete } => {
                if let Some(id) = complete {
                    cx.emit(QuickComposerEvent::CancelGit(id));
                }
                self.set_git_open(false, cx);
            }
            ShowAction::Open { id } => {
                let mut choice = self.workspace();
                choice.base =
                    (choice.mode == WorkspaceMode::Worktree).then(|| self.worktree_base());
                let bounds = self.trigger_bounds[match kind {
                    QuickGitKind::Workspace => 0,
                    _ => 1,
                }];
                let anchor = bounds
                    .map(|bounds| QuickGitAnchor {
                        x: f64::from(f32::from(bounds.origin.x)),
                        y: f64::from(f32::from(bounds.origin.y)),
                        width: f64::from(f32::from(bounds.size.width)),
                        height: f64::from(f32::from(bounds.size.height)),
                    })
                    .unwrap_or(QuickGitAnchor {
                        x: 0.,
                        y: 0.,
                        width: 0.,
                        height: 0.,
                    });
                let request = QuickGitRequest {
                    id,
                    kind,
                    choice,
                    branches: self.branches.branches.clone(),
                    anchor,
                };
                self.set_git_open(true, cx);
                cx.emit(QuickComposerEvent::OpenGit(Box::new(request)));
            }
        }
    }

    /// The popup could not open.
    pub fn git_open_failed(&mut self, id: &str, error: String, cx: &mut Context<Self>) {
        if self.git.open_failed(id) {
            self.set_git_open(false, cx);
            self.error = Some(error);
            cx.notify();
        }
    }

    /// `QUICK_GIT_RESULT`: the popup finished.
    pub fn apply_git_result(
        &mut self,
        result: QuickGitResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(effect) = self.git.result(&result, self.workspace().cwd.as_deref()) else {
            return;
        };
        self.set_git_open(false, cx);
        if let Some(choice) = effect.choice {
            self.workspace_choice = choice;
        }
        self.host.git_changed(cx);
        self.refresh_branches(cx);
        if effect.restore_focus {
            self.focus_prompt(window, cx);
        }
        cx.notify();
    }

    fn trigger_probe(&self, slot: usize, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity().downgrade();
        canvas(
            move |bounds: Bounds<Pixels>, _, cx: &mut App| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, _| this.trigger_bounds[slot] = Some(bounds));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .into_any_element()
    }

    /// The two triggers.
    pub(crate) fn render_workspace_controls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let workspace = self.workspace();
        let worktree = workspace.mode == WorkspaceMode::Worktree;
        let disabled = self.git_controls_disabled();
        let open = self.git.open();
        let settled = self.branches.settled;
        let label = if worktree {
            "New worktree"
        } else {
            "Current checkout"
        };
        let trigger = |id: &'static str, expanded: bool| {
            let hover_bg = theme.content(0.08);
            let hover_ink = theme.colors.content;
            let mut button = div()
                .id(id)
                .relative()
                .ml(u(-6.))
                .flex()
                .h(u(24.))
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .px(u(6.))
                .text_px(12.)
                .text_color(theme.content(0.55));
            if expanded {
                button = button.bg(hover_bg).text_color(hover_ink);
            }
            if disabled {
                button = button.opacity(0.4);
            } else {
                button = button.hover(move |style| style.bg(hover_bg).text_color(hover_ink));
            }
            button
        };
        let ink = |expanded: bool| {
            if expanded {
                theme.colors.content
            } else {
                theme.content(0.55)
            }
        };
        let workspace_open = open == Some(QuickGitKind::Workspace);
        let workspace_button = trigger("quick-workspace", workspace_open)
            .max_w(u(192.))
            .tooltip(monocode_ui::widgets::tooltip(SharedString::from(format!(
                "Workspace {label}"
            ))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.git_begin_click(QuickGitKind::Workspace);
                }),
            )
            .on_click(cx.listener(|this, _, _, cx| this.show_git(QuickGitKind::Workspace, cx)))
            .child(
                icon(if worktree {
                    IconName::FolderTree
                } else {
                    IconName::Folder
                })
                .size(u(14.))
                .text_color(ink(workspace_open)),
            )
            .child(div().min_w_0().truncate().child(label))
            .child(self.trigger_probe(0, cx));

        let branch_kind = if worktree {
            QuickGitKind::Base
        } else {
            QuickGitKind::Branch
        };
        let branch_open = matches!(open, Some(QuickGitKind::Branch | QuickGitKind::Base));
        let current = self
            .branches
            .branches
            .as_ref()
            .and_then(|branches| branches.current.clone());
        let branch_label = if worktree {
            format!("From {}", self.worktree_base())
        } else {
            current.unwrap_or_else(|| {
                if settled {
                    "No repo".into()
                } else {
                    "Loading\u{2026}".into()
                }
            })
        };
        let in_worktree = workspace.in_linked_worktree();
        // `GitPickerTrigger`: the loading skeleton keeps the same line box.
        let text: AnyElement = if !settled {
            div()
                .relative()
                .min_w_0()
                .flex_1()
                .child(div().invisible().child("main"))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(u(6.))
                        .h(u(6.))
                        .rounded_full()
                        .bg(ink(branch_open))
                        .opacity(0.5),
                )
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .child(branch_label.clone())
                .into_any_element()
        };
        let mut branch_button = trigger("quick-branch", branch_open)
            .max_w(u(256.))
            .tooltip(monocode_ui::widgets::tooltip(if worktree {
                SharedString::from(format!("Create worktree from {}", self.worktree_base()))
            } else {
                SharedString::from("Choose branch")
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.git_begin_click(branch_kind);
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.show_git(branch_kind, cx)))
            .child(
                icon(if in_worktree {
                    IconName::FolderTree
                } else {
                    IconName::GitBranch
                })
                .size(u(14.))
                .text_color(ink(branch_open)),
            )
            .child(text)
            .child(self.trigger_probe(1, cx));
        if in_worktree {
            branch_button = branch_button.child(
                div()
                    .flex_none()
                    .rounded(u(theme.radius.sm))
                    .bg(theme.content(0.08))
                    .px(u(4.))
                    .text_px(10.)
                    .text_color(theme.content(0.45))
                    .child("Worktree"),
            );
        }
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .pl(u(6.))
            .child(workspace_button)
            .child(branch_button)
            .into_any_element()
    }
}
