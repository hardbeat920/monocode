//! Port of `ImageView` in src/features/files/ui/BinaryFileView.tsx.
//!
//! The image fits the pane until it is clicked or zoomed. A click toggles
//! between fit and 100%, the footer buttons zoom by 1.5x, and dragging pans
//! a zoomed image (the React view panned with its scrollbars only).
//! The MIME type comes from the bytes, never the file name.

use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui::{
    Context, Image, ImageFormat, ImageSource, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Pixels, Point, Render,
    ScrollHandle, SharedString, Size, StatefulInteractiveElement as _, Styled, StyledImage as _,
    Window, canvas, div, img, point, px, size,
};

use crate::{
    icons::IconKind,
    language::basename,
    theme::EditorTheme,
    viewer::{
        Zoom, checkerboard, clamp_zoom, footer, format_file_size, message, sniff_image_mime,
        zoom_button,
    },
};

/// Padding around the image (`p-4`).
const PADDING: f32 = 16.;

struct Drag {
    start: Point<Pixels>,
    offset: Point<Pixels>,
    moved: bool,
}

enum Content {
    Ready {
        image: Arc<Image>,
        natural: Option<(u32, u32)>,
        mime: &'static str,
    },
    /// Bytes that are not an image the viewer reads.
    Unsupported,
}

pub struct ImageView {
    path: SharedString,
    theme: EditorTheme,
    size: u64,
    content: Content,
    zoom: Zoom,
    scroll: ScrollHandle,
    viewport: Rc<Cell<Size<Pixels>>>,
    drag: Option<Drag>,
}

fn format_for(mime: &str) -> Option<ImageFormat> {
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/bmp" => ImageFormat::Bmp,
        "image/x-icon" => ImageFormat::Ico,
        "image/webp" => ImageFormat::Webp,
        // TODO(port): GPUI cannot decode AVIF, which WebKit could.
        _ => return None,
    })
}

/// Width and height from the image header, without decoding the pixels.
fn natural_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

impl ImageView {
    pub fn new(path: impl Into<SharedString>, bytes: Vec<u8>, theme: EditorTheme) -> Self {
        let size = bytes.len() as u64;
        Self {
            path: path.into(),
            theme,
            size,
            content: Self::content_for(bytes),
            zoom: Zoom::Fit,
            scroll: ScrollHandle::new(),
            viewport: Rc::new(Cell::new(Size::default())),
            drag: None,
        }
    }

    fn content_for(bytes: Vec<u8>) -> Content {
        let Some(mime) = sniff_image_mime(&bytes) else {
            return Content::Unsupported;
        };
        let Some(format) = format_for(mime) else {
            return Content::Unsupported;
        };
        let natural = natural_size(&bytes);
        Content::Ready {
            image: Arc::new(Image::from_bytes(format, bytes)),
            natural,
            mime,
        }
    }

    /// New bytes for the same file, after it changed on disk.
    pub fn reload(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.size = bytes.len() as u64;
        self.content = Self::content_for(bytes);
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: EditorTheme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = match zoom {
            Zoom::Fit => Zoom::Fit,
            Zoom::Scale(scale) => Zoom::Scale(clamp_zoom(scale)),
        };
        cx.notify();
    }

    fn current_scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => 1.,
            Zoom::Scale(scale) => scale,
        }
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(Zoom::Scale(self.current_scale() * 1.5), cx);
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(Zoom::Scale(self.current_scale() / 1.5), cx);
    }

    /// The image's displayed size. Fit scales down only, like
    /// `max-h-full max-w-full object-contain`.
    fn display_size(&self, natural: (u32, u32)) -> Size<Pixels> {
        let (width, height) = (natural.0 as f32, natural.1 as f32);
        let scale = match self.zoom {
            Zoom::Fit => {
                let viewport = self.viewport.get();
                let available_width = (f32::from(viewport.width) - PADDING * 2.).max(1.);
                let available_height = (f32::from(viewport.height) - PADDING * 2.).max(1.);
                (available_width / width)
                    .min(available_height / height)
                    .min(1.)
            }
            Zoom::Scale(scale) => scale,
        };
        size(px(width * scale), px(height * scale))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = Some(Drag {
            start: event.position,
            offset: self.scroll.offset(),
            moved: false,
        });
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let delta = event.position - drag.start;
        if delta.x.abs() > px(3.) || delta.y.abs() > px(3.) {
            drag.moved = true;
        }
        if drag.moved {
            let max = self.scroll.max_offset();
            let next = point(
                (drag.offset.x + delta.x).clamp(-max.x.abs(), px(0.)),
                (drag.offset.y + delta.y).clamp(-max.y.abs(), px(0.)),
            );
            self.scroll.set_offset(next);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.moved {
            // `onClick`: toggle between fit and 100%.
            self.zoom = if self.zoom == Zoom::Fit {
                Zoom::Scale(1.)
            } else {
                Zoom::Fit
            };
            cx.notify();
        }
    }
}

impl Render for ImageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let (image, natural, mime) = match &self.content {
            Content::Ready {
                image,
                natural,
                mime,
            } => (image.clone(), *natural, *mime),
            Content::Unsupported => {
                return message(
                    &theme,
                    basename(&self.path).to_owned(),
                    Some(format!(
                        "{} · not a readable image or PDF",
                        format_file_size(self.size)
                    )),
                )
                .into_any_element();
            }
        };
        let display = natural.map(|natural| self.display_size(natural));
        let viewport = self.viewport.clone();
        let facts: Vec<SharedString> = vec![
            natural
                .map_or("—".to_string(), |(width, height)| {
                    format!("{width} × {height}")
                })
                .into(),
            format_file_size(self.size).into(),
            mime.trim_start_matches("image/").to_uppercase().into(),
        ];
        let picture = img(ImageSource::Image(image)).object_fit(ObjectFit::Contain);
        // Lay the image out by hand: centered while it fits, scrollable from
        // its top-left corner once it is larger than the pane.
        let view = self.viewport.get();
        let content = match display {
            Some(display) => {
                let width = view.width.max(display.width + px(PADDING * 2.));
                let height = view.height.max(display.height + px(PADDING * 2.));
                div().relative().w(width).h(height).child(
                    picture
                        .absolute()
                        .left((width - display.width) / 2.)
                        .top((height - display.height) / 2.)
                        .w(display.width)
                        .h(display.height),
                )
            }
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(PADDING))
                .child(picture.max_w_full().max_h_full()),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(checkerboard())
                    .child(
                        // Measures the viewport for fit.
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
                            .id("image-scroll")
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .overflow_scroll()
                            .track_scroll(&self.scroll)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .child(content),
                    ),
            )
            .child(
                footer(&theme, facts)
                    .child(
                        zoom_button("zoom-out", IconKind::Minus, &theme)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
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
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
                    ),
            )
            .into_any_element()
    }
}
