//! Port of src/features/quick-composer/ui/QuickPermissions.tsx: the four
//! runtime modes as a keyboard-driven list. The composer opens it as its own
//! picker from the toolbar's permissions button.

use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_core::RuntimeMode;
use monocode_core::harness::RUNTIME_MODES;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::colors;

/// `ICONS`.
pub fn permission_icon(mode: RuntimeMode) -> IconName {
    match mode {
        RuntimeMode::Supervised => IconName::Lock,
        RuntimeMode::AutoAcceptEdits => IconName::Pencil,
        RuntimeMode::Auto => IconName::Sparkles,
        RuntimeMode::FullAccess => IconName::Shield,
    }
}

/// `QuickPermissionIcon`: full access is amber.
pub fn permission_icon_element(
    mode: RuntimeMode,
    size: f32,
    ink: gpui::Hsla,
    theme: &Theme,
) -> AnyElement {
    let color = if mode == RuntimeMode::FullAccess {
        colors::amber(theme, 0.9)
    } else {
        ink
    };
    icon(permission_icon(mode))
        .size(u(size))
        .text_color(color)
        .into_any_element()
}

/// What the list reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickPermissionsEvent {
    /// `onChange`.
    Change(RuntimeMode),
    /// `onClose`.
    Close,
}

pub struct QuickPermissions {
    focus_handle: FocusHandle,
    value: RuntimeMode,
    active: usize,
}

impl EventEmitter<QuickPermissionsEvent> for QuickPermissions {}

impl Focusable for QuickPermissions {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl QuickPermissions {
    /// Mounting focuses the list, so the arrows and Enter work at once.
    pub fn new(value: RuntimeMode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            focus_handle,
            value,
            active: index_of(value),
        }
    }

    pub fn value(&self) -> RuntimeMode {
        self.value
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// `pick`: report the mode, then close the list.
    pub fn pick(&mut self, mode: RuntimeMode, cx: &mut Context<Self>) {
        self.value = mode;
        cx.emit(QuickPermissionsEvent::Change(mode));
        cx.emit(QuickPermissionsEvent::Close);
        cx.notify();
    }

    /// The list's keys. Returns true when the key was handled.
    pub fn key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let count = RUNTIME_MODES.len();
        match key {
            "escape" => cx.emit(QuickPermissionsEvent::Close),
            "down" | "up" => {
                let step = if key == "down" { 1 } else { count - 1 };
                self.active = (self.active + step) % count;
                cx.notify();
            }
            "enter" | "space" => self.pick(RUNTIME_MODES[self.active], cx),
            _ => return false,
        }
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        if self.key(event.keystroke.key.as_str(), cx) {
            cx.stop_propagation();
        }
    }
}

fn index_of(value: RuntimeMode) -> usize {
    RUNTIME_MODES
        .iter()
        .position(|mode| *mode == value)
        .unwrap_or(0)
}

impl Render for QuickPermissions {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("quick-permissions")
            .key_context("QuickPermissions")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_y_scroll()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .p(u(8.));
        for (index, mode) in RUNTIME_MODES.into_iter().enumerate() {
            let highlighted = self.active == index;
            let ink = if highlighted {
                theme.colors.content
            } else {
                theme.content(0.75)
            };
            let hover = theme.colors.selection_hover;
            let mut row = div()
                .id(SharedString::from(format!(
                    "quick-permission-{}",
                    mode.as_str()
                )))
                .flex()
                .w_full()
                .items_center()
                .gap(u(12.))
                .rounded(u(theme.radius.lg))
                .px(u(12.))
                .py(u(8.))
                .text_color(ink)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.active != index {
                        this.active = index;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.pick(mode, cx)))
                .child(permission_icon_element(mode, 16., ink, &theme))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .child(div().text_px(13.).medium().child(mode.label()))
                        .child(
                            div()
                                .mt(u(2.))
                                .text_px(11.)
                                .text_color(theme.content(0.45))
                                .child(mode.hint()),
                        ),
                );
            row = if highlighted {
                row.bg(theme.colors.selection_emphasis)
            } else {
                row.hover(move |style| style.bg(hover))
            };
            if self.value == mode {
                row = row.child(
                    icon(IconName::Check)
                        .size(u(14.))
                        .text_color(theme.colors.accent),
                );
            }
            root = root.child(row);
        }
        root
    }
}
