//! Port of src/shared/ui/SearchableSelect.tsx, the `field` and `pill`
//! variants the worktree views use: a trigger showing the chosen label and a
//! popover with a search box and a keyboard-driven option list.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, bare_input, contains, css_px, track_bounds,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub keywords: Option<String>,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            keywords: None,
        }
    }

    pub fn keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectVariant {
    /// A 36px bordered field (`bg-background-base`).
    #[default]
    Field,
    /// A 30px filled pill, sized to its label.
    Pill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectEvent {
    Change(String),
}

pub struct SearchableSelect {
    label: SharedString,
    value: String,
    options: Vec<SelectOption>,
    placeholder: SharedString,
    search_placeholder: SharedString,
    empty_label: SharedString,
    disabled: bool,
    variant: SelectVariant,
    layer: Option<usize>,
    open: bool,
    query: Entity<InputState>,
    active: usize,
    scroll: ScrollHandle,
    bounds: BoundsCell,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SelectEvent> for SearchableSelect {}

impl SearchableSelect {
    pub fn new(
        label: impl Into<SharedString>,
        value: impl Into<String>,
        options: Vec<SelectOption>,
        search_placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_placeholder: SharedString = search_placeholder.into();
        let query = {
            let placeholder = search_placeholder.clone();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.active = 0;
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.enter(window, cx),
                        _ => {}
                    }
                }),
            ];
        Self {
            label: label.into(),
            value: value.into(),
            options,
            placeholder: "Choose an option…".into(),
            search_placeholder,
            empty_label: "No matching options".into(),
            disabled: false,
            variant: SelectVariant::Field,
            layer: None,
            open: false,
            query,
            active: 0,
            scroll: ScrollHandle::new(),
            bounds: BoundsCell::default(),
            _subscriptions: subscriptions,
        }
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn empty_label(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_label = text.into();
        self
    }

    pub fn variant(mut self, variant: SelectVariant) -> Self {
        self.variant = variant;
        self
    }

    /// The popover layer, for a select inside a dialog.
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = Some(layer);
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn search_placeholder(&self) -> &SharedString {
        &self.search_placeholder
    }

    /// The trigger's accessible name: `<label>: <chosen or placeholder>`.
    pub fn trigger_label(&self) -> String {
        let shown = self
            .selected()
            .map(|option| option.label.clone())
            .unwrap_or_else(|| self.placeholder.to_string());
        format!("{}: {shown}", self.label)
    }

    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.value = value.into();
        cx.notify();
    }

    pub fn set_options(&mut self, options: Vec<SelectOption>, cx: &mut Context<Self>) {
        if self.options != options {
            self.options = options;
            cx.notify();
        }
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled == disabled {
            return;
        }
        self.disabled = disabled;
        if disabled {
            self.open = false;
        }
        cx.notify();
    }

    fn selected(&self) -> Option<&SelectOption> {
        self.options
            .iter()
            .find(|option| option.value == self.value)
    }

    /// Options matching the search.
    pub fn filtered(&self, cx: &gpui::App) -> Vec<SelectOption> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        if query.is_empty() {
            return self.options.clone();
        }
        self.options
            .iter()
            .filter(|option| {
                format!(
                    "{}\n{}",
                    option.label,
                    option.keywords.as_deref().unwrap_or("")
                )
                .to_lowercase()
                .contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.active = self
            .options
            .iter()
            .position(|option| option.value == self.value)
            .unwrap_or(0);
        self.open = true;
        self.query.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    pub fn pick(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.value = value.clone();
        cx.emit(SelectEvent::Change(value));
        self.close(window, cx);
    }

    /// Arrow keys in the search.
    pub fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.filtered(cx).len();
        if len == 0 {
            return;
        }
        self.active = (self.active as isize + delta).clamp(0, len as isize - 1) as usize;
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    /// Enter picks the highlighted option. With no match it does nothing and
    /// keeps the menu open.
    pub fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filtered = self.filtered(cx);
        if let Some(option) = filtered.get(self.active) {
            self.pick(option.value.clone(), window, cx);
        }
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }
}

impl Render for SearchableSelect {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let selected = self.selected().map(|option| option.label.clone());
        let pill = self.variant == SelectVariant::Pill;
        let mut trigger = div()
            .id("select-trigger")
            .flex()
            .items_center()
            .gap(u(if pill { 4. } else { 8. }))
            .rounded(u(6.));
        trigger = if pill {
            trigger
                .h(u(30.))
                .max_w_full()
                .bg(theme.content(0.05))
                .pl(u(10.))
                .pr(u(8.))
                .text_px(13.)
                .hover(|s| s.bg(theme.content(0.12)))
        } else {
            trigger
                .h(u(36.))
                .w_full()
                .justify_between()
                .border_1()
                .border_color(theme.content(if self.open { 0.25 } else { 0.10 }))
                .bg(c.background_base)
                .px(u(10.))
                .text_px(13.)
                .hover(|s| s.border_color(theme.content(0.20)))
        };
        if self.disabled {
            trigger = trigger.opacity(0.5);
        } else {
            trigger = trigger.on_click(cx.listener(|this, _, window, cx| {
                if this.open {
                    this.close(window, cx);
                } else {
                    this.open_menu(window, cx);
                }
            }));
        }
        trigger = trigger
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(!pill, |el| el.flex_1())
                    .text_color(if selected.is_some() {
                        c.content
                    } else {
                        theme.content(0.40)
                    })
                    .child(selected.unwrap_or_else(|| self.placeholder.to_string())),
            )
            .child(
                icon(if self.open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(u(if pill { 12. } else { 14. }))
                .text_color(theme.content(0.45)),
            );
        let mut root = div()
            .relative()
            .min_w_0()
            .when(pill, |el| el.flex().flex_none())
            .child(track_bounds(&self.bounds))
            .child(trigger);
        if !self.open {
            return root;
        }
        let width = self
            .bounds
            .get()
            .map(|bounds| css_px(bounds.size.width, window))
            .unwrap_or(240.);
        let width = if pill { width.max(240.) } else { width };
        let filtered = self.filtered(cx);
        let search = div()
            .flex()
            .flex_none()
            .h(u(32.))
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .px(u(10.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(bare_input(&self.query, 12., cx)),
            );
        let mut list = div()
            .id("select-options")
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
                    .flex()
                    .justify_center()
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child(self.empty_label.clone()),
            );
        }
        for (index, option) in filtered.into_iter().enumerate() {
            let highlighted = index == self.active;
            let is_selected = option.value == self.value;
            let value = option.value.clone();
            let mut row = div()
                .id(SharedString::from(format!("option-{}", option.value)))
                .flex()
                .flex_none()
                .h(u(if pill { 28. } else { 32. }))
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(6.))
                .px(u(8.))
                .text_px(if pill { 12. } else { 13. })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.pick(value.clone(), window, cx)),
                );
            row = if highlighted {
                row.bg(c.selection).text_color(c.content)
            } else {
                row.text_color(theme.content(0.75))
                    .hover(|s| s.bg(theme.content(0.05)).text_color(c.content))
            };
            row = row
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .size(u(14.))
                        .items_center()
                        .justify_center()
                        .when(is_selected, |el| {
                            el.child(icon(IconName::Check).size(u(12.)).text_color(c.content))
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .child(option.label.clone()),
                );
            list = list.child(row);
        }
        let frame = div()
            .id("select-popover")
            .occlude()
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_active(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_active(1, cx)))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| this.close(window, cx)))
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if !contains(&this.bounds, event.position) {
                    this.close(window, cx);
                }
            }))
            .child(
                popover_frame("select-frame")
                    .width(width)
                    .max_height(240.)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .max_h(u(240.))
                            .child(search)
                            .child(list),
                    ),
            );
        let layer = self.layer.unwrap_or(theme.layer.popover);
        root = root.child(anchored_popover(
            PopoverPlacement::BottomStart,
            4.,
            layer,
            frame,
            window,
        ));
        root
    }
}
