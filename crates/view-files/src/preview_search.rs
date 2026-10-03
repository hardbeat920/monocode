//! Port of src/features/files/ui/FilePreviewSearch.tsx: the find bar over a
//! rendered preview (the Markdown preview of a file).
//!
//! The React component walked the DOM text under it. Here the content
//! reports the text it painted as [`RenderedText`] runs, the shape
//! `MarkdownView::rendered_text` returns: each run's text, window bounds,
//! and layout. The bar matches against those runs, paints the highlights
//! over them, and scrolls the matched run into view.
//!
//! `handleFilePreviewFindKey` and the controller registry picked the visible
//! preview for Mod+F, F3, Mod+G, and Escape. GPUI routes keys to the focused
//! element, so those keys are actions bound in this view's key context.

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Pixels, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, actions, canvas, div,
    fill, point, prelude::FluentBuilder as _, px, size,
};
use gpui_base::input::Input;
use gpui_component::input::{Enter, Escape, InputEvent, InputState};
use monocode_core::Platform;
use monocode_markdown::RenderedText;
use monocode_ui::{IconName, Theme, UiStyled as _, color::with_alpha, icon, u, widgets::tooltip};
use regex::{Regex, RegexBuilder};

/// `MATCH_CAP`.
pub const MATCH_CAP: usize = 999;

const KEY_CONTEXT: &str = "FilePreviewSearch";
const BAR_CONTEXT: &str = "FilePreviewSearchBar";

actions!(
    file_preview_search,
    [
        /// Open the find bar (`Editor: Find`).
        OpenFind,
        /// Go to the next match.
        FindNext,
        /// Go to the previous match.
        FindPrevious,
        /// Close the find bar.
        CloseFind,
        /// Toggle Match Case.
        ToggleCaseSensitive,
        /// Toggle Match Whole Word.
        ToggleWholeWord,
        /// Toggle Use Regular Expression.
        ToggleRegexp,
    ]
);

pub(crate) fn init(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    let bar = Some(BAR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-f", OpenFind, context),
        KeyBinding::new("f3", FindNext, context),
        KeyBinding::new("shift-f3", FindPrevious, context),
        KeyBinding::new("secondary-g", FindNext, context),
        KeyBinding::new("secondary-shift-g", FindPrevious, context),
        KeyBinding::new("escape", CloseFind, context),
        KeyBinding::new("alt-c", ToggleCaseSensitive, bar),
        KeyBinding::new("alt-w", ToggleWholeWord, bar),
        KeyBinding::new("alt-r", ToggleRegexp, bar),
    ]);
}

/// `SearchOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regexp: bool,
}

/// `SearchResult`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResult<T> {
    pub matches: Vec<T>,
    pub capped: bool,
    pub invalid: bool,
}

/// `searchPattern`. JavaScript used the `u` flag and `i` unless case
/// sensitive; Rust's `regex` is Unicode-aware by default.
// TODO(port): patterns follow Rust regex syntax, which has no lookaround or
// backreferences.
fn search_pattern(query: &str, options: SearchOptions) -> Option<Regex> {
    if query.is_empty() {
        return None;
    }
    let source = if options.regexp {
        query.to_string()
    } else {
        regex::escape(query)
    };
    RegexBuilder::new(&source)
        .case_insensitive(!options.case_sensitive)
        .build()
        .ok()
}

/// `isWordCharacter`: `[\p{L}\p{N}_]`.
fn is_word_character(character: Option<char>) -> bool {
    character.is_some_and(|c| c.is_alphabetic() || c.is_numeric() || c == '_')
}

/// `isWholeWord`.
fn is_whole_word(text: &str, from: usize, to: usize) -> bool {
    !is_word_character(text[..from].chars().next_back())
        && !is_word_character(text[to..].chars().next())
}

/// Matches of `pattern` in `text`, appended to `out` with `wrap`. Returns
/// false once the cap is hit.
fn collect_matches<T>(
    text: &str,
    pattern: &Regex,
    options: SearchOptions,
    out: &mut Vec<T>,
    wrap: impl Fn(Range<usize>) -> T,
) -> bool {
    for found in pattern.find_iter(text) {
        if found.is_empty() {
            continue;
        }
        if options.whole_word && !is_whole_word(text, found.start(), found.end()) {
            continue;
        }
        if out.len() >= MATCH_CAP {
            return false;
        }
        out.push(wrap(found.range()));
    }
    true
}

/// `findPreviewTextMatches`: byte ranges of the matches in `text`.
pub fn find_preview_text_matches(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> SearchResult<Range<usize>> {
    let Some(pattern) = search_pattern(query, options) else {
        return SearchResult {
            matches: Vec::new(),
            capped: false,
            invalid: !query.is_empty(),
        };
    };
    let mut matches = Vec::new();
    let capped = !collect_matches(text, &pattern, options, &mut matches, |range| range);
    SearchResult {
        matches,
        capped,
        invalid: false,
    }
}

/// One match: a byte range in run `run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewMatch {
    pub run: usize,
    pub range: Range<usize>,
}

/// `findPreviewRanges` over the runs' texts.
pub fn find_preview_run_matches(
    runs: &[SharedString],
    query: &str,
    options: SearchOptions,
) -> SearchResult<PreviewMatch> {
    let Some(pattern) = search_pattern(query, options) else {
        return SearchResult {
            matches: Vec::new(),
            capped: false,
            invalid: !query.is_empty(),
        };
    };
    let mut matches = Vec::new();
    for (run, text) in runs.iter().enumerate() {
        if !collect_matches(text, &pattern, options, &mut matches, |range| {
            PreviewMatch { run, range }
        }) {
            return SearchResult {
                matches,
                capped: true,
                invalid: false,
            };
        }
    }
    SearchResult {
        matches,
        capped: false,
        invalid: false,
    }
}

/// The text runs the content painted in its last frame.
pub type RunsSource = Rc<dyn Fn(&App) -> Vec<RenderedText>>;

/// The find bar and its content.
pub struct FilePreviewSearch {
    content: AnyView,
    source: RunsSource,
    scroll: Option<gpui::ScrollHandle>,
    active: bool,
    open: bool,
    query: Entity<InputState>,
    options: SearchOptions,
    runs: Vec<SharedString>,
    matches: Vec<PreviewMatch>,
    current: Option<usize>,
    capped: bool,
    invalid: bool,
    signature: Option<String>,
    reveal: bool,
    platform: Platform,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Tells the host the bar closed, so it can focus the preview again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindClosed;

impl EventEmitter<FindClosed> for FilePreviewSearch {}

impl Focusable for FilePreviewSearch {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FilePreviewSearch {
    /// Wrap `content`. `source` reads the runs it painted; `scroll` is the
    /// container that scrolls it, for revealing matches.
    pub fn new(
        content: AnyView,
        source: RunsSource,
        scroll: Option<gpui::ScrollHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let subscription = cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.reveal = true;
                this.refresh(cx);
            }
        });
        Self {
            content,
            source,
            scroll,
            active: true,
            open: false,
            query,
            options: SearchOptions::default(),
            runs: Vec::new(),
            matches: Vec::new(),
            current: None,
            capped: false,
            invalid: false,
            signature: None,
            reveal: false,
            platform: Platform::current(),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }

    pub fn options(&self) -> SearchOptions {
        self.options
    }

    pub fn matches(&self) -> &[PreviewMatch] {
        &self.matches
    }

    /// The highlighted match.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// `active`: the preview is showing. An inactive preview closes the bar.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.open = false;
        }
        self.refresh(cx);
    }

    /// `contentVersion` changed: search the new text.
    pub fn content_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    /// `openSearch`: show the bar and focus the field with its text selected.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.reveal = true;
        self.query.update(cx, |state, cx| {
            state.focus(window, cx);
            let len = state.value().len();
            state.set_selected_range(0..len, cx);
        });
        self.refresh(cx);
    }

    /// `closeSearch`.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        window.focus(&self.focus_handle, cx);
        self.refresh(cx);
        cx.emit(FindClosed);
    }

    /// `step`: move to the next (`1`) or previous (`-1`) match.
    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.matches.len();
        if len == 0 {
            return;
        }
        let from = self.current.unwrap_or(0) as isize;
        let next = (from + delta).rem_euclid(len as isize) as usize;
        self.current = Some(next);
        self.reveal_current(cx);
        cx.notify();
    }

    fn toggle(&mut self, toggle: impl FnOnce(&mut SearchOptions), cx: &mut Context<Self>) {
        self.reveal = true;
        toggle(&mut self.options);
        self.refresh(cx);
    }

    /// The count label: `2 of 5`, `No results`, or `Invalid regex`.
    pub fn count_label(&self, cx: &App) -> String {
        let query = self.query.read(cx).value();
        let total = self.matches.len();
        if self.invalid {
            "Invalid regex".into()
        } else if !query.is_empty() && total == 0 {
            "No results".into()
        } else if total > 0 {
            format!(
                "{} of {}{}",
                self.current.map_or(0, |current| current + 1),
                total,
                if self.capped { "+" } else { "" }
            )
        } else {
            String::new()
        }
    }

    /// Re-run the search over the content's runs (the layout effect).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().to_string();
        let runs: Vec<RenderedText> = (self.source)(cx);
        self.runs = runs.iter().map(run_text).collect();
        if !self.open || !self.active || query.is_empty() {
            self.matches.clear();
            self.current = None;
            self.capped = false;
            self.invalid = false;
            cx.notify();
            return;
        }
        let signature = format!(
            "{query}\0{}\0{}\0{}",
            self.options.case_sensitive, self.options.whole_word, self.options.regexp
        );
        let search = find_preview_run_matches(&self.runs, &query, self.options);
        let next = if search.matches.is_empty() {
            None
        } else if self.signature.as_deref() == Some(signature.as_str()) {
            Some(self.current.unwrap_or(0).min(search.matches.len() - 1))
        } else {
            Some(0)
        };
        self.signature = Some(signature);
        self.matches = search.matches;
        self.capped = search.capped;
        self.invalid = search.invalid;
        self.current = next;
        if self.reveal && next.is_some() {
            self.reveal_current_in(&runs);
        }
        self.reveal = false;
        cx.notify();
    }

    fn reveal_current(&mut self, cx: &mut Context<Self>) {
        let runs = (self.source)(cx);
        self.reveal_current_in(&runs);
    }

    /// `revealRange`: scroll the current match to the middle when it is
    /// not fully visible.
    fn reveal_current_in(&self, runs: &[RenderedText]) {
        let (Some(scroll), Some(found)) = (
            self.scroll.as_ref(),
            self.current.and_then(|current| self.matches.get(current)),
        ) else {
            return;
        };
        let Some(rect) = runs
            .get(found.run)
            .and_then(|run| range_rects(run, &found.range).into_iter().next())
        else {
            return;
        };
        let view = scroll.bounds();
        if rect.top() >= view.top() && rect.bottom() <= view.bottom() {
            return;
        }
        let offset = scroll.offset();
        let delta = rect.top() - view.top() - view.size.height / 2.;
        scroll.set_offset(point(offset.x, offset.y - delta));
    }

    fn on_open(&mut self, _: &OpenFind, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            return cx.propagate();
        }
        self.open(window, cx);
    }

    fn on_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return cx.propagate();
        }
        self.step(1, cx);
    }

    fn on_previous(&mut self, _: &FindPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return cx.propagate();
        }
        self.step(-1, cx);
    }

    fn on_close(&mut self, _: &CloseFind, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return cx.propagate();
        }
        self.close(window, cx);
    }

    fn render_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::of(cx).clone();
        let query_empty = self.query.read(cx).value().is_empty();
        let failing = (!query_empty && self.matches.is_empty()) || self.invalid;
        let alt = self.platform.alt_label();
        let module = self.platform.mod_label();
        let shift = self.platform.shift_label();
        let total = self.matches.len();
        let toggle = |id: &'static str, label: &'static str, title: String, pressed: bool| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .size(u(24.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .font_family(theme.fonts.mono.clone())
                .text_px(theme.text.caption)
                .semibold()
                .map(|button| {
                    if pressed {
                        button
                            .bg(theme.accent(0.25))
                            .text_color(theme.colors.content)
                    } else {
                        button.text_color(theme.content(0.60))
                    }
                })
                .hover(|style| {
                    style
                        .bg(theme.content(0.10))
                        .text_color(theme.colors.content)
                })
                .tooltip(tooltip(title))
                .child(label)
        };
        let button = |id: &'static str, name: IconName, title: String, disabled: bool| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .size(u(24.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .group("preview-find-button")
                .when(disabled, |button| button.opacity(0.35))
                .when(!disabled, |button| {
                    button.hover(|style| style.bg(theme.content(0.10)))
                })
                .tooltip(tooltip(title))
                .child(
                    icon(name)
                        .size(u(14.))
                        .text_color(theme.content(0.60))
                        .when(!disabled, |glyph| {
                            glyph.group_hover("preview-find-button", |style| {
                                style.text_color(theme.colors.content)
                            })
                        }),
                )
        };
        div()
            .id("preview-find-bar")
            .key_context(BAR_CONTEXT)
            .capture_action(cx.listener(|this, action: &Enter, _, cx| {
                cx.stop_propagation();
                this.step(if action.shift { -1 } else { 1 }, cx);
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                cx.stop_propagation();
                this.close(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCaseSensitive, _, cx| {
                this.toggle(
                    |options| options.case_sensitive = !options.case_sensitive,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &ToggleWholeWord, _, cx| {
                this.toggle(|options| options.whole_word = !options.whole_word, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRegexp, _, cx| {
                this.toggle(|options| options.regexp = !options.regexp, cx)
            }))
            .relative()
            .flex()
            .h(u(35.))
            .flex_none()
            .items_center()
            .gap(u(4.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(8.))
            .py(u(4.))
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .child(
                div()
                    .flex()
                    .h(u(26.))
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(if failing {
                        with_alpha(theme.colors.danger, 0.55)
                    } else {
                        theme.content(0.10)
                    })
                    .bg(theme.content(0.06))
                    .px(u(8.))
                    .child(
                        div()
                            .flex()
                            .h(u(24.))
                            .min_w_0()
                            .flex_1()
                            .items_center()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.label)
                            .child(Input::new(&self.query)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(u(80.))
                            .pl(u(8.))
                            .overflow_hidden()
                            .truncate()
                            .flex()
                            .justify_end()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.caption)
                            .tabular()
                            .text_color(if failing {
                                theme.colors.danger
                            } else {
                                theme.content(0.45)
                            })
                            .child(self.count_label(cx)),
                    ),
            )
            .child(
                toggle(
                    "match-case",
                    "Aa",
                    format!("Match Case ({alt}C)"),
                    self.options.case_sensitive,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle(
                        |options| options.case_sensitive = !options.case_sensitive,
                        cx,
                    )
                })),
            )
            .child(
                toggle(
                    "whole-word",
                    "ab",
                    format!("Match Whole Word ({alt}W)"),
                    self.options.whole_word,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle(|options| options.whole_word = !options.whole_word, cx)
                })),
            )
            .child(
                toggle(
                    "regexp",
                    ".*",
                    format!("Use Regular Expression ({alt}R)"),
                    self.options.regexp,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle(|options| options.regexp = !options.regexp, cx)
                })),
            )
            .child(
                button(
                    "previous-match",
                    IconName::ChevronUp,
                    format!("Previous Match ({module}{shift}G)"),
                    total == 0,
                )
                .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))),
            )
            .child(
                button(
                    "next-match",
                    IconName::ChevronDown,
                    format!("Next Match ({module}G)"),
                    total == 0,
                )
                .on_click(cx.listener(|this, _, _, cx| this.step(1, cx))),
            )
            .child(
                button("close-find", IconName::X, "Close (Escape)".into(), false)
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
            .into_any_element()
    }

    /// Paints the match highlights over the content, from the runs it
    /// painted this frame. A change in the painted runs searches again,
    /// like the React MutationObserver.
    fn render_highlights(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let source = self.source.clone();
        let matches = self.matches.clone();
        let current = self.current;
        let expected = self.runs.clone();
        let searching = self.open && self.active && !self.query.read(cx).value().is_empty();
        let theme = Theme::of(cx);
        let match_color = crate::editor_theme(cx).search_match;
        let current_color = theme.accent(0.62);
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, cx| {
                if !searching {
                    return;
                }
                let runs = source(cx);
                let texts: Vec<SharedString> = runs.iter().map(run_text).collect();
                if texts != expected {
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        weak.update(cx, |this, cx| this.refresh(cx)).ok();
                    });
                    return;
                }
                for (index, found) in matches.iter().enumerate() {
                    let Some(run) = runs.get(found.run) else {
                        continue;
                    };
                    let color = if Some(index) == current {
                        current_color
                    } else {
                        match_color
                    };
                    for rect in range_rects(run, &found.range) {
                        window.paint_quad(fill(rect, color));
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
}

/// The text a run laid out. Match offsets index this text, so they line up
/// with the layout even where the reported text differs from it (the
/// padding around inline code).
fn run_text(run: &RenderedText) -> SharedString {
    run.layout.text().into()
}

/// Window rectangles covering `range` of a painted run, one per visual line.
pub fn range_rects(run: &RenderedText, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
    let layout = &run.layout;
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    let line_height = layout.line_height();
    let bounds = run.bounds;
    if (start.y - end.y).abs() < px(0.5) {
        return vec![Bounds::new(start, size(end.x - start.x, line_height))];
    }
    let mut rects = vec![Bounds::new(
        start,
        size(bounds.right() - start.x, line_height),
    )];
    let mut y = start.y + line_height;
    while y + px(0.5) < end.y {
        rects.push(Bounds::new(
            point(bounds.left(), y),
            size(bounds.size.width, line_height),
        ));
        y += line_height;
    }
    rects.push(Bounds::new(
        point(bounds.left(), end.y),
        size(end.x - bounds.left(), line_height),
    ));
    rects
}

impl Render for FilePreviewSearch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = (self.open && self.active).then(|| self.render_bar(cx));
        let highlights = self.render_highlights(cx);
        div()
            .id("file-preview-search")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_next))
            .on_action(cx.listener(Self::on_previous))
            .on_action(cx.listener(Self::on_close))
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .children(bar)
            .child(
                div()
                    .relative()
                    .min_h_0()
                    .flex_1()
                    .child(self.content.clone())
                    .child(highlights),
            )
    }
}

#[cfg(test)]
#[path = "preview_search_tests.rs"]
mod tests;
