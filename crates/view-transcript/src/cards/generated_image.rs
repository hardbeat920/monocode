//! Port of src/features/sessions/ui/GeneratedImage.tsx and the parts of
//! src/shared/ui/ImageLightbox.tsx it opens: an image the agent produced,
//! read from its path, with its name and size under it. A click opens it
//! full screen over the window; Escape, the close button, or a click on the
//! dimmed backdrop closes it.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnchoredPositionMode, AnyElement, App, Context, FocusHandle, Focusable, Image, ImageFormat,
    InteractiveElement as _, IntoElement, KeyDownEvent, ObjectFit, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Task, Window, anchored,
    deferred, div, img, point, px,
};
use monocode_core::block::GeneratedImageMeta;
use monocode_core::{Attachment, AttachmentKind};
use monocode_ui::styled::{UiStyled as _, glass_backdrop};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

/// `sniffImageMime` in src/features/files/model/filePreview.ts.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts = |offset: usize, magic: &[u8]| {
        bytes
            .get(offset..)
            .is_some_and(|rest| rest.len() >= magic.len() && rest.starts_with(magic))
    };
    if starts(0, &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if starts(0, &[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if starts(0, &[0x47, 0x49, 0x46, 0x38]) {
        return Some("image/gif");
    }
    if starts(0, &[0x42, 0x4d]) {
        return Some("image/bmp");
    }
    if starts(0, &[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    // RIFF....WEBP: the four size bytes at offset 4 are skipped.
    if starts(0, &[0x52, 0x49, 0x46, 0x46]) && starts(8, &[0x57, 0x45, 0x42, 0x50]) {
        return Some("image/webp");
    }
    // ....ftyp{avif,avis}: an ISO base media box, shared with HEIF and MP4.
    if starts(4, &[0x66, 0x74, 0x79, 0x70]) {
        let brand = bytes.get(8..12).unwrap_or(&[]);
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
    }
    None
}

/// `formatFileSize`: bytes in the units a file manager shows.
pub fn format_file_size(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB"];
    let mut size = bytes as f64 / 1024.;
    let mut unit = 0;
    while size >= 1024. && unit < units.len() - 1 {
        size /= 1024.;
        unit += 1;
    }
    if size < 10. {
        format!("{size:.1} {}", units[unit])
    } else {
        format!("{} {}", size.round(), units[unit])
    }
}

/// The GPUI decoder for a sniffed MIME type. AVIF has none.
fn image_format(mime: &str) -> Option<ImageFormat> {
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/bmp" => ImageFormat::Bmp,
        "image/x-icon" => ImageFormat::Ico,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    })
}

/// The pixel size an image file declares in its header, for laying the
/// frame out at the image's shape before it decodes.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as u32);
    let le16 = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as u32);
    let be32 = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let le32 = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let le24 = |at: usize| {
        let b = bytes.get(at..at + 3)?;
        Some(b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16)
    };
    match sniff_image_mime(bytes)? {
        "image/png" => Some((be32(16)?, be32(20)?)),
        "image/gif" => Some((le16(6)?, le16(8)?)),
        "image/bmp" => Some((le32(18)?, le32(22)?.cast_signed().unsigned_abs())),
        "image/x-icon" => {
            let size = |byte: u8| if byte == 0 { 256 } else { byte as u32 };
            Some((size(*bytes.get(6)?), size(*bytes.get(7)?)))
        }
        "image/webp" => match bytes.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let bits = le32(21)?;
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            _ => None,
        },
        "image/jpeg" => {
            // Walk the markers to the first start-of-frame.
            let mut at = 2;
            while at + 9 < bytes.len() {
                if bytes[at] != 0xff {
                    return None;
                }
                let marker = bytes[at + 1];
                let length = be16(at + 2)? as usize;
                if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                    return Some((be16(at + 7)?, be16(at + 5)?));
                }
                at += 2 + length;
            }
            None
        }
        _ => None,
    }
}

/// A read image: the decodable bytes, the file size, and the declared size.
pub type LoadedImage = (Arc<Image>, i64, Option<(u32, u32)>);

/// Read and decode-check an image file off the UI thread.
pub fn load_image(path: &std::path::Path) -> Result<LoadedImage, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    load_image_bytes(bytes)
}

fn load_image_bytes(bytes: Vec<u8>) -> Result<LoadedImage, String> {
    let mime = sniff_image_mime(&bytes).ok_or("not an image")?;
    let size = bytes.len() as i64;
    if mime == "image/avif" {
        let rgba = monocode_editor::image_view::decode_avif_rgba(&bytes)?;
        let dimensions = rgba.dimensions();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        return Ok((
            Arc::new(Image::from_bytes(ImageFormat::Png, png.into_inner())),
            size,
            Some(dimensions),
        ));
    }
    let format = image_format(mime).ok_or("unsupported image format")?;
    let dimensions = image_dimensions(&bytes);
    Ok((Arc::new(Image::from_bytes(format, bytes)), size, dimensions))
}

/// `State`.
#[derive(Clone)]
pub enum GeneratedImageState {
    Loading,
    Ready {
        image: Arc<Image>,
        size: i64,
        /// Width and height in pixels, when the header gives them.
        dimensions: Option<(u32, u32)>,
    },
    PreviewUrl(String),
    Error,
}

/// `<GeneratedImage image />`.
pub struct GeneratedImage {
    meta: GeneratedImageMeta,
    state: GeneratedImageState,
    open: bool,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    thumbnail: bool,
    attachment: Option<Attachment>,
    _load: Option<Task<()>>,
}

impl GeneratedImage {
    /// Starts reading the file at `meta.path`.
    pub fn new(meta: GeneratedImageMeta, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            meta: GeneratedImageMeta {
                path: String::new(),
                ..meta.clone()
            },
            state: GeneratedImageState::Loading,
            open: false,
            focus: cx.focus_handle(),
            previous_focus: None,
            thumbnail: false,
            attachment: None,
            _load: None,
        };
        this.set_image(meta, cx);
        this
    }

    /// An attachment thumbnail with the same fullscreen preview.
    pub fn new_attachment(file: Attachment, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            meta: attachment_meta(&file),
            state: GeneratedImageState::Loading,
            open: false,
            focus: cx.focus_handle(),
            previous_focus: None,
            thumbnail: true,
            attachment: None,
            _load: None,
        };
        this.set_attachment(file, cx);
        this
    }

    pub fn set_attachment(&mut self, file: Attachment, cx: &mut Context<Self>) {
        if self.attachment.as_ref() == Some(&file) {
            return;
        }
        self.meta = attachment_meta(&file);
        self.attachment = Some(file.clone());
        self.state = GeneratedImageState::Loading;
        self.open = false;
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { load_attachment_preview(&file) })
                .await;
            this.update(cx, |this, cx| {
                this.state = match loaded {
                    Ok(AttachmentPreview::Bytes((image, size, dimensions))) => {
                        GeneratedImageState::Ready {
                            image,
                            size,
                            dimensions,
                        }
                    }
                    Ok(AttachmentPreview::Url(url)) => GeneratedImageState::PreviewUrl(url),
                    Err(_) => GeneratedImageState::Error,
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Show another image. A new path reads the file again.
    pub fn set_image(&mut self, meta: GeneratedImageMeta, cx: &mut Context<Self>) {
        if meta == self.meta {
            return;
        }
        if meta.path == self.meta.path {
            self.meta = meta;
            cx.notify();
            return;
        }
        self.meta = meta;
        self.state = GeneratedImageState::Loading;
        let path = PathBuf::from(&self.meta.path);
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { load_image(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.state = match loaded {
                    Ok((image, size, dimensions)) => GeneratedImageState::Ready {
                        image,
                        size,
                        dimensions,
                    },
                    Err(_) => GeneratedImageState::Error,
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn state(&self) -> &GeneratedImageState {
        &self.state
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the full screen preview.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.state,
            GeneratedImageState::Ready { .. } | GeneratedImageState::PreviewUrl(_)
        ) && !self.open
        {
            self.previous_focus = window.focused(cx);
            self.open = true;
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.previous_focus = None;
        cx.notify();
    }

    fn close_in_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.previous_focus.take() {
            focus.focus(window, cx);
        }
        self.close(cx);
    }

    fn lightbox(
        &self,
        image: impl Into<gpui::ImageSource>,
        dimensions: Option<(u32, u32)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let viewport = window.viewport_size();
        let layer = theme.layer.dialog;
        let alt = self
            .meta
            .alt
            .clone()
            .filter(|alt| !alt.is_empty())
            .unwrap_or_else(|| self.meta.name.clone());
        let white = gpui::white();
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Window)
                .position(point(px(0.), px(0.)))
                .child(
                    div()
                        .id("image-lightbox")
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.close_in_window(window, cx);
                            }
                        }))
                        .relative()
                        .w(viewport.width)
                        .h(viewport.height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(u(24.))
                        // `bg-black/85 backdrop-blur-sm`.
                        .child(glass_backdrop(0., 4., gpui::black().opacity(0.85)))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_in_window(window, cx);
                            }),
                        )
                        .child(
                            div()
                                .debug_selector(|| "image-lightbox-image".into())
                                .max_w_full()
                                .max_h_full()
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    img(image)
                                        .max_w_full()
                                        .max_h_full()
                                        .map(|el| match lightbox_size(dimensions, window) {
                                            Some((width, height)) => el.w(width).h(height),
                                            None => el,
                                        })
                                        .object_fit(ObjectFit::Contain)
                                        .shadow_2xl(),
                                ),
                        )
                        .child(
                            div()
                                .id("image-lightbox-close")
                                .absolute()
                                .right(u(16.))
                                .top(u(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(u(36.))
                                .rounded_full()
                                .border_1()
                                .border_color(white.opacity(0.15))
                                .bg(gpui::black().opacity(0.45))
                                .shadow_lg()
                                .cursor_pointer()
                                .hover(|s| s.bg(gpui::black().opacity(0.65)))
                                .tooltip(tooltip("Close"))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_in_window(window, cx);
                                }))
                                .child(
                                    icon(IconName::X)
                                        .size(u(16.))
                                        .text_color(white.opacity(0.8)),
                                ),
                        )
                        .child(
                            // `aria-label`: the image's description, kept for
                            // assistive tech once GPUI exposes it.
                            div().absolute().size_0().overflow_hidden().child(alt),
                        ),
                ),
        )
        .with_priority(layer)
        .into_any_element()
    }

    fn render_thumbnail(
        &self,
        source: Option<gpui::ImageSource>,
        dimensions: Option<(u32, u32)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let lightbox = self
            .open
            .then(|| source.clone())
            .flatten()
            .map(|image| self.lightbox(image, dimensions, window, cx));
        let thumbnail = div()
            .id("attachment-image")
            .debug_selector(|| "attachment-image".into())
            .flex_none()
            .size(u(36.))
            .overflow_hidden()
            .rounded(u(8.))
            .bg(theme.content(0.1))
            .tooltip(tooltip(format!("Open {} full screen", self.meta.name)))
            .map(|element| match source {
                Some(source) => element
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.open(window, cx);
                    }))
                    .child(img(source).size_full().object_fit(ObjectFit::Cover)),
                None => element.child(icon(IconName::ImagePlus).size(u(20.))),
            });
        div().child(thumbnail).children(lightbox).into_any_element()
    }
}

fn attachment_meta(file: &Attachment) -> GeneratedImageMeta {
    GeneratedImageMeta {
        path: file.path.clone().unwrap_or_default(),
        name: file.name.clone(),
        mime_type: file.mime_type.clone(),
        size: file.size,
        alt: Some(file.name.clone()),
        extra: Default::default(),
    }
}

enum AttachmentPreview {
    Bytes(LoadedImage),
    Url(String),
}

fn load_attachment_preview(file: &Attachment) -> Result<AttachmentPreview, String> {
    use base64::Engine as _;
    if file.kind != AttachmentKind::Image {
        return Err("not an image".into());
    }
    if let Some(url) = file.preview_url.as_deref().filter(|url| !url.is_empty()) {
        if let Some(data) = url.strip_prefix("data:") {
            let (header, bytes) = data.split_once(',').ok_or("invalid image URL")?;
            if !header.ends_with(";base64") {
                return Err("invalid image URL".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(bytes)
                .map_err(|error| error.to_string())?;
            return load_image_bytes(bytes).map(AttachmentPreview::Bytes);
        }
        return Ok(AttachmentPreview::Url(url.into()));
    }
    if let Some(data) = file.data.as_deref().filter(|data| !data.is_empty()) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| error.to_string())?;
        return load_image_bytes(bytes).map(AttachmentPreview::Bytes);
    }
    file.path
        .as_deref()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "missing image preview".into())
        .and_then(|path| load_image(std::path::Path::new(path)))
        .map(AttachmentPreview::Bytes)
}

/// `max-h-[min(70vh,640px)] max-w-full`: the image at its own size, scaled
/// down to the height cap. A width past the column letterboxes.
fn display_size(
    dimensions: Option<(u32, u32)>,
    max_height: gpui::Pixels,
    window: &Window,
) -> Option<(gpui::Pixels, gpui::Pixels)> {
    let (width, height) = dimensions.filter(|(w, h)| *w > 0 && *h > 0)?;
    // One image pixel is one CSS pixel.
    let scale = f32::from(window.rem_size()) / 16.;
    let natural_height = height as f32 * scale;
    let shown_height = natural_height.min(f32::from(max_height));
    let shown_width = width as f32 * scale * shown_height / natural_height;
    Some((px(shown_width), px(shown_height)))
}

/// `max-h-full max-w-full` inside the `p-6` lightbox: the natural size,
/// scaled down to fit.
fn lightbox_size(
    dimensions: Option<(u32, u32)>,
    window: &Window,
) -> Option<(gpui::Pixels, gpui::Pixels)> {
    let (width, height) = dimensions.filter(|(w, h)| *w > 0 && *h > 0)?;
    let scale = f32::from(window.rem_size()) / 16.;
    let viewport = window.viewport_size();
    let pad = 48. * scale;
    let (natural_w, natural_h) = (width as f32 * scale, height as f32 * scale);
    let fit = (1f32)
        .min((f32::from(viewport.width) - pad) / natural_w)
        .min((f32::from(viewport.height) - pad) / natural_h)
        .max(0.);
    Some((px(natural_w * fit), px(natural_h * fit)))
}

impl Focusable for GeneratedImage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GeneratedImage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.thumbnail {
            let (source, dimensions) = match &self.state {
                GeneratedImageState::Ready {
                    image, dimensions, ..
                } => (Some(image.clone().into()), *dimensions),
                GeneratedImageState::PreviewUrl(url) => (
                    Some(gpui::ImageSource::from(gpui::SharedString::from(
                        url.clone(),
                    ))),
                    None,
                ),
                _ => (None, None),
            };
            return self.render_thumbnail(source, dimensions, window, cx);
        }
        let theme = Theme::of(cx).clone();
        match self.state.clone() {
            GeneratedImageState::Loading => div()
                .px(u(16.))
                .py(u(12.))
                .font_family(theme.fonts.sans.clone())
                .text_px(12.)
                .line_height(u(16.))
                .text_color(theme.content(0.45))
                .child("Loading generated image\u{2026}")
                .into_any_element(),
            GeneratedImageState::Error | GeneratedImageState::PreviewUrl(_) => div()
                .px(u(16.))
                .py(u(12.))
                .font_family(theme.fonts.sans.clone())
                .text_px(12.)
                .line_height(u(16.))
                .text_color(theme.content(0.5))
                .child("Could not open generated image.")
                .into_any_element(),
            GeneratedImageState::Ready {
                image,
                size,
                dimensions,
            } => {
                let name = self.meta.name.clone();
                let lightbox = self
                    .open
                    .then(|| self.lightbox(image.clone(), dimensions, window, cx));
                // `max-h-[min(70vh,640px)]`.
                let max_height =
                    (window.viewport_size().height * 0.7).min(u(640.).to_pixels(window.rem_size()));
                div()
                    .min_w_0()
                    .px(u(16.))
                    .pb(u(12.))
                    .pt(u(12.))
                    // The frame hugs the image, as the React button did.
                    .flex()
                    .flex_col()
                    .items_start()
                    .font_family(theme.fonts.sans.clone())
                    .child(
                        div()
                            .id("generated-image")
                            .flex()
                            .max_w_full()
                            .overflow_hidden()
                            .rounded(u(12.))
                            .border_1()
                            .border_color(theme.content(0.1))
                            .bg(theme.content(0.05))
                            .cursor_pointer()
                            .tooltip(tooltip(format!("Open {name} full screen")))
                            .on_click(cx.listener(|this, _, window, cx| this.open(window, cx)))
                            .child(
                                img(image)
                                    .max_w_full()
                                    .max_h(max_height)
                                    .map(|el| match display_size(dimensions, max_height, window) {
                                        Some((width, height)) => el.w(width).h(height),
                                        None => el,
                                    })
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(8.))
                            .text_px(11.)
                            .line_height(u(16.))
                            .text_color(theme.content(0.45))
                            .child(div().min_w_0().truncate().child(name))
                            .child(format_file_size(size)),
                    )
                    .when_some(lightbox, |el, lightbox| el.child(lightbox))
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_image_types_from_their_magic_bytes() {
        assert_eq!(
            sniff_image_mime(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0]),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(
            sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_image_mime(b"\0\0\0\x1cftypavif"), Some("image/avif"));
        assert_eq!(sniff_image_mime(b"\0\0\0\x1cftypmp42"), None);
        assert_eq!(sniff_image_mime(b"hello"), None);
        assert_eq!(sniff_image_mime(&[]), None);
    }

    #[test]
    fn reads_the_size_from_the_header() {
        let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(image_dimensions(&png), Some((640, 480)));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0x20, 0x00, 0x10, 0x00]);
        assert_eq!(image_dimensions(&gif), Some((32, 16)));
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00,
            0x64, 0x00, 0xc8, 0x03,
        ];
        assert_eq!(image_dimensions(&jpeg), Some((200, 100)));
        assert_eq!(image_dimensions(b"nope"), None);
    }

    #[test]
    fn formats_sizes_like_a_file_manager() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
        assert_eq!(format_file_size(20 * 1024), "20 KB");
        assert_eq!(format_file_size(20 * 1024 * 1024), "20 MB");
        assert_eq!(format_file_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn a_missing_or_foreign_file_is_an_error() {
        assert!(load_image(std::path::Path::new("/nonexistent/image.png")).is_err());
        let dir = std::env::temp_dir().join(format!("mc-cards-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "not an image").unwrap();
        assert_eq!(load_image(&path).err().as_deref(), Some("not an image"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generated_avif_images_load_with_original_file_size_and_dimensions() {
        let bytes = include_bytes!("../../../editor/tests/fixtures/red-blue.avif");
        let path =
            std::env::temp_dir().join(format!("mc-generated-image-{}.avif", uuid::Uuid::new_v4()));
        std::fs::write(&path, bytes).unwrap();
        let loaded = load_image(&path);
        std::fs::remove_file(&path).unwrap();
        let (_, size, dimensions) = loaded.unwrap();
        assert_eq!(size, bytes.len() as i64);
        assert_eq!(dimensions, Some((32, 16)));
    }

    #[test]
    fn remote_attachment_bytes_do_not_require_the_hosts_file_path() {
        use base64::Engine as _;
        let bytes = include_bytes!("../../../editor/tests/fixtures/red-blue.avif");
        let file = Attachment {
            kind: AttachmentKind::Image,
            mime_type: "image/avif".into(),
            path: Some("/missing/remote-host/image.avif".into()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            ..Attachment::default()
        };
        let AttachmentPreview::Bytes((_, size, dimensions)) =
            load_attachment_preview(&file).unwrap()
        else {
            panic!("inline attachment bytes should load");
        };
        assert_eq!(size, bytes.len() as i64);
        assert_eq!(dimensions, Some((32, 16)));
    }

    #[test]
    fn a_preview_data_url_takes_precedence_over_the_inline_payload() {
        use base64::Engine as _;
        let bytes = include_bytes!("../../../editor/tests/fixtures/red-blue.avif");
        let file = Attachment {
            kind: AttachmentKind::Image,
            preview_url: Some(format!(
                "data:image/avif;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )),
            data: Some("invalid base64".into()),
            ..Attachment::default()
        };
        let AttachmentPreview::Bytes((_, _, dimensions)) = load_attachment_preview(&file).unwrap()
        else {
            panic!("the preview URL should load");
        };
        assert_eq!(dimensions, Some((32, 16)));
    }
}
