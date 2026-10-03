//! Port of src/features/sessions/ui/SessionFolderPicker.tsx: the
//! `/add-to-folder` panel. Type to filter the sidebar folders, or name a new
//! one.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveUp};
use monocode_ui::styled::glass_backdrop;

use super::field::plain_input;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// A sidebar folder, from `SessionFolder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFolderRow {
    pub id: SharedString,
    pub name: SharedString,
    /// `sessionIds.length`.
    pub session_count: usize,
}

/// `SessionFolderTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionFolderTarget {
    Existing { folder_id: String },
    New { name: String },
}

/// `rows`: matching folders, then a "Create" row unless the name exists.
pub fn folder_targets(folders: &[SessionFolderRow], query: &str) -> Vec<SessionFolderTarget> {
    let name = monocode_core::js::trim(query);
    let needle = name.to_lowercase();
    let mut rows: Vec<SessionFolderTarget> = folders
        .iter()
        .filter(|folder| needle.is_empty() || folder.name.to_lowercase().contains(&needle))
        .map(|folder| SessionFolderTarget::Existing {
            folder_id: folder.id.to_string(),
        })
        .collect();
    let exact = folders
        .iter()
        .any(|folder| folder.name.to_lowercase() == needle);
    if !name.is_empty() && !exact {
        rows.push(SessionFolderTarget::New {
            name: name.to_string(),
        });
    }
    rows
}

type PickFn = Rc<dyn Fn(&SessionFolderTarget, &mut Window, &mut App)>;
type DismissFn = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct SessionFolderPicker {
    folders: Vec<SessionFolderRow>,
    query: String,
    active: usize,
    input: Entity<InputState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    on_pick: Option<PickFn>,
    on_dismiss: Option<DismissFn>,
    _input_events: Subscription,
}

impl SessionFolderPicker {
    pub fn new(
        folders: Vec<SessionFolderRow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder("Choose or name a session folder…"));
        input.update(cx, |input, cx| input.focus(window, cx));
        let input_events = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                this.active = 0;
                cx.notify();
            }
        });
        Self {
            folders,
            query: String::new(),
            active: 0,
            input,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            on_pick: None,
            on_dismiss: None,
            _input_events: input_events,
        }
    }

    pub fn on_pick(
        mut self,
        f: impl Fn(&SessionFolderTarget, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }

    pub fn on_dismiss(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }

    pub fn set_folders(&mut self, folders: Vec<SessionFolderRow>, cx: &mut Context<Self>) {
        self.folders = folders;
        let len = self.rows().len();
        self.active = if len == 0 {
            0
        } else {
            self.active.min(len - 1)
        };
        cx.notify();
    }

    pub fn rows(&self) -> Vec<SessionFolderTarget> {
        folder_targets(&self.folders, &self.query)
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// Arrows wrap.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let len = self.rows().len();
        if len == 0 {
            return;
        }
        self.active = if down {
            (self.active + 1) % len
        } else {
            (self.active + len - 1) % len
        };
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    pub fn pick(
        &mut self,
        target: SessionFolderTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(f) = self.on_pick.clone() {
            window.defer(cx, move |window, cx| f(&target, window, cx));
        }
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_dismiss.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.dismiss(window, cx);
        }
    }
}

impl Focusable for SessionFolderPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SessionFolderPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let ink = theme.colors.content;
        let close_hover = theme.content(0.10);
        let header = div()
            .relative()
            .flex()
            .items_center()
            .gap(u(8.))
            .px(u(10.))
            .py(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                icon(IconName::Folder)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(u(20.))
                    .text_px(theme.text.body)
                    .text_color(ink)
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveDown, _, cx| {
                        cx.stop_propagation();
                        this.step(true, cx);
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &MoveUp, _, cx| {
                        cx.stop_propagation();
                        this.step(false, cx);
                    }))
                    .capture_action(cx.listener(|this: &mut Self, _: &Enter, window, cx| {
                        cx.stop_propagation();
                        if let Some(target) = this.rows().get(this.active).cloned() {
                            this.pick(target, window, cx);
                        }
                    }))
                    .flex()
                    .items_center()
                    .child(plain_input(&self.input, cx)),
            )
            .child(
                div()
                    .id("session-folder-cancel")
                    .group("session-folder-cancel")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(24.))
                    .rounded(u(theme.radius.md))
                    .hover(move |s| s.bg(close_hover))
                    .tooltip(monocode_ui::widgets::tooltip("Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
                    .child(
                        icon(IconName::X)
                            .size(u(14.))
                            .text_color(theme.content(0.45))
                            .group_hover("session-folder-cancel", move |s| s.text_color(ink)),
                    ),
            );

        let rows = self.rows();
        let mut list = div()
            .id("session-folder-list")
            .relative()
            .flex()
            .flex_col()
            .max_h(u(240.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(4.));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child("Type a name to create the first folder"),
            );
        }
        let hover = theme.content(0.05);
        for (index, row) in rows.into_iter().enumerate() {
            let highlighted = index == self.active;
            let folder = match &row {
                SessionFolderTarget::Existing { folder_id } => self
                    .folders
                    .iter()
                    .find(|folder| folder.id.as_ref() == folder_id)
                    .cloned(),
                SessionFolderTarget::New { .. } => None,
            };
            let label = match (&row, &folder) {
                (_, Some(folder)) => folder.name.to_string(),
                (SessionFolderTarget::New { name }, None) => format!("Create “{name}”"),
                (SessionFolderTarget::Existing { .. }, None) => String::new(),
            };
            let selector = label.clone();
            let picked = row.clone();
            let mut el = div()
                .id(("session-folder-row", index))
                .debug_selector(move || format!("session-folder-{selector}"))
                .flex()
                .flex_none()
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .py(u(8.))
                .rounded(u(theme.radius.md))
                .text_px(theme.text.body)
                .leading(theme.leading.normal)
                .map(|el| {
                    if highlighted {
                        el.bg(monocode_ui::color::with_alpha(theme.colors.skill, 0.15))
                            .text_color(ink)
                    } else {
                        el.text_color(theme.content(0.75))
                            .hover(move |s| s.bg(hover).text_color(ink))
                    }
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.pick(picked.clone(), window, cx)),
                );
            el = match &folder {
                Some(_) => el.child(
                    icon(IconName::Folder)
                        .size(u(14.))
                        .text_color(theme.content(0.50)),
                ),
                None => el.child(
                    icon(IconName::Plus)
                        .size(u(14.))
                        .text_color(theme.colors.skill),
                ),
            };
            el = el
                .child(div().flex_1().min_w_0().truncate().child(label))
                .when_some(folder, |el, folder| {
                    el.child(
                        div()
                            .flex_none()
                            .text_px(theme.text.caption)
                            .tabular()
                            .text_color(theme.content(0.40))
                            .child(folder.session_count.to_string()),
                    )
                });
            list = list.child(el);
        }

        div()
            .id("session-folder-picker")
            .debug_selector(|| "session-folder-picker".into())
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .font_family(theme.fonts.sans.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)))
            .child(header)
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> SessionFolderRow {
        SessionFolderRow {
            id: "work".into(),
            name: "Work".into(),
            session_count: 2,
        }
    }

    #[test]
    fn lists_matching_folders_then_a_create_row() {
        let folders = vec![work()];
        assert_eq!(
            folder_targets(&folders, ""),
            vec![SessionFolderTarget::Existing {
                folder_id: "work".into()
            }]
        );
        assert_eq!(
            folder_targets(&folders, " wor "),
            vec![
                SessionFolderTarget::Existing {
                    folder_id: "work".into()
                },
                SessionFolderTarget::New { name: "wor".into() },
            ]
        );
        // An exact name, in any case, offers no create row.
        assert_eq!(folder_targets(&folders, "WORK").len(), 1);
        assert_eq!(
            folder_targets(&[], "Launch work"),
            vec![SessionFolderTarget::New {
                name: "Launch work".into()
            }]
        );
    }
}
