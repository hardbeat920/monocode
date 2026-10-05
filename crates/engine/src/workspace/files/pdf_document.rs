//! Port of the sizing rule in src/features/files/model/pdfDocument.ts.
//! Loading pdf.js has no port; the PDF viewer crate renders pages itself.

/// `MAX_CANVAS_AREA`: the largest bitmap a page draws into, in pixels.
pub const MAX_CANVAS_AREA: f64 = 4096.0 * 4096.0;
/// `MAX_CANVAS_SIDE`.
pub const MAX_CANVAS_SIDE: f64 = 8192.0;

/// `renderPixelRatio`: device pixels per logical pixel for a page drawn
/// `width` by `height`. It is the display's ratio unless that would push
/// the bitmap past a limit, and then the page draws at lower resolution.
pub fn render_pixel_ratio(width: f64, height: f64, device_pixel_ratio: f64) -> f64 {
    if width <= 0.0 || height <= 0.0 {
        return device_pixel_ratio;
    }
    device_pixel_ratio
        .min((MAX_CANVAS_AREA / (width * height)).sqrt())
        .min(MAX_CANVAS_SIDE / width)
        .min(MAX_CANVAS_SIDE / height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_display_ratio_when_the_bitmap_fits() {
        assert_eq!(render_pixel_ratio(612.0, 792.0, 2.0), 2.0);
    }

    #[test]
    fn lowers_the_ratio_to_keep_a_zoomed_page_within_the_area_limit() {
        let width = 612.0 * 16.0;
        let height = 792.0 * 16.0;
        let ratio = render_pixel_ratio(width, height, 2.0);
        assert!(ratio < 2.0);
        assert!(width * ratio * height * ratio <= MAX_CANVAS_AREA * 1.0001);
    }

    #[test]
    fn lowers_the_ratio_to_keep_a_long_narrow_page_within_the_side_limit() {
        let ratio = render_pixel_ratio(200.0, 9000.0, 2.0);
        assert!(9000.0 * ratio <= MAX_CANVAS_SIDE);
    }
}
