//! Port of src/features/sessions/ui/McpServerPicker.tsx: the `/mcp` panel
//! above the composer. A search field, the configured servers with usable
//! ones first, and a link to the MCP settings.
//!
//! The rows come from the owner's ranking function, which wraps
//! `mcp_picker_servers` in monocode-engine, so the search here only passes
//! the query through.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveUp};
use monocode_core::HarnessId;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::anchor::DismissReason;
use super::field::plain_input;
use super::model_picker::harness_logo;

/// `McpPickerServer.availability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpAvailability {
    Available,
    Authentication,
    Unavailable,
}

/// One server row, from `McpPickerServer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServerRow {
    /// `${provider}:${scope}:${configPath}:${name}`.
    pub key: SharedString,
    pub name: SharedString,
    /// The provider's logo; Claude Desktop uses Claude's.
    pub icon: HarnessId,
    /// `MCP_PROVIDER_LABELS[provider]`.
    pub provider_label: SharedString,
    pub scope: SharedString,
    pub availability: McpAvailability,
    /// Why an unavailable server cannot be used.
    pub detail: SharedString,
}

impl McpServerRow {
    /// The right-hand status text.
    pub fn status(&self) -> SharedString {
        match self.availability {
            McpAvailability::Authentication => "Needs authentication".into(),
            McpAvailability::Unavailable => self.detail.clone(),
            McpAvailability::Available => "Available".into(),
        }
    }
}

type RowsFn = Rc<dyn Fn(&str) -> Vec<McpServerRow>>;
type PickFn = Rc<dyn Fn(&McpServerRow, &mut Window, &mut App)>;
type ManageFn = Rc<dyn Fn(&mut Window, &mut App)>;
type DismissFn = Rc<dyn Fn(DismissReason, &mut Window, &mut App)>;

/// Indices of the rows that can be picked.
pub fn selectable(rows: &[McpServerRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.availability == McpAvailability::Available)
        .map(|(index, _)| index)
        .collect()
}

pub struct McpServerPicker {
    rows_for: RowsFn,
    rows: Vec<McpServerRow>,
    loading: bool,
    error: SharedString,
    query: String,
    active: usize,
    search: Entity<InputState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    on_pick: Option<PickFn>,
    on_manage: Option<ManageFn>,
    on_dismiss: Option<DismissFn>,
    _search_events: Subscription,
}

impl McpServerPicker {
    /// `rows_for(query)` returns the matching rows, usable ones first.
    pub fn new(
        rows_for: impl Fn(&str) -> Vec<McpServerRow> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search MCP servers…"));
        search.update(cx, |search, cx| search.focus(window, cx));
        let search_events = cx.subscribe_in(&search, window, |this, search, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().to_string();
                this.refresh_rows(cx);
            }
        });
        let mut picker = Self {
            rows_for: Rc::new(rows_for),
            rows: Vec::new(),
            loading: false,
            error: SharedString::default(),
            query: String::new(),
            active: 0,
            search,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            on_pick: None,
            on_manage: None,
            on_dismiss: None,
            _search_events: search_events,
        };
        picker.refresh_rows(cx);
        picker
    }

    pub fn on_pick(mut self, f: impl Fn(&McpServerRow, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }

    pub fn on_manage(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_manage = Some(Rc::new(f));
        self
    }

    pub fn on_dismiss(
        mut self,
        f: impl Fn(DismissReason, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }

    /// New connections, harness, or Claude status: replace the ranking.
    pub fn set_rows_for(
        &mut self,
        rows_for: impl Fn(&str) -> Vec<McpServerRow> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.rows_for = Rc::new(rows_for);
        self.refresh_rows(cx);
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        self.loading = loading;
        cx.notify();
    }

    pub fn set_error(&mut self, error: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = error.into();
        cx.notify();
    }

    pub fn rows(&self) -> &[McpServerRow] {
        &self.rows
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The query, connections, or status changed: highlight the first
    /// usable row.
    fn refresh_rows(&mut self, cx: &mut Context<Self>) {
        self.rows = (self.rows_for)(&self.query);
        self.active = selectable(&self.rows).first().copied().unwrap_or(0);
        cx.notify();
    }

    /// Arrow keys step through usable rows only, wrapping.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let usable = selectable(&self.rows);
        if usable.is_empty() {
            return;
        }
        let len = usable.len() as isize;
        let at = usable
            .iter()
            .position(|index| *index == self.active)
            .map(|at| at as isize)
            .unwrap_or(-1);
        let next = (at + if down { 1 } else { -1 } + len).rem_euclid(len);
        self.active = usable[next as usize];
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    pub fn pick_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.active).cloned()
            && row.availability == McpAvailability::Available
        {
            self.pick(row, window, cx);
        }
    }

    fn pick(&mut self, row: McpServerRow, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_pick.clone() {
            window.defer(cx, move |window, cx| f(&row, window, cx));
        }
    }

    pub fn dismiss(&mut self, reason: DismissReason, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_dismiss.clone() {
            window.defer(cx, move |window, cx| f(reason, window, cx));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.dismiss(DismissReason::Escape, window, cx);
        }
    }
}

impl Focusable for McpServerPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for McpServerPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let close_hover = theme.content(0.08);
        let close_ink = theme.colors.content;
        let search = div()
            .relative()
            .flex()
            .items_center()
            .gap(u(8.))
            .px(u(12.))
            .py(u(8.))
            .border_b_1()
            .border_color(theme.content(0.10))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(u(20.))
                    .text_px(theme.text.body)
                    .text_color(theme.colors.content)
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
                        this.pick_active(window, cx);
                    }))
                    .flex()
                    .items_center()
                    .child(plain_input(&self.search, cx)),
            )
            .child(
                div()
                    .id("mcp-picker-close")
                    .debug_selector(|| "mcp-picker-close".into())
                    .group("mcp-picker-close")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(28.))
                    .rounded(u(theme.radius.md))
                    .hover(move |s| s.bg(close_hover))
                    .tooltip(monocode_ui::widgets::tooltip(
                        "Back to the conversation (Esc)",
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(DismissReason::Escape, window, cx)
                    }))
                    .child(
                        icon(IconName::ChevronDown)
                            .size(u(16.))
                            .text_color(theme.content(0.45))
                            .group_hover("mcp-picker-close", move |s| s.text_color(close_ink)),
                    ),
            );

        let status = |text: SharedString, color| {
            div()
                .px(u(8.))
                .py(u(8.))
                .text_px(theme.text.label)
                .leading(theme.leading.normal)
                .text_color(color)
                .child(text)
        };
        let mut list = div()
            .id("mcp-picker-list")
            .relative()
            .flex()
            .flex_col()
            .max_h(u(184.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(4.));
        if self.loading {
            list = list.child(status("Checking MCP servers…".into(), theme.content(0.50)));
        } else if !self.error.is_empty() {
            list = list.child(status(self.error.clone(), theme.colors.danger));
        } else if self.rows.is_empty() {
            let text = if self.query.is_empty() {
                "No MCP servers found"
            } else {
                "No matching MCP servers"
            };
            list = list.child(status(text.into(), theme.content(0.50)));
        } else {
            let hover = theme.content(0.05);
            for (index, row) in self.rows.iter().enumerate() {
                let available = row.availability == McpAvailability::Available;
                let active = available && index == self.active;
                let status_ink = if row.availability == McpAvailability::Authentication {
                    theme.colors.warning
                } else {
                    theme.content(0.45)
                };
                let name = row.name.clone();
                let picked = row.clone();
                list = list.child(
                    div()
                        .id(("mcp-row", index))
                        .debug_selector(move || format!("mcp-row-{name}"))
                        .flex()
                        .flex_none()
                        .w_full()
                        .items_center()
                        .gap(u(8.))
                        .h(u(44.))
                        .px(u(8.))
                        .py(u(6.))
                        .rounded(u(theme.radius.md))
                        .map(|el| {
                            if !available {
                                el.text_color(theme.content(0.40))
                            } else if active {
                                el.bg(theme.colors.selection)
                                    .text_color(theme.colors.content)
                            } else {
                                el.text_color(theme.colors.content)
                                    .hover(move |s| s.bg(hover))
                            }
                        })
                        .when(available, |el| {
                            el.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && this.active != index {
                                    this.active = index;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(
                                move |this, _, window, cx| this.pick(picked.clone(), window, cx),
                            ))
                        })
                        .child(provider_logo(harness_logo(row.icon)).size(16.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .truncate()
                                        .text_px(theme.text.body)
                                        .leading(theme.leading.normal)
                                        .child(row.name.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_px(theme.text.caption)
                                        .leading(theme.leading.normal)
                                        .text_color(theme.content(0.45))
                                        .child(format!("{} · {}", row.provider_label, row.scope)),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .max_w(gpui::relative(0.4))
                                .truncate()
                                .text_px(theme.text.caption)
                                .text_color(status_ink)
                                .child(row.status()),
                        ),
                );
            }
        }

        let manage_hover = theme.content(0.05);
        let manage = div()
            .id("mcp-picker-manage")
            .debug_selector(|| "mcp-picker-manage".into())
            .relative()
            .flex()
            .w_full()
            .items_center()
            .gap(u(8.))
            .px(u(12.))
            .py(u(8.))
            .border_t_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .text_color(theme.content(0.65))
            .group("mcp-picker-manage")
            .hover(move |s| s.bg(manage_hover).text_color(close_ink))
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(f) = this.on_manage.clone() {
                    window.defer(cx, move |window, cx| f(window, cx));
                }
            }))
            .child(
                icon(IconName::Settings)
                    .size(u(14.))
                    .text_color(theme.content(0.65))
                    .group_hover("mcp-picker-manage", move |s| s.text_color(close_ink)),
            )
            .child("Manage MCP Servers…");

        div()
            .id("mcp-server-picker")
            .debug_selector(|| "mcp-server-picker".into())
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .font_family(theme.fonts.sans.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.dismiss(DismissReason::Outside, window, cx)),
            )
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)))
            .child(search)
            .child(list)
            .child(manage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, availability: McpAvailability) -> McpServerRow {
        McpServerRow {
            key: format!("claude:user::{name}").into(),
            name: name.to_string().into(),
            icon: HarnessId::Claude,
            provider_label: "Claude Code".into(),
            scope: "user".into(),
            availability,
            detail: "Different provider".into(),
        }
    }

    #[test]
    fn status_text_follows_availability() {
        assert_eq!(row("a", McpAvailability::Available).status(), "Available");
        assert_eq!(
            row("a", McpAvailability::Authentication).status(),
            "Needs authentication"
        );
        assert_eq!(
            row("a", McpAvailability::Unavailable).status(),
            "Different provider"
        );
    }

    #[test]
    fn only_available_rows_are_selectable() {
        let rows = vec![
            row("a", McpAvailability::Available),
            row("b", McpAvailability::Authentication),
            row("c", McpAvailability::Available),
            row("d", McpAvailability::Unavailable),
        ];
        assert_eq!(selectable(&rows), vec![0, 2]);
    }
}
