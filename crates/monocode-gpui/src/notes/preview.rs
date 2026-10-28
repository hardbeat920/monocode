//! Notes use shared Markdown rendering with their desktop image asset paths.
use crate::{
    markdown::{self, MarkdownRow, Part, Tone},
    markdown_model,
    runtime::{Database, locked},
    theme::*,
};
use gpui::{AnyElement, Context, ImageSource, Render, Role, Window, div, prelude::*, px};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{path::Path, time::Instant};

enum Row {
    Markdown(MarkdownRow),
    Image { source: ImageSource, alt: String },
    Missing(String),
}
pub(super) struct Preview {
    rows: Vec<Row>,
}
impl Preview {
    pub(super) fn new(text: &str, cwd: &str, store: &Database) -> Self {
        let mut spans = Vec::new();
        let mut open = None;
        for (event, range) in Parser::new_ext(text, Options::all()).into_offset_iter() {
            match event {
                Event::Start(Tag::Image { dest_url, .. }) => {
                    open = Some((range.start, dest_url.to_string(), String::new()))
                }
                Event::Text(value) if open.is_some() => open.as_mut().unwrap().2.push_str(&value),
                Event::End(TagEnd::Image) => {
                    if let Some((start, url, alt)) = open.take() {
                        spans.push((start..range.end, url, alt));
                    }
                }
                _ => {}
            }
        }
        let mut rows = Vec::new();
        let mut at = 0;
        for (span, url, alt) in spans {
            if span.start < at || span.end > text.len() {
                continue;
            }
            append_markdown(&mut rows, &text[at..span.start], cwd);
            let source = if url.starts_with("/note-assets/") {
                locked(store)
                    .and_then(|store| store.note_image_path(&url))
                    .map(ImageSource::from)
            } else if url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("data:image/")
            {
                Ok(ImageSource::from(url))
            } else {
                let path = Path::new(&url);
                Ok(ImageSource::from(if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    Path::new(cwd).join(path)
                }))
            };
            rows.push(match source {
                Ok(source) => Row::Image { source, alt },
                Err(error) => Row::Missing(error),
            });
            at = span.end;
        }
        append_markdown(&mut rows, &text[at..], cwd);
        Self { rows }
    }
}
fn append_markdown(rows: &mut Vec<Row>, text: &str, cwd: &str) {
    for block in &markdown_model::parse_full_hard_breaks(text).blocks {
        for mut row in markdown::rows(&block.block) {
            markdown::resolve_file_links(&mut row, cwd);
            rows.push(Row::Markdown(row));
        }
    }
}
impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(self.rows.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(13.))
                        .text_color(content(0.45))
                        .child("No description"),
                )
            })
            .children(self.rows.iter().enumerate().map(|(ix, row)| {
                match row {
                    Row::Image { source, alt } => div()
                        .mt_3()
                        .mb_3()
                        .w_full()
                        .min_w_0()
                        .child(
                            crate::image_memory::image(source.clone())
                                .id(("note-image", ix))
                                .role(Role::Image)
                                .aria_label(alt.clone())
                                .max_w_full()
                                .max_h(px(640.))
                                .rounded_md()
                                .object_fit(gpui::ObjectFit::Contain),
                        )
                        .into_any_element(),
                    Row::Missing(error) => div()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child(error.clone())
                        .into_any_element(),
                    Row::Markdown(row) => {
                        let id: gpui::SharedString = format!("note-markdown-{ix}").into();
                        let body: AnyElement = match &row.part {
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
                        div()
                            .pt(px(row.gap))
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
                            .child(div().flex_1().min_w_0().child(body))
                            .into_any_element()
                    }
                }
            }))
    }
}
