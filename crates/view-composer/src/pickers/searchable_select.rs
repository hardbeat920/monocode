//! Port of src/shared/ui/SearchableSelect.tsx: a select whose popover can
//! filter its options, in five trigger looks (field, transparent, row,
//! panel, pill).

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, InputState, MoveDown, MoveEnd, MoveHome, MoveUp};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::anchor::{BoundsCell, Side, anchored_popover, popover_layer, popover_surface};
use super::field::plain_input;
use super::style::check_mark;

/// `SearchableSelectOption`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchableSelectOption {
    pub value: SharedString,
    pub label: SharedString,
    /// Extra words the search matches.
    pub keywords: Option<SharedString>,
}

impl SearchableSelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            keywords: None,
        }
    }

    pub fn keywords(mut self, keywords: impl Into<SharedString>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }
}

/// The trigger's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectVariant {
    #[default]
    Field,
    Transparent,
    Row,
    Panel,
    Pill,
}

impl SelectVariant {
    fn compact(self) -> bool {
        matches!(self, SelectVariant::Row | SelectVariant::Pill)
    }
}

/// The options matching `query`, by label and keywords.
pub fn filter_options(
    options: &[SearchableSelectOption],
    query: &str,
) -> Vec<SearchableSelectOption> {
    let needle = monocode_core::js::trim(query).to_lowercase();
    if needle.is_empty() {
        return options.to_vec();
    }
    options
        .iter()
        .filter(|option| {
            format!(
                "{}\n{}",
                option.label,
                option.keywords.as_deref().unwrap_or("")
            )
            .to_lowercase()
            .contains(&needle)
        })
        .cloned()
        .collect()
}

type PickFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct SearchableSelect {
    label: SharedString,
    value: SharedString,
    options: Vec<SearchableSelectOption>,
    placeholder: SharedString,
    search_placeholder: SharedString,
    empty_label: SharedString,
    disabled: bool,
    layer: Option<usize>,
    variant: SelectVariant,
    searchable: bool,
    open: bool,
    query: String,
    active: usize,
    menu_width: Option<f32>,
    focus: FocusHandle,
    search: Entity<InputState>,
    root_bounds: BoundsCell,
    scroll: ScrollHandle,
    on_change: Option<PickFn>,
    _search_events: Subscription,
}

impl SearchableSelect {
    pub fn new(
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
        options: Vec<SearchableSelectOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_placeholder: SharedString = "Search options…".into();
        let placeholder = search_placeholder.clone();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let search_events = cx.subscribe_in(&search, window, |this, search, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = search.read(cx).value().to_string();
                this.active = 0;
                this.scroll.scroll_to_item(0);
                cx.notify();
            }
        });
        Self {
            label: label.into(),
            value: value.into(),
            options,
            placeholder: "Choose an option…".into(),
            search_placeholder,
            empty_label: "No matching options".into(),
            disabled: false,
            layer: None,
            variant: SelectVariant::Field,
            searchable: true,
            open: false,
            query: String::new(),
            active: 0,
            menu_width: None,
            focus: cx.focus_handle(),
            search,
            root_bounds: BoundsCell::default(),
            scroll: ScrollHandle::new(),
            on_change: None,
            _search_events: search_events,
        }
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn variant(mut self, variant: SelectVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn set_search_placeholder(
        &mut self,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_placeholder = text.into();
        let placeholder = self.search_placeholder.clone();
        self.search.update(cx, |search, cx| {
            search.set_placeholder(placeholder, window, cx)
        });
    }

    pub fn empty_label(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_label = text.into();
        self
    }

    /// The paint layer, for a select inside a modal (`LAYER.dialogPopover`).
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = Some(layer);
        self
    }

    pub fn set_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.value = value.into();
        cx.notify();
    }

    pub fn set_options(&mut self, options: Vec<SearchableSelectOption>, cx: &mut Context<Self>) {
        self.options = options;
        self.clamp_active();
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.disabled = disabled;
        if disabled && self.open {
            self.open = false;
            self.clear_query(window, cx);
        }
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The options the open list shows.
    pub fn filtered(&self) -> Vec<SearchableSelectOption> {
        filter_options(&self.options, &self.query)
    }

    /// The trigger's accessible name: `Agent: Codex`.
    pub fn trigger_label(&self) -> String {
        format!(
            "{}: {}",
            self.label,
            self.selected_label().unwrap_or(&self.placeholder)
        )
    }

    fn selected_label(&self) -> Option<&SharedString> {
        self.options
            .iter()
            .find(|option| option.value == self.value)
            .map(|option| &option.label)
    }

    fn clamp_active(&mut self) {
        let len = self.filtered().len();
        self.active = if len == 0 {
            0
        } else {
            self.active.min(len - 1)
        };
    }

    fn clear_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query.clear();
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
    }

    /// `close`: `restore` puts focus back on the trigger.
    pub fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.clear_query(window, cx);
        if restore {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// `openMenu`.
    pub fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.menu_width = self.measure_menu_width(window);
        self.clear_query(window, cx);
        self.active = self
            .options
            .iter()
            .position(|option| option.value == self.value)
            .unwrap_or(0);
        self.scroll.scroll_to_item(self.active);
        self.open = true;
        if self.searchable {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// The menu matches the trigger's width; compact triggers get at least
    /// 240px.
    fn measure_menu_width(&self, window: &Window) -> Option<f32> {
        let root = self
            .root_bounds
            .get()
            .map(|bounds| f32::from(bounds.size.width) / f32::from(window.rem_size()) * 16.0);
        if self.variant.compact() {
            Some(root.unwrap_or(0.0).max(240.0))
        } else {
            root
        }
    }

    pub fn pick(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_change.clone() {
            let value = value.to_string();
            window.defer(cx, move |window, cx| f(&value, window, cx));
        }
        self.close(true, window, cx);
    }

    /// `onSearchKeyDown`: arrows, Home, End, and Enter.
    pub fn list_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let len = self.filtered().len();
        match key {
            "down" => {
                if len > 0 {
                    self.active = (self.active + 1).min(len - 1);
                }
            }
            "up" => {
                if len > 0 {
                    self.active = self.active.saturating_sub(1);
                }
            }
            "home" => self.active = 0,
            "end" => self.active = len.saturating_sub(1),
            "enter" => {
                if let Some(option) = self.filtered().get(self.active).cloned() {
                    self.pick(&option.value, window, cx);
                }
                return true;
            }
            _ => return false,
        }
        self.scroll.scroll_to_item(self.active);
        cx.notify();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if !self.open {
            // The trigger opens on ArrowDown and ArrowUp.
            if matches!(key, "down" | "up") && !self.disabled {
                self.open_menu(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if key == "escape" {
            self.close(true, window, cx);
            cx.stop_propagation();
            return;
        }
        if self.searchable && self.search.focus_handle(cx).is_focused(window) {
            return;
        }
        if self.list_key(key, window, cx) {
            cx.stop_propagation();
        }
    }

    fn render_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.selected_label().cloned();
        let ink = if selected.is_some() {
            theme.colors.content
        } else {
            theme.content(0.40)
        };
        let text = selected.unwrap_or_else(|| self.placeholder.clone());
        let compact = self.variant.compact();
        let mut trigger = div()
            .id("searchable-select-trigger")
            .debug_selector(|| "searchable-select-trigger".into())
            .flex()
            .items_center()
            .text_color(theme.colors.content);
        let fill_hover = theme.content(0.14);
        trigger = match self.variant {
            SelectVariant::Row | SelectVariant::Pill => trigger
                .h(u(28.))
                .max_w_full()
                .gap(u(4.))
                .pl(u(8.))
                .pr(u(6.))
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.10))
                .text_px(theme.text.label)
                .hover(move |s| s.bg(fill_hover)),
            SelectVariant::Panel => {
                let hover = theme.content(0.08);
                trigger
                    .h(u(56.))
                    .w_full()
                    .justify_end()
                    .gap(u(12.))
                    .px(u(16.))
                    .rounded(u(theme.radius.xl))
                    .border_1()
                    .border_color(theme.content(0.06))
                    .bg(theme.content(0.06))
                    .text_px(theme.text.ui)
                    .medium()
                    .hover(move |s| s.bg(hover))
            }
            SelectVariant::Field | SelectVariant::Transparent => {
                let border_hover = theme.content(0.20);
                let el = trigger
                    .h(u(36.))
                    .w_full()
                    .justify_between()
                    .gap(u(8.))
                    .px(u(10.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .text_px(theme.text.body)
                    .hover(move |s| s.border_color(border_hover));
                if self.variant == SelectVariant::Field {
                    el.bg(theme.colors.background_base)
                } else {
                    el
                }
            }
        };
        if self.disabled {
            trigger = trigger.opacity(0.5);
        } else {
            trigger = trigger.on_click(cx.listener(|this, _, window, cx| {
                if this.open {
                    this.close(false, window, cx);
                } else {
                    this.open_menu(window, cx);
                }
            }));
        }
        let mut label = div().min_w_0().truncate().text_color(ink).child(text);
        label = match self.variant {
            SelectVariant::Panel => label.flex_1().text_right(),
            SelectVariant::Row | SelectVariant::Pill => label,
            _ => label.flex_1(),
        };
        trigger.child(label).child(
            icon(if self.open {
                IconName::ChevronUp
            } else {
                IconName::ChevronDown
            })
            .size(u(if compact { 12. } else { 14. }))
            .text_color(theme.content(0.45)),
        )
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let filtered = self.filtered();
        let compact = self.variant.compact();
        let long_menu = self.searchable || self.options.len() > 12;
        let mut content = div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(theme.fonts.sans.clone())
            .debug_selector(|| "searchable-select-menu".into());
        if long_menu {
            content = content.max_h(u(238.));
        }
        if self.searchable {
            content = content.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(8.))
                    .h(u(32.))
                    .px(u(10.))
                    .border_b_1()
                    .border_color(theme.colors.stroke)
                    .child(
                        icon(IconName::Search)
                            .size(u(14.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(u(18.))
                            .text_px(theme.text.label)
                            .text_color(theme.colors.content)
                            .capture_action(cx.listener(
                                |this: &mut Self, _: &MoveDown, window, cx| {
                                    this.list_key("down", window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .capture_action(cx.listener(
                                |this: &mut Self, _: &MoveUp, window, cx| {
                                    this.list_key("up", window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .capture_action(cx.listener(
                                |this: &mut Self, _: &MoveHome, window, cx| {
                                    this.list_key("home", window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .capture_action(cx.listener(
                                |this: &mut Self, _: &MoveEnd, window, cx| {
                                    this.list_key("end", window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .capture_action(cx.listener(
                                |this: &mut Self, _: &Enter, window, cx| {
                                    cx.stop_propagation();
                                    this.list_key("enter", window, cx);
                                },
                            ))
                            .flex()
                            .items_center()
                            .child(plain_input(&self.search, cx)),
                    ),
            );
        }
        let mut list = div()
            .id("searchable-select-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(4.));
        if filtered.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(12.))
                    .text_center()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child(self.empty_label.clone()),
            );
        }
        let hover = theme.content(0.05);
        let hover_ink = theme.colors.content;
        for (index, option) in filtered.into_iter().enumerate() {
            let highlighted = index == self.active;
            let selected = option.value == self.value;
            let picked = option.value.clone();
            let label = option.label.clone();
            list = list.child(
                div()
                    .id(("searchable-select-option", index))
                    .debug_selector(move || format!("searchable-select-option-{label}"))
                    .flex()
                    .flex_none()
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .px(u(8.))
                    .h(u(if compact { 28. } else { 32. }))
                    .rounded(u(theme.radius.md))
                    .text_px(if compact {
                        theme.text.label
                    } else {
                        theme.text.body
                    })
                    .leading(theme.leading.none)
                    .map(|row| {
                        if highlighted {
                            row.bg(theme.colors.selection)
                                .text_color(theme.colors.content)
                        } else {
                            row.text_color(theme.content(0.75))
                                .hover(move |s| s.bg(hover).text_color(hover_ink))
                        }
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.active != index {
                            this.active = index;
                            cx.notify();
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.pick(&picked, window, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(u(14.))
                            .when(selected, |el| el.child(check_mark(1.0, theme).size(u(12.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(option.label.clone()),
                    ),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.root_bounds.contains(event.position) {
                this.close(false, window, cx);
            }
        });
        popover_surface(
            "searchable-select-menu",
            self.menu_width,
            None,
            outside,
            content.child(list),
        )
    }
}

impl Focusable for SearchableSelect {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SearchableSelect {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("searchable-select")
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.root_bounds.probe());
        root = if self.variant.compact() {
            root.flex().flex_none().max_w(u(224.))
        } else {
            root.min_w_0()
        };
        root = root.child(self.render_trigger(&theme, cx));
        if self.open {
            if self.menu_width.is_none() {
                // Opened before the first paint measured the trigger.
                self.menu_width = self.measure_menu_width(window);
            }
            let layer = self.layer.unwrap_or_else(|| popover_layer(cx));
            let menu = self.render_menu(&theme, cx).into_any_element();
            root = root.child(anchored_popover(Side::Bottom, 4., layer, window, menu));
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<SearchableSelectOption> {
        vec![
            SearchableSelectOption::new("codex", "Codex"),
            SearchableSelectOption::new("claude", "Claude").keywords("anthropic sonnet"),
        ]
    }

    #[test]
    fn filters_by_label_and_keywords() {
        let all = options();
        assert_eq!(filter_options(&all, "").len(), 2);
        assert_eq!(filter_options(&all, "  COD ")[0].value, "codex");
        assert_eq!(filter_options(&all, "sonnet")[0].value, "claude");
        assert!(filter_options(&all, "zzz").is_empty());
    }
}
