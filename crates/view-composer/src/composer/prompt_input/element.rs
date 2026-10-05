//! The element that lays out, paints, and takes platform text input for a
//! [`PromptInput`]. It plays both layers of the React composer at once: the
//! textarea (caret, selection, IME) and the highlight overlay (colored runs,
//! icons over `@` and `/`).

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, ElementInputHandler,
    Entity, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
    Point, SharedString, Style, TextRun, UnderlineStyle, Window, fill, point, px, relative, size,
    transparent_black,
};
use monocode_ui::u;

use super::layout::{TextLayout, slice_runs};
use super::{OverlayPlacement, PromptDecorations, PromptInput};

pub struct PromptTextElement {
    input: Entity<PromptInput>,
}

impl PromptTextElement {
    pub fn new(input: Entity<PromptInput>) -> Self {
        Self { input }
    }
}

impl IntoElement for PromptTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// What a frame needs, read out of the entity so layout can borrow the
/// window freely.
#[derive(Clone)]
struct Snapshot {
    text: SharedString,
    placeholder: SharedString,
    selection: Range<usize>,
    caret: super::layout::Caret,
    marked: Option<Range<usize>>,
    decorations: Rc<PromptDecorations>,
    padding: [f32; 4],
    max_height: Option<f32>,
    min_rows: usize,
    scroll_y: Pixels,
    caret_visible: bool,
    autoscroll: bool,
    selection_color: Hsla,
    placeholder_color: Hsla,
    caret_color: Hsla,
}

pub struct PrepaintState {
    layout: Option<Rc<TextLayout>>,
    placeholder: Option<gpui::ShapedLine>,
    text_origin: Point<Pixels>,
    selections: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    overlays: Vec<AnyElement>,
    line_height: Pixels,
}

fn snapshot(input: &Entity<PromptInput>, window: &mut Window, cx: &mut App) -> Snapshot {
    let decorations = input.update(cx, |input, cx| input.decorations(cx));
    let input = input.read(cx);
    let focused = input.focus_handle.is_focused(window);
    Snapshot {
        text: input.buffer.text().to_string().into(),
        placeholder: input.placeholder.clone(),
        selection: input.buffer.selection(),
        caret: input.caret(),
        marked: input.buffer.marked(),
        decorations,
        padding: input.padding,
        max_height: input.max_height,
        min_rows: input.min_rows,
        scroll_y: input.scroll_y,
        caret_visible: focused && input.caret_visible && !input.disabled,
        autoscroll: input.autoscroll,
        selection_color: input.colors.selection,
        placeholder_color: input.colors.placeholder,
        caret_color: input.colors.caret,
    }
}

/// Text runs over the whole text: the base style, the decoration colors,
/// hidden glyphs, and the IME underline.
pub(super) fn build_runs(
    text_len: usize,
    base: &TextRun,
    decorations: &PromptDecorations,
    marked: Option<&Range<usize>>,
) -> Vec<TextRun> {
    let mut cuts = vec![0, text_len];
    for (range, _) in &decorations.spans {
        cuts.push(range.start.min(text_len));
        cuts.push(range.end.min(text_len));
    }
    for range in &decorations.hidden {
        cuts.push(range.start.min(text_len));
        cuts.push(range.end.min(text_len));
    }
    if let Some(range) = marked {
        cuts.push(range.start.min(text_len));
        cuts.push(range.end.min(text_len));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut runs: Vec<TextRun> = Vec::new();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let mut color = base.color;
        for (range, span_color) in &decorations.spans {
            if range.start <= start && end <= range.end {
                color = *span_color;
            }
        }
        if decorations
            .hidden
            .iter()
            .any(|range| range.start <= start && end <= range.end)
        {
            color = transparent_black();
        }
        let underline = marked
            .filter(|range| range.start <= start && end <= range.end)
            .map(|_| UnderlineStyle {
                color: Some(base.color),
                thickness: px(1.),
                wavy: false,
            });
        if let Some(last) = runs.last_mut()
            && last.color == color
            && last.underline == underline
        {
            last.len += end - start;
            continue;
        }
        runs.push(TextRun {
            len: end - start,
            color,
            underline,
            ..base.clone()
        });
    }
    runs
}

struct Metrics {
    font: gpui::Font,
    font_size: Pixels,
    line_height: Pixels,
    color: Hsla,
    padding: [Pixels; 4],
    max_height: Option<Pixels>,
    indent: Pixels,
}

fn metrics(snapshot: &Snapshot, window: &Window) -> Metrics {
    let style = window.text_style();
    let rem = window.rem_size();
    let font_size = style.font_size.to_pixels(rem);
    let line_height = style.line_height_in_pixels(rem);
    let p = snapshot.padding;
    Metrics {
        font: style.font(),
        font_size,
        line_height,
        color: style.color,
        padding: [
            u(p[0]).to_pixels(rem),
            u(p[1]).to_pixels(rem),
            u(p[2]).to_pixels(rem),
            u(p[3]).to_pixels(rem),
        ],
        max_height: snapshot.max_height.map(|h| u(h).to_pixels(rem)),
        indent: u(snapshot.decorations.first_line_indent).to_pixels(rem),
    }
}

fn layout_text(
    snapshot: &Snapshot,
    metrics: &Metrics,
    width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> TextLayout {
    let base = TextRun {
        len: snapshot.text.len(),
        font: metrics.font.clone(),
        color: metrics.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = build_runs(
        snapshot.text.len(),
        &base,
        &snapshot.decorations,
        snapshot.marked.as_ref(),
    );
    let inner = (width - metrics.padding[1] - metrics.padding[3]).max(px(1.));
    TextLayout::new(
        &snapshot.text,
        &runs,
        &metrics.font,
        metrics.font_size,
        metrics.line_height,
        inner,
        metrics.indent,
        window,
        cx,
    )
}

fn element_height(snapshot: &Snapshot, metrics: &Metrics, layout: &TextLayout) -> Pixels {
    let rows = layout.rows.len().max(snapshot.min_rows).max(1);
    let content = metrics.line_height * rows as f32;
    let total = content + metrics.padding[0] + metrics.padding[2];
    match metrics.max_height {
        Some(max) => total.min(max),
        None => total,
    }
}

impl Element for PromptTextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let snapshot = snapshot(&self.input, window, cx);
        // The text style is only on the stack now, not when taffy measures.
        let metrics = metrics(&snapshot, window);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout_id =
            window.request_measured_layout(style, move |known, available, window, cx| {
                let width = known.width.unwrap_or(match available.width {
                    AvailableSpace::Definite(width) => width,
                    _ => px(10_000.),
                });
                let layout = layout_text(&snapshot, &metrics, width, window, cx);
                size(width, element_height(&snapshot, &metrics, &layout))
            });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let snapshot = snapshot(&self.input, window, cx);
        let metrics = metrics(&snapshot, window);
        let layout = Rc::new(layout_text(
            &snapshot,
            &metrics,
            bounds.size.width,
            window,
            cx,
        ));
        let [top, _right, bottom, left] = metrics.padding;
        let viewport = (bounds.size.height - top - bottom).max(px(0.));
        let max_scroll = (layout.height() - viewport).max(px(0.));

        let mut scroll_y = snapshot.scroll_y.min(max_scroll).max(px(0.));
        if snapshot.autoscroll {
            let caret = layout.position_for(snapshot.caret);
            if caret.y < scroll_y {
                scroll_y = caret.y;
            } else if caret.y + layout.line_height > scroll_y + viewport {
                scroll_y = (caret.y + layout.line_height - viewport).max(px(0.));
            }
            scroll_y = scroll_y.min(max_scroll);
        }

        let text_origin = point(bounds.origin.x + left, bounds.origin.y + top - scroll_y);
        let selections = layout
            .range_bounds(snapshot.selection.clone())
            .into_iter()
            .map(|rect| {
                fill(
                    Bounds::new(rect.origin + text_origin, rect.size),
                    snapshot.selection_color,
                )
            })
            .collect();
        let caret = (snapshot.caret_visible && snapshot.selection.is_empty()).then(|| {
            let at = layout.position_for(snapshot.caret);
            fill(
                Bounds::new(
                    text_origin + at,
                    size(
                        u(1.).to_pixels(window.rem_size()).max(px(1.)),
                        layout.line_height,
                    ),
                ),
                snapshot.caret_color,
            )
        });

        let placeholder = (snapshot.text.is_empty()
            && snapshot.marked.is_none()
            && !snapshot.placeholder.is_empty())
        .then(|| {
            let run = TextRun {
                len: snapshot.placeholder.len(),
                font: metrics.font.clone(),
                color: snapshot.placeholder_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let width = (bounds.size.width - left - metrics.padding[1]).max(px(1.));
            let mut wrapper = window
                .text_system()
                .line_wrapper(metrics.font.clone(), metrics.font_size);
            let runs = [run];
            let (text, runs) = wrapper.truncate_line(
                snapshot.placeholder.clone(),
                width,
                "…",
                &runs,
                gpui::TruncateFrom::End,
            );
            let runs = slice_runs(&runs, 0..text.len(), &metrics.font);
            window
                .text_system()
                .shape_line(text, metrics.font_size, &runs, None)
        });

        let rem = window.rem_size();
        let mut overlays = Vec::new();
        for overlay in &snapshot.decorations.overlays {
            let Some(char_bounds) = layout.char_bounds(overlay.range.clone()) else {
                continue;
            };
            let side = u(overlay.size).to_pixels(rem);
            let y = char_bounds.origin.y + (layout.line_height - side) / 2.;
            let x = match overlay.placement {
                OverlayPlacement::Center => {
                    char_bounds.origin.x + (char_bounds.size.width - side) / 2.
                }
                OverlayPlacement::Before(offset) => char_bounds.origin.x - u(offset).to_pixels(rem),
            };
            let origin = text_origin + point(x, y);
            let mut element = (overlay.render)(window, cx);
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                element.prepaint_as_root(
                    origin,
                    size(
                        AvailableSpace::Definite(side),
                        AvailableSpace::Definite(side),
                    ),
                    window,
                    cx,
                );
            });
            overlays.push(element);
        }

        let line_height = layout.line_height;
        let layout_for_state = layout.clone();
        self.input.update(cx, |input, _| {
            input.scroll_y = scroll_y;
            input.max_scroll = max_scroll;
            input.autoscroll = false;
            input.last_layout = Some(layout_for_state);
            input.last_text_origin = text_origin;
            input.last_bounds = Some(bounds);
        });

        PrepaintState {
            layout: Some(layout),
            placeholder,
            text_origin,
            selections,
            caret,
            overlays,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, disabled) = {
            let input = self.input.read(cx);
            (input.focus_handle.clone(), input.disabled)
        };
        if !disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        let origin = prepaint.text_origin;
        let line_height = prepaint.line_height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in prepaint.selections.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(placeholder) = prepaint.placeholder.take() {
                let _ =
                    placeholder.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
            }
            if let Some(layout) = prepaint.layout.as_ref() {
                for row in &layout.rows {
                    let top = origin.y + row.y;
                    if top + line_height < bounds.top() || top > bounds.bottom() {
                        continue;
                    }
                    let _ = row.line.paint(
                        point(origin.x + row.x, top),
                        line_height,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
            for overlay in prepaint.overlays.iter_mut() {
                overlay.paint(window, cx);
            }
            if let Some(caret) = prepaint.caret.take() {
                window.paint_quad(caret);
            }
        });
    }
}
