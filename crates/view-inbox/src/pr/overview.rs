//! Port of src/features/inbox/ui/InboxPrOverview.tsx: the linked side
//! panel's Summary pieces. `descriptionExcerpt` turns a markdown body into a
//! short plain-text lead, `InboxDescriptionSummary` shows that lead until the
//! reader asks for the full body, and `InboxPrChangesGlance` lists the first
//! changed files of a pull request.

use std::rc::Rc;
use std::sync::LazyLock;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use monocode_core::js;
use monocode_ui::styled::format_integer;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};
use regex::Regex;

use crate::data::{Action, PrDiff};
use crate::pr::comments::MarkdownCache;
use crate::style::loader;

const EXCERPT_LINES: usize = 4;
const EXCERPT_CHARS: usize = 360;
const GLANCE_FILES: usize = 6;

/// `DescriptionExcerpt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionExcerpt {
    pub text: String,
    pub images: usize,
    /// True when the excerpt drops anything the full render would show.
    pub truncated: bool,
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

static MARKDOWN_IMAGE: LazyLock<Regex> = LazyLock::new(|| regex(r"!\[[^\]]*\]\([^)]*\)"));
static HTML_IMAGE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)<img(?-u:\b)"));
static HTML_VIDEO: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)<video(?-u:\b)"));
static BLOCK_LINE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?m)^\s*(```|~~~|\|.*\|)"));
static DETAILS_TAG: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)<details(?-u:\b)"));
static HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| regex(r"(?s)<!--.*?-->"));
static DETAILS_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?is)<details(?-u:\b).*?(?:</details>|\z)"));
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| regex(r"<[^>]+>"));
static LINK: LazyLock<Regex> = LazyLock::new(|| regex(r"\[([^\]]*)\]\([^)]*\)"));
static LINE_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^\s{0,3}(#{1,6}\s+|>\s?|[-*+]\s+\[[ xX]\]\s+|[-*+]\s+|[0-9]+[.)]\s+)")
});
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| regex(r"(\*\*|__|~~|`)"));
static RULE: LazyLock<Regex> = LazyLock::new(|| regex(r"^\s*([-*_]\s*){3,}$"));
static TABLE_ROW: LazyLock<Regex> = LazyLock::new(|| regex(r"^\s*\|.*\|\s*$"));

/// `.replace(/(```|~~~)[\s\S]*?(\1|$)/g, "")`. The `regex` crate has no
/// back references, so this walks the fences by hand: each fence runs to the
/// next fence of the same kind, or to the end of the body.
fn strip_fences(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    loop {
        let open = ["```", "~~~"]
            .into_iter()
            .filter_map(|fence| rest.find(fence).map(|at| (at, fence)))
            .min_by_key(|(at, _)| *at);
        let Some((at, fence)) = open else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        let after = &rest[at + fence.len()..];
        match after.find(fence) {
            Some(close) => rest = &after[close + fence.len()..],
            None => return out,
        }
    }
}

/// `descriptionExcerpt`: the plain-text lead of a markdown body. Showing
/// this instead of the full markdown keeps images, embeds, and code blocks
/// unloaded until the reader asks for them.
pub fn description_excerpt(body: &str) -> DescriptionExcerpt {
    let images = MARKDOWN_IMAGE.find_iter(body).count()
        + HTML_IMAGE.find_iter(body).count()
        + HTML_VIDEO.find_iter(body).count();
    let has_blocks = BLOCK_LINE.is_match(body) || DETAILS_TAG.is_match(body);
    let cleaned = HTML_COMMENT.replace_all(body, "");
    let cleaned = strip_fences(&cleaned);
    let cleaned = DETAILS_BLOCK.replace_all(&cleaned, "");
    let cleaned = MARKDOWN_IMAGE.replace_all(&cleaned, "");
    let cleaned = HTML_TAG.replace_all(&cleaned, "");
    let cleaned = LINK.replace_all(&cleaned, "${1}");
    let lines: Vec<String> = cleaned
        .split('\n')
        .map(|line| {
            let line = LINE_MARKER.replace(line, "");
            let line = EMPHASIS.replace_all(&line, "");
            let line = RULE.replace(&line, "");
            let line = TABLE_ROW.replace(&line, "");
            js::trim(&line).to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();
    let mut text = lines
        .iter()
        .take(EXCERPT_LINES)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let clipped = js::len(&text) > EXCERPT_CHARS;
    if clipped {
        text = format!("{}…", js::trim_end(js::slice_prefix(&text, EXCERPT_CHARS)));
    }
    DescriptionExcerpt {
        text,
        images,
        truncated: clipped || lines.len() > EXCERPT_LINES || images > 0 || has_blocks,
    }
}

/// `InboxDescriptionSummary`: no description, the whole body when the
/// excerpt drops nothing, or the excerpt in a card that expands to the full
/// body. The owner keeps `expanded` and flips it in `on_toggle`.
pub fn inbox_description_summary(
    body: &str,
    expanded: bool,
    on_toggle: Action,
    markdown: &mut MarkdownCache,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx).clone();
    if js::trim(body).is_empty() {
        return div()
            .text_px(theme.text.body)
            .text_color(theme.content(0.45))
            .child("No description")
            .into_any_element();
    }
    let excerpt = description_excerpt(body);
    if !excerpt.truncated {
        let view = markdown.view("body", body, false, cx);
        return div().min_w_0().child(view).into_any_element();
    }
    let lead: AnyElement = if expanded {
        let view = markdown.view("body", body, false, cx);
        div().min_w_0().child(view).into_any_element()
    } else if !excerpt.text.is_empty() {
        div()
            .line_clamp(EXCERPT_LINES)
            .text_px(theme.text.body)
            .leading(theme.leading.relaxed)
            .text_color(theme.content(0.75))
            .child(excerpt.text.clone())
            .into_any_element()
    } else {
        div()
            .text_px(theme.text.body)
            .text_color(theme.content(0.45))
            .child("Description is media only")
            .into_any_element()
    };
    let hover = theme.colors.content;
    let toggle = div()
        .id("description-toggle")
        .flex()
        .flex_none()
        .self_start()
        .items_center()
        .gap(u(4.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .hover(move |s| s.text_color(hover))
        .on_click(move |_, window, cx| on_toggle(window, cx))
        .child(
            icon(if expanded {
                IconName::ChevronUp
            } else {
                IconName::ChevronDown
            })
            .size(u(14.)),
        )
        .child(if expanded {
            "Show less"
        } else {
            "Show full description"
        })
        .when(!expanded && excerpt.images > 0, |toggle| {
            toggle.child(div().text_color(theme.content(0.35)).child(format!(
                "· {} {}",
                excerpt.images,
                if excerpt.images == 1 {
                    "image"
                } else {
                    "images"
                }
            )))
        });
    div()
        .flex()
        .flex_col()
        .gap(u(8.))
        .rounded(u(theme.radius.lg))
        .border_1()
        .border_color(theme.colors.stroke)
        .bg(theme.content(0.02))
        .px(u(14.))
        .py(u(12.))
        .child(lead)
        .child(toggle)
        .into_any_element()
}

/// Opens the Code tab, focused on a file or on the whole diff.
pub type OpenFile = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

/// The width of a file's churn bar and of its green part, in CSS px.
pub fn churn_bar(additions: i64, deletions: i64, max_churn: i64) -> (f32, f32) {
    let churn = additions + deletions;
    let width = js::round(churn as f64 / max_churn.max(1) as f64 * 48.).max(8.) as f32;
    let added = if churn > 0 {
        additions as f32 / churn as f32 * width
    } else {
        0.
    };
    (width, added)
}

/// `InboxPrChangesGlance`: "Changed files" with totals, and the first six
/// files with their churn. A click on a file opens the Code tab on it.
pub fn inbox_pr_changes_glance(
    diff: Option<&PrDiff>,
    loading: bool,
    error: Option<&str>,
    on_open_file: OpenFile,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx).clone();
    let files = diff.map(|diff| diff.files.as_slice()).unwrap_or_default();
    let max_churn = files
        .iter()
        .map(|file| file.additions + file.deletions)
        .max()
        .unwrap_or(0)
        .max(1);
    let shown = &files[..files.len().min(GLANCE_FILES)];
    let hover = theme.colors.content;

    let mut header = div()
        .flex()
        .items_center()
        .gap(u(8.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.50))
        .child(div().text_color(theme.content(0.70)).child("Changed files"));
    if let Some(diff) = diff {
        header = header
            .child(div().tabular().child(format!(
                "{} {}",
                files.len(),
                if files.len() == 1 { "file" } else { "files" }
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .text_px(theme.text.caption)
                    .semibold()
                    .tabular()
                    .child(
                        div()
                            .text_color(theme.colors.success)
                            .child(format!("+{}", format_integer(diff.additions))),
                    )
                    .child(
                        div()
                            .text_color(theme.colors.danger)
                            .child(format!("-{}", format_integer(diff.deletions))),
                    ),
            );
    }
    if loading {
        header = header.child(loader("glance-loading", 12., theme.content(0.35)));
    }
    if !files.is_empty() {
        let open = on_open_file.clone();
        header = header.child(
            div()
                .id("glance-view-all")
                .ml_auto()
                .flex()
                .items_center()
                .gap(u(2.))
                .hover(move |s| s.text_color(hover))
                .on_click(move |_, window, cx| open(None, window, cx))
                .child(if files.len() > shown.len() {
                    format!("View all {}", files.len())
                } else {
                    "View diff".to_string()
                })
                .child(icon(IconName::ChevronRight).size(u(14.))),
        );
    }

    let mut section = div().flex().flex_col().gap(u(8.)).child(header);
    let note = |text: String| {
        div()
            .text_px(theme.text.label)
            .text_color(theme.content(0.45))
            .child(text)
    };
    if let Some(error) = error.filter(|_| diff.is_none()) {
        section = section.child(note(error.to_string()));
    } else if diff.is_some() && files.is_empty() {
        section = section.child(note("No file changes".into()));
    } else if !shown.is_empty() {
        let mut list = div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.colors.stroke);
        let row_hover = theme.content(0.05);
        for (index, file) in shown.iter().enumerate() {
            let (dir, name) = match file.path.rfind('/') {
                Some(slash) => file.path.split_at(slash + 1),
                None => ("", file.path.as_str()),
            };
            let (width, added) = churn_bar(file.additions, file.deletions, max_churn);
            let path = file.path.clone();
            let open = on_open_file.clone();
            let row = div()
                .id(ElementId::Name(format!("glance-file:{}", file.path).into()))
                .flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap(u(8.))
                .bg(theme.content(0.02))
                .px(u(12.))
                .py(u(6.))
                .hover(move |s| s.bg(row_hover))
                .tooltip(tooltip(file.path.clone()))
                .on_click(move |_, window, cx| open(Some(path.clone()), window, cx))
                .child(file_type_icon(name.to_string()).size(16.))
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .flex_1()
                        .font_family(theme.fonts.mono.clone())
                        .text_px(theme.text.label)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.content(0.40))
                                .child(dir.to_string()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.content(0.85))
                                .child(name.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .h(u(6.))
                        .w(u(width))
                        .overflow_hidden()
                        .rounded_full()
                        .child(
                            div()
                                .flex_none()
                                .w(u(added))
                                .h_full()
                                .bg(monocode_ui::color::with_alpha(theme.colors.success, 0.8)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .h_full()
                                .bg(monocode_ui::color::with_alpha(theme.colors.danger, 0.8)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .w(u(80.))
                        .items_center()
                        .justify_end()
                        .gap(u(6.))
                        .text_px(theme.text.caption)
                        .semibold()
                        .tabular()
                        .when(file.additions > 0, |stats| {
                            stats.child(
                                div()
                                    .text_color(theme.colors.success)
                                    .child(format!("+{}", format_integer(file.additions))),
                            )
                        })
                        .when(file.deletions > 0, |stats| {
                            stats.child(
                                div()
                                    .text_color(theme.colors.danger)
                                    .child(format!("-{}", format_integer(file.deletions))),
                            )
                        }),
                );
            list = list.child(
                div()
                    .when(index + 1 < shown.len(), |item| {
                        item.border_b_1().border_color(theme.colors.stroke)
                    })
                    .child(row),
            );
        }
        section = section.child(list);
    }
    section.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    // InboxPrOverview.test.ts

    #[test]
    fn keeps_short_plain_descriptions_whole() {
        assert_eq!(
            description_excerpt("Fixes the retry loop."),
            DescriptionExcerpt {
                text: "Fixes the retry loop.".into(),
                images: 0,
                truncated: false,
            }
        );
    }

    #[test]
    fn strips_markdown_and_media_counting_images_for_the_expand_hint() {
        let excerpt = description_excerpt(
            &[
                "<!-- template -->",
                "## Summary",
                "- Adds an **idempotency** key ([ENG-142](https://x.dev))",
                "![shot](https://x.dev/a.png)",
                "<img src=\"https://x.dev/b.png\">",
                "```ts",
                "const hidden = true;",
                "```",
            ]
            .join("\n"),
        );
        assert_eq!(excerpt.text, "Summary\nAdds an idempotency key (ENG-142)");
        assert_eq!(excerpt.images, 2);
        assert!(excerpt.truncated);
    }

    #[test]
    fn collapses_long_bodies_to_the_lead_lines() {
        let excerpt = description_excerpt("a\nb\nc\nd\ne\nf");
        assert_eq!(excerpt.text, "a\nb\nc\nd");
        assert!(excerpt.truncated);
    }

    #[test]
    fn an_unclosed_fence_drops_the_rest_of_the_body() {
        assert_eq!(strip_fences("lead\n~~~\nhidden"), "lead\n");
        assert_eq!(strip_fences("a ``` b ~~~ c ``` d"), "a  d");
    }

    #[test]
    fn clips_a_long_lead_with_an_ellipsis() {
        let excerpt = description_excerpt(&"word ".repeat(100));
        assert_eq!(js::len(&excerpt.text), 360);
        assert!(excerpt.text.ends_with("word…"));
        assert!(excerpt.truncated);
    }

    #[test]
    fn sizes_churn_bars_against_the_busiest_file() {
        assert_eq!(churn_bar(30, 10, 40), (48., 36.));
        assert_eq!(churn_bar(1, 0, 400), (8., 8.));
        assert_eq!(churn_bar(0, 0, 1), (8., 0.));
    }
}
