//! Port of `FindPanel` and the find keymap in
//! src/features/files/editor/editorSearch.ts.
//!
//! Keys: Cmd-F (gpui-base's `Search`, captured by the editor) opens the bar,
//! Cmd-Alt-F opens it with the replace row, F3 and Cmd-G go to the next
//! match, Shift with either goes back, Enter and Shift-Enter step from the
//! find field, Enter in the replace field replaces, Alt-C, Alt-W, and Alt-R
//! toggle the options, and Escape closes the bar.

use std::{ops::Range, sync::Arc};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, FontWeight, Hsla, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Window, actions, div, prelude::FluentBuilder as _, px,
};
use gpui_base::input::{Input, InputEditorStyle, InputEvent, InputState};

use crate::{
    code_editor::CodeEditor,
    icons::{IconKind, icon},
    search::{CountLabel, SearchQuery, count_label},
    theme::EditorTheme,
};

pub(crate) const FIND_CONTEXT: &str = "CodeEditorFind";

actions!(
    code_editor_find,
    [
        /// Alt-C.
        ToggleMatchCase,
        /// Alt-W.
        ToggleWholeWord,
        /// Alt-R.
        ToggleRegex,
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-c", ToggleMatchCase, Some(FIND_CONTEXT)),
        KeyBinding::new("alt-w", ToggleWholeWord, Some(FIND_CONTEXT)),
        KeyBinding::new("alt-r", ToggleRegex, Some(FIND_CONTEXT)),
    ]);
}

#[derive(Debug, Clone, Copy)]
enum Flag {
    CaseSensitive,
    WholeWord,
    Regexp,
}

pub(crate) struct FindBar {
    pub open: bool,
    pub replace_visible: bool,
    pub query: SearchQuery,
    pub matches: Arc<Vec<Range<usize>>>,
    pub capped: bool,
    query_input: Entity<InputState>,
    replace_input: Entity<InputState>,
}

fn input_style(theme: &EditorTheme) -> InputEditorStyle {
    InputEditorStyle {
        foreground: theme.foreground,
        muted_foreground: theme.content(0.45),
        background: gpui::transparent_black(),
        border: theme.border,
        selection: theme.selection,
        caret: theme.caret,
        ..Default::default()
    }
}

impl FindBar {
    pub fn new(theme: &EditorTheme, window: &mut Window, cx: &mut Context<CodeEditor>) -> Self {
        let make =
            |placeholder: &'static str, window: &mut Window, cx: &mut Context<CodeEditor>| {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
                input.update(cx, |state, _| state.set_editor_style(input_style(theme)));
                input
            };
        Self {
            open: false,
            replace_visible: false,
            query: SearchQuery::default(),
            matches: Arc::new(Vec::new()),
            capped: false,
            query_input: make("Find", window, cx),
            replace_input: make("Replace", window, cx),
        }
    }

    pub fn subscribe(
        &self,
        window: &mut Window,
        cx: &mut Context<CodeEditor>,
    ) -> Vec<Subscription> {
        vec![
            cx.subscribe_in(
                &self.query_input,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::Change => this.commit_find(true, window, cx),
                    InputEvent::PressEnter { shift, .. } => {
                        if *shift {
                            this.find_previous(window, cx);
                        } else {
                            this.find_next(window, cx);
                        }
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &self.replace_input,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::Change => this.commit_find(false, window, cx),
                    InputEvent::PressEnter { .. } => this.replace_next(window, cx),
                    _ => {}
                },
            ),
        ]
    }

    pub fn set_theme(&self, theme: &EditorTheme, cx: &mut App) {
        for input in [&self.query_input, &self.replace_input] {
            input.update(cx, |state, cx| {
                state.set_editor_style(input_style(theme));
                cx.notify();
            });
        }
    }
}

impl CodeEditor {
    /// `openSearchPanel`, and `openReplacePanel` when `replace` is set.
    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        // `defaultQuery`: a one-line selection becomes the search text.
        let selected = {
            let state = self.state.read(cx);
            let range = state.selected_range();
            (!range.is_empty())
                .then(|| state.text().slice(range).to_string())
                .filter(|text| !text.contains('\n'))
        };
        if let Some(selected) = selected {
            self.find.query_input.update(cx, |input, cx| {
                input.set_value(selected.clone(), window, cx);
            });
            self.find.query.search = selected;
        }
        self.find.open = true;
        if replace {
            self.find.replace_visible = true;
            let replace_input = self.find.replace_input.clone();
            replace_input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        } else {
            let query_input = self.find.query_input.clone();
            query_input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        self.refresh_matches(cx);
        self.reveal_match(window, cx);
        cx.notify();
    }

    /// Open the find bar with `query` filled in, as if typed.
    pub fn set_find_query(
        &mut self,
        query: SearchQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (search, replace) = (query.search.clone(), query.replace.clone());
        self.find
            .query_input
            .update(cx, |input, cx| input.set_value(search, window, cx));
        self.find
            .replace_input
            .update(cx, |input, cx| input.set_value(replace, window, cx));
        self.find.replace_visible |= !query.replace.is_empty();
        self.find.query = query;
        self.find.open = true;
        let query_input = self.find.query_input.clone();
        query_input.update(cx, |input, cx| input.focus(window, cx));
        self.refresh_matches(cx);
        self.reveal_match(window, cx);
        cx.notify();
    }

    /// `closeSearchPanel`.
    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        self.refresh_matches(cx);
        self.focus(window, cx);
        cx.notify();
    }

    pub fn is_find_open(&self) -> bool {
        self.find.open
    }

    /// The query typed in the find bar.
    pub fn find_query(&self) -> &SearchQuery {
        self.search_query()
    }

    /// `setReplaceVisible`.
    fn set_replace_visible(
        &mut self,
        visible: bool,
        focus_replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find.replace_visible = visible;
        if visible && focus_replace {
            let replace_input = self.find.replace_input.clone();
            replace_input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// `commit`: read both fields into the query.
    fn commit_find(&mut self, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        let search = self.find.query_input.read(cx).value().to_string();
        let replace = self.find.replace_input.read(cx).value().to_string();
        let query = SearchQuery {
            search,
            replace,
            ..self.find.query.clone()
        };
        if query != self.find.query {
            self.find.query = query;
            self.refresh_matches(cx);
        }
        if reveal {
            self.reveal_match(window, cx);
        }
    }

    /// `toggle`.
    fn toggle_flag(&mut self, flag: Flag, window: &mut Window, cx: &mut Context<Self>) {
        let query = &mut self.find.query;
        match flag {
            Flag::CaseSensitive => query.case_sensitive = !query.case_sensitive,
            Flag::WholeWord => query.whole_word = !query.whole_word,
            Flag::Regexp => query.regexp = !query.regexp,
        }
        self.refresh_matches(cx);
        self.reveal_match(window, cx);
    }

    /// `reveal`: select the first match from the selection on, unless the
    /// text itself has focus.
    fn reveal_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor_focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        if editor_focused {
            return;
        }
        let Some(compiled) = self.find.query.compile() else {
            return;
        };
        let selection = self.state.read(cx).selected_range();
        let text = self.text(cx);
        if let Some(found) = compiled.reveal_match(&text, selection) {
            self.select_and_reveal(found, true, cx);
            self.refresh_search_decorations(cx);
        }
    }

    /// `findNext`.
    pub fn find_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(compiled) = self.find.query.compile() else {
            self.open_find(false, window, cx);
            return;
        };
        let to = self.state.read(cx).selected_range().end;
        let text = self.text(cx);
        if let Some(next) = compiled.next_match(&text, to, to) {
            self.select_and_reveal(next.range, true, cx);
            self.refresh_search_decorations(cx);
        }
    }

    /// `findPrevious`.
    pub fn find_previous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(compiled) = self.find.query.compile() else {
            self.open_find(false, window, cx);
            return;
        };
        let from = self.state.read(cx).selected_range().start;
        let text = self.text(cx);
        if let Some(previous) = compiled.prev_match(&text, from, from) {
            self.select_and_reveal(previous.range, true, cx);
            self.refresh_search_decorations(cx);
        }
    }

    /// `replaceNext`.
    pub fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }
        let Some(compiled) = self.find.query.compile() else {
            self.open_find(false, window, cx);
            return;
        };
        let selection = self.state.read(cx).selected_range();
        let text = self.text(cx);
        let Some(step) = compiled.replace_next(&text, selection) else {
            return;
        };
        if let Some(change) = &step.change {
            self.state.update(cx, |state, cx| {
                state.set_selected_range(change.from..change.to, cx);
                state.replace(change.insert.clone(), window, cx);
            });
        }
        if let Some(select) = step.select {
            self.select_and_reveal(select, true, cx);
        }
        self.refresh_matches(cx);
    }

    /// `replaceAll`: one undoable edit, cursor and scroll kept.
    pub fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_read_only() {
            return;
        }
        let Some(compiled) = self.find.query.compile() else {
            return;
        };
        let text = self.text(cx);
        let (replaced, count) = compiled.replace_all(&text);
        if count == 0 || replaced == text {
            return;
        }
        self.state.update(cx, |state, cx| {
            let selection = state.selected_range();
            let scroll = state.scroll_offset();
            state.replace_all(replaced.clone(), window, cx);
            let start = selection.start.min(replaced.len());
            state.set_selected_range(start..start, cx);
            state.set_scroll_offset(scroll, cx);
        });
        self.refresh_matches(cx);
    }

    pub(crate) fn count_label(&self, cx: &App) -> CountLabel {
        let selection = self.state.read(cx).selected_range();
        count_label(
            &self.find.query,
            &self.find.matches,
            self.find.capped,
            &selection,
        )
    }

    pub(crate) fn render_find_bar(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme.clone();
        let label = self.count_label(cx);
        let query = self.find.query.clone();
        let replace_visible = self.find.replace_visible;
        let error_border = gpui::Hsla {
            a: 0.55,
            ..theme.danger
        };

        let field = |input: &Entity<InputState>, error: bool| {
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h(px(26.))
                .items_center()
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(if error { error_border } else { theme.border })
                .bg(theme.content(0.06))
                .font_family(theme.mono_font.clone())
                .text_size(px(12.))
                .line_height(px(24.))
                .child(div().flex_1().min_w_0().child(Input::new(input)))
        };
        let toggle = |id: &'static str, text: &'static str, active: bool| {
            div()
                .id(id)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .font_family(theme.mono_font.clone())
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if active {
                    theme.foreground
                } else {
                    theme.content(0.62)
                })
                .when(active, |this| {
                    this.bg(Hsla {
                        a: 0.28,
                        ..theme.accent
                    })
                })
                .when(!active, |this| this.hover(|this| this.bg(theme.hover)))
                .child(text)
        };
        let icon_button = |id: &'static str, kind: IconKind| {
            div()
                .id(id)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .hover(|this| this.bg(theme.hover))
                .child(icon(kind, px(14.), theme.content(0.62)))
        };
        let text_button = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .h(px(24.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .text_size(px(11.))
                .text_color(theme.content(0.62))
                .hover(|this| this.bg(theme.hover).text_color(theme.foreground))
                .child(text)
        };

        let count_color = if label.is_error() {
            theme.danger
        } else {
            theme.content(0.45)
        };
        let count: SharedString = label.text().to_owned().into();

        div()
            .key_context(FIND_CONTEXT)
            .on_action(cx.listener(|this, _: &ToggleMatchCase, window, cx| {
                this.toggle_flag(Flag::CaseSensitive, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleWholeWord, window, cx| {
                this.toggle_flag(Flag::WholeWord, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRegex, window, cx| {
                this.toggle_flag(Flag::Regexp, window, cx)
            }))
            .flex()
            .flex_none()
            .w_full()
            .gap(px(4.))
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(theme.stroke)
            .font_family(theme.ui_font.clone())
            .text_color(theme.foreground)
            .child(
                div()
                    .id("find-expand")
                    .w(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .hover(|this| this.bg(theme.hover))
                    .child(icon(
                        if replace_visible {
                            IconKind::ChevronDown
                        } else {
                            IconKind::ChevronRight
                        },
                        px(14.),
                        theme.content(0.62),
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_replace_visible(!replace_visible, replace_visible, window, cx)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                field(&self.find.query_input, label.is_error()).child(
                                    div()
                                        .flex_none()
                                        .w(px(84.))
                                        .pl(px(8.))
                                        .flex()
                                        .justify_end()
                                        .overflow_hidden()
                                        .text_size(px(11.))
                                        .text_color(count_color)
                                        .child(count),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(
                                        toggle("find-case", "Aa", query.case_sensitive).on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.toggle_flag(Flag::CaseSensitive, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(toggle("find-word", "ab", query.whole_word).on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.toggle_flag(Flag::WholeWord, window, cx)
                                        }),
                                    ))
                                    .child(toggle("find-regex", ".*", query.regexp).on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.toggle_flag(Flag::Regexp, window, cx)
                                        }),
                                    )),
                            ),
                    )
                    .when(replace_visible, |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(field(&self.find.replace_input, false))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(2.))
                                        .child(text_button("find-replace", "Replace").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.replace_next(window, cx)
                                            }),
                                        ))
                                        .child(text_button("find-replace-all", "All").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.replace_all(window, cx)
                                            }),
                                        )),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(2.))
                    .child(icon_button("find-previous", IconKind::ChevronUp).on_click(
                        cx.listener(|this, _, window, cx| this.find_previous(window, cx)),
                    ))
                    .child(
                        icon_button("find-next", IconKind::ChevronDown).on_click(
                            cx.listener(|this, _, window, cx| this.find_next(window, cx)),
                        ),
                    )
                    .child(
                        icon_button("find-close", IconKind::Close).on_click(
                            cx.listener(|this, _, window, cx| this.close_find(window, cx)),
                        ),
                    ),
            )
    }
}
