//! The terminal view entity: owns the emulator and the PTY, turns keys,
//! IME input, the mouse, and the clipboard into PTY bytes, and emits
//! [`TerminalEvent`]s for the host.
//!
//! Port of the behavior in src/features/terminal/ui/TerminalView.tsx: the
//! custom key handler (copy, paste, macOS shortcuts, Cmd+K clear), the wheel
//! handler, focus on mouse down, the exit notice, and xterm.js defaults that
//! file relied on (scroll to bottom on input, clear selection on input,
//! right click selects a word on macOS, Cmd+A selects all on macOS).

use std::ops::Range;
use std::time::Duration;

use gpui::{
    App, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled, Subscription, Task, UTF16Selection, Window, actions, div,
};

use crate::element::{CellHit, LayoutInfo, TerminalElement};
use crate::emulator::{Emulator, GridSize, Link, SelectionType, Side};
use crate::keys::{
    KeyMode, MouseAction, ReportButton, focus_report, is_mac_clear_shortcut, is_text_input,
    keystroke_bytes, mac_shortcut_bytes, mouse_report, paste_bytes,
};
use crate::pty::{Pty, PtyEvent, PtySize};
use crate::theme::{TerminalTheme, resolve_font_family};

actions!(
    terminal,
    [
        /// Copy the selection to the clipboard.
        Copy,
        /// Paste the clipboard into the terminal.
        Paste,
        /// Select the whole buffer.
        SelectAll,
        /// Drop the scrollback and move the prompt line to the top.
        Clear,
        /// Scroll the viewport up one page.
        ScrollPageUp,
        /// Scroll the viewport down one page.
        ScrollPageDown,
        /// Scroll to the top of the scrollback.
        ScrollToTop,
        /// Scroll to the live bottom.
        ScrollToBottom,
    ]
);

/// The key context the view sets, for host key bindings.
pub const KEY_CONTEXT: &str = "Terminal";

/// xterm.js cursor blink interval.
pub const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(600);

/// Pointer travel before a press becomes a drag selection, so the click
/// that focuses the terminal does not select a cell.
pub const SELECTION_DRAG_THRESHOLD: f32 = 2.0;

const SELECTION_SCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// What the view tells its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The program set or reset the window title (OSC 0 or 2).
    TitleChanged(Option<String>),
    /// The program rang the bell.
    Bell,
    /// The user Cmd+clicked (Ctrl+click off macOS) a link.
    OpenUrl(String),
    /// The grid changed size. The PTY was already told.
    Resized(PtySize),
    /// The child process exited.
    Exited(Option<i32>),
}

#[derive(Debug, Clone, Copy)]
struct SelectionDrag {
    origin: Point<Pixels>,
    position: Point<Pixels>,
    /// False until the pointer passes the drag threshold.
    armed: bool,
}

/// The terminal view. Create it with [`TerminalView::new`] inside `cx.new`.
pub struct TerminalView {
    emulator: Emulator,
    pty: Box<dyn Pty>,
    focus_handle: FocusHandle,
    font_family: Option<SharedString>,
    layout: Option<LayoutInfo>,
    blink_visible: bool,
    blink_epoch: usize,
    blink_task: Option<Task<()>>,
    marked_text: Option<String>,
    drag: Option<SelectionDrag>,
    selection_scroll_task: Option<Task<()>>,
    report_button: Option<ReportButton>,
    last_report_cell: Option<(usize, usize)>,
    scroll_remainder: f32,
    hovered_link: Option<Link>,
    copy_on_select: bool,
    exited: bool,
    _pump: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TerminalView {
    /// A view over `pty`. The grid starts at 80 by 24 and follows the
    /// element's bounds after the first layout.
    pub fn new(
        mut pty: impl Pty,
        theme: TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |this, _window, cx| {
                this.focus_changed(true, cx)
            }),
            cx.on_blur(&focus_handle, window, |this, _window, cx| {
                this.focus_changed(false, cx)
            }),
        ];
        let pump = pty.take_events().map(|events| {
            cx.spawn(async move |this, cx| {
                while let Ok(first) = events.recv().await {
                    // Drain whatever else is queued so a burst of output
                    // costs one notify.
                    let mut batch = vec![first];
                    while let Ok(next) = events.try_recv() {
                        batch.push(next);
                    }
                    let alive = this.update(cx, |view, cx| {
                        for event in batch {
                            view.handle_pty_event(event, cx);
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            })
        });
        Self {
            emulator: Emulator::new(GridSize::new(80, 24), theme),
            pty: Box::new(pty),
            focus_handle,
            font_family: None,
            layout: None,
            blink_visible: true,
            blink_epoch: 0,
            blink_task: None,
            marked_text: None,
            drag: None,
            selection_scroll_task: None,
            report_button: None,
            last_report_cell: None,
            scroll_remainder: 0.0,
            hovered_link: None,
            copy_on_select: false,
            exited: false,
            _pump: pump,
            _subscriptions: subscriptions,
        }
    }

    pub fn emulator(&self) -> &Emulator {
        &self.emulator
    }

    pub fn emulator_mut(&mut self) -> &mut Emulator {
        &mut self.emulator
    }

    pub fn theme(&self) -> &TerminalTheme {
        self.emulator.theme()
    }

    /// Change colors or font, for example on a light and dark switch.
    pub fn set_theme(&mut self, theme: TerminalTheme, cx: &mut Context<Self>) {
        if theme.font_family != self.theme().font_family
            || theme.font_fallbacks != self.theme().font_fallbacks
        {
            self.font_family = None;
        }
        self.emulator.set_theme(theme);
        cx.notify();
    }

    /// Copy the selection whenever a mouse selection ends. Off by default,
    /// as in TerminalView.tsx.
    pub fn set_copy_on_select(&mut self, enabled: bool) {
        self.copy_on_select = enabled;
    }

    /// Current grid size.
    pub fn grid_size(&self) -> GridSize {
        GridSize::new(self.emulator.cols() as u16, self.emulator.rows() as u16)
    }

    /// Whether the child exited.
    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// Feed PTY output directly, for hosts that do not hand the view an
    /// event stream (for example to replay a buffer).
    pub fn feed(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let replies = self.emulator.feed(bytes);
        if !replies.is_empty() {
            self.pty.write(&replies);
        }
        if self.emulator.take_title_changed() {
            cx.emit(TerminalEvent::TitleChanged(
                self.emulator.title().map(str::to_string),
            ));
        }
        if self.emulator.take_bell() {
            cx.emit(TerminalEvent::Bell);
        }
        if let Some(link) = &self.hovered_link {
            // Output may have moved or erased the hovered link.
            let still = self.emulator.link_at(link.start);
            if still.as_ref() != Some(link) {
                self.hovered_link = None;
            }
        }
        cx.notify();
    }

    fn handle_pty_event(&mut self, event: PtyEvent, cx: &mut Context<Self>) {
        match event {
            PtyEvent::Output(bytes) => self.feed(&bytes, cx),
            PtyEvent::Exited(code) => {
                if !self.exited {
                    self.exited = true;
                    self.emulator.write_exit_notice(code);
                    cx.emit(TerminalEvent::Exited(code));
                    cx.notify();
                }
            }
        }
    }

    /// Send user input to the PTY. Like xterm.js, this scrolls to the bottom,
    /// clears the selection, and restarts the cursor blink.
    pub fn input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if bytes.is_empty() || self.exited {
            return;
        }
        self.emulator.scroll_to_bottom();
        self.emulator.clear_selection();
        self.pty.write(bytes);
        self.restart_blink(cx);
        cx.notify();
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.emulator.selection_text() else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bytes = paste_bytes(&text, self.emulator.bracketed_paste());
        self.input(&bytes, cx);
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.emulator.clear();
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.emulator.select_all();
        cx.notify();
    }

    // Element callbacks.

    /// The resolved font family, cached until the theme's font changes.
    pub(crate) fn font_family(&mut self, window: &Window) -> SharedString {
        if let Some(family) = &self.font_family {
            return family.clone();
        }
        let installed = window.text_system().all_font_names();
        let family = resolve_font_family(self.theme().font_families(), &installed);
        self.font_family = Some(family.clone());
        family
    }

    /// The element measured the grid. Resize the emulator and the PTY when
    /// the cell count changed.
    pub(crate) fn apply_layout(&mut self, layout: LayoutInfo, cx: &mut Context<Self>) {
        self.layout = Some(layout);
        self.emulator
            .set_cell_size(f32::from(layout.cell_width), f32::from(layout.line_height));
        let size = GridSize::new(layout.cols as u16, layout.rows as u16);
        if size.cols as usize == self.emulator.cols() && size.rows as usize == self.emulator.rows()
        {
            return;
        }
        self.emulator.resize(size);
        let pty_size = PtySize {
            cols: size.cols,
            rows: size.rows,
            pixel_width: (f32::from(layout.cell_width) * size.cols as f32).round() as u16,
            pixel_height: (f32::from(layout.line_height) * size.rows as f32).round() as u16,
        };
        self.pty.resize(pty_size);
        cx.emit(TerminalEvent::Resized(pty_size));
    }

    /// The last layout the element measured.
    pub fn layout(&self) -> Option<LayoutInfo> {
        self.layout
    }

    pub(crate) fn focus_handle_ref(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn cursor_visible(&self, window: &Window) -> bool {
        !self.focus_handle.is_focused(window) || self.blink_visible
    }

    pub(crate) fn marked_text(&self) -> Option<&str> {
        self.marked_text.as_deref()
    }

    pub(crate) fn hovered_link(&self) -> Option<&Link> {
        self.hovered_link.as_ref()
    }

    /// Whether the pointer should show as an arrow because the program owns
    /// the mouse.
    pub(crate) fn mouse_reporting(&self, modifiers: &Modifiers) -> bool {
        self.emulator.mouse_mode().reporting() && !modifiers.shift
    }

    fn focus_changed(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.emulator.set_focused(focused);
        if self.emulator.focus_reporting() && !self.exited {
            self.pty.write(focus_report(focused));
        }
        if focused {
            self.restart_blink(cx);
        } else {
            self.blink_epoch += 1;
            self.blink_task = None;
            self.blink_visible = true;
            self.hovered_link = None;
        }
        cx.notify();
    }

    /// Show the cursor and start its blink cycle over, as xterm.js does on
    /// input.
    pub fn restart_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.restart_blink(cx);
        cx.notify();
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_visible = true;
        self.blink_epoch += 1;
        let epoch = self.blink_epoch;
        self.blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;
                let keep_going = this.update(cx, |view, cx| {
                    if view.blink_epoch != epoch {
                        return false;
                    }
                    view.blink_visible = !view.blink_visible;
                    cx.notify();
                    true
                });
                if !matches!(keep_going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    // Keyboard.

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let key = keystroke.key.to_ascii_lowercase();
        let composing = self.marked_text.is_some();

        if cfg!(target_os = "macos") {
            if let Some(bytes) = mac_shortcut_bytes(keystroke) {
                cx.stop_propagation();
                self.input(bytes, cx);
                return;
            }
            if is_mac_clear_shortcut(keystroke) {
                cx.stop_propagation();
                if !composing {
                    self.clear(cx);
                }
                return;
            }
            // xterm.js Keyboard.ts: Cmd+A selects all on macOS.
            if m.platform && !m.control && !m.alt && !m.shift && key == "a" {
                cx.stop_propagation();
                self.select_all(cx);
                return;
            }
        }

        // The rest of attachCustomKeyEventHandler: Cmd or Ctrl with C copies
        // when there is a selection, Cmd+C alone never reaches the shell,
        // and Cmd or Ctrl with V pastes.
        if (m.platform || m.control) && !m.alt {
            if key == "c" {
                if self.emulator.has_selection() {
                    cx.stop_propagation();
                    self.copy(cx);
                    return;
                }
                if m.platform && !m.control {
                    return;
                }
            } else if key == "v" {
                cx.stop_propagation();
                self.paste(cx);
                return;
            }
        }

        // xterm.js scrolls the viewport on Shift+PageUp and Shift+PageDown
        // in the normal buffer.
        if m.shift && !m.control && !m.alt && !m.platform && !self.emulator.alt_screen() {
            match key.as_str() {
                "pageup" => {
                    cx.stop_propagation();
                    self.emulator.scroll_page_up();
                    cx.notify();
                    return;
                }
                "pagedown" => {
                    cx.stop_propagation();
                    self.emulator.scroll_page_down();
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        // Plain text goes through the platform input handler, so dead keys
        // and input methods compose it. See replace_text_in_range.
        if composing || event.prefer_character_input || is_text_input(keystroke) {
            return;
        }

        let mode = KeyMode {
            app_cursor: self.emulator.app_cursor(),
        };
        if let Some(bytes) = keystroke_bytes(keystroke, mode) {
            cx.stop_propagation();
            self.input(&bytes, cx);
        }
    }

    fn copy_action(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy(cx);
    }

    fn paste_action(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        self.paste(cx);
    }

    fn select_all_action(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.select_all(cx);
    }

    fn clear_action(&mut self, _: &Clear, _window: &mut Window, cx: &mut Context<Self>) {
        self.clear(cx);
    }

    fn scroll_action(&mut self, delta: ScrollTarget, cx: &mut Context<Self>) {
        match delta {
            ScrollTarget::PageUp => self.emulator.scroll_page_up(),
            ScrollTarget::PageDown => self.emulator.scroll_page_down(),
            ScrollTarget::Top => self.emulator.scroll_to_top(),
            ScrollTarget::Bottom => self.emulator.scroll_to_bottom(),
        }
        cx.notify();
    }

    // Mouse. The element registers window-level listeners and forwards here,
    // so a drag that leaves the terminal keeps extending the selection.

    fn link_modifier(modifiers: &Modifiers) -> bool {
        if cfg!(target_os = "macos") {
            modifiers.platform
        } else {
            modifiers.control
        }
    }

    fn report_button(button: MouseButton) -> Option<ReportButton> {
        match button {
            MouseButton::Left => Some(ReportButton::Left),
            MouseButton::Middle => Some(ReportButton::Middle),
            MouseButton::Right => Some(ReportButton::Right),
            MouseButton::Navigate(_) => None,
        }
    }

    fn send_mouse_report(
        &mut self,
        button: ReportButton,
        action: MouseAction,
        modifiers: &Modifiers,
        hit: CellHit,
    ) {
        if let Some(bytes) = mouse_report(
            button,
            action,
            modifiers,
            hit.col,
            hit.row,
            self.emulator.mouse_mode(),
        ) {
            self.pty.write(&bytes);
        }
    }

    pub(crate) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // TerminalView.tsx focuses the terminal on any mouse down.
        window.focus(&self.focus_handle, cx);
        let Some(layout) = self.layout else {
            return;
        };
        let hit = layout.cell_at(event.position);

        if self.mouse_reporting(&event.modifiers) && !self.exited {
            if let Some(button) = Self::report_button(event.button) {
                self.report_button = Some(button);
                self.last_report_cell = Some((hit.row, hit.col));
                self.send_mouse_report(button, MouseAction::Press, &event.modifiers, hit);
            }
            return;
        }

        let point = self.emulator.grid_point(hit.row, hit.col);
        if event.button == MouseButton::Left
            && Self::link_modifier(&event.modifiers)
            && let Some(link) = self.emulator.link_at(point)
        {
            cx.emit(TerminalEvent::OpenUrl(link.uri));
            return;
        }

        match event.button {
            MouseButton::Left => {}
            // xterm.js rightClickSelectsWord, on by default on macOS.
            MouseButton::Right if cfg!(target_os = "macos") => {
                self.emulator
                    .start_selection(SelectionType::Semantic, point, Side::Left);
                cx.notify();
                return;
            }
            _ => return,
        }

        let ty = match event.click_count {
            0 | 1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        if ty == SelectionType::Simple {
            if event.modifiers.shift && self.emulator.has_selection() {
                self.emulator.update_selection(point, hit.side);
                self.drag = Some(SelectionDrag {
                    origin: event.position,
                    position: event.position,
                    armed: true,
                });
            } else {
                self.emulator.clear_selection();
                self.drag = Some(SelectionDrag {
                    origin: event.position,
                    position: event.position,
                    armed: false,
                });
            }
        } else {
            self.emulator.start_selection(ty, point, hit.side);
            self.drag = Some(SelectionDrag {
                origin: event.position,
                position: event.position,
                armed: true,
            });
        }
        cx.notify();
    }

    pub(crate) fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.layout else {
            return;
        };
        let hit = layout.cell_at(event.position);

        if self.report_button.is_some() || (hovered && self.mouse_reporting(&event.modifiers)) {
            if self.last_report_cell != Some((hit.row, hit.col)) {
                self.last_report_cell = Some((hit.row, hit.col));
                let button = self.report_button.unwrap_or(ReportButton::None);
                self.send_mouse_report(button, MouseAction::Motion, &event.modifiers, hit);
            }
            return;
        }

        if let Some(mut drag) = self.drag {
            if event.pressed_button != Some(MouseButton::Left) {
                self.drag = None;
            } else {
                drag.position = event.position;
                if !drag.armed {
                    let dx = f32::from(event.position.x - drag.origin.x);
                    let dy = f32::from(event.position.y - drag.origin.y);
                    if dx.hypot(dy) < SELECTION_DRAG_THRESHOLD {
                        self.drag = Some(drag);
                        return;
                    }
                    // Anchor at the press so the selection covers the whole
                    // gesture.
                    let anchor = layout.cell_at(drag.origin);
                    let point = self.emulator.grid_point(anchor.row, anchor.col);
                    self.emulator
                        .start_selection(SelectionType::Simple, point, anchor.side);
                    drag.armed = true;
                }
                self.drag = Some(drag);
                let point = self.emulator.grid_point(hit.row, hit.col);
                self.emulator.update_selection(point, hit.side);
                self.schedule_selection_scroll(cx);
                cx.notify();
                return;
            }
        }

        self.update_hovered_link(event.position, &event.modifiers, hovered, cx);
    }

    pub(crate) fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if let Some(button) = self.report_button.take() {
            if let Some(layout) = self.layout {
                let hit = layout.cell_at(event.position);
                self.send_mouse_report(button, MouseAction::Release, &event.modifiers, hit);
            }
            self.last_report_cell = None;
            return;
        }
        if self.drag.take().is_some() {
            self.selection_scroll_task = None;
            if self.copy_on_select {
                self.copy(cx);
            }
        }
    }

    pub(crate) fn scroll_wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else {
            return;
        };
        let line_height = layout.line_height;
        let delta = f32::from(event.delta.pixel_delta(line_height).y) / f32::from(line_height);
        let total = self.scroll_remainder + delta;
        let lines = total.trunc() as i32;
        self.scroll_remainder = total - lines as f32;
        if lines == 0 {
            return;
        }

        if self.mouse_reporting(&event.modifiers) {
            if !self.exited {
                let hit = layout.cell_at(event.position);
                let button = if lines > 0 {
                    ReportButton::WheelUp
                } else {
                    ReportButton::WheelDown
                };
                for _ in 0..lines.unsigned_abs() {
                    self.send_mouse_report(button, MouseAction::Press, &event.modifiers, hit);
                }
            }
            return;
        }
        // attachCustomWheelEventHandler: without mouse reporting, the wheel
        // does nothing in the alternate buffer.
        if self.emulator.alt_screen() {
            return;
        }
        self.emulator.scroll(lines);
        cx.notify();
    }

    pub(crate) fn modifiers_changed(
        &mut self,
        modifiers: &Modifiers,
        position: Point<Pixels>,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        self.update_hovered_link(position, modifiers, hovered, cx);
    }

    fn update_hovered_link(
        &mut self,
        position: Point<Pixels>,
        modifiers: &Modifiers,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let link = match self.layout {
            Some(layout) if hovered && Self::link_modifier(modifiers) => {
                let hit = layout.cell_at(position);
                self.emulator
                    .link_at(self.emulator.grid_point(hit.row, hit.col))
            }
            _ => None,
        };
        if link != self.hovered_link {
            self.hovered_link = link;
            cx.notify();
        }
    }

    /// While a drag selection is above or below the grid, keep scrolling and
    /// extending it.
    fn schedule_selection_scroll(&mut self, cx: &mut Context<Self>) {
        if self.selection_scroll_task.is_some() {
            return;
        }
        let (Some(drag), Some(layout)) = (self.drag, self.layout) else {
            return;
        };
        if !drag.armed || layout.edge_scroll_lines(drag.position) == 0 {
            return;
        }
        self.selection_scroll_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(SELECTION_SCROLL_INTERVAL)
                .await;
            let _ = this.update(cx, |view, cx| {
                view.selection_scroll_task = None;
                let (Some(drag), Some(layout)) = (view.drag, view.layout) else {
                    return;
                };
                let lines = layout.edge_scroll_lines(drag.position);
                if lines == 0 || view.emulator.alt_screen() {
                    return;
                }
                view.emulator.scroll(lines);
                let hit = layout.cell_at(drag.position);
                let point = view.emulator.grid_point(hit.row, hit.col);
                view.emulator.update_selection(point, hit.side);
                cx.notify();
                view.schedule_selection_scroll(cx);
            });
        }));
    }
}

#[derive(Clone, Copy)]
enum ScrollTarget {
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key_down))
            .on_modifiers_changed(
                cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                    let position = window.mouse_position();
                    let hovered = this
                        .layout
                        .is_some_and(|layout| layout.bounds.contains(&position));
                    this.modifiers_changed(&event.modifiers, position, hovered, cx);
                }),
            )
            .on_action(cx.listener(Self::copy_action))
            .on_action(cx.listener(Self::paste_action))
            .on_action(cx.listener(Self::select_all_action))
            .on_action(cx.listener(Self::clear_action))
            .on_action(cx.listener(|this, _: &ScrollPageUp, _, cx| {
                this.scroll_action(ScrollTarget::PageUp, cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| {
                this.scroll_action(ScrollTarget::PageDown, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ScrollToTop, _, cx| {
                    this.scroll_action(ScrollTarget::Top, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| {
                this.scroll_action(ScrollTarget::Bottom, cx)
            }))
            .child(TerminalElement::new(cx.entity()))
    }
}

// IME. Committed text arrives in replace_text_in_range, which is also how
// plain typing reaches the PTY. Preedit text is drawn at the cursor and never
// sent until the input method commits it.
impl gpui::EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self
            .marked_text
            .as_deref()
            .map_or(0, |text| text.encode_utf16().count());
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_text
            .as_deref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        self.input(text.as_bytes(), cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        let layout = self.layout?;
        let (row, col) = self.emulator.cursor_cell().unwrap_or((0, 0));
        Some(layout.cell_bounds(row, col, 1))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
