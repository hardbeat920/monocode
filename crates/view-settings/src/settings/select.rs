//! Port of `Select` in SettingsView.tsx: a theme-aware dropdown, a trigger
//! button opening a popover listbox. The project background dialog uses the
//! same entity with SearchableSelect's transparent trigger
//! (`searchable={false}`).

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::widgets::popover_frame;

use super::controls::{TriggerBounds, anchored_popover, opens_above};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// Builds an option's leading icon each frame.
pub type OptionIcon = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
    pub icon: Option<OptionIcon>,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: OptionIcon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// The trigger's look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectStyle {
    /// SettingsView's `Select`.
    #[default]
    Settings,
    /// SearchableSelect's `variant="transparent"`, full width.
    Transparent,
}

type ChangeHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct Select {
    label: SharedString,
    value: SharedString,
    options: Vec<SelectOption>,
    style: SelectStyle,
    layer: Option<usize>,
    open: bool,
    active: usize,
    focus: FocusHandle,
    scroll: ScrollHandle,
    trigger_bounds: TriggerBounds,
    on_change: Option<ChangeHandler>,
}

impl Select {
    pub fn new(
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
        options: Vec<SelectOption>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            options,
            style: SelectStyle::Settings,
            layer: None,
            open: false,
            active: 0,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            trigger_bounds: TriggerBounds::default(),
            on_change: None,
        }
    }

    pub fn style(mut self, style: SelectStyle) -> Self {
        self.style = style;
        self
    }

    /// The paint layer, for a select inside a dialog (`LAYER.dialogPopover`).
    pub fn layer(mut self, layer: usize) -> Self {
        self.layer = Some(layer);
        self
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn set_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        let value = value.into();
        if value != self.value {
            self.value = value;
            cx.notify();
        }
    }

    pub fn set_options(&mut self, options: Vec<SelectOption>, cx: &mut Context<Self>) {
        self.options = options;
        self.active = self.active.min(self.options.len().saturating_sub(1));
        cx.notify();
    }

    /// Updates the value and options from the owner's render without a
    /// notify; the select draws later in the same frame.
    pub fn sync(&mut self, value: impl Into<SharedString>, options: Vec<SelectOption>) {
        self.value = value.into();
        self.options = options;
        self.active = self.active.min(self.options.len().saturating_sub(1));
    }

    pub fn value(&self) -> &SharedString {
        &self.value
    }

    pub fn options(&self) -> &[SelectOption] {
        &self.options
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn active(&self) -> usize {
        self.active
    }

    fn selected(&self) -> Option<&SelectOption> {
        self.options
            .iter()
            .find(|option| option.value == self.value)
    }

    /// The trigger's accessible name: `Interface scale: 100%`.
    pub fn trigger_label(&self) -> String {
        let shown = self
            .selected()
            .map(|option| option.label.clone())
            .unwrap_or_else(|| self.value.clone());
        format!("{}: {shown}", self.label)
    }

    fn selected_index(&self) -> usize {
        self.options
            .iter()
            .position(|option| option.value == self.value)
            .unwrap_or(0)
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(false, window, cx);
        } else {
            self.active = self.selected_index();
            self.scroll.scroll_to_item(self.active);
            self.open = true;
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// Closes the list. `restore` puts focus back on the trigger.
    pub fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        if restore {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// `pick`: report the value and close.
    pub fn pick(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.value = value.to_string().into();
        if let Some(on_change) = self.on_change.clone() {
            let value = value.to_string();
            window.defer(cx, move |window, cx| on_change(&value, window, cx));
        }
        self.close(true, window, cx);
    }

    /// `onMenuKey`.
    pub fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let last = self.options.len().saturating_sub(1);
        match key {
            "down" => self.active = (self.active + 1).min(last),
            "up" => self.active = self.active.saturating_sub(1),
            "home" => self.active = 0,
            "end" => self.active = last,
            "tab" => {
                if let Some(option) = self.options.get(self.active).cloned()
                    && option.value != self.value
                {
                    self.pick(&option.value, window, cx);
                } else {
                    self.close(true, window, cx);
                }
                return true;
            }
            "enter" => {
                if let Some(option) = self.options.get(self.active).cloned() {
                    self.pick(&option.value, window, cx);
                }
                return true;
            }
            "escape" => {
                self.close(true, window, cx);
                return true;
            }
            _ => return false,
        }
        self.scroll.scroll_to_item(self.active);
        cx.notify();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || event.keystroke.modifiers.modified() {
            return;
        }
        if self.menu_key(&event.keystroke.key, window, cx) {
            cx.stop_propagation();
        }
    }

    fn render_trigger(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let selected = self.selected().cloned();
        let label = self.label.clone();
        let text = selected
            .as_ref()
            .map(|option| option.label.clone())
            .unwrap_or_else(|| self.value.clone());
        let selected_icon = selected
            .as_ref()
            .and_then(|option| option.icon.clone())
            .map(|icon| icon(window, cx));
        let mut trigger = div()
            .id("settings-select-trigger")
            .flex()
            .items_center()
            .w_full()
            .text_color(theme.colors.content)
            .debug_selector(move || format!("select:{label}"));
        trigger = match self.style {
            SelectStyle::Settings => {
                let hover = theme.content(0.20);
                trigger
                    .justify_between()
                    .gap(u(8.))
                    .px(u(8.))
                    .py(u(4.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.05))
                    .text_px(theme.text.label)
                    .hover(move |s| s.border_color(hover))
            }
            SelectStyle::Transparent => {
                let hover = theme.content(0.20);
                trigger
                    .h(u(36.))
                    .justify_between()
                    .gap(u(8.))
                    .px(u(10.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .text_px(theme.text.body)
                    .hover(move |s| s.border_color(hover))
            }
        };
        trigger
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .when_some(selected_icon, |el, icon| {
                        el.child(
                            div()
                                .flex()
                                .flex_none()
                                .size(u(16.))
                                .items_center()
                                .justify_center()
                                .child(icon),
                        )
                    })
                    .child(div().min_w_0().truncate().child(text)),
            )
            .child(
                icon(if self.open && self.style == SelectStyle::Settings {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(u(14.))
                .text_color(theme.content(0.50)),
            )
            .into_any_element()
    }

    fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let label = self.label.clone();
        let mut list = div()
            .id("settings-select-list")
            .flex()
            .flex_col()
            .p(u(4.))
            .max_h(u(320.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .debug_selector(move || format!("listbox:{label}"));
        for (index, option) in self.options.iter().enumerate() {
            let is_selected = option.value == self.value;
            let highlighted = index == self.active;
            let value = option.value.clone();
            let selector = format!("option:{}:{}", self.label, option.label);
            let icon_element = option.icon.clone().map(|icon| icon(window, cx));
            let mut item = div()
                .id(ElementId::from(index))
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .py(u(6.))
                .rounded(u(theme.radius.lg))
                .text_px(theme.text.label)
                .text_color(theme.colors.content)
                .debug_selector(move || selector)
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(&value, window, cx)));
            if highlighted || is_selected {
                item = item.bg(theme.colors.selection);
            } else {
                let hover = theme.content(0.05);
                item = item.hover(move |s| s.bg(hover));
            }
            item = item
                .when_some(icon_element, |el, icon| {
                    el.child(
                        div()
                            .flex()
                            .flex_none()
                            .size(u(16.))
                            .items_center()
                            .justify_center()
                            .child(icon),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(option.label.clone()),
                )
                .when(is_selected, |el| {
                    el.child(
                        icon(IconName::Check)
                            .size(u(14.))
                            .text_color(theme.colors.content),
                    )
                });
            list = list.child(item);
        }
        let width = match self.style {
            SelectStyle::Settings => 280.0,
            SelectStyle::Transparent => 144.0,
        };
        let frame = popover_frame(ElementId::from(SharedString::from(format!(
            "select-popover-{}",
            self.label
        ))))
        .width(width)
        .child(list);
        let layer = self.layer.unwrap_or_else(|| Theme::of(cx).layer.popover);
        let needed = (self.options.len() as f32 * 30.0 + 10.0).min(330.0);
        let above = opens_above(self.trigger_bounds.get(), needed, window);
        anchored_popover(
            above,
            true,
            layer,
            window,
            div()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close(false, window, cx)))
                .child(frame),
        )
    }
}

impl Focusable for Select {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger = self.render_trigger(window, cx);
        let menu = self.open.then(|| self.render_menu(window, cx));
        let root = div()
            .id("settings-select")
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.trigger_bounds.probe())
            .child(trigger)
            .children(menu);
        match self.style {
            SelectStyle::Settings => root.max_w(u(208.)),
            SelectStyle::Transparent => root.w_full(),
        }
    }
}
