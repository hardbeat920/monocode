//! McpSettings.tsx `McpPicker`: a labeled `h-8` select with optional
//! provider logos, whose list opens in the dialog popover layer and moves
//! with the arrow keys.

use std::rc::Rc;

use gpui::{
    App, Context, ElementId, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};
use monocode_view_composer::pickers::anchor::{
    BoundsCell, Side, anchored_popover, popover_surface,
};

/// One choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpPickerOption {
    pub value: SharedString,
    pub label: SharedString,
    pub logo: Option<ProviderLogo>,
}

type ChangeFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct McpPicker {
    label: SharedString,
    value: SharedString,
    options: Vec<McpPickerOption>,
    open: bool,
    /// The focused option; none until an arrow key moves into the list.
    active: Option<usize>,
    focus: FocusHandle,
    bounds: BoundsCell,
    on_change: Option<ChangeFn>,
}

impl McpPicker {
    pub fn new(
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
        options: Vec<McpPickerOption>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            options,
            open: false,
            active: None,
            focus: cx.focus_handle(),
            bounds: BoundsCell::default(),
            on_change: None,
        }
    }

    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn set_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.value = value.into();
        cx.notify();
    }

    pub fn set_options(&mut self, options: Vec<McpPickerOption>, cx: &mut Context<Self>) {
        self.options = options;
        cx.notify();
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// The trigger's accessible name: `Provider: Claude Code`.
    pub fn trigger_label(&self) -> String {
        let selected = self
            .options
            .iter()
            .find(|option| option.value == self.value)
            .map_or(self.value.clone(), |option| option.label.clone());
        format!("{}: {selected}", self.label)
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = !self.open;
        self.active = None;
        if self.open {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub fn pick(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.value = value.to_string().into();
        self.open = false;
        self.active = None;
        self.focus.focus(window, cx);
        if let Some(f) = self.on_change.clone() {
            let value = value.to_string();
            window.defer(cx, move |window, cx| f(&value, window, cx));
        }
        cx.notify();
    }

    /// Arrow keys move through the options and wrap; the first press lands
    /// on the first (down) or last (up) option.
    pub fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.options.len();
        if count == 0 {
            return;
        }
        self.active = Some(match (self.active, down) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(current), true) => (current + 1) % count,
            (Some(current), false) => (current + count - 1) % count,
        });
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.step(true, cx),
            "up" => self.step(false, cx),
            "enter" | "space" => {
                if let Some(option) = self.active.and_then(|index| self.options.get(index)) {
                    let value = option.value.clone();
                    self.pick(&value, window, cx);
                }
            }
            "escape" => {
                self.open = false;
                self.active = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn logo(logo: Option<ProviderLogo>) -> Option<impl IntoElement> {
        logo.map(|logo| provider_logo(logo).size(14.))
    }
}

impl Focusable for McpPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for McpPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let selected = self
            .options
            .iter()
            .find(|option| option.value == self.value)
            .cloned();
        let label = selected
            .as_ref()
            .map_or(self.value.clone(), |option| option.label.clone());
        let trigger_label = self.trigger_label();
        let border_hover = theme.content(0.20);
        let trigger = div()
            .id("mcp-picker-trigger")
            .debug_selector(move || format!("mcp-picker {trigger_label}"))
            .mt(u(4.))
            .flex()
            .w_full()
            .h(u(32.))
            .items_center()
            .gap(u(8.))
            .px(u(8.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .text_px(theme.text.label)
            .text_color(theme.colors.content)
            .hover(move |s| s.border_color(border_hover))
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .children(Self::logo(selected.as_ref().and_then(|option| option.logo)))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(
                icon(if self.open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(u(14.))
                .text_color(theme.content(0.50)),
            );
        let mut root = div()
            .id(ElementId::Name(format!("mcp-picker-{}", self.label).into()))
            .relative()
            .min_w_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.bounds.probe())
            .child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.65))
                    .child(self.label.clone()),
            )
            .child(trigger);
        if self.open {
            let hover = theme.content(0.05);
            let mut list = div()
                .id("mcp-picker-list")
                .flex()
                .flex_col()
                .max_h(u(320.))
                .overflow_y_scroll()
                .p(u(4.))
                .font_family(theme.fonts.sans.clone());
            for (index, option) in self.options.iter().enumerate() {
                let picked = option.value == self.value;
                let focused = self.active == Some(index);
                let value = option.value.clone();
                let selector = option.label.clone();
                list = list.child(
                    div()
                        .id(("mcp-picker-option", index))
                        .debug_selector(move || format!("mcp-option {selector}"))
                        .flex()
                        .w_full()
                        .items_center()
                        .gap(u(8.))
                        .px(u(8.))
                        .py(u(6.))
                        .rounded(u(theme.radius.lg))
                        .text_px(theme.text.label)
                        .text_color(theme.colors.content)
                        .map(|row| {
                            if picked {
                                row.bg(theme.colors.selection)
                            } else {
                                row.hover(move |s| s.bg(hover))
                            }
                        })
                        .when(focused, |row| {
                            row.border_1().border_color(theme.colors.accent)
                        })
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.pick(&value, window, cx)),
                        )
                        .children(Self::logo(option.logo))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(option.label.clone()),
                        )
                        .when(picked, |row| {
                            row.child(
                                icon(IconName::Check)
                                    .size(u(14.))
                                    .text_color(theme.colors.content),
                            )
                        }),
                );
            }
            let outside = cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !this.bounds.contains(event.position) {
                    this.open = false;
                    this.active = None;
                    cx.notify();
                }
            });
            let menu = popover_surface("mcp-picker-menu", Some(240.), Some(320.), outside, list);
            root = root.child(anchored_popover(
                Side::Bottom,
                6.,
                theme.layer.dialog_popover,
                window,
                menu,
            ));
        }
        root
    }
}
