//! Port of src/features/files/ui/PdfView.tsx.
//!
//! A scrolling column of pages. The document opens on a background thread
//! with `hayro` (Apache-2.0 OR MIT); each page draws to a bitmap on a
//! background thread only when it comes within one viewport of the visible
//! area, and pages far from view drop their bitmaps. Like the pdf.js
//! version, pages have no text layer.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AppContext as _, Context, ImageSource, InteractiveElement, IntoElement, ObjectFit,
    ParentElement, Pixels, Render, RenderImage, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement as _, Styled, StyledImage as _, Task, Window, canvas, div, img,
    prelude::FluentBuilder as _, px,
};
use hayro::{
    RenderCache, RenderSettings,
    hayro_interpret::InterpreterSettings,
    hayro_syntax::{LoadPdfError, Pdf},
    vello_cpu::color::palette::css::WHITE,
};

use crate::{
    icons::IconKind,
    theme::EditorTheme,
    viewer::{
        Zoom, clamp_zoom, footer, format_file_size, message, render_pixel_ratio, zoom_button,
    },
};

/// `PAGE_GAP`: space between pages and around the column.
const PAGE_GAP: f32 = 16.;
/// A short delay lets a burst of zoom clicks settle before drawing.
const RENDER_DELAY: Duration = Duration::from_millis(30);

enum State {
    Loading,
    Ready(Arc<Pdf>),
    Error(String),
}

struct PageImage {
    /// The bitmap scale (zoom times pixel ratio) it was drawn at.
    scale: f32,
    image: Arc<RenderImage>,
}

pub struct PdfView {
    theme: EditorTheme,
    size: u64,
    state: State,
    /// Page sizes in PDF points.
    page_sizes: Vec<(f32, f32)>,
    zoom: Zoom,
    scroll: ScrollHandle,
    viewport: Rc<Cell<Size<Pixels>>>,
    pages: HashMap<usize, PageImage>,
    failures: HashMap<usize, String>,
    rendering: HashMap<usize, (f32, Task<()>)>,
    current_page: usize,
    _open: Option<Task<()>>,
}

/// `describePdfError`.
fn describe_error(error: LoadPdfError) -> String {
    match error {
        LoadPdfError::Decryption(_) => "This PDF is password protected.".into(),
        LoadPdfError::Invalid => "This file is not a readable PDF.".into(),
    }
}

/// Draw page `index` at `scale` and convert it to GPUI's BGRA layout.
fn render_page(pdf: &Pdf, index: usize, scale: f32) -> Result<Arc<RenderImage>, String> {
    let page = pdf.pages().get(index).ok_or("missing page")?;
    let settings = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..Default::default()
    };
    let pixmap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hayro::render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &settings,
        )
    }))
    .map_err(|_| "the renderer failed".to_string())?;
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    // Premultiplied RGBA on an opaque white page, so only the channel order changes.
    let mut data = pixmap.data_as_u8_slice().to_vec();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, data).ok_or("bad bitmap")?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

impl PdfView {
    pub fn new(bytes: Vec<u8>, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        let size = bytes.len() as u64;
        let job = cx.background_spawn(async move {
            let pdf = Pdf::new(bytes).map_err(describe_error)?;
            let sizes: Vec<(f32, f32)> = pdf
                .pages()
                .iter()
                .map(|page| page.render_dimensions())
                .collect();
            if sizes.is_empty() {
                return Err("This PDF has no pages.".to_string());
            }
            Ok((Arc::new(pdf), sizes))
        });
        let open = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((pdf, sizes)) => {
                        this.page_sizes = sizes;
                        this.state = State::Ready(pdf);
                    }
                    Err(message) => this.state = State::Error(message),
                }
                cx.notify();
            });
        });
        Self {
            theme,
            size,
            state: State::Loading,
            page_sizes: Vec::new(),
            zoom: Zoom::Fit,
            scroll: ScrollHandle::new(),
            viewport: Rc::new(Cell::new(Size::default())),
            pages: HashMap::new(),
            failures: HashMap::new(),
            rendering: HashMap::new(),
            current_page: 1,
            _open: Some(open),
        }
    }

    pub fn page_count(&self) -> usize {
        self.page_sizes.len()
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, State::Ready(_))
    }

    pub fn error(&self) -> Option<&str> {
        match &self.state {
            State::Error(message) => Some(message),
            _ => None,
        }
    }

    pub fn set_theme(&mut self, theme: EditorTheme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// `fitScale`: the widest page fills the viewport width.
    fn fit_scale(&self) -> f32 {
        let widest = self
            .page_sizes
            .iter()
            .fold(0f32, |max, (width, _)| max.max(*width));
        let viewport = f32::from(self.viewport.get().width);
        if widest > 0. && viewport > 0. {
            clamp_zoom((viewport - PAGE_GAP * 2.) / widest)
        } else {
            1.
        }
    }

    pub fn scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fit_scale(),
            Zoom::Scale(scale) => scale,
        }
    }

    pub fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = match zoom {
            Zoom::Fit => Zoom::Fit,
            Zoom::Scale(scale) => Zoom::Scale(clamp_zoom(scale)),
        };
        cx.notify();
    }

    /// `stepZoom`.
    pub fn step_zoom(&mut self, factor: f32, cx: &mut Context<Self>) {
        let scale = self.scale();
        self.set_zoom(Zoom::Scale(scale * factor), cx);
    }

    /// Top of each page in the scroll content, and the content height.
    fn page_tops(&self, scale: f32) -> Vec<f32> {
        let mut tops = Vec::with_capacity(self.page_sizes.len());
        let mut top = PAGE_GAP;
        for (_, height) in &self.page_sizes {
            tops.push(top);
            top += height * scale + PAGE_GAP;
        }
        tops
    }

    /// Request bitmaps for pages near the viewport, drop the rest, and
    /// track the current page (`onScroll`).
    fn update_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready(pdf) = &self.state else {
            return;
        };
        let pdf = pdf.clone();
        let scale = self.scale();
        let tops = self.page_tops(scale);
        let viewport = self.viewport.get();
        let view_height = f32::from(viewport.height).max(1.);
        let scroll_top = -f32::from(self.scroll.offset().y);
        let middle = scroll_top + view_height / 2.;
        self.current_page = tops
            .iter()
            .rposition(|top| *top <= middle)
            .map_or(1, |index| index + 1);

        // `RENDER_MARGIN = "100% 0px"`.
        let near_top = scroll_top - view_height;
        let near_bottom = scroll_top + view_height * 2.;
        let device_ratio = window.scale_factor();
        let mut near = HashSet::new();
        for (index, top) in tops.iter().enumerate() {
            let (width, height) = self.page_sizes[index];
            let bottom = top + height * scale;
            if bottom < near_top || *top > near_bottom {
                continue;
            }
            near.insert(index);
            let ratio = render_pixel_ratio(width * scale, height * scale, device_ratio);
            let bitmap_scale = scale * ratio;
            let current = self
                .pages
                .get(&index)
                .is_some_and(|page| (page.scale - bitmap_scale).abs() < 0.001);
            let queued = self
                .rendering
                .get(&index)
                .is_some_and(|(queued, _)| (queued - bitmap_scale).abs() < 0.001);
            if current || queued || self.failures.contains_key(&index) {
                continue;
            }
            let pdf = pdf.clone();
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RENDER_DELAY).await;
                let result = cx
                    .background_spawn(async move { render_page(&pdf, index, bitmap_scale) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.rendering.remove(&index);
                    match result {
                        // The old bitmap stays until the new one is ready.
                        Ok(image) => {
                            this.pages.insert(
                                index,
                                PageImage {
                                    scale: bitmap_scale,
                                    image,
                                },
                            );
                        }
                        Err(message) => {
                            this.failures.insert(index, message);
                        }
                    }
                    cx.notify();
                });
            });
            self.rendering.insert(index, (bitmap_scale, task));
        }
        // Free bitmaps far from view.
        self.pages.retain(|index, _| near.contains(index));
        self.rendering.retain(|index, _| near.contains(index));
    }
}

impl Render for PdfView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        if let State::Error(error) = &self.state {
            return message(&theme, "Couldn’t open PDF", Some(error.clone())).into_any_element();
        }
        self.update_pages(window, cx);
        let scale = self.scale();
        let viewport = self.viewport.clone();
        let ready = matches!(self.state, State::Ready(_));
        let page_label: SharedString = if ready {
            format!("Page {} of {}", self.current_page, self.page_sizes.len()).into()
        } else {
            "—".into()
        };
        let pages = self
            .page_sizes
            .iter()
            .enumerate()
            .map(|(index, (width, height))| {
                let image = self.pages.get(&index).map(|page| page.image.clone());
                let failure = self.failures.get(&index).cloned();
                div()
                    .flex_none()
                    .relative()
                    .w(px(width * scale))
                    .h(px(height * scale))
                    .bg(gpui::white())
                    .border_1()
                    .border_color(gpui::black().opacity(0.1))
                    .shadow_sm()
                    .when_some(image, |this, image| {
                        this.child(
                            img(ImageSource::Render(image))
                                .size_full()
                                .object_fit(ObjectFit::Fill),
                        )
                    })
                    .when_some(failure, |this, failure| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .text_size(px(12.))
                                .text_color(gpui::rgb(0x737373))
                                .child(format!("Couldn’t draw page {}: {failure}", index + 1)),
                        )
                    })
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .font_family(theme.ui_font.clone())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .bg(theme.content(0.05))
                    .child(
                        canvas(
                            move |bounds, window, _| {
                                if viewport.get() != bounds.size {
                                    viewport.set(bounds.size);
                                    window.refresh();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(
                        div()
                            .id("pdf-scroll")
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
                            .when(!ready, |this| {
                                this.child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(12.))
                                        .text_color(theme.content(0.45))
                                        .child("Rendering PDF…"),
                                )
                            })
                            .when(ready, |this| {
                                this.child(
                                    div()
                                        .min_w_full()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(PAGE_GAP))
                                        .p(px(PAGE_GAP))
                                        .children(pages),
                                )
                            }),
                    ),
            )
            .child(
                footer(
                    &theme,
                    vec![page_label, format_file_size(self.size).into(), "PDF".into()],
                )
                .child(
                    zoom_button("zoom-out", IconKind::Minus, &theme)
                        .on_click(cx.listener(|this, _, _, cx| this.step_zoom(1. / 1.25, cx))),
                )
                .child(
                    div()
                        .id("zoom-fit")
                        .w(px(44.))
                        .flex()
                        .justify_center()
                        .rounded(px(4.))
                        .hover(|this| this.text_color(theme.foreground))
                        .child(self.zoom.label())
                        .on_click(cx.listener(|this, _, _, cx| this.set_zoom(Zoom::Fit, cx))),
                )
                .child(
                    zoom_button("zoom-in", IconKind::Plus, &theme)
                        .on_click(cx.listener(|this, _, _, cx| this.step_zoom(1.25, cx))),
                ),
            )
            .into_any_element()
    }
}
