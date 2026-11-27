//! SkillsPage's independently scrolling document, without chat alignment or fades.
use crate::{
    markdown::{self, MarkdownRow, Part, Tone},
    markdown_model,
    theme::*,
};
use gpui::{
    AnyElement, Context, ListAlignment, ListState, Render, Role, SharedString, Window, div, list,
    prelude::*, px,
};
use std::{sync::Arc, time::Instant};

pub(super) struct DocumentData {
    metadata: Option<String>,
    rows: Vec<MarkdownRow>,
    source: Vec<String>,
}
impl DocumentData {
    /// Run on the file-reading worker, once per selected file.
    pub fn new(text: String) -> Arc<Self> {
        let (metadata, body) = frontmatter(&text);
        let rows = markdown_model::parse_full(body)
            .blocks
            .iter()
            .flat_map(|block| markdown::rows(&block.block))
            .collect();
        let lines: Vec<_> = text.split_inclusive('\n').collect();
        let source = lines.chunks(24).map(|lines| lines.concat()).collect();
        Arc::new(Self {
            metadata: metadata.map(str::to_owned),
            rows,
            source,
        })
    }
}

pub(super) struct SkillDocument {
    data: Arc<DocumentData>,
    source: bool,
    metadata_open: bool,
    list: ListState,
}
impl SkillDocument {
    pub fn new(data: Arc<DocumentData>, source: bool) -> Self {
        let count = if source {
            data.source.len()
        } else {
            data.rows.len() + usize::from(data.metadata.is_some())
        };
        Self {
            data,
            source,
            metadata_open: false,
            list: ListState::new(count, ListAlignment::Top, px(320.)),
        }
    }
    fn row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let frame = div().px_4().w_full().min_w_0();
        if self.source {
            let text = self.data.source[ix].clone();
            let mut offset = 0;
            let headings: Vec<_> = text
                .split_inclusive('\n')
                .filter_map(|line| {
                    let start = offset;
                    offset += line.len();
                    let trimmed = line.trim_start_matches(' ');
                    let level = trimmed.bytes().take_while(|b| *b == b'#').count();
                    (line.len() - trimmed.len() <= 3
                        && (1..=6).contains(&level)
                        && trimmed[level..].starts_with(char::is_whitespace))
                    .then(|| {
                        (
                            start..offset,
                            gpui::HighlightStyle {
                                color: Some(syntax_color("var(--color-markdown-heading)")),
                                ..Default::default()
                            },
                        )
                    })
                })
                .collect();
            return frame
                .pt(px(if ix == 0 { 12. } else { 0. }))
                .font_family(crate::fonts::mono())
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(content(0.85))
                .child(gpui::StyledText::new(text).with_highlights(headings))
                .into_any_element();
        }
        if ix == 0
            && let Some(metadata) = &self.data.metadata
        {
            return frame
                .pt_4()
                .pb_6()
                .child(
                    div()
                        .rounded_lg()
                        .border_1()
                        .border_color(content(0.1))
                        .bg(content(0.03))
                        .child(
                            div()
                                .id("skill-metadata")
                                .debug_selector(|| "skill-metadata".into())
                                .role(Role::Button)
                                .aria_label("Skill metadata")
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px_3()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(content(0.6))
                                .cursor_pointer()
                                .hover(|s| s.text_color(content(1.)))
                                .child(icon(
                                    if self.metadata_open {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    14.,
                                ))
                                .child("Skill metadata")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.metadata_open = !this.metadata_open;
                                    this.list.remeasure_items(0..1);
                                    cx.notify();
                                })),
                        )
                        .when(self.metadata_open, |d| {
                            d.child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .border_t_1()
                                    .border_color(content(0.1))
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(12.))
                                    .line_height(px(20.))
                                    .text_color(content(0.7))
                                    .child(metadata.clone()),
                            )
                        }),
                )
                .into_any_element();
        }
        let at = ix - usize::from(self.data.metadata.is_some());
        let row = &self.data.rows[at];
        let id: SharedString = format!("skill-document-row-{at}").into();
        let body = match &row.part {
            Part::Code { fence, lines } => div()
                .id(id.clone())
                .overflow_x_scroll()
                .rounded_lg()
                .border_1()
                .border_color(content(0.1))
                .bg(content(0.03))
                .py_2()
                .child(markdown::code_lines(fence, lines.clone(), None, "", false))
                .into_any_element(),
            _ => markdown::body(row, id, Tone::Normal, None, Instant::now()),
        };
        frame
            .pt(px(if at == 0 { 16. } else { row.gap }))
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .gap_2()
                    .pl(px(row.indent as f32 * 16.))
                    .when(row.quote > 0, |d| {
                        d.border_l_2().border_color(content(0.15)).pl_3()
                    })
                    .when_some(row.marker.clone(), |d, marker| {
                        d.child(
                            div()
                                .w_4()
                                .flex_shrink_0()
                                .text_color(content(0.7))
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .child(marker),
                        )
                    })
                    .child(div().min_w_0().flex_1().child(body)),
            )
            .into_any_element()
    }
}
impl Render for SkillDocument {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("skill-document")
            .debug_selector(|| "skill-document".into())
            .role(Role::Region)
            .aria_label(if self.source {
                "Skill source"
            } else {
                "Skill preview document"
            })
            .size_full()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix, _, cx| this.row(ix, cx)),
                )
                .size_full()
                .pb_5(),
            )
    }
}

fn frontmatter(text: &str) -> (Option<&str>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(first_end) = text.find('\n') else {
        return (None, text);
    };
    if text[..first_end].trim_end_matches('\r') != "---" {
        return (None, text);
    }
    let mut at = first_end + 1;
    for line in text[at..].split_inclusive('\n') {
        if matches!(line.trim_end_matches(['\r', '\n']), "---" | "...") {
            return (
                Some(text[first_end + 1..at].trim_end_matches(['\r', '\n'])),
                &text[at + line.len()..],
            );
        }
        at += line.len();
    }
    (None, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skill_header_is_collapsed_metadata_and_body_starts_at_the_heading() {
        let data = DocumentData::new(
            "\u{feff}---\r\nname: review\r\n...\r\n# Review\n\nInstructions".into(),
        );
        assert_eq!(data.metadata.as_deref(), Some("name: review"));
        assert!(matches!(data.rows[0].part, Part::Text { heading: 1, .. }));
        let view = SkillDocument::new(data, false);
        assert!(!view.metadata_open);
        assert_eq!(view.list.logical_scroll_top().item_ix, 0);
        assert!(frontmatter("---\nunterminated").0.is_none());
    }
}
