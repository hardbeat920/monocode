//! Port of src/features/source-control/ui/SwitchBranchDialog.tsx: git
//! refused a checkout over local changes, so offer to stash them or commit
//! them on the current branch first.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, deferred, div, prelude::FluentBuilder as _, relative,
};
use gpui_component::input::{Enter, Escape, InputEvent, TextareaState};
use monocode_ui::styled::glass_backdrop;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::hooks::CommitMessageRequest;
use crate::paths::MOD;
use crate::scm::Scm;
use crate::ui::common::{palette, plain_textarea, spin_icon, with_alpha};
use crate::ui::dialogs::{DialogButton, dialog_button, error_line};

/// Which resolution is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchBusy {
    Stash,
    Commit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchBranchEvent {
    Stash,
    Commit(String),
    Cancel,
}

pub struct SwitchBranchDialog {
    scm: Scm,
    cwd: String,
    branch: String,
    creating: bool,
    busy: Option<SwitchBusy>,
    error: Option<String>,
    message: Entity<TextareaState>,
    generate: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SwitchBranchEvent> for SwitchBranchDialog {}

impl SwitchBranchDialog {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        branch: impl Into<String>,
        creating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .placeholder(format!("Message ({MOD}↩ to commit)"))
        });
        message.update(cx, |state, cx| state.focus(window, cx));
        let subscriptions = vec![cx.subscribe(&message, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        Self {
            scm,
            cwd: cwd.into(),
            branch: branch.into(),
            creating,
            busy: None,
            error: None,
            message,
            generate: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_state(
        &mut self,
        busy: Option<SwitchBusy>,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.busy = busy;
        self.error = error;
        cx.notify();
    }

    pub fn message(&self, cx: &gpui::App) -> String {
        self.message.read(cx).value().to_string()
    }

    pub fn generating(&self) -> bool {
        self.generate.is_some()
    }

    fn trimmed(&self, cx: &gpui::App) -> String {
        self.message(cx).trim().to_string()
    }

    pub fn can_commit(&self, cx: &gpui::App) -> bool {
        !self.trimmed(cx).is_empty() && self.busy.is_none() && self.generate.is_none()
    }

    pub fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() || self.generate.is_some() {
            return;
        }
        let Some(generator) = self.scm.hooks.generate_commit_message.clone() else {
            return;
        };
        let task = generator(
            CommitMessageRequest {
                cwd: self.cwd.clone(),
                text_harness: None,
            },
            cx,
        );
        self.message
            .update(cx, |state, cx| state.set_disabled(true, cx));
        self.generate = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.generate = None;
                this.message.update(cx, |state, cx| {
                    state.set_disabled(false, cx);
                    if let Ok(text) = &result {
                        state.set_value(text.clone(), window, cx);
                    }
                    state.focus(window, cx);
                });
                if let Err(error) = result {
                    this.scm.hooks.alert(error, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn cancel_generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generate = None;
        self.message.update(cx, |state, cx| {
            state.set_disabled(false, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    pub fn stash(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_none() && self.generate.is_none() {
            cx.emit(SwitchBranchEvent::Stash);
        }
    }

    pub fn commit(&mut self, cx: &mut Context<Self>) {
        if self.can_commit(cx) {
            cx.emit(SwitchBranchEvent::Commit(self.trimmed(cx)));
        }
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_none() && self.generate.is_none() {
            cx.emit(SwitchBranchEvent::Cancel);
        }
    }
}

impl Render for SwitchBranchDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let generating = self.generate.is_some();
        let busy = self.busy.is_some();
        let detail = if self.creating {
            format!(
                "Creating “{}” would overwrite your local changes. Stash them for later, or commit them on this branch first.",
                self.branch
            )
        } else {
            format!(
                "Switching to “{}” would overwrite your local changes. Stash them for later, or commit them on this branch first.",
                self.branch
            )
        };
        let wand_title = if generating {
            "Cancel commit message generation"
        } else {
            "Generate commit message"
        };
        let mut wand = div()
            .id("switch-generate")
            .group("switch-generate")
            .absolute()
            .top(u(4.))
            .right(u(4.))
            .flex()
            .size(u(20.))
            .items_center()
            .justify_center()
            .rounded(u(6.))
            .bg(theme.content(0.10))
            .tooltip(tooltip(wand_title));
        if busy {
            wand = wand.opacity(0.4);
        } else {
            wand = wand
                .hover(|s| s.bg(theme.content(0.20)))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.generate.is_some() {
                        this.cancel_generate(window, cx);
                    } else {
                        this.generate(window, cx);
                    }
                }));
        }
        wand = if generating {
            wand.child(
                div()
                    .group_hover("switch-generate", |s| s.invisible())
                    .child(spin_icon("switch-generate-spin", 14., c.content)),
            )
            .child(
                div()
                    .absolute()
                    .invisible()
                    .group_hover("switch-generate", |s| s.visible())
                    .child(icon(IconName::X).size(u(14.)).text_color(c.content)),
            )
        } else {
            wand.child(
                icon(IconName::WandSparkles)
                    .size(u(12.))
                    .text_color(c.content),
            )
        };
        let message_box = div()
            .relative()
            .capture_action(cx.listener(|this, action: &Enter, _, cx| {
                if action.secondary && this.can_commit(cx) {
                    this.commit(cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(
                div()
                    .w_full()
                    .rounded(u(6.))
                    .bg(theme.content(0.10))
                    .py(u(4.))
                    .pl(u(8.))
                    .pr(u(32.))
                    .text_px(13.)
                    .line_height(u(20.))
                    .when(busy || generating, |el| el.opacity(0.4))
                    .child(plain_textarea(&self.message, cx)),
            )
            .child(wand);
        let this = cx.entity().downgrade();
        let on = |action: fn(&mut Self, &mut Context<Self>)| {
            let this = this.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, action);
            }
        };
        let can_commit = self.can_commit(cx);
        let mut panel = div()
            .id("switch-branch-dialog")
            .relative()
            .flex()
            .flex_col()
            .gap(u(12.))
            .w(u(420.))
            .max_w_full()
            .rounded(u(8.))
            .border_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .overflow_hidden()
            .p(u(16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_action(cx.listener(|this, _: &Escape, _, cx| this.cancel(cx)))
            .child(glass_backdrop(8., 24., with_alpha(c.background_base, 0.55)))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(theme.content(0.05)),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(u(4.))
                    .child(
                        div()
                            .text_px(13.)
                            .medium()
                            .text_color(c.content)
                            .child("Uncommitted changes"),
                    )
                    .child(
                        div()
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .child(detail),
                    ),
            )
            .child(message_box);
        if let Some(error) = &self.error {
            panel = panel.child(div().relative().child(error_line(error.clone(), cx)));
        }
        panel = panel.child(
            div()
                .relative()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap(u(8.))
                .child(dialog_button(
                    "switch-cancel",
                    "Cancel",
                    DialogButton::Ghost,
                    busy || generating,
                    false,
                    on(Self::cancel),
                    cx,
                ))
                .child(dialog_button(
                    "switch-commit",
                    "Commit & switch",
                    DialogButton::Secondary,
                    !can_commit,
                    self.busy == Some(SwitchBusy::Commit),
                    on(Self::commit),
                    cx,
                ))
                .child(dialog_button(
                    "switch-stash",
                    "Stash & switch",
                    DialogButton::Primary,
                    busy || generating,
                    self.busy == Some(SwitchBusy::Stash),
                    on(Self::stash),
                    cx,
                )),
        );
        let backdrop = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(with_alpha(palette::black(), 0.30))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            );
        deferred(
            div()
                .id("switch-branch-layer")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(backdrop)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .px(u(12.))
                        .child(div().flex_none().h(relative(0.22)))
                        .child(panel),
                ),
        )
        .with_priority(theme.layer.dialog)
    }
}
