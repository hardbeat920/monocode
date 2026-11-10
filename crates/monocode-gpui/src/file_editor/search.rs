//! editorSearch.ts's two-row Find/Replace panel, using the same query engine.
use super::*;
impl FileEditor {
    pub(super) fn search_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = if self.search_invalid {
            "Invalid regex".into()
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
