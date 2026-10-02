//! Shared pieces of the image and PDF viewers.
//!
//! Ports of src/features/files/model/filePreview.ts (`isImagePath`,
//! `isPdfPath`, `isPdfBytes`, `sniffImageMime`, `formatFileSize`),
//! `renderPixelRatio` in pdfDocument.ts, and `clampZoom` and `ZoomButton` in
//! src/features/files/ui/ViewerControls.tsx.

use gpui::{
    App, Bounds, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, SharedString,
    Stateful, Styled, Window, canvas, div, fill, point, prelude::FluentBuilder as _, px, size,
};

use crate::{
    icons::{IconKind, icon},
    language::basename,
    theme::EditorTheme,
};

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 16.;

/// `MAX_CANVAS_AREA` and `MAX_CANVAS_SIDE`: the largest bitmap a page renders to.
pub const MAX_CANVAS_AREA: f32 = 4096. * 4096.;
pub const MAX_CANVAS_SIDE: f32 = 8192.;

/// `"fit"` or a fixed scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    Fit,
    Scale(f32),
}

impl Zoom {
    /// The footer label: `Fit` or a percentage.
    pub fn label(self) -> String {
        match self {
            Self::Fit => "Fit".into(),
            Self::Scale(scale) => format!("{}%", (scale * 100.).round()),
        }
    }
}

/// `clampZoom`.
pub fn clamp_zoom(value: f32) -> f32 {
    value.clamp(MIN_ZOOM, MAX_ZOOM)
}

const IMAGE_EXTENSIONS: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".bmp", ".ico",
];

fn extension_of(path: &str) -> String {
    let name = basename(path).to_lowercase();
    name.rfind('.')
        .map_or(String::new(), |index| name[index..].to_owned())
}

/// `isImagePath`. SVG stays with the editor.
pub fn is_image_path(path: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension_of(path).as_str())
}

/// `isPdfPath`.
pub fn is_pdf_path(path: &str) -> bool {
    extension_of(path) == ".pdf"
}

/// `isPdfBytes`: `%PDF-` anywhere in the first 1024 bytes.
pub fn is_pdf_bytes(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(5).any(|window| window == b"%PDF-")
}

/// `sniffImageMime`: the MIME type from the magic number, never the name.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts =
        |offset: usize, magic: &[u8]| bytes.get(offset..offset + magic.len()) == Some(magic);
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
    if starts(0, b"RIFF") && starts(8, b"WEBP") {
        return Some("image/webp");
    }
    if starts(4, b"ftyp") && (starts(8, b"avif") || starts(8, b"avis")) {
        return Some("image/avif");
    }
    None
}

/// `formatFileSize`.
pub fn format_file_size(bytes: u64) -> String {
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

/// `renderPixelRatio`: the device ratio, lowered so a page bitmap stays
/// within [`MAX_CANVAS_AREA`] and [`MAX_CANVAS_SIDE`].
pub fn render_pixel_ratio(width: f32, height: f32, device_pixel_ratio: f32) -> f32 {
    if width <= 0. || height <= 0. {
        return device_pixel_ratio;
    }
    device_pixel_ratio
        .min((MAX_CANVAS_AREA / (width * height)).sqrt())
        .min(MAX_CANVAS_SIDE / width)
        .min(MAX_CANVAS_SIDE / height)
}

/// `ZoomButton`.
pub(crate) fn zoom_button(
    id: &'static str,
    kind: IconKind,
    theme: &EditorTheme,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .hover(|this| this.bg(theme.hover))
        .child(icon(kind, px(12.), theme.content(0.6)))
}

/// The viewer footer: facts on the left, controls on the right.
pub(crate) fn footer(theme: &EditorTheme, facts: Vec<SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .h(px(32.))
        .items_center()
        .gap(px(12.))
        .border_t_1()
        .border_color(theme.stroke)
        .px(px(12.))
        .font_family(theme.ui_font.clone())
        .text_size(px(11.))
        .text_color(theme.content(0.5))
        .children(facts.into_iter().map(|fact| div().child(fact)))
        .child(div().flex_1())
}

/// The transparency checkerboard behind images: 8px squares at 10% gray.
pub(crate) fn checkerboard() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let cell = px(8.);
            let color = Hsla {
                h: 0.,
                s: 0.,
                l: 0.5,
                a: 0.10,
            };
            let columns = (bounds.size.width / cell).ceil() as usize;
            let rows = (bounds.size.height / cell).ceil() as usize;
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                for row in 0..rows {
                    for column in (row % 2..columns).step_by(2) {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + cell * column as f32,
                                    bounds.top() + cell * row as f32,
                                ),
                                size(cell, cell),
                            ),
                            color,
                        ));
                    }
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A centered message card, like `FileCard` and the loading states.
pub(crate) fn message(
    theme: &EditorTheme,
    title: impl Into<SharedString>,
    detail: Option<String>,
) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .p(px(24.))
        .font_family(theme.ui_font.clone())
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.foreground)
                .child(title.into()),
        )
        .when_some(detail, |this, detail| {
            this.child(
                div()
                    .max_w(px(448.))
                    .text_size(px(12.))
                    .text_color(theme.content(0.5))
                    .child(detail),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_images_and_pdfs_by_extension() {
        assert!(is_image_path("/a/photo.PNG"));
        assert!(is_image_path("x.avif"));
        assert!(!is_image_path("logo.svg"));
        assert!(!is_image_path("Makefile"));
        assert!(is_pdf_path("/docs/Guide.pdf"));
        assert!(!is_pdf_path("pdf"));
    }

    #[test]
    fn finds_the_pdf_header_at_the_start_or_within_the_first_1024_bytes() {
        assert!(is_pdf_bytes(b"%PDF-1.7\n"));
        assert!(is_pdf_bytes(
            format!("{}%PDF-1.4", " ".repeat(500)).as_bytes()
        ));
        assert!(!is_pdf_bytes(
            format!("{}%PDF-1.4", " ".repeat(1024)).as_bytes()
        ));
        assert!(!is_pdf_bytes(b"<html>%PD"));
        assert!(!is_pdf_bytes(b""));
    }

    #[test]
    fn identifies_each_supported_format_by_magic_number() {
        assert_eq!(
            sniff_image_mime(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(
            sniff_image_mime(&[0x47, 0x49, 0x46, 0x38, 0x39, 0x61]),
            Some("image/gif")
        );
        assert_eq!(
            sniff_image_mime(&[0x52, 0x49, 0x46, 0x46, 1, 2, 3, 4, 0x57, 0x45, 0x42, 0x50]),
            Some("image/webp")
        );
        assert_eq!(
            sniff_image_mime(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66
            ]),
            Some("image/avif")
        );
    }

    #[test]
    fn refuses_content_that_is_not_a_supported_image() {
        assert_eq!(sniff_image_mime(b"<html><script>x()</script>"), None);
        assert_eq!(sniff_image_mime(b"%PDF-1.7"), None);
        assert_eq!(sniff_image_mime(&[0x89, 0x50]), None);
        assert_eq!(sniff_image_mime(&[]), None);
        assert_eq!(
            sniff_image_mime(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d
            ]),
            None
        );
    }

    #[test]
    fn scales_to_the_largest_unit_that_keeps_a_leading_digit() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(900), "900 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
        assert_eq!(format_file_size(48 * 1024), "48 KB");
        assert_eq!(format_file_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(format_file_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn keeps_the_display_ratio_when_the_bitmap_fits() {
        assert_eq!(render_pixel_ratio(612., 792., 2.), 2.);
    }

    #[test]
    fn lowers_the_ratio_to_keep_a_zoomed_page_within_the_area_limit() {
        let (width, height) = (612. * 16., 792. * 16.);
        let ratio = render_pixel_ratio(width, height, 2.);
        assert!(ratio < 2.);
        assert!(width * ratio * height * ratio <= MAX_CANVAS_AREA * 1.0001);
    }

    #[test]
    fn lowers_the_ratio_to_keep_a_long_narrow_page_within_the_side_limit() {
        let ratio = render_pixel_ratio(200., 9000., 2.);
        assert!(9000. * ratio <= MAX_CANVAS_SIDE);
    }

    #[test]
    fn zoom_labels() {
        assert_eq!(Zoom::Fit.label(), "Fit");
        assert_eq!(Zoom::Scale(1.5).label(), "150%");
        assert_eq!(clamp_zoom(100.), MAX_ZOOM);
        assert_eq!(clamp_zoom(0.), MIN_ZOOM);
    }
}
