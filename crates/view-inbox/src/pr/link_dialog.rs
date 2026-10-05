//! Port of src/features/sessions/ui/LinkSessionWorkItemDialog.tsx: the
//! modal that links a session to a GitHub issue or pull request URL, edits
//! that link, or removes it.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, Input, InputEvent, InputState};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::widgets::{ModalSize, modal};
use monocode_ui::{Theme, UiStyled as _, u};

use crate::data::LinkedWorkItem;
use crate::model::parse_github_work_item_url;

/// What the dialog decided.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkDialogEvent {
    /// Save this link, or remove the link with `None`.
    Save(Option<LinkedWorkItem>),
    Close,
}

pub struct LinkSessionWorkItemDialog {
    initial: Option<LinkedWorkItem>,
    session_title: String,
    url: Entity<InputState>,
    error: String,
    animate: bool,
    _events: Subscription,
}

impl EventEmitter<LinkDialogEvent> for LinkSessionWorkItemDialog {}

impl LinkSessionWorkItemDialog {
    pub fn new(
        initial: Option<LinkedWorkItem>,
        session_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let value = initial
            .as_ref()
            .map(|item| item.url.clone())
            .unwrap_or_default();
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://github.com/owner/repo/pull/123")
                .default_value(value)
        });
        if crate::autofocus() {
            url.update(cx, |url, cx| url.focus(window, cx));
        }
        let events = cx.subscribe(&url, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && !this.error.is_empty() {
                this.error.clear();
                cx.notify();
            }
        });
        Self {
            initial,
            session_title,
            url,
            error: String::new(),
            animate: true,
            _events: events,
        }
    }

    /// Turns the open animation off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    /// Types into the URL field, for tests.
    pub fn set_url(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.url
            .update(cx, |field, cx| field.set_value(url.to_string(), window, cx));
    }

    /// The Link button and Enter.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.url.read(cx).value().trim().to_string();
        match parse_github_work_item_url(&text) {
            Some(item) => cx.emit(LinkDialogEvent::Save(Some(item))),
            None => {
                self.error = "Enter a valid GitHub issue or pull request URL.".into();
                cx.notify();
            }
        }
    }
}

impl Render for LinkSessionWorkItemDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let editing = self.initial.is_some();
        let has_error = !self.error.is_empty();
        let red = theme.colors.danger;
        let field = div()
            .flex()
            .h(u(36.))
            .items_center()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(if has_error {
                with_alpha(red, 0.6)
            } else {
                theme.content(0.10)
            })
            .bg(theme.content(0.05))
            .px(u(10.))
            .text_px(theme.text.body)
            .text_color(theme.colors.content)
            .capture_action(cx.listener(|this, _: &Enter, _, cx| {
                cx.stop_propagation();
                this.submit(cx);
            }))
            .child(
                Input::new(&self.url)
                    .appearance(false)
                    .p_0()
                    .text_px(theme.text.body),
            );
        let hint = if has_error {
            div()
                .text_px(theme.text.caption)
                .text_color(red)
                .child(self.error.clone())
        } else {
            div()
                .text_px(theme.text.caption)
                .text_color(theme.content(0.45))
                .child(
                    "Paste the full github.com URL. The linked item will appear on the session card.",
                )
        };
        let quiet_hover = theme.content(0.08);
        let red_hover = with_alpha(red, 0.10);
        let accent = theme.colors.accent;
        let buttons = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(u(8.))
            .when(editing, |row| {
                row.child(
                    div()
                        .id("link-remove")
                        .mr_auto()
                        .rounded(u(theme.radius.md))
                        .px(u(12.))
                        .py(u(6.))
                        .text_color(red)
                        .hover(move |s| s.bg(red_hover))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(LinkDialogEvent::Save(None))))
                        .child("Remove link"),
                )
            })
            .child(
                div()
                    .id("link-cancel")
                    .rounded(u(theme.radius.md))
                    .px(u(12.))
                    .py(u(6.))
                    .hover(move |s| s.bg(quiet_hover))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(LinkDialogEvent::Close)))
                    .child("Cancel"),
            )
            .child(
                div()
                    .id("link-submit")
                    .rounded(u(theme.radius.md))
                    .bg(accent)
                    .px(u(12.))
                    .py(u(6.))
                    .medium()
                    .text_color(hex(0xffffff))
                    .hover(move |s| s.bg(accent.blend(with_alpha(hex(0xffffff), 0.1))))
                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx)))
                    .child(if editing { "Update link" } else { "Link" }),
            );
        let entity = cx.entity().downgrade();
        modal(
            "link-work-item",
            if editing {
                "Edit GitHub link"
            } else {
                "Link GitHub issue or PR"
            },
        )
        .description(self.session_title.clone())
        .size(ModalSize::Sm)
        .animate(self.animate)
        .on_close(move |_, cx| {
            if let Some(dialog) = entity.upgrade() {
                dialog.update(cx, |_, cx| cx.emit(LinkDialogEvent::Close));
            }
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap(u(16.))
                .p(u(16.))
                .text_px(theme.text.label)
                .text_color(theme.colors.content)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(u(6.))
                        .child(
                            div()
                                .medium()
                                .text_color(theme.content(0.80))
                                .child("Issue or pull request URL"),
                        )
                        .child(field)
                        .child(hint),
                )
                .child(buttons),
        )
    }
}
