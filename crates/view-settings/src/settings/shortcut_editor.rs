//! Port of `ShortcutEditor` and `shortcutModifier` in SettingsView.tsx: the
//! key cell that records a new chord.
//!
//! The webview listened on the window in the capture phase while recording,
//! so app shortcuts never fired mid-recording. GPUI's keystroke interceptor
//! runs before key bindings in the same way. Modifier presses arrive as
//! modifier changes, which update the preview.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Keystroke, ModifiersChangedEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, deferred, div,
    prelude::FluentBuilder as _, relative,
};
use monocode_core::Platform;
use monocode_core::shortcut::{
    Modifiers, ShortcutEvent, quick_composer_shortcut_preview, shortcut_from_key_event,
};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

/// What a recorder asks its owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutRequest {
    /// Use this chord, in stored form (`Command+Shift+KeyM`).
    Apply(String),
    /// Delete was pressed on its own.
    Disable,
    /// The reset button.
    Reset,
}

/// Runs a request. An error is shown under the cell.
pub type ShortcutAction =
    Rc<dyn Fn(ShortcutRequest, &mut Window, &mut App) -> Task<Result<(), String>>>;

/// `KeyboardEvent.code` for a GPUI key name, so recorded chords keep the
/// stored form the TypeScript used. Shifted symbols map to their key.
pub fn code_for_key(key: &str) -> Option<String> {
    let mut chars = key.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphabetic() {
            return Some(format!("Key{}", ch.to_ascii_uppercase()));
        }
        if ch.is_ascii_digit() {
            return Some(format!("Digit{ch}"));
        }
        let code = match ch {
            '`' | '~' => "Backquote",
            '\\' | '|' => "Backslash",
            '[' | '{' => "BracketLeft",
            ']' | '}' => "BracketRight",
            ',' | '<' => "Comma",
            '=' | '+' => "Equal",
            '-' | '_' => "Minus",
            '.' | '>' => "Period",
            '\'' | '"' => "Quote",
            ';' | ':' => "Semicolon",
            '/' | '?' => "Slash",
            '!' => "Digit1",
            '@' => "Digit2",
            '#' => "Digit3",
            '$' => "Digit4",
            '%' => "Digit5",
            '^' => "Digit6",
            '&' => "Digit7",
            '*' => "Digit8",
            '(' => "Digit9",
            ')' => "Digit0",
            ' ' => "Space",
            _ => return None,
        };
        return Some(code.into());
    }
    let code = match key {
        "space" => "Space",
        "enter" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "escape" => "Escape",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => {
            let digits = key.strip_prefix('f')?;
            let n: u8 = digits.parse().ok()?;
            return (1..=24).contains(&n).then(|| format!("F{n}"));
        }
    };
    Some(code.into())
}

/// GPUI modifier state in the TypeScript's shape. `platform` is ⌘ on macOS
/// and the Windows or Super key elsewhere, as `metaKey` was.
pub fn modifiers_of(modifiers: &gpui::Modifiers) -> Modifiers {
    Modifiers {
        meta_key: modifiers.platform,
        ctrl_key: modifiers.control,
        alt_key: modifiers.alt,
        shift_key: modifiers.shift,
    }
}

fn union(a: Modifiers, b: Modifiers) -> Modifiers {
    Modifiers {
        meta_key: a.meta_key || b.meta_key,
        ctrl_key: a.ctrl_key || b.ctrl_key,
        alt_key: a.alt_key || b.alt_key,
        shift_key: a.shift_key || b.shift_key,
    }
}

pub struct ShortcutEditor {
    name: SharedString,
    display: Option<SharedString>,
    reset_visible: bool,
    recording: bool,
    busy: bool,
    error: Option<String>,
    preview: String,
    held: Modifiers,
    platform: Platform,
    focus: FocusHandle,
    action: ShortcutAction,
    job: Option<Task<()>>,
    intercept: Option<Subscription>,
    _blur: Subscription,
}

impl ShortcutEditor {
    pub fn new(
        name: impl Into<SharedString>,
        platform: Platform,
        action: ShortcutAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.stop_recording(cx);
        });
        Self {
            name: name.into(),
            display: None,
            reset_visible: false,
            recording: false,
            busy: false,
            error: None,
            preview: String::new(),
            held: Modifiers::default(),
            platform,
            focus,
            action,
            job: None,
            intercept: None,
            _blur: blur,
        }
    }

    /// The owner's props: the current label (`None` when disabled) and
    /// whether reset is offered.
    pub fn set_state(&mut self, display: Option<SharedString>, reset_visible: bool) {
        self.display = display;
        self.reset_visible = reset_visible;
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// What the cell shows.
    pub fn value(&self) -> String {
        if self.recording || self.busy {
            if self.preview.is_empty() {
                "Record…".into()
            } else {
                self.preview.clone()
            }
        } else {
            self.display
                .as_ref()
                .map(|display| display.to_string())
                .unwrap_or_else(|| "Disabled".into())
        }
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording {
            self.recording = false;
            self.intercept = None;
            cx.notify();
        }
    }

    /// `beginRecording`.
    pub fn begin_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.held = Modifiers::default();
        self.preview.clear();
        self.error = None;
        self.recording = true;
        self.focus.focus(window, cx);
        let this = cx.entity().downgrade();
        let handle = window.window_handle();
        self.intercept = Some(cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != handle {
                return;
            }
            let keystroke = event.keystroke.clone();
            let handled = this
                .update(cx, |this, cx| this.on_key_down(&keystroke, window, cx))
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
        cx.notify();
    }

    /// `run`: stop recording, then perform the request.
    pub fn run(&mut self, request: ShortcutRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = false;
        self.intercept = None;
        self.busy = true;
        self.error = None;
        let task = (self.action)(request, window, cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(error) = result {
                    this.error = Some(error);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The capture-phase `keydown` handler. Returns whether the key was
    /// consumed.
    pub fn on_key_down(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.recording {
            return false;
        }
        let pressed = modifiers_of(&keystroke.modifiers);
        let bare = !pressed.meta_key && !pressed.ctrl_key && !pressed.alt_key && !pressed.shift_key;
        let code = code_for_key(&keystroke.key).unwrap_or_else(|| keystroke.key.clone());
        // An unmodified Tab leaves the recorder instead of trapping focus.
        if bare && code == "Tab" {
            self.stop_recording(cx);
            return false;
        }
        if code == "Escape" {
            self.recording = false;
            self.intercept = None;
            self.error = None;
            cx.notify();
            return true;
        }
        // Delete disables, but only on its own so Cmd+Delete still records.
        if bare && (code == "Backspace" || code == "Delete") {
            self.run(ShortcutRequest::Disable, window, cx);
            return true;
        }
        let modifiers = union(pressed, self.held);
        let key = keystroke
            .key_char
            .clone()
            .unwrap_or_else(|| keystroke.key.clone());
        self.preview =
            quick_composer_shortcut_preview(modifiers, Some(&code), Some(&key), self.platform);
        cx.notify();
        let next = shortcut_from_key_event(&ShortcutEvent { code, modifiers });
        if let Some(next) = next {
            self.run(ShortcutRequest::Apply(next), window, cx);
        }
        true
    }

    /// Modifier presses and releases: the held modifiers and the preview.
    pub fn on_modifiers_changed(&mut self, modifiers: &gpui::Modifiers, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        self.held = modifiers_of(modifiers);
        self.preview = quick_composer_shortcut_preview(self.held, None, None, self.platform);
        cx.notify();
    }

    fn render_error(&self, cx: &App) -> Option<AnyElement> {
        let error = self.error.clone()?;
        let theme = Theme::of(cx);
        let name = self.name.clone();
        Some(
            div()
                .absolute()
                .top(relative(1.))
                .left_0()
                .child(
                    deferred(
                        div()
                            .mt(u(6.))
                            .max_w(u(256.))
                            .px(u(8.))
                            .py(u(4.))
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.10))
                            .bg(gpui::Hsla {
                                a: 0.95,
                                ..theme.colors.background_base
                            })
                            .shadow_lg()
                            .whitespace_nowrap()
                            .text_px(theme.text.caption)
                            .text_color(theme.colors.danger)
                            .debug_selector(move || format!("shortcut-error:{name}"))
                            .child(error),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}

impl Focusable for ShortcutEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ShortcutEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let disabled = self.display.is_none();
        let name = self.name.clone();
        let mut cell = div()
            .id("shortcut-cell")
            .flex()
            .items_center()
            .h(u(24.))
            .w(u(112.))
            .flex_none()
            .px(u(6.))
            .rounded(u(theme.radius.md))
            .border_1()
            .font_family(theme.fonts.mono.clone())
            .text_px(theme.text.caption)
            .leading(theme.leading.none)
            .overflow_hidden()
            .whitespace_nowrap()
            .debug_selector(move || format!("shortcut:{name}"))
            .on_click(cx.listener(|this, _, window, cx| this.begin_recording(window, cx)))
            .child(div().truncate().child(self.value()));
        cell = if self.recording {
            cell.border_color(theme.colors.accent)
        } else {
            cell.border_color(theme.content(0.15))
        };
        cell = if disabled && !self.recording {
            cell.border_dashed().text_color(theme.content(0.35))
        } else {
            let hover = theme.content(0.10);
            cell.text_color(theme.content(0.80))
                .hover(move |s| s.bg(hover))
        };
        if self.busy {
            cell = cell.opacity(0.5);
        }
        let reset = self.reset_visible.then(|| {
            let name = self.name.clone();
            let hover_fill = theme.content(0.10);
            let hover_ink = theme.colors.content;
            let mut button = div()
                .id("shortcut-reset")
                .px(u(4.))
                .py(u(4.))
                .rounded(u(theme.radius.md))
                .text_color(theme.content(0.35))
                .debug_selector(move || format!("shortcut-reset:{name}"))
                .child(
                    icon(IconName::RotateCcw)
                        .size(u(14.))
                        .text_color(theme.content(0.35)),
                );
            if self.busy {
                button = button.opacity(0.5);
            } else {
                button = button
                    .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run(ShortcutRequest::Reset, window, cx)
                    }));
            }
            button
        });
        let hint = self.recording.then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(relative(1.))
                .mr(u(12.))
                .flex()
                .items_center()
                .text_px(theme.text.micro)
                .whitespace_nowrap()
                .text_color(theme.content(0.50))
                .debug_selector(|| "shortcut-hint".into())
                .child("Del disables · Esc cancels")
        });
        let error = self.render_error(cx);
        div()
            .id(ElementId::from(self.name.clone()))
            .relative()
            .w(u(160.))
            .flex_none()
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.on_modifiers_changed(&event.modifiers, cx)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(2.))
                    .child(cell)
                    .children(reset),
            )
            .when_some(hint, |el, hint| el.child(hint))
            .children(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_gpui_keys_to_keyboard_codes() {
        assert_eq!(code_for_key("m").as_deref(), Some("KeyM"));
        assert_eq!(code_for_key("7").as_deref(), Some("Digit7"));
        assert_eq!(code_for_key("{").as_deref(), Some("BracketLeft"));
        assert_eq!(code_for_key("delete").as_deref(), Some("Delete"));
        assert_eq!(code_for_key("backspace").as_deref(), Some("Backspace"));
        assert_eq!(code_for_key("f12").as_deref(), Some("F12"));
        assert_eq!(code_for_key("f25"), None);
        assert_eq!(code_for_key("up").as_deref(), Some("ArrowUp"));
        assert_eq!(code_for_key("capslock"), None);
    }
}
