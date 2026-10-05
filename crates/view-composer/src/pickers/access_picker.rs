//! Port of src/features/sessions/ui/AccessPicker.tsx: the runtime mode
//! (supervised, auto-accept edits, auto, full access) as a toolbar pill
//! with a listbox popover.

use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseDownEvent, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};
use monocode_core::{RUNTIME_MODES, RuntimeMode};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::anchor::{BoundsCell, Side, anchored_popover, popover_layer, popover_surface};
use super::model_picker::CloseFn;
use super::style::{pill_chevron, pill_label, toolbar_pill};

/// `MENU_WIDTH`.
const MENU_WIDTH: f32 = 288.0;

type ModeFn = Rc<dyn Fn(RuntimeMode, &mut Window, &mut App)>;

/// `ICONS`.
pub fn mode_icon(mode: RuntimeMode) -> IconName {
    match mode {
        RuntimeMode::Supervised => IconName::Lock,
        RuntimeMode::AutoAcceptEdits => IconName::Pencil,
        RuntimeMode::Auto => IconName::Sparkles,
        RuntimeMode::FullAccess => IconName::Shield,
    }
}

/// The trigger's tooltip: the mode's hint, and a note while a turn runs.
pub fn access_title(mode: RuntimeMode, busy: bool) -> String {
    format!(
        "{}{}",
        mode.hint(),
        if busy {
            " Changes apply to the next turn."
        } else {
            ""
        }
    )
}

fn index_of(mode: RuntimeMode) -> usize {
    RUNTIME_MODES
        .iter()
        .position(|item| *item == mode)
        .unwrap_or(0)
}

pub struct AccessPicker {
    value: RuntimeMode,
    busy: bool,
    open: bool,
    active: usize,
    focus: FocusHandle,
    trigger_bounds: BoundsCell,
    on_change: Option<ModeFn>,
    on_close: Option<CloseFn>,
}

impl AccessPicker {
    pub fn new(value: RuntimeMode, cx: &mut Context<Self>) -> Self {
        Self {
            value,
            busy: false,
            open: false,
            active: index_of(value),
            focus: cx.focus_handle(),
            trigger_bounds: BoundsCell::default(),
            on_change: None,
            on_close: None,
        }
    }

    pub fn on_change(mut self, f: impl Fn(RuntimeMode, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn set_value(&mut self, value: RuntimeMode, cx: &mut Context<Self>) {
        self.value = value;
        if self.open {
            self.active = index_of(value);
        }
        cx.notify();
    }

    /// A turn is running, so a change applies to the next one.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn active(&self) -> usize {
        self.active
    }

    fn dismiss(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        if restore && let Some(f) = self.on_close.clone() {
            window.defer(cx, move |window, cx| f(window, cx));
        }
        cx.notify();
    }

    /// The trigger's click.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.dismiss(true, window, cx);
            return;
        }
        self.open = true;
        self.active = index_of(self.value);
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub fn pick(&mut self, mode: RuntimeMode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_change.clone() {
            window.defer(cx, move |window, cx| f(mode, window, cx));
        }
        self.dismiss(true, window, cx);
    }

    /// `onMenuKey`, plus Escape.
    pub fn key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.open {
            return false;
        }
        match key {
            "down" => self.active = (self.active + 1).min(RUNTIME_MODES.len() - 1),
            "up" => self.active = self.active.saturating_sub(1),
            "enter" => {
                if let Some(mode) = RUNTIME_MODES.get(self.active).copied() {
                    self.pick(mode, window, cx);
                }
            }
            "escape" => self.dismiss(true, window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        if self.key(event.keystroke.key.as_str(), window, cx) {
            cx.stop_propagation();
        }
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut list = div()
            .flex()
            .flex_col()
            .p(u(4.))
            .debug_selector(|| "access-menu".into());
        let hover = theme.content(0.05);
        for (index, mode) in RUNTIME_MODES.into_iter().enumerate() {
            let selected = mode == self.value;
            let highlighted = index == self.active;
            let glyph_ink = if mode == RuntimeMode::FullAccess {
                monocode_ui::color::with_alpha(theme.colors.warning, 0.9)
            } else {
                theme.content(0.70)
            };
            let row = div()
                .id(("access-option", index))
                .flex()
                .w_full()
                .items_start()
                .gap(u(10.))
                .px(u(8.))
                .py(u(8.))
                .rounded(u(theme.radius.lg))
                .text_color(theme.colors.content)
                .debug_selector(move || format!("access-option-{}", mode.as_str()))
                .map(|row| {
                    if highlighted || selected {
                        row.bg(theme.colors.selection)
                    } else {
                        row.hover(move |s| s.bg(hover))
                    }
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(mode, window, cx)))
                .child(
                    icon(mode_icon(mode))
                        .mt(u(2.))
                        .size(u(14.))
                        .text_color(glyph_ink),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .text_px(theme.text.body)
                                .medium()
                                .line_height(u(20.))
                                .child(mode.label()),
                        )
                        .child(
                            div()
                                .mt(u(2.))
                                .text_px(theme.text.caption)
                                .line_height(u(16.))
                                .text_color(theme.content(0.50))
                                .child(mode.hint()),
                        ),
                );
            list = list.child(row);
        }
        if self.busy {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(6.))
                    .text_px(theme.text.caption)
                    .line_height(u(16.))
                    .text_color(theme.content(0.50))
                    .child(
                        "Access changes apply to the next turn. Stop and resend to apply them now.",
                    ),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, window, cx| {
            if !this.trigger_bounds.contains(event.position) {
                this.dismiss(false, window, cx);
            }
        });
        popover_surface(
            "access-menu",
            Some(MENU_WIDTH),
            None,
            outside,
            list.font_family(theme.fonts.sans.clone()),
        )
    }
}

impl Focusable for AccessPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AccessPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let value = self.value;
        let glyph_ink = if value == RuntimeMode::FullAccess {
            monocode_ui::color::with_alpha(theme.colors.warning, 0.9)
        } else {
            theme.colors.content
        };
        let trigger = toolbar_pill("access-picker-trigger", self.open, &theme)
            .max_w(u(208.))
            .debug_selector(|| "access-picker-trigger".into())
            .tooltip(monocode_ui::widgets::tooltip(access_title(
                value, self.busy,
            )))
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(self.trigger_bounds.probe())
            .child(icon(mode_icon(value)).size(u(14.)).text_color(glyph_ink))
            .child(pill_label(value.label(), None, &theme))
            .child(pill_chevron(self.open, &theme));
        let mut root = div()
            .id("access-picker")
            .relative()
            .flex_none()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(trigger);
        if self.open {
            let menu = self.render_menu(&theme, cx).into_any_element();
            root = root.child(anchored_popover(
                Side::Top,
                super::anchor::DEFAULT_GAP,
                popover_layer(cx),
                window,
                menu,
            ));
        }
        root
    }
}
