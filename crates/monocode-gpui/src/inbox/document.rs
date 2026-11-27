//! Flowing Inbox prose. The detail page owns scrolling; this is never a chat.
use crate::{
    markdown::{self, MarkdownRow, Part, Tone},
    markdown_model,
    theme::*,
};
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Context, Element, ElementId, Entity, GlobalElementId,
    Image, ImageCacheError, ImageFormat, InspectorElementId, LayoutId, Pixels, RenderImage, Role,
    SharedString, Style, TextStyle, Window, div, img, point, prelude::*, px, size,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::{path::PathBuf, sync::Arc, time::Instant};

enum Row {
    Markdown(MarkdownRow, Pixels),
    Image { url: String, alt: String },
}

pub(super) struct InboxDocument {
    rows: Vec<Row>,
    layout: Option<Measurements>,
    #[cfg(test)]
    measured: usize,
    #[cfg(test)]
    rendered: Vec<usize>,
    #[cfg(test)]
    code_rendered: std::rc::Rc<std::cell::Cell<usize>>,
}

impl InboxDocument {
    pub(super) fn new(text: String, cwd: PathBuf, _: &mut Context<Self>) -> Self {
        let mut spans = Vec::new();
        let mut open: Option<(usize, String, String)> = None;
        for (event, range) in Parser::new_ext(&text, Options::all()).into_offset_iter() {
            match event {
                Event::Start(Tag::Image { dest_url, .. }) => {
                    open = Some((range.start, dest_url.into_string(), String::new()));
                }
                Event::Text(value) if open.is_some() => {
                    open.as_mut().unwrap().2.push_str(&value);
                }
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
        for (range, url, alt) in spans {
            if range.start < at || range.end > text.len() {
                continue;
            }
            append_markdown(&mut rows, &text[at..range.start], &cwd);
            rows.push(Row::Image { url, alt });
            at = range.end;
        }
        append_markdown(&mut rows, &text[at..], &cwd);
        Self {
            rows,
            layout: None,
            #[cfg(test)]
            measured: 0,
            #[cfg(test)]
            rendered: vec![],
            #[cfg(test)]
            code_rendered: Default::default(),
        }
    }
}

fn append_markdown(rows: &mut Vec<Row>, text: &str, cwd: &std::path::Path) {
    for block in &markdown_model::parse_full(text).blocks {
        for mut row in markdown::rows(&block.block) {
            markdown::resolve_file_links(&mut row, &cwd.to_string_lossy());
            // Transcript code is chunked for virtualization. A document displays
            // the whole fence in a single bordered code block instead.
            if let Part::Code { fence, lines } = &row.part
                && let Some(Row::Markdown(previous, _)) = rows.last_mut()
                && let Part::Code {
                    fence: last,
                    lines: last_lines,
                } = &mut previous.part
                && Arc::ptr_eq(fence, last)
                && last_lines.end == lines.start
            {
                last_lines.end = lines.end;
                continue;
            }
            if rows.is_empty() {
                row.gap = 0.;
            }
            let code_width = match &row.part {
                Part::Code { fence, .. } => px(fence
                    .lines
                    .iter()
                    .map(|range| fence.code[range.clone()].chars().count())
                    .max()
                    .unwrap_or(0) as f32
                    * 7.3
                    + 48.),
                _ => px(0.),
            };
            rows.push(Row::Markdown(row, code_width));
        }
    }
}

/// Issue images decode into the shared content budget, not GPUI's permanent
/// asset cache, so pages read long ago give their pixels back.
fn attachment(
    url: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    let url = url.to_owned();
    let view = window.current_view();
    let key = gpui::hash(&("inbox", &url));
    crate::image_memory::images(cx).update(cx, |cache, cx| {
        cache.request(
            key,
            view,
            move |cx| {
                let renderer = cx.svg_renderer();
                async move {
                    let bytes = monocode_core::inbox::media_png(&url)
                        .map_err(|error| ImageCacheError::Asset(error.into()))?;
                    Image::from_bytes(ImageFormat::Png, bytes)
                        .to_image_data(renderer)
                        .map_err(|error| ImageCacheError::Other(Arc::new(error)))
                }
            },
            cx,
        )
    })
}

/// Exact heights are retained at a given width/style. A collapsed body only
/// measures its leading rows; scrolling only constructs rows intersecting the
/// page's content mask. This keeps the original flowing document and one scroll.
struct Measurements {
    diagram_generation: u64,
    width: Pixels,
    text: TextStyle,
    rem: Pixels,
    scale: f32,
    images: Vec<bool>,
    bottoms: Vec<Pixels>,
}

impl InboxDocument {
    #[cfg(test)]
    pub(super) fn measured_rows(&self) -> usize {
        self.measured
    }

    fn row(&self, ix: usize, width: Pixels, window: &mut Window, cx: &mut App) -> AnyElement {
        let row = &self.rows[ix];
        match row {
            Row::Image { url, alt } => {
                let source = attachment(url, window, cx);
                let label = if alt.trim().is_empty() { "Image" } else { alt }.to_string();
                let url = url.clone();
                let media = match source {
                    Some(Ok(image)) => img(image)
                        .w_full()
                        .max_w(px(576.))
                        .max_h(px(448.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(content(0.1))
                        .object_fit(gpui::ObjectFit::Contain)
                        .into_any_element(),
                    Some(Err(_)) => div()
                        .text_size(px(14.))
                        .text_color(crate::theme::accent_color())
                        .child(label.clone())
                        .into_any_element(),
                    None => div()
                        .w_full()
                        .max_w(px(576.))
                        .h(px(128.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(content(0.1))
                        .bg(content(0.06))
                        .into_any_element(),
                };
                div()
                    .id(("inbox-image", ix))
                    .role(Role::Image)
                    .aria_label(label)
                    .py_2()
                    .min_w_0()
                    .cursor_pointer()
                    .child(media)
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element()
            }
            Row::Markdown(row, code_width) => {
                let id: SharedString = format!("inbox-document-{ix}").into();
                let body: AnyElement = match &row.part {
                    Part::Code { fence, lines } => div()
                        .id(id.clone())
                        .w_full()
                        .min_w_0()
                        .overflow_x_scroll()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(content(0.1))
                        .bg(content(0.03))
                        .py_2()
                        .child(CodeLines {
                            #[cfg(test)]
                            rendered: self.code_rendered.clone(),
                            fence: fence.clone(),
                            lines: lines.clone(),
                            width: (*code_width).max(
                                (width
                                    - px(2.
                                        + if row.quote > 0 {
                                            14.
                                        } else {
                                            row.indent
                                                .saturating_sub(usize::from(row.marker.is_some()))
                                                as f32
                                                * 16.
                                        }
                                        + if row.marker.is_some() { 24. } else { 0. }))
                                .max(px(1.)),
                            ),
                        })
                        .into_any_element(),
                    _ => markdown::body(row, id, Tone::Normal, None, Instant::now()),
                };
                div()
                    .w_full()
                    .pt(px(row.gap))
                    .min_w_0()
                    .flex()
                    .gap_2()
                    .pl(px(
                        row.indent.saturating_sub(usize::from(row.marker.is_some())) as f32 * 16.,
                    ))
                    .when(row.quote > 0, |d| {
                        d.border_l_2().border_color(content(0.16)).pl_3()
                    })
                    .when_some(row.marker.clone(), |d, marker| {
                        d.child(
                            div()
                                .min_w(px(16.))
                                .flex_shrink_0()
                                .text_size(px(14.))
                                .line_height(px(24.))
                                .text_color(content(0.78))
                                .child(marker),
                        )
                    })
                    .child(div().flex_1().min_w_0().child(body))
                    .into_any_element()
            }
        }
    }

    fn image_states(&self, count: usize, window: &mut Window, cx: &mut App) -> Vec<bool> {
        self.rows
            .iter()
            .take(count)
            .filter_map(|row| match row {
                Row::Image { url, .. } => Some(attachment(url, window, cx).is_some()),
                _ => None,
            })
            .collect()
    }

    fn measure(
        &mut self,
        width: Pixels,
        limit: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Pixels {
        // Only already measured images can affect retained heights. Assets in
        // the hidden tail of a collapsed review must stay unloaded.
        let count = self
            .layout
            .as_ref()
            .map_or(0, |layout| layout.bottoms.len());
        let images = self.image_states(count, window, cx);
        let diagram_generation = markdown::mermaid::generation(cx);
        let text = window.text_style();
        if self.layout.as_ref().is_none_or(|layout| {
            layout.width != width
                || layout.diagram_generation != diagram_generation
                || layout.text != text
                || layout.rem != window.rem_size()
                || layout.scale != window.scale_factor()
                || layout.images != images
        }) {
            self.layout = Some(Measurements {
                diagram_generation,
                width,
                text,
                rem: window.rem_size(),
                scale: window.scale_factor(),
                images,
                bottoms: vec![],
            });
        }
        loop {
            let layout = self.layout.as_ref().unwrap();
            let height = layout.bottoms.last().copied().unwrap_or(px(0.));
            let ix = layout.bottoms.len();
            if ix == self.rows.len() || limit.is_some_and(|limit| height >= limit) {
                let images = self.image_states(ix, window, cx);
                self.layout.as_mut().unwrap().images = images;
                return limit.map_or(height, |limit| height.min(limit));
            }
            let measured = self
                .row(ix, width, window, cx)
                .layout_as_root(
                    size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
                    window,
                    cx,
                )
                .height;
            #[cfg(test)]
            {
                self.measured += 1;
            }
            self.layout
                .as_mut()
                .unwrap()
                .bottoms
                .push(height + measured);
        }
    }
}

pub(super) struct DocumentElement {
    document: Entity<InboxDocument>,
    width: Pixels,
    limit: Option<Pixels>,
}
impl DocumentElement {
    pub(super) fn new(
        document: Entity<InboxDocument>,
        width: Pixels,
        limit: Option<Pixels>,
    ) -> Self {
        Self {
            document,
            width: width.max(px(1.)),
            limit,
        }
    }
}
impl IntoElement for DocumentElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for DocumentElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        Some(ElementId::View(self.document.entity_id()))
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let height = self.document.update(cx, |doc, cx| {
            doc.measure(self.width, self.limit, window, cx)
        });
        let mut style = Style::default();
        style.size.width = self.width.into();
        style.size.height = height.into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let visible = window.content_mask().bounds.intersect(&bounds);
        self.document.update(cx, |doc, cx| {
            #[cfg(test)]
            {
                doc.rendered.clear();
            }
            if visible.size.height <= px(0.) || visible.size.width <= px(0.) {
                return vec![];
            }
            let layout = doc.layout.as_ref().unwrap();
            let start = layout
                .bottoms
                .partition_point(|bottom| *bottom <= visible.top() - bounds.top());
            let end = (layout
                .bottoms
                .partition_point(|bottom| *bottom < visible.bottom() - bounds.top())
                + 1)
            .min(layout.bottoms.len());
            let mut children = Vec::new();
            for ix in start..end {
                let top = if ix == 0 {
                    px(0.)
                } else {
                    doc.layout.as_ref().unwrap().bottoms[ix - 1]
                };
                let mut child = doc.row(ix, self.width, window, cx);
                child.layout_as_root(
                    size(
                        AvailableSpace::Definite(self.width),
                        AvailableSpace::MinContent,
                    ),
                    window,
                    cx,
                );
                child.prepaint_at(bounds.origin + point(px(0.), top), window, cx);
                #[cfg(test)]
                {
                    doc.rendered.push(ix);
                }
                children.push(child);
            }
            children
        })
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        children: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for child in children {
            child.paint(window, cx);
        }
    }
}

/// Preserve a fence's full dimensions and single horizontal scroll container,
/// while only constructing the code lines inside the vertical content mask.
struct CodeLines {
    #[cfg(test)]
    rendered: std::rc::Rc<std::cell::Cell<usize>>,
    fence: Arc<markdown::Fence>,
    lines: std::ops::Range<usize>,
    width: Pixels,
}
impl IntoElement for CodeLines {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for CodeLines {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;
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
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = self.width.into();
        style.size.height = px(self.lines.len() as f32 * 20.).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let visible = window.content_mask().bounds.intersect(&bounds);
        if visible.size.height <= px(0.) || visible.size.width <= px(0.) {
            return None;
        }
        let start = ((visible.top() - bounds.top()).as_f32() / 20.)
            .floor()
            .max(0.) as usize;
        let end = (((visible.bottom() - bounds.top()).as_f32() / 20.).ceil() as usize)
            .min(self.lines.len());
        #[cfg(test)]
        {
            self.rendered.set(end - start);
        }
        let mut body = markdown::code_lines(
            &self.fence,
            self.lines.start + start..self.lines.start + end,
            None,
            "",
            false,
        )
        .into_any_element();
        body.layout_as_root(
            size(
                AvailableSpace::Definite(self.width),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        body.prepaint_at(
            bounds.origin + point(px(0.), px(start as f32 * 20.)),
            window,
            cx,
        );
        Some(body)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        child: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = child {
            child.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, ScrollHandle, TestAppContext, WindowBounds, WindowHandle, WindowOptions};

    struct Fixture {
        document: Entity<InboxDocument>,
        scroll: ScrollHandle,
        width: Pixels,
        limit: Option<Pixels>,
        eager: bool,
    }
    impl Render for Fixture {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let body = if self.eager {
                self.document.update(cx, |doc, cx| {
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .children((0..doc.rows.len()).map(|ix| doc.row(ix, self.width, window, cx)))
                        .into_any_element()
                })
            } else {
                DocumentElement::new(self.document.clone(), self.width, self.limit)
                    .into_any_element()
            };
            div()
                .size_full()
                .id("document-scroll")
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(body)
        }
    }
    fn fixture(
        text: String,
        limit: Option<Pixels>,
        cx: &mut TestAppContext,
    ) -> WindowHandle<Fixture> {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(640.), px(320.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| Fixture {
                        document: cx.new(|cx| InboxDocument::new(text, PathBuf::from("/repo"), cx)),
                        scroll: ScrollHandle::new(),
                        width: px(640.),
                        limit,
                        eager: false,
                    })
                },
            )
            .unwrap()
        });
        draw(window, cx);
        window
    }
    fn draw(window: WindowHandle<Fixture>, cx: &mut TestAppContext) {
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
    }
    fn prose() -> String {
        (0..300).map(|ix| format!("### Finding {ix}\n\nCheck **retries** in `src/main.rs` before returning from this operation. Preserve the reader's place.\n\n")).collect()
    }
    #[gpui::test]
    fn mermaid_completion_updates_retained_document_heights(cx: &mut TestAppContext) {
        let window = fixture(
            "```mermaid\ngraph LR; A-->B-->C-->D\n```\n".into(),
            None,
            cx,
        );
        let initial = window
            .update(cx, |view, _, cx| {
                *view
                    .document
                    .read(cx)
                    .layout
                    .as_ref()
                    .unwrap()
                    .bottoms
                    .last()
                    .unwrap()
            })
            .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        draw(window, cx);
        let ready = window
            .update(cx, |view, _, cx| {
                *view
                    .document
                    .read(cx)
                    .layout
                    .as_ref()
                    .unwrap()
                    .bottoms
                    .last()
                    .unwrap()
            })
            .unwrap();
        assert_ne!(
            initial, ready,
            "the loading placeholder must be remeasured after rendering"
        );
        window
            .update(cx, |view, _, cx| {
                view.eager = true;
                cx.notify();
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, cx| {
                assert_eq!(
                    *view
                        .document
                        .read(cx)
                        .layout
                        .as_ref()
                        .unwrap()
                        .bottoms
                        .last()
                        .unwrap(),
                    ready
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn scroll_keeps_exact_heights_and_only_materializes_visible_markdown(cx: &mut TestAppContext) {
        let window = fixture(prose(), None, cx);
        let (before, height) = window
            .update(cx, |view, _, cx| {
                let doc = view.document.read(cx);
                assert!(doc.rendered.len() < 15);
                assert_eq!(doc.measured, doc.rows.len());
                (doc.measured, view.scroll.max_offset().y)
            })
            .unwrap();
        window
            .update(cx, |view, _, _| {
                view.scroll.set_offset(point(px(0.), px(-6000.)))
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, cx| {
                let doc = view.document.read(cx);
                assert_eq!(
                    doc.measured, before,
                    "scrolling must reuse exact row heights"
                );
                assert!(doc.rendered.len() < 15);
                assert!(doc.rendered[0] > 30, "scroll must reach later prose");
                assert_eq!(view.scroll.max_offset().y, height);
                view.eager = true;
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, _| {
                assert!(
                    (view.scroll.max_offset().y - height).abs() < px(1.),
                    "flow geometry must match the original eager column: eager {:?}, retained {:?}",
                    view.scroll.max_offset().y,
                    height
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn collapsed_body_skips_hidden_layout_and_expands_at_the_same_width(cx: &mut TestAppContext) {
        let window = fixture(prose(), Some(px(180.)), cx);
        window
            .update(cx, |view, _, cx| {
                let doc = view.document.read(cx);
                assert!(doc.measured < 10);
                assert!(doc.measured < doc.rows.len());
                assert_eq!(view.scroll.max_offset().y, px(0.));
                view.limit = None;
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, cx| {
                let doc = view.document.read(cx);
                assert_eq!(doc.measured, doc.rows.len());
                assert!(view.scroll.max_offset().y > px(1000.));
                view.width = px(320.);
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, cx| {
                let doc = view.document.read(cx);
                assert_eq!(doc.layout.as_ref().unwrap().width, px(320.));
                assert_eq!(
                    doc.measured,
                    doc.rows.len() * 2,
                    "width changes must invalidate cached heights"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn tall_code_fence_keeps_full_scroll_range_but_builds_only_visible_lines(
        cx: &mut TestAppContext,
    ) {
        let code: String = (0..4000)
            .map(|ix| format!("const line_{ix} = {ix};\n"))
            .collect();
        let window = fixture(format!("```ts\n{code}```"), None, cx);
        window
            .update(cx, |view, _, cx| {
                assert!(view.scroll.max_offset().y > px(79000.));
                assert!(view.document.read(cx).code_rendered.get() < 20);
                view.scroll.set_offset(point(px(0.), px(-40000.)));
            })
            .unwrap();
        draw(window, cx);
        window
            .update(cx, |view, _, cx| {
                assert!(view.document.read(cx).code_rendered.get() < 20);
                assert!(view.document.read(cx).code_rendered.get() > 0);
                assert!(view.scroll.offset().y < px(-39000.));
            })
            .unwrap();
    }
}
