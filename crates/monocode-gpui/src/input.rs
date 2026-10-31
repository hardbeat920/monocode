// Native text-input plumbing adapted from GPUI's Apache-2.0 examples/input.rs.
// Copyright Zed Industries. See NOTICE for source and license.
use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, GlobalElementId, Hsla, ImgResourceLoader, InspectorElementId, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Resource, Role,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
    actions, div, fill, point, prelude::*, px, relative, rgb, size,
};
use std::{borrow::Cow, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

mod code;
mod command;
mod markdown;
mod mention;
mod spelling;
pub(crate) use command::{Command, CommandRemoved};
use mention::{DisplayLayout, MentionIcon};

const MAX_BYTES: usize = 64 * 1024;
const LINE_HEIGHT: f32 = 22.;
// The composer field starts two lines tall and grows to 140px before
// scrolling. The enclosing composer supplies its own vertical padding.
const MIN_HEIGHT: f32 = LINE_HEIGHT * 2.;
const MAX_HEIGHT: f32 = 140.;
const FONT_SIZE: f32 = 14.;
actions!(
    composer,
    [
        FoldCode,
        UnfoldCode,
        FoldAll,
        UnfoldAll,
        Complete,
        DismissCompletion,
        Undo,
        Redo,
        Indent,
        Outdent,
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocumentStart,
        DocumentEnd,
        SelectDocumentStart,
        SelectDocumentEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToHome,
        DeleteToEnd,
        Paste,
        Cut,
        Copy,
        Newline,
        Submit,
        SubmitSecondary,
        Up,
        Down
    ]
);
pub struct Submitted;
/// Cmd/Ctrl+Enter in a composer that sends either way, asking for the
/// opposite of its usual follow-up behavior.
pub struct AlternateSubmitted;
pub struct Changed;
pub(crate) enum CodeAction {
    Stage(usize),
    Revert(usize),
}
pub struct Rejected(pub &'static str);

pub fn bind(cx: &mut App) {
    let selection_modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([KeyBinding::new(
        &format!("{selection_modifier}-c"),
        Copy,
        Some("TranscriptSelection"),
    )]);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("Composer")),
        KeyBinding::new("delete", Delete, Some("Composer")),
        KeyBinding::new("left", Left, Some("Composer")),
        KeyBinding::new("right", Right, Some("Composer")),
        KeyBinding::new("shift-left", SelectLeft, Some("Composer")),
        KeyBinding::new("shift-right", SelectRight, Some("Composer")),
        KeyBinding::new("up", Up, Some("Composer")),
        KeyBinding::new("down", Down, Some("Composer")),
        KeyBinding::new("shift-up", SelectUp, Some("Composer")),
        KeyBinding::new("shift-down", SelectDown, Some("Composer")),
        KeyBinding::new("home", Home, Some("Composer")),
        KeyBinding::new("end", End, Some("Composer")),
        KeyBinding::new("shift-home", SelectHome, Some("Composer")),
        KeyBinding::new("shift-end", SelectEnd, Some("Composer")),
        KeyBinding::new("shift-enter", Newline, Some("Composer")),
        KeyBinding::new("enter", Submit, Some("Composer")),
    ]);
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-z"), Undo, Some("CodeEditor")),
        KeyBinding::new(&format!("{modifier}-shift-z"), Redo, Some("CodeEditor")),
        KeyBinding::new("ctrl-space", Complete, Some("CodeEditor")),
        KeyBinding::new("cmd-alt-[", FoldCode, Some("CodeEditor")),
        KeyBinding::new("cmd-alt-]", UnfoldCode, Some("CodeEditor")),
        KeyBinding::new("ctrl-shift-[", FoldCode, Some("CodeEditor")),
        KeyBinding::new("ctrl-shift-]", UnfoldCode, Some("CodeEditor")),
        KeyBinding::new("cmd-alt-shift-[", FoldAll, Some("CodeEditor")),
        KeyBinding::new("cmd-alt-shift-]", UnfoldAll, Some("CodeEditor")),
        KeyBinding::new("escape", DismissCompletion, Some("CodeEditor")),
        KeyBinding::new("tab", Indent, Some("CodeEditor")),
        KeyBinding::new("shift-tab", Outdent, Some("CodeEditor")),
    ]);
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-a"), SelectAll, Some("Composer")),
        KeyBinding::new(&format!("{modifier}-c"), Copy, Some("Composer")),
        KeyBinding::new(&format!("{modifier}-v"), Paste, Some("Composer")),
        KeyBinding::new(&format!("{modifier}-x"), Cut, Some("Composer")),
        KeyBinding::new(
            &format!("{modifier}-enter"),
            SubmitSecondary,
            Some("Composer"),
        ),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-left", Home, Some("Composer")),
            KeyBinding::new("cmd-right", End, Some("Composer")),
            KeyBinding::new("cmd-shift-left", SelectHome, Some("Composer")),
            KeyBinding::new("cmd-shift-right", SelectEnd, Some("Composer")),
            KeyBinding::new("cmd-up", DocumentStart, Some("Composer")),
            KeyBinding::new("cmd-down", DocumentEnd, Some("Composer")),
            KeyBinding::new("cmd-shift-up", SelectDocumentStart, Some("Composer")),
            KeyBinding::new("cmd-shift-down", SelectDocumentEnd, Some("Composer")),
            KeyBinding::new("cmd-backspace", DeleteToHome, Some("Composer")),
            KeyBinding::new("cmd-delete", DeleteToEnd, Some("Composer")),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-home", DocumentStart, Some("Composer")),
            KeyBinding::new("ctrl-end", DocumentEnd, Some("Composer")),
            KeyBinding::new("ctrl-shift-home", SelectDocumentStart, Some("Composer")),
            KeyBinding::new("ctrl-shift-end", SelectDocumentEnd, Some("Composer")),
        ]);
    }
    let word_modifier = if cfg!(target_os = "macos") {
        "alt"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{word_modifier}-left"), WordLeft, Some("Composer")),
        KeyBinding::new(
            &format!("{word_modifier}-right"),
            WordRight,
            Some("Composer"),
        ),
        KeyBinding::new(
            &format!("{word_modifier}-shift-left"),
            SelectWordLeft,
            Some("Composer"),
        ),
        KeyBinding::new(
            &format!("{word_modifier}-shift-right"),
            SelectWordRight,
            Some("Composer"),
        ),
        KeyBinding::new(
            &format!("{word_modifier}-backspace"),
            DeleteWordLeft,
            Some("Composer"),
        ),
        KeyBinding::new(
            &format!("{word_modifier}-delete"),
            DeleteWordRight,
            Some("Composer"),
        ),
    ]);
}

pub struct Input {
    code: Option<code::Code>,
    focus: FocusHandle,
    height: f32,
    single_line: bool,
    read_only: bool,
    secret: bool,
    max_chars: Option<usize>,
    centered: bool,
    placeholder: SharedString,
    placeholder_alpha: Option<f32>,
    label: SharedString,
    font_size: f32,
    line_height: f32,
    min_height: f32,
    max_height: f32,
    /// A textarea that takes Enter as a newline and submits on Cmd/Ctrl+Enter.
    secondary_submit: bool,
    /// Emit [`AlternateSubmitted`] on Cmd/Ctrl+Enter instead of ignoring it.
    alternate_submit: bool,
    pub content: SharedString,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    composer_tokens: Vec<Range<usize>>,
    inline_commands: bool,
    /// Draw markdown syntax (headings, emphasis, code) styled while typing.
    markdown: bool,
    selected_command: Option<Command>,
    lines: Vec<WrappedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    scroll_y: Pixels,
    manual_scroll: bool,
    reveal_in_parent: bool,
    reveal_pending: bool,
    selecting: bool,
    drag_position: Option<Point<Pixels>>,
    autoscroll: Option<gpui::Task<()>>,
}
impl EventEmitter<Submitted> for Input {}
impl EventEmitter<AlternateSubmitted> for Input {}
impl EventEmitter<Changed> for Input {}
impl EventEmitter<Rejected> for Input {}
impl EventEmitter<CodeAction> for Input {}
impl Focusable for Input {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Input {
    /// Keep authentication text out of drawing, accessibility and clipboard.
    pub(crate) fn set_secret(&mut self, secret: bool) {
        self.secret = secret;
    }
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            code: None,
            focus: cx.focus_handle(),
            height: MIN_HEIGHT,
            single_line: false,
            read_only: false,
            secret: false,
            max_chars: None,
            centered: false,
            placeholder: "Ask, build, or describe a change…".into(),
            placeholder_alpha: None,
            label: "Message composer".into(),
            font_size: FONT_SIZE,
            line_height: LINE_HEIGHT,
            min_height: MIN_HEIGHT,
            max_height: MAX_HEIGHT,
            secondary_submit: false,
            alternate_submit: false,
            content: "".into(),
            selection: 0..0,
            reversed: false,
            marked: None,
            composer_tokens: Vec::new(),
            inline_commands: false,
            markdown: false,
            selected_command: None,
            lines: Vec::new(),
            bounds: None,
            scroll_x: px(0.),
            scroll_y: px(0.),
            manual_scroll: false,
            reveal_in_parent: false,
            reveal_pending: false,
            selecting: false,
            drag_position: None,
            autoscroll: None,
        }
    }
    pub fn search(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.height = 20.;
        input.single_line = true;
        input.placeholder = "Search".into();
        input.label = "Search sessions".into();
        input
    }
    /// A one-line field inside a popover, such as a picker's search box.
    /// Only `placeholder_color`'s alpha is kept; its ink follows the theme.
    pub fn field(
        label: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        placeholder_color: Hsla,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input = Self::search(cx);
        input.label = label.into();
        input.placeholder = placeholder.into();
        input.placeholder_alpha = Some(placeholder_color.a);
        // Picker search fields are `text-[13px]`.
        input.font_size = 13.;
        input
    }
    /// GitChangesPanel's commit message: a one-row `text-[13px] leading-5`
    /// textarea growing to `max-h-40` less its `py-1`.
    pub fn commit_message(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.label = "Commit message".into();
        input.font_size = 13.;
        input.line_height = 20.;
        input.height = 20.;
        input.min_height = 20.;
        input.max_height = 152.;
        input.placeholder_alpha = Some(0.35);
        input.secondary_submit = true;
        input
    }
    pub(crate) fn multiline_field(label: &str, placeholder: &str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = label.to_owned().into();
        input.font_size = 12.;
        input.placeholder = placeholder.to_owned().into();
        input.height = 124.;
        input.min_height = 124.;
        input.max_height = 124.;
        input
    }
    /// An action's shell command: three rows growing to six.
    pub(crate) fn action_command(cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = "Command".into();
        input.placeholder = "bun test".into();
        input.height = 60.;
        input.min_height = 60.;
        input.max_height = 120.;
        input
    }
    /// InboxComments' two-row textarea grows within the discussion page.
    pub(crate) fn inbox_comment(cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = "Comment".into();
        input.placeholder = if cfg!(target_os = "macos") {
            "Leave a comment (⌘↩)"
        } else {
            "Leave a comment (Ctrl+Enter)"
        }
        .into();
        input.height = 40.;
        input.min_height = 40.;
        input.max_height = 144.;
        input
    }
    /// AutomationEditor uses a proportional, naturally growing prompt field.
    pub(crate) fn automation_instructions(cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = "Instructions".into();
        input.placeholder = "Tell the agent what to do when this automation runs…".into();
        input.font_size = 14.;
        input.line_height = 22.;
        input.height = 88.;
        input.min_height = 88.;
        input.max_height = f32::MAX;
        input.reveal_in_parent = true;
        input
    }
    pub(crate) fn automation_name(cx: &mut Context<Self>) -> Self {
        let mut input = Self::search(cx);
        input.label = "Automation name".into();
        input.placeholder = "Untitled".into();
        input.font_size = 20.;
        input.line_height = 25.;
        input.height = 25.;
        input
    }
    pub(crate) fn mono_composer(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.min_height != 18. {
            self.height = 18.;
        }
        self.label = format!("Message {name}").into();
        self.placeholder = self.label.clone();
        self.font_size = 13.;
        self.line_height = 18.;
        self.min_height = 18.;
        self.max_height = 144.;
        self.height = self.height.clamp(self.min_height, self.max_height);
        cx.notify();
    }
    /// NewHabitPage's three-row AutoTextarea, capped like React's AutoTextarea.
    pub(crate) fn habit_instructions(cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = "What it does".into();
        input.placeholder =
            "What it should do on each run, and when it's worth telling you about.".into();
        input.font_size = 12.;
        input.line_height = 20.;
        input.height = 60.;
        input.min_height = 60.;
        input.max_height = 320.;
        input.max_chars = Some(4000);
        input
    }
    pub(crate) fn markdown_editor(label: &str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = label.to_string().into();
        input.markdown = true;
        input.min_height = 120.;
        input.max_height = 2000.;
        input.height = 120.;
        input
    }
    pub(crate) fn set_placeholder(
        &mut self,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        if self.placeholder != text {
            self.placeholder = text;
            cx.notify();
        }
    }
    /// Markdown spans mapped into the displayed text (after any command prefix).
    fn markdown_display_spans(&self, layout: &DisplayLayout) -> markdown::Spans {
        if !self.markdown || self.secret || self.single_line || self.code.is_some() {
            return Vec::new();
        }
        markdown::spans(&self.content)
            .into_iter()
            .map(|(range, style)| {
                (
                    layout.to_display(range.start)..layout.to_display(range.end),
                    style,
                )
            })
            .collect()
    }
    pub(crate) fn set_alternate_submit(&mut self, value: bool) {
        self.alternate_submit = value;
    }
    pub(crate) fn set_read_only(&mut self, value: bool) {
        self.read_only = value;
    }
    pub(crate) fn set_max_chars(&mut self, value: usize) {
        self.max_chars = Some(value);
    }
    pub(crate) fn set_font_size(&mut self, size: f32) {
        self.font_size = size;
    }
    pub(crate) fn set_centered(&mut self, centered: bool) {
        self.centered = centered;
    }
    pub(crate) fn set_min_height(&mut self, height: f32) {
        self.min_height = height;
        self.height = self.height.max(height);
    }
    pub(crate) fn set_max_height(&mut self, height: f32) {
        self.max_height = height;
    }
    pub(crate) fn set_line_height(&mut self, height: f32) {
        self.line_height = height;
    }
    pub(crate) fn memory_fact(label: &str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::commit_message(cx);
        input.label = label.to_string().into();
        input.font_size = 12.;
        input.line_height = 18.;
        input.min_height = 18.;
        input.max_height = 320.;
        input.height = 18.;
        input.secondary_submit = false;
        input
    }
    /// MessageQueue.tsx keeps its one-row textarea height while Shift+Enter
    /// inserts newlines; the field scrolls to the caret within that row.
    pub(crate) fn queued_message(cx: &mut Context<Self>) -> Self {
        let mut input = Self::memory_fact("Edit queued message", cx);
        input.max_height = input.min_height;
        input
    }
    pub(crate) fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        let range = self.clip_range(range);
        self.unfold_range(&range);
        self.reveal_code();
        self.selection = range;
        self.reversed = false;
        cx.notify();
    }
    pub fn take(&mut self, cx: &mut Context<Self>) -> Option<String> {
        if self.is_composing() || self.content.trim().is_empty() {
            return None;
        }
        let text = self.content.to_string();
        self.content = "".into();
        self.lines.clear();
        self.selection = 0..0;
        self.reversed = false;
        self.scroll_x = px(0.);
        self.scroll_y = px(0.);
        cx.notify();
        Some(text)
    }
    /// An empty composer hands its document and line keys (Cmd+arrows) on to
    /// the workspace's session and project navigation, as React's
    /// `emptyComposer` condition does. Returns whether it did.
    fn yields_navigation(&self, cx: &mut Context<Self>) -> bool {
        let yields = self.code.is_none()
            && !self.single_line
            && self.content.is_empty()
            && !self.is_composing();
        if yields {
            cx.propagate();
        }
        yields
    }
    pub(crate) fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        self.clear_folds();
        self.content = text.into();
        self.lines.clear();
        self.code_changed();
        self.selection = self.content.len()..self.content.len();
        self.reversed = false;
        self.marked = None;
        self.scroll_x = px(0.);
        self.scroll_y = px(0.);
        cx.emit(Changed);
        cx.notify();
    }
    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }
    pub(crate) fn selection_dragging(&self) -> bool {
        self.selecting
    }
    pub(crate) fn selection_anchor(&self) -> Option<Bounds<Pixels>> {
        if self.selection.is_empty() {
            return None;
        }
        let bounds = self.bounds?;
        let origin =
            bounds.origin + self.position(self.cursor()) - point(self.scroll_x, self.scroll_y);
        bounds
            .contains(&origin)
            .then(|| Bounds::new(origin, size(px(1.), self.line_height())))
    }
    pub(crate) fn cursor_anchor(&self) -> Option<Bounds<Pixels>> {
        let bounds = self.bounds?;
        let origin =
            bounds.origin + self.position(self.cursor()) - point(self.scroll_x, self.scroll_y);
        Some(Bounds::new(origin, size(px(1.), self.line_height())))
    }
    pub(crate) fn completion_token(&self) -> Option<monocode_core::composer::Token> {
        if self.is_composing() || !self.selection.is_empty() {
            return None;
        }
        monocode_core::composer::token_at(&self.content, self.cursor())
    }
    pub(crate) fn highlight_composer_tokens(
        &mut self,
        ranges: Vec<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        if self.composer_tokens != ranges {
            self.composer_tokens = ranges;
            cx.notify();
        }
    }
    pub(crate) fn replace_completion(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection = self.clip_range(range);
        self.replace_text_in_range(None, text, window, cx);
    }
    /// Layouts and native text callbacks can still refer to the previous value.
    /// Every cursor and edit range must index the current UTF-8 buffer.
    fn clip_offset(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.content.len());
        while !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
    fn clip_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clip_offset(range.start);
        let end = self.clip_offset(range.end);
        start.min(end)..start.max(end)
    }
    fn replacement_range(&self, range: Option<Range<usize>>) -> Range<usize> {
        self.clip_range(
            range
                .map(|range| self.range8(range))
                .or_else(|| self.marked.clone())
                .unwrap_or_else(|| self.selection.clone()),
        )
    }
    fn cursor(&self) -> usize {
        self.clip_offset(if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        })
    }
    fn scroll(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code.is_some() {
            self.code_scroll(event, window, cx);
            return;
        }
        if self.single_line {
            return;
        }
        let line_height = px(self.line_height);
        let total = self
            .lines
            .iter()
            .map(|line| line.size(line_height).height)
            .fold(px(0.), |a, b| a + b);
        let viewport = self.bounds.map_or(px(0.), |b| b.size.height);
        let max_scroll = (total - viewport).max(px(0.));
        if max_scroll <= px(0.) {
            return;
        }
        let next =
            (self.scroll_y - event.delta.pixel_delta(line_height).y).clamp(px(0.), max_scroll);
        if next == self.scroll_y {
            // Let the surrounding page scroll once the textarea reaches an edge.
            return;
        }
        self.scroll_y = next;
        self.manual_scroll = true;
        cx.stop_propagation();
        cx.notify();
    }
    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        self.reveal_code();
        self.clear_completion();
        let offset = self.clip_offset(offset);
        self.selection = offset..offset;
        self.reversed = false;
        cx.notify();
    }
    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        self.reveal_code();
        let offset = self.clip_offset(offset);
        self.selection = self.clip_range(self.selection.clone());
        if self.reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.start > self.selection.end {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        cx.notify();
    }
    fn prev(&self, at: usize) -> usize {
        if let Some(at) = self.folded_boundary(at, true) {
            return at;
        }
        if self.code.is_some() {
            return self.content[..at]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(at, _)| at);
        }
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(ix, _)| (ix < at).then_some(ix))
            .unwrap_or(0)
    }
    fn next(&self, at: usize) -> usize {
        if let Some(at) = self.folded_boundary(at, false) {
            return at;
        }
        if self.code.is_some() {
            return self.content[at..]
                .grapheme_indices(true)
                .nth(1)
                .map_or(self.content.len(), |(next, _)| at + next);
        }
        self.content
            .grapheme_indices(true)
            .find_map(|(ix, _)| (ix > at).then_some(ix))
            .unwrap_or(self.content.len())
    }
    fn from16(&self, offset: usize) -> usize {
        from16(&self.content, offset)
    }
    fn to16(&self, offset: usize) -> usize {
        self.content[..self.clip_offset(offset)]
            .encode_utf16()
            .count()
    }
    fn range16(&self, r: Range<usize>) -> Range<usize> {
        let r = self.clip_range(r);
        self.to16(r.start)..self.to16(r.end)
    }
    fn range8(&self, r: Range<usize>) -> Range<usize> {
        self.clip_range(self.from16(r.start)..self.from16(r.end))
    }
    fn line_height(&self) -> Pixels {
        px(if self.single_line {
            self.height
        } else {
            self.line_height
        })
    }
    fn previous_word(&self) -> usize {
        self.content
            .unicode_word_indices()
            .rev()
            .find_map(|(start, _)| (start < self.cursor()).then_some(start))
            .unwrap_or(0)
    }
    fn next_word(&self) -> usize {
        self.content
            .unicode_word_indices()
            .map(|(start, word)| start + word.len())
            .find(|end| *end > self.cursor())
            .unwrap_or(self.content.len())
    }
    fn line_edge(&self, end: bool) -> usize {
        if self.code.is_some() {
            if self.word_wrap() {
                let p = self.code_position(self.cursor());
                return self.code_index(point(if end { px(f32::MAX) } else { px(0.) }, p.y));
            }
            let at = self.cursor();
            return if end {
                self.content[at..]
                    .find('\n')
                    .map_or(self.content.len(), |n| at + n)
            } else {
                self.content[..at].rfind('\n').map_or(0, |n| n + 1)
            };
        }
        if self.single_line {
            return if end { self.content.len() } else { 0 };
        }
        let p = self.position(self.cursor());
        self.index(point(if end { px(f32::MAX) } else { px(0.) }, p.y))
    }
    fn vertical(&self, down: bool) -> usize {
        if self.code.is_some() {
            return self.code_vertical(down);
        }
        let p = self.position(self.cursor());
        let y = p.y + self.line_height() * if down { 1. } else { -1. };
        if y < px(0.) {
            0
        } else {
            self.index(point(p.x, y))
        }
    }
    fn normalized<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.single_line && text.contains(['\r', '\n']) {
            Cow::Owned(text.replace("\r\n", " ").replace(['\r', '\n'], " "))
        } else {
            Cow::Borrowed(text)
        }
    }
    fn position(&self, at: usize) -> Point<Pixels> {
        if self.code.is_some() {
            return self.code_position(at);
        }
        let at = self.display_layout().to_display(at);
        let mut base = 0;
        let mut y = px(0.);
        for line in &self.lines {
            if at <= base + line.len() {
                return line
                    .position_for_index(at - base, self.line_height())
                    .unwrap_or_default()
                    + point(px(0.), y);
            }
            base += line.len() + 1;
            y += line.size(self.line_height()).height;
        }
        point(px(0.), y)
    }
    fn index(&self, p: Point<Pixels>) -> usize {
        // The placeholder is painted as text, but is never editable content.
        if self.content.is_empty() {
            return 0;
        }
        if self.code.is_some() {
            return self.clip_offset(self.code_index(p));
        }
        let mut base = 0;
        let mut y = p.y;
        for line in &self.lines {
            let h = line.size(self.line_height()).height;
            if y < h {
                let at = base
                    + line
                        .closest_index_for_position(point(p.x, y.max(px(0.))), self.line_height())
                        .unwrap_or_else(|ix| ix);
                let at = self.display_layout().to_source(at);
                return self.clip_offset(at);
            }
            y -= h;
            base += line.len() + 1;
        }
        self.content.len()
    }
    fn mouse_index(&self, p: Point<Pixels>) -> usize {
        self.bounds
            .map(|b| self.index(p - b.origin + point(self.scroll_x, self.scroll_y)))
            .unwrap_or(0)
            .min(self.content.len())
    }
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            if self.selection.is_empty() {
                self.prev(self.cursor())
            } else {
                self.selection.start
            },
            cx,
        );
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            if self.selection.is_empty() {
                self.next(self.cursor())
            } else {
                self.selection.end
            },
            cx,
        );
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev(self.cursor()), cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next(self.cursor()), cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_edge(false), cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_edge(true), cx);
    }
    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(self.line_edge(false), cx);
    }
    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(self.line_edge(true), cx);
    }
    fn document_start(&mut self, _: &DocumentStart, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.move_to(0, cx);
    }
    fn document_end(&mut self, _: &DocumentEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.move_to(self.content.len(), cx);
    }
    fn select_document_start(
        &mut self,
        _: &SelectDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(0, cx);
    }
    fn select_document_end(
        &mut self,
        _: &SelectDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(self.content.len(), cx);
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word(), cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word(), cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(self.previous_word(), cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.yields_navigation(cx) {
            return;
        }
        self.select_to(self.next_word(), cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completion_step(true, cx) {
            self.move_to(self.vertical(false), cx);
        }
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if !self.completion_step(false, cx) {
            self.move_to(self.vertical(true), cx);
        }
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(false), cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(true), cx);
    }
    fn delete_word_left(&mut self, _: &DeleteWordLeft, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.previous_word(), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn delete_word_right(&mut self, _: &DeleteWordRight, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.next_word(), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn delete_to_home(&mut self, _: &DeleteToHome, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.line_edge(false), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn delete_to_end(&mut self, _: &DeleteToEnd, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.line_edge(true), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn backspace(&mut self, _: &Backspace, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection == (0..0)
            && self.inline_commands
            && Command::leading(&self.content).is_none()
            && let Some(command) = self.selected_command
        {
            cx.emit(CommandRemoved(command.name()));
            return;
        }
        if self.selection.is_empty() {
            self.select_to(self.prev(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn delete(&mut self, _: &Delete, w: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            self.select_to(self.next(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", w, cx);
    }
    fn newline(&mut self, _: &Newline, w: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.insert_newline(w, cx);
        }
    }
    /// Enter in a markdown list continues it (`1.` → `2.`); Enter on an empty item ends it.
    fn insert_newline(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let at = self.selection.start;
        if self.selection.is_empty()
            && self.markdown
            && self.code.is_none()
            && !self.secret
            && self.marked.is_none()
            && !markdown::in_fence(&self.content, at)
            && let Some(step) = list_step(&self.content, at)
        {
            match step {
                ListStep::Continue(text) => self.replace_text_in_range(None, &text, w, cx),
                ListStep::End(start) => {
                    self.selection = start..at;
                    self.replace_text_in_range(None, "", w, cx);
                }
            }
            return;
        }
        self.replace_text_in_range(None, "\n", w, cx);
    }
    fn submit(&mut self, _: &Submit, w: &mut Window, cx: &mut Context<Self>) {
        if self.accept_completion(cx) {
            return;
        }
        if self.secondary_submit {
            if self.code.is_some() {
                let at = self.selection.start;
                let start = self.content[..at].rfind('\n').map_or(0, |n| n + 1);
                let indent: String = self.content[start..at]
                    .chars()
                    .take_while(|c| matches!(c, ' ' | '\t'))
                    .collect();
                self.replace_text_in_range(None, &format!("\n{indent}"), w, cx);
            } else {
                self.insert_newline(w, cx);
            }
        } else if self.marked.is_none() {
            cx.emit(Submitted);
        }
    }
    fn submit_secondary(&mut self, _: &SubmitSecondary, _: &mut Window, cx: &mut Context<Self>) {
        if self.secondary_submit {
            if self.marked.is_none() {
                cx.emit(Submitted);
            }
        } else if self.alternate_submit {
            if self.marked.is_none() {
                cx.emit(AlternateSubmitted);
            }
        } else {
            cx.propagate();
        }
    }
    fn paste(&mut self, _: &Paste, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace_text_in_range(None, &text.replace("\r\n", "\n"), w, cx);
        }
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.secret && !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selection.clone()].into(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, w: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, w, cx);
        self.replace_text_in_range(None, "", w, cx);
    }
    fn mouse_down(&mut self, e: &MouseDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        crate::traffic_lights::focus_content(w);
        if self.code_fold_down(e, cx) || self.code_scrollbar_down(e, w, cx) {
            return;
        }
        w.focus(&self.focus, cx);
        self.selecting = true;
        self.drag_position = Some(e.position);
        let ix = self.mouse_index(e.position);
        if e.modifiers.shift {
            self.select_to(ix, cx)
        } else if e.click_count >= 3 {
            let start = self.content[..ix].rfind('\n').map_or(0, |i| i + 1);
            let end = self.content[ix..]
                .find('\n')
                .map_or(self.content.len(), |i| ix + i);
            self.move_to(start, cx);
            self.select_to(end, cx);
        } else if e.click_count == 2 {
            let range = self
                .content
                .split_word_bound_indices()
                .find(|(start, word)| ix >= *start && ix < start + word.len())
                .map(|(start, word)| start..start + word.len())
                .unwrap_or(ix..ix);
            self.move_to(range.start, cx);
            self.select_to(range.end, cx);
        } else {
            self.move_to(ix, cx)
        }
    }
    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.end_drag();
        self.code_scrollbar_up();
    }
    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.code_hover(e, cx);
        self.code_scrollbar_move(e, cx);
    }
    fn end_drag(&mut self) {
        self.selecting = false;
        self.drag_position = None;
        self.autoscroll = None;
    }
    /// Extends a drag selection wherever the pointer goes, not only while it
    /// hovers the field, and keeps scrolling while it is held past an edge.
    fn drag_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if e.pressed_button != Some(MouseButton::Left) {
            self.end_drag();
            return;
        }
        self.drag_position = Some(e.position);
        self.drag_select(cx);
        if self.autoscroll.is_none() && self.drag_overshoot() != px(0.) {
            self.autoscroll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    if !this
                        .update(cx, |input, cx| input.autoscroll_step(cx))
                        .unwrap_or(false)
                    {
                        break;
                    }
                }
            }));
        }
    }
    /// Selects up to the pointer, pinned inside the viewport so the selection
    /// follows the scroll instead of jumping ahead of it.
    fn drag_select(&mut self, cx: &mut Context<Self>) {
        let (Some(p), Some(b)) = (self.drag_position, self.bounds) else {
            return;
        };
        let y = p.y.max(b.top()).min(b.bottom() - px(1.));
        self.select_to(self.mouse_index(point(p.x, y)), cx);
    }
    /// How far the dragging pointer is above (negative) or below the field.
    fn drag_overshoot(&self) -> Pixels {
        match (self.drag_position, self.bounds) {
            _ if self.single_line => px(0.),
            (Some(p), Some(b)) if p.y < b.top() => p.y - b.top(),
            (Some(p), Some(b)) if p.y > b.bottom() => p.y - b.bottom(),
            _ => px(0.),
        }
    }
    fn max_scroll_y(&self) -> Pixels {
        if self.code.is_some() {
            return self.code_max_scroll_y();
        }
        let total = self
            .lines
            .iter()
            .map(|line| line.size(self.line_height()).height)
            .fold(px(0.), |a, b| a + b);
        (total - self.bounds.map_or(px(0.), |b| b.size.height)).max(px(0.))
    }
    fn autoscroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let overshoot = self.drag_overshoot().as_f32();
        let max = self.max_scroll_y();
        if !self.selecting || overshoot == 0. || max <= px(0.) {
            self.autoscroll = None;
            return false;
        }
        // Speed grows with distance past the edge, like native text views.
        let limit = self.line_height * 3.;
        let step = (overshoot / 4.).clamp(-limit, limit);
        let step = if step.abs() < 1. { step.signum() } else { step };
        self.scroll_y = (self.scroll_y + px(step)).max(px(0.)).min(max);
        self.drag_select(cx);
        true
    }
}
/// GPUI only delivers `on_mouse_move` while the field is hovered, so a drag
/// selection listens at the window level until the button is released.
fn paint_drag_listener(input: &Entity<Input>, w: &mut Window, cx: &App) {
    if !input.read(cx).selecting {
        return;
    }
    let input = input.clone();
    w.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
        if phase == gpui::DispatchPhase::Capture {
            input.update(cx, |input, cx| input.drag_move(e, cx));
        }
    });
}
fn from16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

impl EntityInputHandler for Input {
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.read_only
    }
    fn text_input_configuration(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::TextInputConfiguration {
        let enabled = !self.secret
            && !self.single_line
            && self.inline_commands
            && self.code.is_none()
            && cx
                .try_global::<crate::settings::Preferences>()
                .is_some_and(|p| p.autocorrect);
        gpui::TextInputConfiguration {
            autocorrect: enabled,
            suggestions: enabled,
            ..Default::default()
        }
    }

    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.range8(r);
        *actual = Some(self.range16(r.clone()));
        Some(if self.secret {
            "*".repeat(self.content[r].encode_utf16().count())
        } else {
            self.content[r].into()
        })
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range16(self.selection.clone()),
            reversed: self.reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|r| self.range16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.emit(Changed);
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        let mut r = self.replacement_range(r);
        let mut corrected = None;
        if self.inline_commands
            && !self.single_line
            && self.code.is_none()
            && self.marked.is_none()
            && r.start == r.end
            && text.len() == 1
            && text.chars().all(char::is_whitespace)
            && cx
                .try_global::<crate::settings::Preferences>()
                .is_some_and(|p| p.autocorrect)
        {
            let start = self.content[..r.start]
                .rfind(|c: char| !c.is_alphabetic())
                .map_or(0, |ix| {
                    ix + self.content[ix..].chars().next().unwrap().len_utf8()
                });
            let word = &self.content[start..r.start];
            if word.len() >= 2
                && (start == 0
                    || self.content[..start]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_whitespace))
                && let Some(value) = spelling::correction(word)
            {
                r.start = start;
                corrected = Some(format!("{value}{text}"));
            }
        }
        let text = corrected.as_deref().unwrap_or(text);
        let previous_command = self
            .inline_commands
            .then(|| Command::leading(&self.content))
            .flatten();
        let normalized = self.normalized(text);
        if self.code.is_some() && !self.accept_code_edit(&r, &normalized, cx) {
            return;
        }
        let mut normalized = normalized.into_owned();
        if let Some(limit) = self.max_chars {
            let remaining = limit.saturating_sub(
                self.content[..r.start].encode_utf16().count()
                    + self.content[r.end..].encode_utf16().count(),
            );
            normalized = limit_utf16(&normalized, remaining);
        }
        self.record_undo();
        let text = bounded(
            &normalized,
            (if self.code.is_some() {
                monocode_core::files::document::MAX_TEXT_BYTES
            } else {
                MAX_BYTES
            })
            .saturating_sub(self.content.len() - (r.end - r.start)),
        );
        self.map_code_folds(&r, text.len());
        self.content = format!(
            "{}{}{}",
            &self.content[..r.start],
            text,
            &self.content[r.end..]
        )
        .into();
        let end = r.start + text.len();
        self.code_changed();
        self.selection = end..end;
        self.reversed = false;
        self.marked = None;
        self.command_removed(previous_command, cx);
        cx.emit(Changed);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        self.manual_scroll = false;
        self.reveal_pending = self.reveal_in_parent;
        let r = self.replacement_range(r);
        let normalized = self.normalized(text);
        if self.code.is_some() && !self.accept_code_edit(&r, &normalized, cx) {
            return;
        }
        let mut normalized = normalized.into_owned();
        if let Some(limit) = self.max_chars {
            let remaining = limit.saturating_sub(
                self.content[..r.start].encode_utf16().count()
                    + self.content[r.end..].encode_utf16().count(),
            );
            normalized = limit_utf16(&normalized, remaining);
        }
        self.record_undo();
        let text = bounded(
            &normalized,
            (if self.code.is_some() {
                monocode_core::files::document::MAX_TEXT_BYTES
            } else {
                MAX_BYTES
            })
            .saturating_sub(self.content.len() - (r.end - r.start)),
        );
        self.map_code_folds(&r, text.len());
        self.content = format!(
            "{}{}{}",
            &self.content[..r.start],
            text,
            &self.content[r.end..]
        )
        .into();
        let text = text.to_owned();
        self.code_changed();
        self.marked = (!text.is_empty()).then_some(r.start..r.start + text.len());
        let selection = selection
            .map(|s| r.start + from16(&text, s.start)..r.start + from16(&text, s.end))
            .unwrap_or(r.start + text.len()..r.start + text.len());
        self.selection = self.clip_range(selection);
        self.reversed = false;
        cx.emit(Changed);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let r = self.range8(r);
        let origin =
            self.bounds?.origin + self.position(r.start) - point(self.scroll_x, self.scroll_y);
        Some(Bounds::new(origin, size(px(2.), self.line_height())))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.to16(self.mouse_index(p)))
    }
}
fn limit_utf16(text: &str, units: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= units
        })
        .collect()
}
fn bounded(text: &str, max: usize) -> &str {
    let mut n = max.min(text.len());
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    &text[..n]
}

struct InputElement {
    input: Entity<Input>,
}
struct PaintState {
    lines: Vec<WrappedLine>,
    scroll: Point<Pixels>,
    layout: DisplayLayout,
}
impl IntoElement for InputElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = PaintState;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        w: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let i = self.input.read(cx);
        if i.single_line || i.content.is_empty() {
            style.size.height = px(i.height).into();
            return (w.request_layout(style, [], cx), ());
        }
        // Grow with the wrapped draft, like the auto-resizing textarea.
        let (text, command) = i.display_text();
        let spans = i.markdown_display_spans(&command);
        let font_size = px(i.font_size);
        let (line_height, min_height, max_height) = (i.line_height, i.min_height, i.max_height);
        let font = w.text_style().font();
        let layout = w.request_measured_layout(style, move |known, available, w, _| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = markdown_runs(&run, &spans);
            let height = w
                .text_system()
                .shape_text(text.clone(), font_size, &runs, width, None)
                .map(|lines| {
                    lines
                        .iter()
                        .map(|line| line.size(px(line_height)).height)
                        .fold(px(0.), |a, b| a + b)
                })
                .unwrap_or_default();
            size(
                width.unwrap_or_default(),
                height.max(px(min_height)).min(px(max_height)),
            )
        });
        (layout, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        w: &mut Window,
        cx: &mut App,
    ) -> PaintState {
        let i = self.input.read(cx);
        let style = w.text_style();
        let placeholder = i.content.is_empty();
        let (text, layout) = i.display_text();
        let command = layout.command;
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: match (placeholder, i.placeholder_alpha) {
                // Re-ink every paint so a theme switch never strands the
                // placeholder in the previous theme's color.
                (true, Some(alpha)) => crate::theme::content(alpha),
                // `.composer-field::placeholder`: content ink at 40%.
                (true, None) if !i.single_line => crate::theme::content(0.4),
                _ => style.color,
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let to_display = |at| layout.to_display(at);
        // Mentions read as plain ink beside their icon; other tokens take the accent.
        let highlights: Vec<_> = if placeholder {
            Vec::new()
        } else {
            i.composer_tokens
                .iter()
                .filter(|range| {
                    range.end <= i.content.len()
                        && i.content.is_char_boundary(range.start)
                        && i.content.is_char_boundary(range.end)
                })
                .map(|range| {
                    let mention = layout.mentions.iter().any(|(at, _)| *at == range.start);
                    (to_display(range.start)..to_display(range.end), mention)
                })
                .collect()
        };
        let marked = i
            .marked
            .as_ref()
            .map(|range| to_display(range.start)..to_display(range.end));
        let spans = if placeholder {
            Vec::new()
        } else {
            i.markdown_display_spans(&layout)
        };
        let mut boundaries = vec![0, text.len()];
        for (range, _) in &spans {
            boundaries.extend([range.start, range.end]);
        }
        for range in highlights
            .iter()
            .map(|(range, _)| range)
            .chain(marked.iter())
        {
            boundaries.extend([range.start, range.end]);
        }
        if let Some(command) = command {
            boundaries.extend([command.icon_range().end, command.name_range().end]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut sweep = markdown::Sweep::new(&spans);
        let runs: Vec<_> = boundaries
            .windows(2)
            .filter(|pair| pair[0] < pair[1])
            .map(|pair| {
                let at = pair[0];
                let md = sweep.at(at);
                let color = if command.is_some_and(|c| c.icon_range().contains(&at)) {
                    crate::theme::content(0.)
                } else if let Some(command) = command.filter(|c| c.name_range().contains(&at)) {
                    command.mode.color()
                } else if let Some((_, mention)) =
                    highlights.iter().find(|(range, _)| range.contains(&at))
                {
                    if *mention {
                        crate::theme::content(1.)
                    } else {
                        crate::theme::accent_color()
                    }
                } else {
                    markdown_color(md, run.color)
                };
                TextRun {
                    len: pair[1] - at,
                    color,
                    font: markdown_font(md, &run.font),
                    background_color: md.code.then(|| crate::theme::content(0.08)),
                    strikethrough: md.strike.then(|| gpui::StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(color),
                    }),
                    underline: marked
                        .as_ref()
                        .filter(|range| range.contains(&at))
                        .map(|_| UnderlineStyle {
                            color: Some(style.color),
                            thickness: px(1.),
                            wavy: false,
                        })
                        .or_else(|| {
                            md.link.then(|| UnderlineStyle {
                                color: Some(crate::theme::accent_color()),
                                thickness: px(1.),
                                wavy: false,
                            })
                        }),
                }
            })
            .collect();
        let lines: Vec<_> = w
            .text_system()
            .shape_text(
                text,
                px(i.font_size),
                &runs,
                (!i.single_line).then_some(bounds.size.width),
                None,
            )
            .unwrap_or_default()
            .into_iter()
            .collect();
        // Compute caret position against the new layout before choosing scroll.
        let cursor = to_display(i.cursor());
        let mut base = 0;
        let mut y = px(0.);
        let line_height = i.line_height();
        let mut caret = point(px(0.), px(0.));
        for line in &lines {
            if cursor <= base + line.len() {
                caret = line
                    .position_for_index(cursor - base, line_height)
                    .unwrap_or_default()
                    + point(px(0.), y);
                break;
            }
            base += line.len() + 1;
            y += line.size(line_height).height;
        }
        // Single-line fields never wrap or scroll vertically. In particular,
        // a line taller than its viewport used to alternate between 0 and 2 px
        // on every repaint, making even the empty placeholder visibly jump.
        let scroll_y = if i.manual_scroll {
            i.scroll_y
        } else if i.single_line || caret.y < i.scroll_y {
            caret.y
        } else if caret.y + line_height > i.scroll_y + bounds.size.height {
            caret.y + line_height - bounds.size.height
        } else {
            i.scroll_y
        };
        let scroll_x = if !i.single_line || placeholder {
            px(0.)
        } else if caret.x < i.scroll_x {
            caret.x
        } else if caret.x + px(2.) > i.scroll_x + bounds.size.width {
            (caret.x + px(2.) - bounds.size.width).max(px(0.))
        } else {
            i.scroll_x
        };
        let pending = i.reveal_pending;
        let reveal = pending && i.focus.is_focused(w);
        if reveal {
            w.request_autoscroll(Bounds::new(
                bounds.origin + caret - point(scroll_x, scroll_y + px(8.)),
                size(px(2.), line_height + px(16.)),
            ));
        }
        // Consume the request once. Repainting or manually scrolling a focused
        // document must not keep snapping back to its last caret position.
        if pending {
            self.input
                .update(cx, |input, _| input.reveal_pending = false);
        }
        PaintState {
            lines,
            scroll: point(scroll_x, scroll_y),
            layout,
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        state: &mut PaintState,
        w: &mut Window,
        cx: &mut App,
    ) {
        paint_drag_listener(&self.input, w, cx);
        let i = self.input.read(cx);
        let focus = i.focus.clone();
        let to_display = |at| state.layout.to_display(at);
        let selection = to_display(i.selection.start)..to_display(i.selection.end);
        let cursor = to_display(i.cursor());
        let line_height = i.line_height();
        let center_offset = if i.centered && i.single_line {
            (bounds.size.width
                - state
                    .lines
                    .first()
                    .map(|line| line.size(line_height).width)
                    .unwrap_or_default())
            .max(px(0.))
                / 2.
        } else {
            px(0.)
        };
        w.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        w.with_content_mask(Some(ContentMask { bounds }), |w| {
            let mut base = 0;
            let x = bounds.left() - state.scroll.x + center_offset;
            let mut y = bounds.top() - state.scroll.y;
            for line in &state.lines {
                let height = line.size(line_height).height;
                if y + height >= bounds.top() && y < bounds.bottom() {
                    let start = selection.start.saturating_sub(base).min(line.len());
                    let end = selection.end.saturating_sub(base).min(line.len());
                    if start < end {
                        let a = line
                            .position_for_index(start, line_height)
                            .unwrap_or_default();
                        let b = line
                            .position_for_index(end, line_height)
                            .unwrap_or_default();
                        let mut sy = a.y;
                        while sy <= b.y {
                            let left = if sy == a.y { a.x } else { px(0.) };
                            let right = if sy == b.y { b.x } else { bounds.size.width };
                            w.paint_quad(fill(
                                Bounds::new(
                                    point(x + left, y + sy),
                                    size((right - left).max(px(1.)), line_height),
                                ),
                                crate::theme::text_selection(),
                            ));
                            sy += line_height;
                        }
                    }
                    let _ = line.paint(point(x, y), line_height, TextAlign::Left, None, w, cx);
                    for (source, icon) in &state.layout.mentions {
                        let at = to_display(*source);
                        if at < base || at >= base + line.len() {
                            continue;
                        }
                        // Centered in the em blank; the thin blank after it is the gap.
                        let (Some(a), Some(b)) = (
                            line.position_for_index(at - base, line_height),
                            line.position_for_index(at + mention::ICON_LEN - base, line_height),
                        ) else {
                            continue;
                        };
                        let side = px(mention::ICON_SIDE);
                        let icon_bounds = Bounds::new(
                            point(
                                x + (a.x + b.x.max(a.x)) / 2. - side / 2.,
                                y + a.y + (line_height - side) / 2.,
                            ),
                            size(side, side),
                        );
                        match icon {
                            MentionIcon::File(asset) => {
                                if let Some(Ok(image)) = w.use_asset::<ImgResourceLoader>(
                                    &Resource::Embedded(asset.clone()),
                                    cx,
                                ) {
                                    let _ = w.paint_image(
                                        bounds,
                                        icon_bounds,
                                        Default::default(),
                                        image,
                                        0,
                                        false,
                                    );
                                }
                            }
                            MentionIcon::Note => {
                                let _ = w.paint_svg(
                                    icon_bounds,
                                    "note".into(),
                                    None,
                                    Default::default(),
                                    crate::theme::accent_color(),
                                    cx,
                                );
                            }
                        }
                    }
                    if focus.is_focused(w)
                        && selection.is_empty()
                        && cursor >= base
                        && cursor <= base + line.len()
                    {
                        let p = line
                            .position_for_index(cursor - base, line_height)
                            .unwrap_or_default();
                        w.paint_quad(fill(
                            Bounds::new(point(x + p.x, y + p.y), size(px(1.5), line_height)),
                            crate::theme::content(1.),
                        ));
                    }
                }
                base += line.len() + 1;
                y += height;
            }
            if let Some(command) = state.layout.command {
                let icon_bounds = Bounds::new(
                    bounds.origin
                        + point(
                            -state.scroll.x,
                            (line_height - px(14.)) / 2. - state.scroll.y,
                        ),
                    size(px(14.), px(14.)),
                );
                let _ = w.paint_svg(
                    icon_bounds,
                    command.mode.icon().into(),
                    None,
                    Default::default(),
                    command.mode.color(),
                    cx,
                );
            }
        });
        self.input.update(cx, |i, _| {
            i.lines = std::mem::take(&mut state.lines);
            i.bounds = Some(Bounds::new(
                bounds.origin + point(center_offset, px(0.)),
                bounds.size,
            ));
            i.scroll_x = state.scroll.x;
            i.scroll_y = state.scroll.y;
        });
    }
}
impl Render for Input {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("native-composer")
            .relative()
            .role(if self.single_line {
                Role::TextInput
            } else {
                Role::MultilineTextInput
            })
            .aria_label(self.label.clone())
            .aria_value(if self.secret {
                "*".repeat(self.content.chars().count()).into()
            } else {
                self.content.clone()
            })
            .tab_index(0)
            .w_full()
            .min_w_0()
            .key_context(if self.code.is_some() {
                "Composer CodeEditor"
            } else {
                "Composer"
            })
            .when(self.code.is_some(), |d| {
                d.h_full().font_family(crate::fonts::mono())
            })
            .track_focus(&self.focus)
            .cursor(if self.code_rail_hovered() {
                CursorStyle::Arrow
            } else {
                CursorStyle::IBeam
            })
            .on_action(cx.listener(Self::start_completion))
            .on_action(cx.listener(Self::dismiss_completion))
            .on_action(cx.listener(Self::fold))
            .on_action(cx.listener(Self::unfold))
            .on_action(cx.listener(Self::fold_all))
            .on_action(cx.listener(Self::unfold_all))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::document_start))
            .on_action(cx.listener(Self::document_end))
            .on_action(cx.listener(Self::select_document_start))
            .on_action(cx.listener(Self::select_document_end))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_home))
            .on_action(cx.listener(Self::delete_to_end))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::submit_secondary))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .child(if self.code.is_some() {
                code::CodeElement { input: cx.entity() }.into_any_element()
            } else {
                InputElement { input: cx.entity() }.into_any_element()
            })
            .children(self.code_tooltip())
            .children(self.completion_popup(cx))
            .children(self.hunk_actions(cx))
    }
}

#[cfg(test)]
fn style_at(spans: &markdown::Spans, at: usize) -> markdown::Style {
    spans
        .iter()
        .filter(|(range, _)| range.contains(&at))
        .fold(markdown::Style::default(), |all, (_, style)| {
            all.merge(*style)
        })
}

fn markdown_font(style: markdown::Style, base: &gpui::Font) -> gpui::Font {
    let mut font = if style.code {
        gpui::font(crate::fonts::mono())
    } else {
        base.clone()
    };
    if style.bold {
        font.weight = gpui::FontWeight::BOLD;
    }
    if style.italic {
        font.style = gpui::FontStyle::Italic;
    }
    font
}

fn markdown_color(style: markdown::Style, base: Hsla) -> Hsla {
    if style.dim {
        crate::theme::content(0.4)
    } else if style.accent || style.link {
        crate::theme::accent_color()
    } else if style.quote {
        crate::theme::content(0.65)
    } else {
        base
    }
}

/// Font-only runs, enough to measure how markdown styling changes wrapping.
fn markdown_runs(base: &TextRun, spans: &markdown::Spans) -> Vec<TextRun> {
    let mut boundaries = vec![0, base.len];
    for (range, _) in spans {
        boundaries.extend([range.start.min(base.len), range.end.min(base.len)]);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut sweep = markdown::Sweep::new(spans);
    boundaries
        .windows(2)
        .map(|pair| TextRun {
            len: pair[1] - pair[0],
            font: markdown_font(sweep.at(pair[0]), &base.font),
            ..base.clone()
        })
        .collect()
}

enum ListStep {
    /// Text to insert instead of a bare newline.
    Continue(String),
    /// The item is empty: drop its marker, back to this line start.
    End(usize),
}

/// What Enter should do at `at` when the line is a markdown list item or quote.
fn list_step(content: &str, at: usize) -> Option<ListStep> {
    let start = content[..at].rfind('\n').map_or(0, |n| n + 1);
    let before = &content[start..at];
    let body = before.trim_start_matches([' ', '\t']);
    let indent = &before[..before.len() - body.len()];
    let (marker, rest) = if let Some(rest) = body.strip_prefix("> ") {
        ("> ".to_string(), rest)
    } else if let Some(rest) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| body.strip_prefix(bullet))
    {
        (body[..2].to_string(), rest)
    } else {
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        let (number, tail) = body.split_at(digits);
        let delimiter = tail.chars().next().filter(|c| matches!(c, '.' | ')'))?;
        let rest = tail[1..].strip_prefix(' ')?;
        if number.is_empty() || number.len() > 9 {
            return None;
        }
        (
            format!("{}{delimiter} ", number.parse::<u64>().ok()? + 1),
            rest,
        )
    };
    let checkbox = ["[ ]", "[x]", "[X]"];
    let (text, task) = match checkbox.iter().find_map(|c| rest.strip_prefix(c)) {
        Some(text) if marker != "> " && (text.is_empty() || text.starts_with(' ')) => {
            (text, "[ ] ")
        }
        _ => (rest, ""),
    };
    if text.trim().is_empty() {
        return Some(ListStep::End(start));
    }
    Some(ListStep::Continue(format!("\n{indent}{marker}{task}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(text: &str) -> String {
        match list_step(text, text.len()) {
            Some(ListStep::Continue(next)) => format!("{text}{next}"),
            Some(ListStep::End(start)) => text[..start].to_string(),
            None => format!("{text}\n"),
        }
    }
    #[::core::prelude::v1::test]
    fn enter_continues_and_ends_markdown_lists() {
        assert_eq!(press("1. one"), "1. one\n2. ");
        assert_eq!(press("9) nine"), "9) nine\n10) ");
        assert_eq!(press("- a"), "- a\n- ");
        assert_eq!(press("intro\n  * a"), "intro\n  * a\n  * ");
        assert_eq!(press("- [x] done"), "- [x] done\n- [ ] ");
        assert_eq!(press("> quoted"), "> quoted\n> ");
        assert_eq!(press("1. one\n2. "), "1. one\n");
        assert_eq!(press("- "), "");
        assert_eq!(press("- [ ]"), "");
        assert_eq!(press("plain"), "plain\n");
        assert_eq!(press("2024. was a year"), "2024. was a year\n2025. ");
        assert_eq!(press("-dash"), "-dash\n");
        assert_eq!(press("1.5 liters"), "1.5 liters\n");
    }
    #[::core::prelude::v1::test]
    fn mention_tokens_pick_their_file_icon() {
        let asset = |token| match MentionIcon::of(token) {
            Some(MentionIcon::File(asset)) => asset.to_string(),
            Some(MentionIcon::Note) => "note".into(),
            None => String::new(),
        };
        assert_eq!(asset("@src/main.rs"), "mi/rust.svg");
        assert_eq!(asset("@\"docs/My Notes.md\""), "mi/markdown.svg");
        assert_eq!(asset("@note/todo"), "note");
        assert_eq!(asset("/plan"), "");
    }
    #[::core::prelude::v1::test]
    fn mention_slots_map_offsets_both_ways() {
        let source = "see @a.rs and @b.md";
        let layout = DisplayLayout {
            command: None,
            mentions: vec![(4, MentionIcon::Note), (14, MentionIcon::Note)],
        };
        let display = layout.apply(source.into());
        assert_eq!(
            display.as_ref(),
            "see \u{2003}\u{2009}a.rs and \u{2003}\u{2009}b.md"
        );
        for at in 0..=source.len() {
            assert_eq!(layout.to_source(layout.to_display(at)), at);
        }
        // Clicking inside a blank lands on the nearer side of its `@`.
        assert_eq!(layout.to_source(4 + 3), 4);
        assert_eq!(layout.to_source(4 + 6), 5);
    }
    #[::core::prelude::v1::test]
    fn utf16_and_bounded_paste_respect_unicode() {
        assert_eq!(from16("a🙂漢", 1), 1);
        assert_eq!(from16("a🙂漢", 2), 1);
        assert_eq!(from16("a🙂漢", 3), 5);
        assert_eq!(bounded("🙂a", 3), "");
    }
}

#[cfg(test)]
mod native_input_tests {
    use super::*;
    use gpui::{TestAppContext, WindowBounds, WindowHandle, WindowOptions};

    fn field(cx: &mut TestAppContext, single_line: bool) -> WindowHandle<Input> {
        let window = cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(260.), px(100.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(if single_line {
                        Input::search
                    } else {
                        Input::new
                    })
                },
            )
            .unwrap()
        });
        window
            .update(cx, |input, w, cx| w.focus(&input.focus, cx))
            .unwrap();
        cx.refresh().unwrap();
        window
    }
    #[gpui::test]
    fn bounded_textarea_wheel_scroll_survives_repaint_and_typing_reveals_caret(
        cx: &mut TestAppContext,
    ) {
        let window = field(cx, false);
        window
            .update(cx, |input, _, cx| {
                input.height = 60.;
                input.min_height = 60.;
                input.max_height = 60.;
                input.set_text("Read each line of these instructions.\n".repeat(30), cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let (before, position) = window
            .update(visual, |input, _, _| {
                (input.scroll_y, input.bounds.unwrap().center())
            })
            .unwrap();
        assert!(before > px(80.));
        visual.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(80.))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        visual.run_until_parked();
        let scrolled = window
            .update(visual, |input, _, _| {
                assert!(input.manual_scroll);
                input.scroll_y
            })
            .unwrap();
        assert!(scrolled < before);
        window.update(visual, |_, _, cx| cx.notify()).unwrap();
        visual.run_until_parked();
        assert_eq!(
            window.update(visual, |input, _, _| input.scroll_y).unwrap(),
            scrolled
        );
        visual.simulate_input("Additional instruction");
        visual.run_until_parked();
        window
            .update(visual, |input, _, _| {
                assert!(!input.manual_scroll);
                assert!(input.scroll_y > scrolled);
            })
            .unwrap();
    }
    #[gpui::test]
    fn field_limits_and_disabled_editing_preserve_text(cx: &mut TestAppContext) {
        let window = field(cx, true);
        window
            .update(cx, |input, _, _| input.set_max_chars(48))
            .unwrap();
        replace(window, &format!("{}🙂🙂", "a".repeat(45)), cx);
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.encode_utf16().count(), 47);
                input.set_read_only(true);
            })
            .unwrap();
        replace(window, "must not replace while signing in", cx);
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.encode_utf16().count(), 47)
            })
            .unwrap();
    }
    fn replace(window: WindowHandle<Input>, text: &str, cx: &mut TestAppContext) {
        window
            .update(cx, |input, w, cx| {
                input.select_all(&SelectAll, w, cx);
                input.replace_text_in_range(None, text, w, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
    }
    fn selected(input: &Entity<Input>, cx: &TestAppContext) -> String {
        input.read_with(cx, |i, _| i.content[i.selection.clone()].to_string())
    }

    #[gpui::test]
    fn markdown_styling_keeps_geometry_and_shift_enter_continues_lists(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        window
            .update(cx, |i, _, cx| i.enable_inline_commands(cx))
            .unwrap();
        let text = "## Plan\n**bold** and `code` with [a link](http://x.y) that wraps around the narrow field";
        replace(window, text, cx);
        input.read_with(cx, |i, _| {
            let (display, command) = i.display_text();
            assert_eq!(display.as_ref(), text);
            let spans = i.markdown_display_spans(&command);
            assert!(style_at(&spans, 3).bold);
            for (at, _) in text.char_indices() {
                assert_eq!(i.index(i.position(at)), at, "offset {at}");
            }
        });
        replace(window, "1. one", cx);
        cx.simulate_keystrokes(window.into(), "shift-enter t w o shift-enter");
        input.read_with(cx, |i, _| assert_eq!(i.content, "1. one\n2. two\n3. "));
        cx.simulate_keystrokes(window.into(), "shift-enter");
        input.read_with(cx, |i, _| assert_eq!(i.content, "1. one\n2. two\n"));
    }
    #[gpui::test]
    fn plain_multiline_fields_do_not_continue_markdown_markers(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        for text in ["> output.txt", "- item", "1. step"] {
            replace(window, text, cx);
            cx.simulate_keystrokes(window.into(), "shift-enter");
            input.read_with(cx, |i, _| assert_eq!(i.content, format!("{text}\n")));
        }
    }
    #[gpui::test]
    fn fenced_code_in_the_composer_gets_plain_newlines(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        window
            .update(cx, |i, _, cx| i.enable_inline_commands(cx))
            .unwrap();
        for (text, expected) in [
            ("```yaml\n- a", "```yaml\n- a\n"),
            (
                "```sh\necho hi > out.txt\n> log",
                "```sh\necho hi > out.txt\n> log\n",
            ),
            ("~~~\n1. one", "~~~\n1. one\n"),
            // A closed fence is prose again.
            ("```\ncode\n```\n- a", "```\ncode\n```\n- a\n- "),
        ] {
            replace(window, text, cx);
            cx.simulate_keystrokes(window.into(), "shift-enter");
            input.read_with(cx, |i, _| assert_eq!(i.content, expected));
        }
    }
    #[gpui::test]
    fn long_formatted_drafts_stay_responsive(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        window
            .update(cx, |i, _, cx| i.enable_inline_commands(cx))
            .unwrap();
        let text =
            "## Heading\n- **bold** item with `code` and [a link](http://x.y)\n> *quoted* text\n"
                .repeat(1500);
        let started = std::time::Instant::now();
        replace(window, &text, cx);
        cx.simulate_keystrokes(window.into(), "a b c");
        let elapsed = started.elapsed();
        input.read_with(cx, |i, _| {
            let (_, command) = i.display_text();
            let spans = i.markdown_display_spans(&command);
            assert!(spans.len() > 10_000);
            let mut sweep = markdown::Sweep::new(&spans);
            for at in (0..i.content.len()).step_by(97) {
                assert_eq!(sweep.at(at), style_at(&spans, at), "offset {at}");
            }
        });
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "styling a {}-byte draft took {elapsed:?}",
            text.len()
        );
    }
    #[gpui::test]
    fn inline_commands_preserve_text_geometry_wrapping_and_clipboard(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        window
            .update(cx, |i, _, cx| i.enable_inline_commands(cx))
            .unwrap();
        for (name, icon) in [
            ("plan", "ai-idea"),
            ("operator", "cursor-magic"),
            ("orchestrator", "share"),
        ] {
            let text = format!(
                "/{name} café 🙂\nA long prompt that wraps across several visual lines and keeps the full input width."
            );
            replace(window, &text, cx);
            input.read_with(cx, |i, _| {
                let layout = i.command_layout().unwrap();
                assert_eq!(layout.mode.icon(), icon);
                assert!(i.position(1).x >= px(14.));
                assert_eq!(i.position(name.len() + 2).y, px(0.));
                assert!(i.has_message_text());
                for (at, _) in text.char_indices().skip(1) {
                    assert_eq!(i.index(i.position(at)), at, "{name}: offset {at}");
                }
                let next_line = text.find('\n').unwrap() + 1;
                assert_eq!(i.position(next_line).x, px(0.));
                assert!(!i.lines[1].wrap_boundaries().is_empty());
            });
            cx.simulate_keystrokes(
                window.into(),
                if cfg!(target_os = "macos") {
                    "cmd-a cmd-c"
                } else {
                    "ctrl-a ctrl-c"
                },
            );
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some(text.as_str())
            );
        }
        replace(window, "/plan ", cx);
        assert!(!input.read_with(cx, |i, _| i.has_message_text()));
        replace(window, "/ plan", cx);
        assert!(input.read_with(cx, |i, _| i.command_layout().is_none()
            && i.has_message_text()));
    }

    #[gpui::test]
    fn retained_inline_mode_keeps_placeholder_caret_and_ime_in_sync(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        window
            .update(cx, |i, _, cx| {
                i.enable_inline_commands(cx);
                i.selected_command(Some(Command::Plan), cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        input.read_with(cx, |i, _| {
            assert!(i.content.is_empty());
            assert!(i.position(0).x > px(14.));
            assert_eq!(i.index(i.position(0)), 0);
            assert!(!i.has_message_text());
        });
        window
            .update(cx, |i, w, cx| {
                i.replace_and_mark_text_in_range(None, "日本", Some(2..2), w, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        input.read_with(cx, |i, _| {
            assert!(i.is_composing());
            assert_eq!(i.index(i.position("日本".len())), "日本".len());
        });
        window
            .update(cx, |i, w, cx| {
                i.replace_text_in_range(None, "日本語", w, cx);
                assert_eq!(i.content.as_ref(), "日本語");
                assert_eq!(i.take(cx).as_deref(), Some("日本語"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn keyboard_selection_dispatches_in_search_and_composer(cx: &mut TestAppContext) {
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        let word_modifier = if cfg!(target_os = "macos") {
            "alt"
        } else {
            "ctrl"
        };
        let start = if cfg!(target_os = "macos") {
            "cmd-left"
        } else {
            "home"
        };
        let end = if cfg!(target_os = "macos") {
            "cmd-right"
        } else {
            "end"
        };
        let select_start = if cfg!(target_os = "macos") {
            "cmd-shift-left"
        } else {
            "shift-home"
        };
        let select_end = if cfg!(target_os = "macos") {
            "cmd-shift-right"
        } else {
            "shift-end"
        };
        for single_line in [true, false] {
            let window = field(cx, single_line);
            let input = window.root(cx).unwrap();
            replace(window, "alpha café omega", cx);
            cx.simulate_keystrokes(window.into(), select_start);
            assert_eq!(selected(&input, cx), "alpha café omega");
            assert!(input.read_with(cx, |i, _| i.reversed));
            cx.simulate_keystrokes(
                window.into(),
                &format!("right {word_modifier}-shift-left {modifier}-c"),
            );
            assert_eq!(selected(&input, cx), "omega");
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("omega")
            );
            cx.simulate_keystrokes(
                window.into(),
                &format!("{start} shift-right shift-left {select_end}"),
            );
            assert_eq!(selected(&input, cx), "alpha café omega");
            assert!(!input.read_with(cx, |i, _| i.reversed));
            cx.simulate_keystrokes(window.into(), &format!("{end} {word_modifier}-backspace"));
            assert_eq!(
                input.read_with(cx, |i, _| i.content.to_string()),
                "alpha café "
            );
            replace(window, "café e\u{301}🙂", cx);
            cx.simulate_keystrokes(window.into(), "shift-left shift-left");
            assert_eq!(selected(&input, cx), "e\u{301}🙂");
            cx.simulate_input(window.into(), "X");
            assert_eq!(input.read_with(cx, |i, _| i.content.to_string()), "café X");
        }
    }

    #[gpui::test]
    fn multiline_line_and_document_selection_preserve_the_anchor(cx: &mut TestAppContext) {
        let window = field(cx, false);
        let input = window.root(cx).unwrap();
        replace(window, "first line\nsecond word\nlast line", cx);
        let (start, end, select_start, select_end, doc_start, doc_end) =
            if cfg!(target_os = "macos") {
                (
                    "cmd-left",
                    "cmd-right",
                    "cmd-shift-left",
                    "cmd-shift-right",
                    "cmd-up",
                    "cmd-shift-down",
                )
            } else {
                (
                    "home",
                    "end",
                    "shift-home",
                    "shift-end",
                    "ctrl-home",
                    "ctrl-shift-end",
                )
            };
        cx.simulate_keystrokes(window.into(), &format!("{start} up {end} {select_start}"));
        assert_eq!(selected(&input, cx), "second word");
        cx.simulate_keystrokes(window.into(), &format!("left {select_end}"));
        assert_eq!(selected(&input, cx), "second word");
        cx.simulate_keystrokes(window.into(), "left shift-down");
        assert_eq!(selected(&input, cx), "second word\n");
        cx.simulate_keystrokes(window.into(), &format!("{doc_start} {doc_end}"));
        assert_eq!(selected(&input, cx), "first line\nsecond word\nlast line");
        replace(
            window,
            "A long composer line that wraps around the field width several times to check visual line selection.",
            cx,
        );
        assert!(input.read_with(cx, |i, _| !i.lines[0].wrap_boundaries().is_empty()));
        cx.simulate_keystrokes(window.into(), select_start);
        let last_visual_line = selected(&input, cx);
        assert!(
            last_visual_line.ends_with("selection.")
                && last_visual_line.len() < input.read_with(cx, |i, _| i.content.len())
        );
    }

    #[gpui::test]
    fn search_repaints_keep_placeholder_still_and_long_queries_scroll_horizontally(
        cx: &mut TestAppContext,
    ) {
        let window = field(cx, true);
        let input = window.root(cx).unwrap();
        let bounds = input.read_with(cx, |i, _| i.bounds.unwrap());
        for _ in 0..12 {
            cx.refresh().unwrap();
            input.read_with(cx, |i, _| {
                assert_eq!(
                    i.scroll_y,
                    px(0.),
                    "placeholder must stay on the same baseline"
                );
                assert_eq!(i.bounds.unwrap(), bounds);
            });
        }
        replace(window, &"a long search query 🙂 ".repeat(20), cx);
        input.read_with(cx, |i, _| {
            assert_eq!(i.scroll_y, px(0.));
            assert!(i.scroll_x > px(0.));
            assert_eq!(i.lines.len(), 1);
            assert!(i.lines[0].wrap_boundaries().is_empty());
            let caret = i.position(i.cursor()).x - i.scroll_x;
            assert!(caret >= px(0.) && caret < bounds.size.width);
            let click = bounds.origin + point(px(30.), px(10.));
            let at = i.mouse_index(click);
            assert!(at > 0 && i.content.is_char_boundary(at));
        });
        cx.simulate_keystrokes(
            window.into(),
            if cfg!(target_os = "macos") {
                "cmd-left"
            } else {
                "home"
            },
        );
        cx.refresh().unwrap();
        assert_eq!(input.read_with(cx, |i, _| i.scroll_x), px(0.));
        replace(window, "first\nsecond\r\nthird", cx);
        assert_eq!(
            input.read_with(cx, |i, _| i.content.to_string()),
            "first second third"
        );
        replace(window, "", cx);
        for _ in 0..6 {
            cx.refresh().unwrap();
            assert_eq!(
                input.read_with(cx, |i, _| (i.scroll_x, i.scroll_y)),
                (px(0.), px(0.))
            );
        }
    }
    #[gpui::test]
    fn secret_fields_preserve_unicode_value_and_mask_drawing_ime_and_clipboard(
        cx: &mut TestAppContext,
    ) {
        let window = field(cx, true);
        let secret = "päss🙂e\u{301}";
        replace(window, secret, cx);
        window
            .update(cx, |input, window, cx| {
                input.set_secret(true);
                input.select_all(&SelectAll, window, cx);
                assert_eq!(input.content.as_ref(), secret);
                let (display, _) = input.display_text();
                assert!(!display.is_empty());
                assert!(display.chars().all(|ch| ch == '*'));
                let units = secret.encode_utf16().count();
                let mut actual = None;
                assert_eq!(
                    input.text_for_range(0..units, &mut actual, window, cx),
                    Some("*".repeat(units))
                );
                assert_eq!(actual, Some(0..units));
                cx.write_to_clipboard(ClipboardItem::new_string("existing clipboard".into()));
                input.copy(&Copy, window, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some("existing clipboard")
                );
                input.set_secret(false);
                assert_eq!(input.display_text().0.as_ref(), secret);
                input.copy(&Copy, window, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().as_deref(),
                    Some(secret)
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn ime_replacement_and_grapheme_deletion(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(Input::new))
                .unwrap()
        });
        window
            .update(cx, |input, w, cx| {
                input.replace_text_in_range(None, "a🙂", w, cx);
                input.replace_and_mark_text_in_range(None, "に", Some(1..1), w, cx);
                assert!(input.is_composing());
                assert_eq!(input.content.as_ref(), "a🙂に");
                assert!(
                    input.take(cx).is_none(),
                    "Enter during composition must not submit"
                );
                input.replace_and_mark_text_in_range(None, "日本", Some(2..2), w, cx);
                assert_eq!(input.content.as_ref(), "a🙂日本");
                assert_eq!(input.selected_text_range(false, w, cx).unwrap().range, 5..5);
                input.replace_text_in_range(None, "日本語", w, cx);
                assert!(!input.is_composing());
                assert_eq!(input.content.as_ref(), "a🙂日本語");
                input.select_all(&SelectAll, w, cx);
                input.replace_text_in_range(None, "e\u{301}🙂", w, cx);
                input.backspace(&Backspace, w, cx);
                assert_eq!(input.content.as_ref(), "e\u{301}");
                input.backspace(&Backspace, w, cx);
                assert!(input.content.is_empty());
                input.replace_text_in_range(None, "line one\nline two", w, cx);
                assert_eq!(input.take(cx).as_deref(), Some("line one\nline two"));
            })
            .unwrap();
    }
}
