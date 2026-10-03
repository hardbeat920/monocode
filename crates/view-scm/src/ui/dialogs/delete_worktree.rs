//! Port of src/features/source-control/ui/DeleteWorktreeDialog.tsx: what
//! deleting a working copy discards and keeps, with the choice to delete
//! its sessions too.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, EventEmitter, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div,
};
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::Worktree;
use crate::model::worktrees::{delete_button_label, delete_sessions_text, delete_unpushed_text};
use crate::paths::pretty_cwd;
use crate::ui::common::palette;
use crate::ui::dialogs::{DialogButton, dialog_button};

/// `onRemove(cwd, path, force, deleteSessions)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveRequest {
    pub cwd: String,
    pub path: String,
    pub force: bool,
    pub delete_sessions: bool,
}

pub type RemoveHandler =
    Rc<dyn Fn(RemoveRequest, &mut Window, &mut App) -> Task<Result<(), String>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteWorktreeEvent {
    Deleted,
    Close,
}

pub struct DeleteWorktreeDialog {
    cwd: String,
    tree: Worktree,
    session_count: usize,
    on_remove: RemoveHandler,
    busy: bool,
    delete_sessions: bool,
    error: Option<String>,
    task: Option<Task<()>>,
}

impl EventEmitter<DeleteWorktreeEvent> for DeleteWorktreeDialog {}

#[derive(Clone, Copy)]
enum Tone {
    Danger,
    Warn,
    Muted,
}

impl DeleteWorktreeDialog {
    pub fn new(
        cwd: impl Into<String>,
        tree: Worktree,
        session_count: usize,
        on_remove: RemoveHandler,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            tree,
            session_count,
            on_remove,
            busy: false,
            delete_sessions: false,
            error: None,
            task: None,
        }
    }

    pub fn tree(&self) -> &Worktree {
        &self.tree
    }

    pub fn delete_sessions(&self) -> bool {
        self.delete_sessions
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn button_label(&self) -> String {
        delete_button_label(self.session_count, self.delete_sessions)
    }

    /// The consequence lines, in order, for tests and screen readers.
    pub fn consequences(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.session_count > 0 {
            lines.push(delete_sessions_text(
                self.session_count,
                self.delete_sessions,
            ));
        }
        if self.tree.dirty == Some(true) {
            lines.push("All uncommitted and untracked changes here are discarded.".into());
        }
        if self.tree.dirty.is_none() {
            lines.push(
                "Changes could not be checked. Anything uncommitted here is discarded.".into(),
            );
        }
        lines.push(match &self.tree.branch {
            Some(branch) => format!("The {branch} branch and its commits are kept."),
            None => "The branch is kept.".into(),
        });
        if let Some(unpushed) = self.tree.unpushed.filter(|n| *n != 0) {
            lines.push(delete_unpushed_text(unpushed));
        }
        lines
    }

    pub fn toggle_delete_sessions(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.delete_sessions = !self.delete_sessions;
            cx.notify();
        }
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        // Confirmation covers the complete destructive action, including any
        // local changes that appeared after the last status refresh.
        let request = RemoveRequest {
            cwd: self.cwd.clone(),
            path: self.tree.path.clone(),
            force: true,
            delete_sessions: self.delete_sessions,
        };
        let task = (self.on_remove)(request, window, cx);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => cx.emit(DeleteWorktreeEvent::Deleted),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(DeleteWorktreeEvent::Close);
        }
    }
}

fn consequence(
    glyph: IconName,
    tone: Tone,
    body: impl IntoElement,
    theme: &Theme,
) -> impl IntoElement {
    let color: Hsla = match tone {
        Tone::Danger => theme.colors.danger,
        Tone::Warn => theme.colors.warning,
        Tone::Muted => theme.content(0.35),
    };
    div()
        .flex()
        .items_start()
        .gap(u(10.))
        .child(
            div()
                .mt(u(1.))
                .child(icon(glyph).size(u(14.)).text_color(color)),
        )
        .child(div().min_w_0().flex_1().child(body))
}

impl Render for DeleteWorktreeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let tree = self.tree.clone();
        let mut list = div()
            .mt(u(10.))
            .flex()
            .flex_col()
            .gap(u(8.))
            .border_t_1()
            .border_color(theme.content(0.08))
            .pt(u(10.))
            .text_px(12.5)
            .text_color(theme.content(0.75));
        if self.session_count > 0 {
            list = list.child(consequence(
                IconName::MessageSquare,
                if self.delete_sessions {
                    Tone::Danger
                } else {
                    Tone::Muted
                },
                delete_sessions_text(self.session_count, self.delete_sessions),
                &theme,
            ));
        }
        if tree.dirty == Some(true) {
            list = list.child(consequence(
                IconName::FileDiff,
                Tone::Warn,
                "All uncommitted and untracked changes here are discarded.",
                &theme,
            ));
        }
        if tree.dirty.is_none() {
            list = list.child(consequence(
                IconName::CircleAlert,
                Tone::Warn,
                "Changes could not be checked. Anything uncommitted here is discarded.",
                &theme,
            ));
        }
        let branch_line: AnyElement = match &tree.branch {
            Some(branch) => div()
                .flex()
                .flex_wrap()
                .gap(u(4.))
                .child("The")
                .child(div().medium().text_color(c.content).child(branch.clone()))
                .child("branch and its commits are kept.")
                .into_any_element(),
            None => div().child("The branch is kept.").into_any_element(),
        };
        list = list.child(consequence(
            IconName::GitBranch,
            Tone::Muted,
            branch_line,
            &theme,
        ));
        if let Some(unpushed) = tree.unpushed.filter(|n| *n != 0) {
            list = list.child(consequence(
                IconName::CloudUpload,
                Tone::Muted,
                delete_unpushed_text(unpushed),
                &theme,
            ));
        }
        let card = div()
            .rounded(u(8.))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .p(u(12.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(u(10.))
                    .text_px(12.)
                    .text_color(theme.content(0.55))
                    .child(
                        div().mt(u(1.)).child(
                            icon(IconName::Folder)
                                .size(u(14.))
                                .text_color(theme.content(0.35)),
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .font_family(theme.fonts.mono.clone())
                            .child(pretty_cwd(&tree.path)),
                    ),
            )
            .child(list);
        let mut form = div()
            .flex()
            .flex_col()
            .gap(u(14.))
            .p(u(16.))
            .text_px(13.)
            .child(
                div()
                    .text_color(theme.content(0.75))
                    .child("This permanently deletes the working copy and everything inside it."),
            )
            .child(card);
        if self.session_count > 0 {
            let on = self.delete_sessions;
            let mut toggle = div()
                .id("delete-sessions")
                .relative()
                .flex_none()
                .h(u(20.))
                .w(u(36.))
                .rounded_full()
                .bg(if on {
                    palette::red_500()
                } else {
                    theme.content(0.20)
                })
                .child(
                    div()
                        .absolute()
                        .top(u(2.))
                        .left(u(if on { 18. } else { 2. }))
                        .size(u(16.))
                        .rounded_full()
                        .bg(palette::white()),
                );
            if self.busy {
                toggle = toggle.opacity(0.4);
            } else {
                toggle =
                    toggle.on_click(cx.listener(|this, _, _, cx| this.toggle_delete_sessions(cx)));
            }
            form = form.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .rounded(u(8.))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .p(u(12.))
                    .child(
                        div()
                            .text_px(12.5)
                            .text_color(theme.content(0.75))
                            .child("Also delete associated sessions"),
                    )
                    .child(toggle),
            );
        }
        if let Some(error) = &self.error {
            form = form.child(
                div()
                    .text_px(12.5)
                    .text_color(c.danger)
                    .child(error.clone()),
            );
        }
        let this = cx.entity().downgrade();
        let close = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this, cx| this.close(cx));
            }
        };
        let submit = {
            let this = this.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this, cx| this.submit(window, cx));
            }
        };
        let label: SharedString = self.button_label().into();
        form = form.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(dialog_button(
                    "delete-cancel",
                    "Cancel",
                    DialogButton::Ghost,
                    self.busy,
                    false,
                    close,
                    cx,
                ))
                .child(dialog_button(
                    "delete-confirm",
                    label,
                    DialogButton::Danger,
                    self.busy,
                    self.busy,
                    submit,
                    cx,
                )),
        );
        let modal_close = this.clone();
        modal("delete-worktree-dialog", "Delete worktree?")
            .size(ModalSize::Sm)
            .on_close(move |_, cx| {
                let _ = modal_close.update(cx, |this, cx| this.close(cx));
            })
            .child(form)
    }
}
