//! `PromptInput`: the composer's multi-line text field. It replaces the
//! React composer's textarea plus its `ComposerHighlight` overlay with one
//! element, so styled ranges (skills, `@` mentions, MCP tags, mode commands)
//! sit inside editable text with the platform's caret, selection, and IME.
//!
//! Owners intercept keys with `capture_action` on an ancestor: [`Enter`],
//! [`MoveUp`], [`MoveDown`], [`Tab`], [`Escape`], and [`Paste`] run their
//! default only when no ancestor stops propagation. Key bindings live in
//! the `PromptInput` key context; call [`init`] once.

mod buffer;
mod element;
mod layout;
#[cfg(test)]
mod tests;

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Hsla, InteractiveElement, IntoElement, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    ScrollWheelEvent, SharedString, Styled, Task, UTF16Selection, Window, actions, div, point, px,
};

pub use buffer::{EditKind, PromptBuffer};
pub use element::PromptTextElement;
pub use layout::{Caret, Row, TextLayout};

actions!(
    prompt_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        DeleteToLineEnd,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Enter,
        Newline,
        Tab,
        ShiftTab,
        Escape,
        ShowCharacterPalette,
    ]
);

/// The key context the bindings use.
pub const KEY_CONTEXT: &str = "PromptInput";

/// Binds the editing keys. macOS bindings follow NSTextView; others follow
/// a Windows text box.
pub fn init(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("shift-backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", MoveLeft, ctx),
        KeyBinding::new("right", MoveRight, ctx),
        KeyBinding::new("up", MoveUp, ctx),
        KeyBinding::new("down", MoveDown, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("shift-home", SelectHome, ctx),
        KeyBinding::new("shift-end", SelectEnd, ctx),
        KeyBinding::new("enter", Enter, ctx),
        KeyBinding::new("secondary-enter", Enter, ctx),
        KeyBinding::new("alt-enter", Enter, ctx),
        KeyBinding::new("shift-enter", Newline, ctx),
        KeyBinding::new("tab", Tab, ctx),
        KeyBinding::new("shift-tab", ShiftTab, ctx),
        KeyBinding::new("escape", Escape, ctx),
    ];
    if cfg!(target_os = "macos") {
        bindings.extend([
            KeyBinding::new("ctrl-backspace", Backspace, ctx),
            KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
            KeyBinding::new("alt-delete", DeleteWordRight, ctx),
            KeyBinding::new("cmd-backspace", DeleteToLineStart, ctx),
            KeyBinding::new("cmd-delete", DeleteToLineEnd, ctx),
            KeyBinding::new("ctrl-k", DeleteToLineEnd, ctx),
            KeyBinding::new("alt-left", WordLeft, ctx),
            KeyBinding::new("alt-right", WordRight, ctx),
            KeyBinding::new("alt-shift-left", SelectWordLeft, ctx),
            KeyBinding::new("alt-shift-right", SelectWordRight, ctx),
            KeyBinding::new("cmd-left", Home, ctx),
            KeyBinding::new("cmd-right", End, ctx),
            KeyBinding::new("ctrl-a", Home, ctx),
            KeyBinding::new("ctrl-e", End, ctx),
            KeyBinding::new("cmd-shift-left", SelectHome, ctx),
            KeyBinding::new("cmd-shift-right", SelectEnd, ctx),
            KeyBinding::new("cmd-up", DocStart, ctx),
            KeyBinding::new("cmd-down", DocEnd, ctx),
            KeyBinding::new("cmd-shift-up", SelectDocStart, ctx),
            KeyBinding::new("cmd-shift-down", SelectDocEnd, ctx),
            KeyBinding::new("cmd-a", SelectAll, ctx),
            KeyBinding::new("cmd-c", Copy, ctx),
            KeyBinding::new("cmd-x", Cut, ctx),
            KeyBinding::new("cmd-v", Paste, ctx),
            KeyBinding::new("cmd-z", Undo, ctx),
            KeyBinding::new("cmd-shift-z", Redo, ctx),
            KeyBinding::new("ctrl-enter", Enter, ctx),
            KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, ctx),
        ]);
    } else {
        bindings.extend([
            KeyBinding::new("ctrl-backspace", DeleteWordLeft, ctx),
            KeyBinding::new("ctrl-delete", DeleteWordRight, ctx),
            KeyBinding::new("ctrl-left", WordLeft, ctx),
            KeyBinding::new("ctrl-right", WordRight, ctx),
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, ctx),
            KeyBinding::new("ctrl-shift-right", SelectWordRight, ctx),
            KeyBinding::new("ctrl-home", DocStart, ctx),
            KeyBinding::new("ctrl-end", DocEnd, ctx),
            KeyBinding::new("ctrl-shift-home", SelectDocStart, ctx),
            KeyBinding::new("ctrl-shift-end", SelectDocEnd, ctx),
            KeyBinding::new("ctrl-a", SelectAll, ctx),
            KeyBinding::new("ctrl-c", Copy, ctx),
            KeyBinding::new("ctrl-x", Cut, ctx),
            KeyBinding::new("ctrl-v", Paste, ctx),
            KeyBinding::new("ctrl-z", Undo, ctx),
            KeyBinding::new("ctrl-y", Redo, ctx),
            KeyBinding::new("ctrl-shift-z", Redo, ctx),
        ]);
    }
    cx.bind_keys(bindings);
}

/// What changed in a prompt input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptInputEvent {
    /// The text changed (typing, paste, IME commit, undo).
    Changed,
    /// The caret or selection moved without a text change.
    SelectionChanged,
    Focused,
    Blurred,
}

/// Where an overlay sits relative to the character it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayPlacement {
    /// Centered on the character, like the file icon over a mention's `@`.
    Center,
    /// Its left edge this many CSS px left of the character, like a mode
    /// command's icon in the first-line indent.
    Before(f32),
}

/// Something drawn over one character of the text.
#[derive(Clone)]
pub struct GlyphOverlay {
    /// The character's byte range.
    pub range: Range<usize>,
    pub placement: OverlayPlacement,
    /// Square size in CSS px.
    pub size: f32,
    pub render: OverlayRender,
}

/// Builds an overlay's element each frame.
pub type OverlayRender = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// Styling for the current text. Ranges are byte ranges.
#[derive(Clone, Default)]
pub struct PromptDecorations {
    /// Text colors; later spans win.
    pub spans: Vec<(Range<usize>, Hsla)>,
    /// Glyphs drawn transparent (an overlay stands in for them).
    pub hidden: Vec<Range<usize>>,
    pub overlays: Vec<GlyphOverlay>,
    /// CSS `text-indent` for the first row, in CSS px.
    pub first_line_indent: f32,
}

/// Computes decorations for a text. Called when the text changes or after
/// [`PromptInput::invalidate_decorations`].
pub type Decorator = Rc<dyn Fn(&str, &App) -> PromptDecorations>;

/// Paint colors the theme provides.
#[derive(Clone, Copy, Debug)]
pub struct PromptColors {
    pub selection: Hsla,
    pub placeholder: Hsla,
    pub caret: Hsla,
}

const BLINK_INTERVAL: Duration = Duration::from_millis(500);

pub struct PromptInput {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) buffer: PromptBuffer,
    pub(crate) placeholder: SharedString,
    pub(crate) disabled: bool,
    /// Caret affinity at a soft wrap.
    upstream: bool,
    /// The x a run of vertical moves keeps returning to.
    goal_x: Option<Pixels>,
    decorator: Option<Decorator>,
    decorations: Option<(u64, Rc<PromptDecorations>)>,
    /// Bumps on every text change and decoration invalidation.
    version: u64,
    /// Top, right, bottom, left padding in CSS px.
    pub(crate) padding: [f32; 4],
    /// Maximum element height in CSS px, padding included.
    pub(crate) max_height: Option<f32>,
    pub(crate) min_rows: usize,
    pub(crate) colors: PromptColors,
    pub(crate) scroll_y: Pixels,
    pub(crate) max_scroll: Pixels,
    pub(crate) autoscroll: bool,
    pub(crate) caret_visible: bool,
    blink_epoch: usize,
    _blink: Option<Task<()>>,
    selecting: bool,
    pub(crate) last_layout: Option<Rc<TextLayout>>,
    pub(crate) last_text_origin: Point<Pixels>,
    pub(crate) last_bounds: Option<Bounds<Pixels>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<PromptInputEvent> for PromptInput {}

impl PromptInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |this, _, cx| {
                this.restart_blink(cx);
                cx.emit(PromptInputEvent::Focused);
            }),
            cx.on_blur(&focus_handle, window, |this, _, cx| {
                this.blink_epoch += 1;
                this._blink = None;
                this.caret_visible = false;
                this.selecting = false;
                cx.emit(PromptInputEvent::Blurred);
                cx.notify();
            }),
        ];
        Self {
            focus_handle,
            buffer: PromptBuffer::default(),
            placeholder: SharedString::default(),
            disabled: false,
            upstream: false,
            goal_x: None,
            decorator: None,
            decorations: None,
            version: 0,
            padding: [12., 12., 12., 12.],
            max_height: None,
            min_rows: 1,
            colors: PromptColors {
                selection: gpui::hsla(211. / 360., 0.92, 0.62, 0.3),
                placeholder: gpui::hsla(0., 0., 1., 0.4),
                caret: gpui::white(),
            },
            scroll_y: px(0.),
            max_scroll: px(0.),
            autoscroll: false,
            caret_visible: false,
            blink_epoch: 0,
            _blink: None,
            selecting: false,
            last_layout: None,
            last_text_origin: point(px(0.), px(0.)),
            last_bounds: None,
            _subscriptions: subscriptions,
        }
    }

    // Configuration.

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if placeholder != self.placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    pub fn placeholder(&self) -> &SharedString {
        &self.placeholder
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if disabled != self.disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Top, right, bottom, left padding in CSS px. The text scrolls under it.
    pub fn set_padding(&mut self, padding: [f32; 4], cx: &mut Context<Self>) {
        self.padding = padding;
        cx.notify();
    }

    /// The element grows with its text up to this height (CSS px, padding
    /// included), then scrolls. `resizeComposer` in the React code.
    pub fn set_max_height(&mut self, max_height: Option<f32>, cx: &mut Context<Self>) {
        self.max_height = max_height;
        cx.notify();
    }

    pub fn set_colors(&mut self, colors: PromptColors, cx: &mut Context<Self>) {
        self.colors = colors;
        cx.notify();
    }

    pub fn set_decorator(&mut self, decorator: Option<Decorator>, cx: &mut Context<Self>) {
        self.decorator = decorator;
        self.invalidate_decorations(cx);
    }

    /// Recomputes decorations on the next frame, for when what they depend
    /// on (skill names, mention labels) changed but the text did not.
    pub fn invalidate_decorations(&mut self, cx: &mut Context<Self>) {
        self.version += 1;
        cx.notify();
    }

    /// The decorations for the current text.
    pub fn decorations(&mut self, cx: &App) -> Rc<PromptDecorations> {
        if let Some((version, decorations)) = &self.decorations
            && *version == self.version
        {
            return decorations.clone();
        }
        let decorations = Rc::new(match &self.decorator {
            Some(decorator) => decorator(self.buffer.text(), cx),
            None => PromptDecorations::default(),
        });
        self.decorations = Some((self.version, decorations.clone()));
        decorations
    }

    // Reading.

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn buffer(&self) -> &PromptBuffer {
        &self.buffer
    }

    pub fn selection(&self) -> Range<usize> {
        self.buffer.selection()
    }

    /// `selectionStart`.
    pub fn selection_start(&self) -> usize {
        self.buffer.selection().start
    }

    /// `selectionEnd`.
    pub fn selection_end(&self) -> usize {
        self.buffer.selection().end
    }

    /// The caret offset (the moving end of the selection).
    pub fn cursor(&self) -> usize {
        self.buffer.head()
    }

    pub fn caret(&self) -> Caret {
        Caret {
            offset: self.buffer.head(),
            upstream: self.upstream,
        }
    }

    /// True while an IME composition is open. Keys then belong to the IME,
    /// like `isImeComposition` in the React code.
    pub fn is_composing(&self) -> bool {
        self.buffer.marked().is_some()
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// The last laid-out rows, for tests and owners that place things at the
    /// caret.
    pub fn last_layout(&self) -> Option<Rc<TextLayout>> {
        self.last_layout.clone()
    }

    /// The element's bounds in the last frame.
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    // Writing.

    /// `el.value = text` plus `setSelectionRange(cursor, cursor)`, as one
    /// undo step. Emits [`PromptInputEvent::Changed`] when the text differs.
    pub fn set_text(&mut self, text: impl Into<String>, cursor: usize, cx: &mut Context<Self>) {
        let text = text.into();
        let changed = text != self.buffer.text();
        self.buffer.set_text(text, cursor);
        self.after_edit(changed, cx);
    }

    /// Replaces the text without an undo step or a change event, for a
    /// draft loaded from outside.
    pub fn reset_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let end = text.len();
        self.buffer.reset(text, end);
        self.version += 1;
        self.upstream = false;
        self.goal_x = None;
        self.scroll_y = px(0.);
        self.autoscroll = true;
        cx.notify();
    }

    /// `setSelectionRange(start, end)`.
    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.buffer.select(range, false);
        self.after_move(cx);
    }

    pub fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.move_to(offset);
        self.after_move(cx);
    }

    /// Types `text` over the selection, as a paste would.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buffer.insert(text);
        self.after_edit(true, cx);
    }

    /// Replaces `range` and leaves the caret after the new text.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.buffer.replace(range, text, EditKind::Other);
        self.after_edit(true, cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if !self.disabled {
            window.focus(&self.focus_handle, cx);
        }
    }

    fn after_edit(&mut self, changed: bool, cx: &mut Context<Self>) {
        if changed {
            self.version += 1;
        }
        self.upstream = false;
        self.goal_x = None;
        self.autoscroll = true;
        self.restart_blink(cx);
        cx.emit(if changed {
            PromptInputEvent::Changed
        } else {
            PromptInputEvent::SelectionChanged
        });
        cx.notify();
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        self.restart_blink(cx);
        cx.emit(PromptInputEvent::SelectionChanged);
        cx.notify();
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_visible = true;
        self.blink_epoch += 1;
        let epoch = self.blink_epoch;
        self._blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.blink_epoch != epoch {
                            return false;
                        }
                        this.caret_visible = !this.caret_visible;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
        cx.notify();
    }

    // Movement helpers that need the last layout.

    fn layout_caret_move(
        &mut self,
        select: bool,
        cx: &mut Context<Self>,
        f: impl FnOnce(&TextLayout, Caret, &mut Option<Pixels>) -> Caret,
    ) {
        let caret = self.caret();
        let next = match self.last_layout.clone() {
            Some(layout) => f(&layout, caret, &mut self.goal_x),
            None => caret,
        };
        let goal = self.goal_x;
        if select {
            self.buffer.select_to(next.offset);
        } else {
            self.buffer.move_to(next.offset);
        }
        self.goal_x = goal;
        self.upstream = next.upstream;
        self.after_move(cx);
    }

    fn vertical(&mut self, delta: isize, select: bool, cx: &mut Context<Self>) {
        // A collapsed move with a selection lands on the selection's edge.
        if !select && !self.buffer.selection().is_empty() {
            let edge = if delta < 0 {
                self.buffer.selection().start
            } else {
                self.buffer.selection().end
            };
            self.buffer.move_to(edge);
        }
        self.layout_caret_move(select, cx, |layout, caret, goal| {
            let x = goal.unwrap_or_else(|| layout.position_for(caret).x);
            *goal = Some(x);
            layout.vertical(caret, delta, x)
        });
    }

    fn horizontal(&mut self, to: usize, select: bool, cx: &mut Context<Self>) {
        if select {
            self.buffer.select_to(to);
        } else {
            self.buffer.move_to(to);
        }
        self.upstream = false;
        self.goal_x = None;
        self.after_move(cx);
    }

    // Actions.

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.buffer.backspace() {
            self.after_edit(true, cx);
        } else {
            window.play_system_bell();
        }
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.buffer.delete() {
            self.after_edit(true, cx);
        } else {
            window.play_system_bell();
        }
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.previous_word_start(self.buffer.head());
        if !self.disabled && self.buffer.delete_back_to(to) {
            self.after_edit(true, cx);
        }
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word_end(self.buffer.head());
        if !self.disabled && self.buffer.delete_forward_to(to) {
            self.after_edit(true, cx);
        }
    }

    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caret = self.caret();
        let to = match &self.last_layout {
            Some(layout) => layout.row_start(caret).offset,
            None => self.buffer.line_start(caret.offset),
        };
        // At a row start, delete the newline before it like NSTextView.
        let to = if to == caret.offset {
            self.buffer.previous_grapheme(to)
        } else {
            to
        };
        if !self.disabled && self.buffer.delete_back_to(to) {
            self.after_edit(true, cx);
        }
    }

    fn delete_to_line_end(&mut self, _: &DeleteToLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let head = self.buffer.head();
        let end = self.buffer.line_end(head);
        let to = if end == head {
            self.buffer.next_grapheme(head)
        } else {
            end
        };
        if !self.disabled && self.buffer.delete_forward_to(to) {
            self.after_edit(true, cx);
        }
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        let selection = self.buffer.selection();
        let to = if selection.is_empty() {
            self.buffer.previous_grapheme(self.buffer.head())
        } else {
            selection.start
        };
        self.horizontal(to, false, cx);
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        let selection = self.buffer.selection();
        let to = if selection.is_empty() {
            self.buffer.next_grapheme(self.buffer.head())
        } else {
            selection.end
        };
        self.horizontal(to, false, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.previous_grapheme(self.buffer.head());
        self.horizontal(to, true, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_grapheme(self.buffer.head());
        self.horizontal(to, true, cx);
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, false, cx);
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, false, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, true, cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.previous_word_start(self.buffer.head());
        self.horizontal(to, false, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word_end(self.buffer.head());
        self.horizontal(to, false, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.previous_word_start(self.buffer.head());
        self.horizontal(to, true, cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word_end(self.buffer.head());
        self.horizontal(to, true, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.layout_caret_move(false, cx, |layout, caret, goal| {
            *goal = None;
            layout.row_start(caret)
        });
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.layout_caret_move(false, cx, |layout, caret, goal| {
            *goal = None;
            layout.row_end(caret)
        });
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.layout_caret_move(true, cx, |layout, caret, goal| {
            *goal = None;
            layout.row_start(caret)
        });
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.layout_caret_move(true, cx, |layout, caret, goal| {
            *goal = None;
            layout.row_end(caret)
        });
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(0, false, cx);
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.buffer.len();
        self.horizontal(end, false, cx);
    }

    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.horizontal(0, true, cx);
    }

    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.buffer.len();
        self.horizontal(end, true, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        self.upstream = false;
        self.goal_x = None;
        self.after_move(cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.buffer.selected_text();
        if !selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let selected = self.buffer.selected_text().to_string();
        if selected.is_empty() || self.disabled {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(selected));
        self.buffer
            .replace(self.buffer.selection(), "", EditKind::Other);
        self.after_edit(true, cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert(&normalize_newlines(&text), cx);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && self.buffer.undo() {
            self.after_edit(true, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && self.buffer.redo() {
            self.after_edit(true, cx);
        }
    }

    /// Enter: a newline unless an owner handles it (the composer submits).
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.insert("\n", cx);
        }
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.insert("\n", cx);
        }
    }

    fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn shift_tab(&mut self, _: &ShiftTab, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        cx.propagate();
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    // Mouse.

    fn caret_for_mouse(&self, position: Point<Pixels>) -> Caret {
        match &self.last_layout {
            Some(layout) => layout.caret_for_point(position - self.last_text_origin),
            None => Caret::new(self.buffer.len()),
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        let caret = self.caret_for_mouse(event.position);
        self.goal_x = None;
        self.upstream = caret.upstream;
        match event.click_count {
            2 => {
                let range = self.buffer.word_range_at(caret.offset);
                self.buffer.select(range, false);
            }
            n if n >= 3 => {
                let range = self.buffer.line_range_at(caret.offset);
                self.buffer.select(range, false);
            }
            _ if event.modifiers.shift => self.buffer.select_to(caret.offset),
            _ => self.buffer.move_to(caret.offset),
        }
        self.selecting = true;
        self.after_move(cx);
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let caret = self.caret_for_mouse(event.position);
        if caret.offset != self.buffer.head() {
            self.buffer.select_to(caret.offset);
            self.upstream = caret.upstream;
            self.after_move(cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.max_scroll <= px(0.) {
            cx.propagate();
            return;
        }
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let next = (self.scroll_y - delta).max(px(0.)).min(self.max_scroll);
        if next == self.scroll_y {
            cx.propagate();
            return;
        }
        self.scroll_y = next;
        cx.stop_propagation();
        cx.notify();
    }
}

/// Clipboard text uses `\n`; a Windows copy brings `\r\n`.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl Focusable for PromptInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PromptInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("prompt-input")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .w_full()
            .when(!self.disabled, |el| el.cursor(CursorStyle::IBeam))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::delete_to_line_end))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::shift_tab))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(PromptTextElement::new(cx.entity()))
    }
}

impl EntityInputHandler for PromptInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        Some(self.buffer.text_for_range_utf16(range, adjusted_range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.buffer.selection()),
            reversed: self.buffer.is_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked()
            .map(|range| self.buffer.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark();
        self.version += 1;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.buffer.replace_text_in_range_utf16(range, text);
        self.after_edit(true, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        self.buffer
            .replace_and_mark_utf16(range, new_text, new_selected_range);
        self.after_edit(true, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.buffer.range_from_utf16(&range_utf16);
        let origin = self.last_text_origin;
        if range.is_empty() {
            let at = layout.position_for(Caret::new(range.start));
            return Some(Bounds::new(
                origin + at,
                gpui::size(px(1.), layout.line_height),
            ));
        }
        layout
            .range_bounds(range)
            .first()
            .map(|rect| Bounds::new(rect.origin + origin, rect.size))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let caret = self.caret_for_mouse(point);
        Some(self.buffer.offset_to_utf16(caret.offset))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.buffer.len_utf16())
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.disabled
    }
}

use gpui::prelude::FluentBuilder as _;
