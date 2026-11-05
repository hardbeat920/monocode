//! FileEditor adapter for the pure `editor_search` engine: snapshot ownership,
//! background scheduling, result validation, edits and the Find/Replace panel.
use super::*;
use crate::editor_search::{self, SearchOptions, SearchResult};
use std::time::Instant;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct SearchSnapshot {
    generation: u64,
    source: SharedString,
    query: SharedString,
    replacement: SharedString,
    options: SearchOptions,
}

impl SearchSnapshot {
    fn current(&self, editor: &FileEditor, cx: &App) -> bool {
        editor.search_open
            && editor.search_generation == self.generation
            && editor.input.read(cx).content == self.source
            && editor.search.read(cx).content == self.query
            && editor.replacement.read(cx).content == self.replacement
            && editor.search_options() == self.options
    }

    fn run(&self) -> SearchResult {
        editor_search::search(&self.source, &self.query, &self.replacement, self.options)
    }
}

fn latency_started() -> Option<Instant> {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ENABLED
        .get_or_init(|| std::env::var_os("MONOCODE_SEARCH_LATENCY").is_some())
        .then(Instant::now)
}

impl FileEditor {
    fn search_options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.match_case,
            whole_word: self.whole_word,
            regex: self.regex,
        }
    }

    fn capture_search(&self, cx: &App) -> SearchSnapshot {
        SearchSnapshot {
            generation: self.search_generation,
            source: self.input.read(cx).content.clone(),
            query: self.search.read(cx).content.clone(),
            replacement: self.replacement.read(cx).content.clone(),
            options: self.search_options(),
        }
    }

    pub(super) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.is_none() {
            return;
        }
        self.preview = false;
        let was_open = self.search_open;
        self.search_open = true;
        let mut seeded = false;
        if !was_open {
            let input = self.input.read(cx);
            let selected = input.content[input.selection_range()].to_owned();
            if !selected.is_empty() && !selected.contains('\n') {
                self.search.update(cx, |i, cx| i.set_text(selected, cx));
                seeded = true;
            }
        }
        if !was_open && !seeded && !self.search.read(cx).content.is_empty() {
            self.find_matches(cx);
        }
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn find_matches(&mut self, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        let started = latency_started();
        let previous = self.matches.get(self.match_index).map_or(0, |m| m.from);
        self.search_generation = self.search_generation.wrapping_add(1);
        let snapshot = self.capture_search(cx);
        self.search_snapshot = None;
        self.matches.clear();
        self.match_index = 0;
        self.search_invalid = false;
        self.search_timed_out = false;
        self.input
            .update(cx, |i, cx| i.set_search_matches(Vec::new(), cx));
        self.search_task = None;
        if snapshot.query.is_empty() {
            cx.notify();
            return;
        }
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let (snapshot, result) = cx.background_executor().spawn(async move {
                let result = snapshot.run();
                (snapshot, result)
            }).await;
            let _ = this.update(cx, |this, cx| {
                let applied = this.apply_search_result(snapshot, result, previous, cx);
                if let Some(started) = started {
                    eprintln!("search-latency: {:.2}ms applied={applied} matches={} invalid={} timed_out={}",
                        started.elapsed().as_secs_f64() * 1000.0,
                        this.matches.len(), this.search_invalid, this.search_timed_out);
                }
            });
        }));
        cx.notify();
    }

    fn apply_search_result(
        &mut self,
        snapshot: SearchSnapshot,
        result: SearchResult,
        previous: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if !snapshot.current(self, cx) {
            return false;
        }
        self.search_invalid = result.invalid;
        self.search_timed_out = result.timed_out;
        self.matches = if result.invalid || result.timed_out {
            Vec::new()
        } else {
            result.matches
        };
        self.match_index = self
            .matches
            .iter()
            .position(|m| m.from >= previous)
            .unwrap_or(0);
        self.search_snapshot = Some(snapshot);
        let ranges = self.matches.iter().map(|m| m.from..m.to).collect();
        self.input
            .update(cx, |i, cx| i.set_search_matches(ranges, cx));
        if let Some(m) = self.matches.get(self.match_index) {
            self.input
                .update(cx, |i, cx| i.select_range(m.from..m.to, cx));
        }
        cx.notify();
        true
    }

    fn search_results_current(&self, cx: &App) -> bool {
        !self.search_invalid
            && !self.search_timed_out
            && self
                .search_snapshot
                .as_ref()
                .is_some_and(|s| s.current(self, cx))
    }

    pub(super) fn step_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if self.matches.is_empty() || !self.search_results_current(cx) {
            return;
        }
        self.match_index = (self.match_index as isize + if backwards { -1 } else { 1 })
            .rem_euclid(self.matches.len() as isize) as usize;
        let m = &self.matches[self.match_index];
        self.input
            .update(cx, |i, cx| i.select_range(m.from..m.to, cx));
        cx.notify();
    }

    pub(super) fn replace_matches(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing()
            || self.matches.is_empty()
            || !self.search_results_current(cx)
        {
            return;
        }
        let content = &self.input.read(cx).content;
        let (text, cursor) = if all {
            match editor_search::replace_all(content, &self.matches) {
                Ok(text) => (text, 0),
                Err(_) => return,
            }
        } else {
            let m = &self.matches[self.match_index];
            let mut text = content.to_string();
            text.replace_range(m.from..m.to, &m.insert);
            (text, m.from + m.insert.len())
        };
        // Invalidate synchronously, before the deferred Changed subscription:
        // a second Replace action must never reuse offsets from the old buffer.
        self.search_snapshot = None;
        self.input.update(cx, |i, cx| {
            i.replace_bytes(0..i.content.len(), &text, cursor, cx)
        });
    }

    pub(super) fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            cx.propagate();
            return;
        }
        self.search_open = false;
        self.search_task = None;
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_snapshot = None;
        self.matches.clear();
        self.match_index = 0;
        self.search_invalid = false;
        self.search_timed_out = false;
        self.input
            .update(cx, |i, cx| i.set_search_matches(Vec::new(), cx));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn search_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = if self.search_invalid {
            "Invalid regex".into()
        } else if self.search_timed_out {
            "Timed out".into()
        } else if self.search.read(cx).content.is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "No results".into()
        } else {
            format!(
                "{} of {}{}",
                self.match_index + 1,
                self.matches.len().min(999),
                if self.matches.len() > 999 { "+" } else { "" }
            )
        };
        let bad = self.search_invalid
            || self.search_timed_out
            || (!self.search.read(cx).content.is_empty() && self.matches.is_empty());
        let query = div()
            .h(px(26.))
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(if bad {
                rgb(0xf87171).opacity(0.55).into()
            } else {
                content(0.1)
            })
            .bg(content(0.06))
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .child(
                div()
                    .min_w(px(85.))
                    .text_right()
                    .text_size(px(11.))
                    .text_color(if bad {
                        rgb(0xf87171).into()
                    } else {
                        content(0.45)
                    })
                    .child(count),
            );
        let toggle = |id, label: &'static str, title: &'static str, on| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(title)
                .aria_toggled(if on {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(content(if on { 1. } else { 0.6 }))
                .when(on, |d| d.bg(accent_color().opacity(0.25)))
                .hover(|s| s.bg(content(0.1)))
                .child(label)
        };
        div()
            .flex_shrink_0()
            .flex()
            .items_start()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(stroke())
            .font_family(crate::fonts::mono())
            .child(
                icon_action(
                    "toggle-replace",
                    "Toggle Replace",
                    if self.replace_open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    true,
                )
                .mt(px(3.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.replace_open = !this.replace_open;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(query)
                            .child(
                                toggle("match-case", "Aa", "Match Case", self.match_case).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.match_case = !this.match_case;
                                        this.find_matches(cx);
                                    }),
                                ),
                            )
                            .child(
                                toggle("whole-word", "ab", "Match Whole Word", self.whole_word)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.whole_word = !this.whole_word;
                                        this.find_matches(cx);
                                    })),
                            )
                            .child(
                                toggle("use-regex", ".*", "Use Regular Expression", self.regex)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.regex = !this.regex;
                                        this.find_matches(cx);
                                    })),
                            ),
                    )
                    .when(self.replace_open, |d| {
                        d.child(
                            div()
                                .h(px(26.))
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .h(px(26.))
                                        .flex_1()
                                        .min_w_0()
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(content(0.1))
                                        .bg(content(0.06))
                                        .child(self.replacement.clone()),
                                )
                                .child(
                                    div()
                                        .id("replace-next")
                                        .role(Role::Button)
                                        .aria_label("Replace")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .text_size(px(11.))
                                        .text_color(content(0.6))
                                        .hover(|s| s.bg(content(0.1)))
                                        .child("Replace")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.replace_matches(false, cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .id("replace-all")
                                        .role(Role::Button)
                                        .aria_label("Replace All")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .text_size(px(11.))
                                        .text_color(content(0.6))
                                        .hover(|s| s.bg(content(0.1)))
                                        .child("All")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.replace_matches(true, cx)
                                        })),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        icon_action(
                            "find-prev",
                            "Previous Match",
                            "chevron-up",
                            !self.matches.is_empty(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(true, cx))),
                    )
                    .child(
                        icon_action(
                            "find-next",
                            "Next Match",
                            "chevron-down",
                            !self.matches.is_empty(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(false, cx))),
                    )
                    .child(
                        icon_action("close-find", "Close", "x", true)
                            .size(px(24.))
                            .on_click(cx.listener(|this, _, w, cx| this.dismiss_search(w, cx))),
                    ),
            )
            .into_any_element()
    }
}
