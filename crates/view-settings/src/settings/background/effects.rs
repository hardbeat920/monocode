//! Port of src/features/settings/model/newThreadBackgroundEffects.worker.ts:
//! the pixel work behind the chat background effects.
//!
//! `load_source` decodes and scales an image the way the worker's
//! `loadSource` drew it onto an OffscreenCanvas. `render` runs one effect
//! over the scaled pixels. The webview drew Haze with CSS filters and masks
//! (`.gradient-blur-background` in index.css); GPUI has neither, so
//! `haze_pixels` paints the same layers into the image instead.
//!
//! Everything here is plain computation. `service.rs` runs it on GPUI's
//! background threads.

use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use monocode_core::appearance::NewThreadBackgroundEffect;
use monocode_core::js;

/// The longest side a background keeps after loading.
pub const MAX_SOURCE_SIDE: f64 = 2048.0;

/// `Source`: straight (not premultiplied) RGBA pixels and their luma.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub luma: Vec<u8>,
}

/// `Uint8ClampedArray` assignment: clamp to 0..=255 and round half to even.
fn clamp_u8(value: f64) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(0.0, 255.0).round_ties_even() as u8
}

impl Source {
    /// A source from raw RGBA pixels, computing luma the way `loadSource` did.
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let count = (width * height) as usize;
        let mut luma = vec![0u8; count];
        for (pixel, value) in luma.iter_mut().enumerate() {
            let offset = pixel * 4;
            *value = js::round(
                pixels[offset] as f64 * 0.2126
                    + pixels[offset + 1] as f64 * 0.7152
                    + pixels[offset + 2] as f64 * 0.0722,
            ) as u8;
        }
        Self {
            width,
            height,
            pixels,
            luma,
        }
    }

    /// `coverIndex`.
    fn cover_index(&self, x: f64, y: f64) -> usize {
        let source_x = (x.floor().max(0.0) as u32).min(self.width - 1);
        let source_y = (y.floor().max(0.0) as u32).min(self.height - 1);
        (source_y * self.width + source_x) as usize
    }

    /// `sampleCover`.
    fn sample_cover(&self, x: f64, y: f64) -> u8 {
        self.luma[self.cover_index(x, y)]
    }
}

/// `loadSource`: decode `bytes`, scale the first frame to fit 2048px, and
/// compute luma.
pub fn load_source(bytes: &[u8]) -> Result<Source, String> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|_| "Unable to prepare the background image.".to_string())?;
    Ok(source_from_image(decoded))
}

/// The scaling half of `loadSource`, for an image already decoded.
pub fn source_from_image(decoded: DynamicImage) -> Source {
    let (decoded_width, decoded_height) = (decoded.width() as f64, decoded.height() as f64);
    let scale = 1f64
        .min(MAX_SOURCE_SIDE / decoded_width)
        .min(MAX_SOURCE_SIDE / decoded_height);
    let width = (js::round(decoded_width * scale) as u32).max(1);
    let height = (js::round(decoded_height * scale) as u32).max(1);
    let rgba = decoded.to_rgba8();
    let rgba = if (width, height) == rgba.dimensions() {
        rgba
    } else {
        image::imageops::resize(&rgba, width, height, FilterType::Triangle)
    };
    Source::from_rgba(width, height, rgba.into_raw())
}

/// `ditherColor`.
fn dither_color(r: u8, g: u8, b: u8, a: u8, threshold: u8) -> [u8; 4] {
    let peak = r.max(g).max(b) as f64;
    let bright = peak / 255.0 > (threshold as f64 + 0.5) / 16.0;
    let gain = if bright { 255.0 / peak.max(1.0) } else { 0.08 };
    [
        js::round(r as f64 * gain) as u8,
        js::round(g as f64 * gain) as u8,
        js::round(b as f64 * gain) as u8,
        a,
    ]
}

/// `nonePixels`.
pub fn none_pixels(source: &Source) -> Vec<u8> {
    source.pixels.clone()
}

/// `ditherPixels`: an ordered 4x4 Bayer dither over 2x2 cells.
pub fn dither_pixels(source: &Source) -> Vec<u8> {
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let (width, height) = (source.width as usize, source.height as usize);
    let mut output = vec![0u8; source.pixels.len()];
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let sx = (x + 1).min(width - 1);
            let sy = (y + 1).min(height - 1);
            let base = source.cover_index(sx as f64, sy as f64) * 4;
            let color = dither_color(
                source.pixels[base],
                source.pixels[base + 1],
                source.pixels[base + 2],
                source.pixels[base + 3],
                BAYER[(y / 2) % 4][(x / 2) % 4],
            );
            for dy in 0..2.min(height - y) {
                for dx in 0..2.min(width - x) {
                    let at = ((y + dy) * width + x + dx) * 4;
                    output[at..at + 4].copy_from_slice(&color);
                }
            }
        }
    }
    output
}

/// The 5x7 glyphs `asciiPixels` draws, from sparse to dense.
const GLYPHS: [[u8; 7]; 10] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 4, 0],
    [0, 4, 0, 0, 4, 0, 0],
    [0, 0, 0, 14, 0, 0, 0],
    [0, 0, 14, 0, 14, 0, 0],
    [0, 4, 4, 31, 4, 4, 0],
    [0, 21, 14, 31, 14, 21, 0],
    [10, 10, 31, 10, 31, 10, 10],
    [17, 2, 4, 4, 8, 16, 17],
    [14, 17, 23, 21, 23, 16, 14],
];

/// `asciiPixels`: colored characters in 6x8 cells over black (or white in
/// light mode), blended 40% over the artwork.
pub fn ascii_pixels(source: &Source, light: bool) -> Vec<u8> {
    let (width, height) = (source.width as usize, source.height as usize);
    let mut output = vec![0u8; source.pixels.len()];
    let paper = if light { 255.0 } else { 0.0 };
    for y in 0..height {
        for x in 0..width {
            let sx = ((x / 6) * 6 + 3).min(width - 1);
            let sy = ((y / 8) * 8 + 4).min(height - 1);
            let sample = source.cover_index(sx as f64, sy as f64);
            let ink_density = if light {
                255 - source.luma[sample]
            } else {
                source.luma[sample]
            } as f64;
            let glyph = GLYPHS[((ink_density / 255.0).sqrt() * 9.0).floor() as usize];
            // The row index is below 7 whenever the cell test passes.
            let ink = x % 6 < 5 && y % 8 < 7 && (glyph[y % 8] & (1 << (4 - (x % 6)))) != 0;
            let pixel = (y * width + x) * 4;
            let sampled = sample * 4;
            for color in 0..3 {
                let texture = if ink {
                    source.pixels[sampled + color] as f64
                } else {
                    paper
                };
                output[pixel + color] =
                    clamp_u8(source.pixels[pixel + color] as f64 * 0.6 + texture * 0.4);
            }
            output[pixel + 3] = source.pixels[pixel + 3];
        }
    }
    output
}

/// `halftonePixels`: a dot per 4x4 cell, sized by brightness (or darkness in
/// light mode), blended 40% over the artwork.
pub fn halftone_pixels(source: &Source, light: bool) -> Vec<u8> {
    let (width, height) = (source.width as usize, source.height as usize);
    let paper_byte = if light { 255 } else { 0 };
    let paper = paper_byte as f64;
    let mut output = vec![paper_byte; source.pixels.len()];
    for alpha in (3..output.len()).step_by(4) {
        output[alpha] = 255;
    }
    for y in (0..height).step_by(4) {
        for x in (0..width).step_by(4) {
            let sampled_luma = source.sample_cover(x as f64, y as f64);
            let luma = if light {
                255 - sampled_luma
            } else {
                sampled_luma
            } as f64;
            let radius = 2.0 * (0.3 + 0.7 * (luma / 255.0).sqrt());
            let sx = (x + 2).min(width - 1);
            let sy = (y + 2).min(height - 1);
            let dot = source.cover_index(sx as f64, sy as f64) * 4;
            for dy in 0..4.min(height - y) {
                for dx in 0..4.min(width - x) {
                    let distance = (dx as f64 - 1.5).hypot(dy as f64 - 1.5);
                    let source_pixel = ((y + dy) * width + x + dx) * 4;
                    let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0)
                        * (source.pixels[dot + 3] as f64 / 255.0);
                    for color in 0..3 {
                        let texture =
                            source.pixels[dot + color] as f64 * coverage + paper * (1.0 - coverage);
                        output[source_pixel + color] = clamp_u8(
                            source.pixels[source_pixel + color] as f64 * 0.6 + texture * 0.4,
                        );
                    }
                    output[source_pixel + 3] = source.pixels[source_pixel + 3];
                }
            }
        }
    }
    output
}

/// `scanlinePixels`: every third row at 52% (or lifted toward white in light
/// mode).
pub fn scanline_pixels(source: &Source, light: bool) -> Vec<u8> {
    let (width, height) = (source.width as usize, source.height as usize);
    let mut output = vec![0u8; source.pixels.len()];
    for y in 0..height {
        let gain = if y % 3 == 0 { 0.52 } else { 1.0 };
        for x in 0..width {
            let pixel = (y * width + x) * 4;
            for color in 0..3 {
                let value = source.pixels[pixel + color] as f64;
                output[pixel + color] = clamp_u8(if light {
                    value + (255.0 - value) * (1.0 - gain)
                } else {
                    value * gain
                });
            }
            output[pixel + 3] = source.pixels[pixel + 3];
        }
    }
    output
}

/// The theme key an effect's output depends on: None and Dither look the
/// same in both schemes.
pub fn theme_key(effect: NewThreadBackgroundEffect, light: bool) -> bool {
    match effect {
        NewThreadBackgroundEffect::None | NewThreadBackgroundEffect::Dither => false,
        _ => light,
    }
}

/// `render`'s pixel step for the effects the worker drew. Haze has its own
/// entry point because it also needs the theme color.
pub fn render_pixels(source: &Source, effect: NewThreadBackgroundEffect, light: bool) -> Vec<u8> {
    match effect {
        NewThreadBackgroundEffect::None => none_pixels(source),
        NewThreadBackgroundEffect::Dither => dither_pixels(source),
        NewThreadBackgroundEffect::Ascii => ascii_pixels(source, light),
        NewThreadBackgroundEffect::Halftone => halftone_pixels(source, light),
        // The worker treated every other value as scanlines.
        NewThreadBackgroundEffect::Scanlines | NewThreadBackgroundEffect::GradientBlur => {
            scanline_pixels(source, light)
        }
    }
}

/// Where a Haze image is drawn. Each place in index.css sets its own blur
/// strengths and masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HazeVariant {
    /// `.gradient-blur-preview`: the settings and project dialog previews.
    Preview,
    /// `.gradient-blur-background` behind an empty session.
    Empty,
    /// `[data-session-empty="false"]`: behind a conversation with messages.
    Session,
}

/// A CSS `linear-gradient(to bottom, ...)` of alpha stops, as (position,
/// alpha) pairs with positions in 0..=1.
type AlphaStops = &'static [(f64, f64)];

struct HazeLayers {
    /// `--gradient-blur-soft` and `--gradient-blur-strong`, in CSS px.
    soft: f64,
    strong: f64,
    soft_mask: AlphaStops,
    strong_mask: AlphaStops,
    /// `--gradient-blur-overlay`: background color strength by height.
    overlay: AlphaStops,
    /// `--gradient-blur-mask`: the whole layer's alpha by height.
    mask: AlphaStops,
}

fn haze_layers(variant: HazeVariant) -> HazeLayers {
    const SOFT_MASK: AlphaStops = &[(0.18, 0.0), (0.60, 1.0)];
    const STRONG_MASK: AlphaStops = &[(0.28, 0.0), (0.76, 1.0)];
    const OVERLAY: AlphaStops = &[(0.16, 0.0), (0.38, 0.26), (0.55, 0.55), (0.75, 0.78)];
    const MASK: AlphaStops = &[
        (0.0, 1.0),
        (0.19, 1.0),
        (0.30, 0.94),
        (0.42, 0.72),
        (0.54, 0.42),
        (0.66, 0.16),
        (0.78, 0.04),
        (0.90, 0.0),
    ];
    match variant {
        HazeVariant::Preview => HazeLayers {
            soft: 2.0,
            strong: 5.0,
            soft_mask: SOFT_MASK,
            strong_mask: STRONG_MASK,
            overlay: OVERLAY,
            mask: MASK,
        },
        HazeVariant::Empty => HazeLayers {
            soft: 7.0,
            strong: 18.0,
            soft_mask: SOFT_MASK,
            strong_mask: STRONG_MASK,
            overlay: OVERLAY,
            mask: MASK,
        },
        HazeVariant::Session => HazeLayers {
            soft: 5.0,
            strong: 12.0,
            soft_mask: &[(0.18, 0.0), (0.70, 1.0)],
            strong_mask: &[(0.22, 0.0), (0.76, 1.0)],
            overlay: &[(0.15, 0.0), (0.76, 0.70)],
            mask: &[(0.0, 1.0), (0.18, 1.0), (0.76, 0.0)],
        },
    }
}

/// The value of a CSS gradient at `t` (0 at the top): the first and last
/// stops extend to the edges, and values in between interpolate linearly.
pub fn gradient_at(stops: &[(f64, f64)], t: f64) -> f64 {
    let Some(first) = stops.first() else {
        return 0.0;
    };
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            if b.0 <= a.0 {
                return b.1;
            }
            return a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0);
        }
    }
    stops[stops.len() - 1].1
}

/// Box-blur approximation of CSS `blur(radius)`, where the radius is a
/// Gaussian standard deviation in image pixels.
fn blur(source: &RgbaImage, sigma: f64) -> RgbaImage {
    if sigma < 0.5 {
        return source.clone();
    }
    image::imageops::fast_blur(source, sigma as f32)
}

/// `background-size: cover` into a `width` by `height` box: the centered
/// crop of `source` with the box's aspect ratio, resized to `width` by
/// `height` pixels.
pub fn cover(source: &Source, width: u32, height: u32) -> RgbaImage {
    let image = RgbaImage::from_raw(source.width, source.height, source.pixels.clone())
        .expect("source pixels match their size");
    let (sw, sh) = (source.width as f64, source.height as f64);
    let aspect = width as f64 / height.max(1) as f64;
    let (cw, ch) = if sw / sh > aspect {
        ((sh * aspect).round().max(1.0), sh)
    } else {
        (sw, (sw / aspect).round().max(1.0))
    };
    let x = ((sw - cw) / 2.0).round() as u32;
    let y = ((sh - ch) / 2.0).round() as u32;
    let cropped = image::imageops::crop_imm(&image, x, y, cw as u32, ch as u32).to_image();
    if cropped.dimensions() == (width, height) {
        cropped
    } else {
        image::imageops::resize(&cropped, width, height, FilterType::Triangle)
    }
}

/// Haze (`gradient-blur`): the artwork with a soft and a strong blurred copy
/// masked in toward the bottom, a background-colored gradient and 1px
/// scanlines over it, and the whole layer faded out from top to bottom.
///
/// CSS drew the layers in the element's box, so the image is first cropped
/// to the box (`cover`) at twice its CSS size, up to 2048px. `background` is
/// `--color-background-base`.
pub fn haze_pixels(
    source: &Source,
    variant: HazeVariant,
    background: [u8; 3],
    display_width: f64,
    display_height: f64,
) -> RgbaImage {
    let layers = haze_layers(variant);
    let (display_width, display_height) = (display_width.max(1.0), display_height.max(1.0));
    let scale = (2.0f64)
        .min(MAX_SOURCE_SIDE / display_width)
        .min(MAX_SOURCE_SIDE / display_height);
    let width = (display_width * scale).round().max(1.0) as u32;
    let height = (display_height * scale).round().max(1.0) as u32;
    let base = cover(source, width, height);
    let soft = blur(&base, layers.soft * scale);
    let strong = blur(&base, layers.strong * scale);
    let background = background.map(|channel| channel as f64);
    let mut output = RgbaImage::new(width, height);
    for y in 0..height {
        let t = (y as f64 + 0.5) / height as f64;
        let soft_alpha = gradient_at(layers.soft_mask, t);
        let strong_alpha = gradient_at(layers.strong_mask, t);
        let overlay_alpha = gradient_at(layers.overlay, t);
        let mask = gradient_at(layers.mask, t);
        // `repeating-linear-gradient(... 36% 0 1px, transparent 1px 3px)`
        // in CSS px, mapped to image rows.
        let css_row = y as f64 / scale;
        let scanline = if css_row.rem_euclid(3.0) < 1.0 {
            0.36
        } else {
            0.0
        };
        for x in 0..width {
            let base_px = base.get_pixel(x, y).0;
            let soft_px = soft.get_pixel(x, y).0;
            let strong_px = strong.get_pixel(x, y).0;
            let mut px = [0u8; 4];
            for channel in 0..3 {
                let mut value = base_px[channel] as f64;
                value += (soft_px[channel] as f64 - value) * soft_alpha;
                value += (strong_px[channel] as f64 - value) * strong_alpha;
                // `::after`: the scanline layer is drawn over the overlay
                // gradient, both in the background color.
                value += (background[channel] - value) * overlay_alpha;
                value += (background[channel] - value) * scanline;
                px[channel] = clamp_u8(value);
            }
            px[3] = clamp_u8(base_px[3] as f64 * mask);
            output.put_pixel(x, y, image::Rgba(px));
        }
    }
    output
}

/// Pixels in RGBA order to an RGBA image the size of `source`.
pub fn to_image(source: &Source, pixels: Vec<u8>) -> RgbaImage {
    RgbaImage::from_raw(source.width, source.height, pixels).expect("pixels match the source size")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Source {
        let pixels = rgba
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect();
        Source::from_rgba(width, height, pixels)
    }

    fn gradient(width: u32, height: u32) -> Source {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    (x * 255 / width.max(1)) as u8,
                    (y * 255 / height.max(1)) as u8,
                    128,
                    255,
                ]);
            }
        }
        Source::from_rgba(width, height, pixels)
    }

    #[test]
    fn computes_luma_with_rec_709_weights() {
        let source = solid(1, 1, [255, 0, 0, 255]);
        assert_eq!(source.luma, vec![54]);
        let source = solid(1, 1, [255, 255, 255, 255]);
        assert_eq!(source.luma, vec![255]);
    }

    #[test]
    fn rounds_clamped_bytes_half_to_even() {
        assert_eq!(clamp_u8(0.5), 0);
        assert_eq!(clamp_u8(1.5), 2);
        assert_eq!(clamp_u8(-4.0), 0);
        assert_eq!(clamp_u8(300.0), 255);
    }

    #[test]
    fn none_keeps_the_artwork() {
        let source = gradient(5, 3);
        assert_eq!(none_pixels(&source), source.pixels);
    }

    #[test]
    fn dither_paints_two_by_two_cells_in_full_or_dim_color() {
        let source = solid(4, 4, [100, 50, 25, 255]);
        let output = dither_pixels(&source);
        // Threshold 0 at the top left: bright, so the peak channel goes to
        // 255. 50 * 2.55 is just under 127.5 in floating point.
        assert_eq!(&output[0..4], &[255, 127, 64, 255]);
        // The same cell repeats across its 2x2 block.
        assert_eq!(&output[4..8], &output[0..4]);
        assert_eq!(&output[16..20], &output[0..4]);
        // Threshold 8 at the next cell: 100/255 is not above 8.5/16, so it
        // dims to 8%.
        assert_eq!(&output[8..12], &[8, 4, 2, 255]);
    }

    #[test]
    fn ascii_blends_glyph_ink_over_the_artwork() {
        let black = solid(12, 16, [0, 0, 0, 255]);
        // No ink density: every pixel is 60% artwork plus 40% black paper.
        assert!(
            ascii_pixels(&black, false)
                .chunks(4)
                .all(|px| px == [0, 0, 0, 255])
        );
        let white = solid(12, 16, [255, 255, 255, 255]);
        let output = ascii_pixels(&white, false);
        // The densest glyph's first row is 14 (0b01110): column 0 is paper.
        assert_eq!(&output[0..4], &[153, 153, 153, 255]);
        // Column 1 is ink, which samples the white artwork.
        assert_eq!(&output[4..8], &[255, 255, 255, 255]);
        // Light mode inverts density: white art means no ink on white paper.
        assert!(
            ascii_pixels(&white, true)
                .chunks(4)
                .all(|px| px == [255, 255, 255, 255])
        );
    }

    #[test]
    fn halftone_draws_dots_on_paper() {
        let white = solid(8, 8, [255, 255, 255, 255]);
        let output = halftone_pixels(&white, false);
        // The cell center is fully covered by the dot.
        let center = (8 + 1) * 4;
        assert_eq!(&output[center..center + 4], &[255, 255, 255, 255]);
        let black = solid(8, 8, [0, 0, 0, 255]);
        // Dark mode paper is black, so a black image stays black.
        assert!(
            halftone_pixels(&black, false)
                .chunks(4)
                .all(|px| px == [0, 0, 0, 255])
        );
        // Light mode paper is white: the corners blend toward it.
        let light = halftone_pixels(&black, true);
        assert_eq!(&light[0..4], &[63, 63, 63, 255]);
    }

    #[test]
    fn scanlines_darken_every_third_row() {
        let source = solid(2, 4, [100, 200, 50, 255]);
        let output = scanline_pixels(&source, false);
        assert_eq!(&output[0..4], &[52, 104, 26, 255]);
        assert_eq!(&output[8..12], &[100, 200, 50, 255]);
        assert_eq!(&output[24..28], &[52, 104, 26, 255]);
        let light = scanline_pixels(&source, true);
        assert_eq!(&light[0..4], &[174, 226, 148, 255]);
    }

    #[test]
    fn only_theme_dependent_effects_key_on_the_scheme() {
        assert!(!theme_key(NewThreadBackgroundEffect::None, true));
        assert!(!theme_key(NewThreadBackgroundEffect::Dither, true));
        assert!(theme_key(NewThreadBackgroundEffect::Ascii, true));
        assert!(!theme_key(NewThreadBackgroundEffect::Ascii, false));
    }

    #[test]
    fn scales_large_images_to_fit_2048() {
        let decoded = DynamicImage::ImageRgba8(RgbaImage::new(4096, 1024));
        let source = source_from_image(decoded);
        assert_eq!((source.width, source.height), (2048, 512));
        assert_eq!(source.luma.len(), 2048 * 512);
        let small = source_from_image(DynamicImage::ImageRgba8(RgbaImage::new(3, 2)));
        assert_eq!((small.width, small.height), (3, 2));
    }

    #[test]
    fn rejects_bytes_that_are_not_an_image() {
        assert_eq!(
            load_source(b"not an image"),
            Err("Unable to prepare the background image.".into())
        );
    }

    #[test]
    fn decodes_png_bytes() {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, image::Rgba([9, 8, 7, 255])))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let source = load_source(&bytes).unwrap();
        assert_eq!(&source.pixels[0..4], &[9, 8, 7, 255]);
    }

    #[test]
    fn gradients_extend_their_end_stops_and_interpolate() {
        let stops = [(0.2, 0.0), (0.6, 1.0)];
        assert_eq!(gradient_at(&stops, 0.0), 0.0);
        assert!((gradient_at(&stops, 0.4) - 0.5).abs() < 1e-9);
        assert_eq!(gradient_at(&stops, 0.9), 1.0);
    }

    #[test]
    fn haze_fades_to_transparent_at_the_bottom_and_tints_toward_the_background() {
        let source = solid(4, 100, [200, 200, 200, 255]);
        let output = haze_pixels(&source, HazeVariant::Empty, [10, 20, 30], 4.0, 100.0);
        // Twice the CSS size.
        assert_eq!(output.dimensions(), (8, 200));
        // The top keeps the artwork and full alpha. CSS row 0 carries a
        // scanline, so read CSS row 1.
        assert_eq!(output.get_pixel(0, 2).0, [200, 200, 200, 255]);
        // The bottom rows are fully masked out.
        assert_eq!(output.get_pixel(0, 199).0[3], 0);
        // Midway down, the overlay pulls the color toward the background.
        assert!(output.get_pixel(0, 122).0[0] < 150);
    }

    #[test]
    fn covers_a_box_with_a_centered_crop() {
        let mut pixels = Vec::new();
        for _y in 0..2 {
            for x in 0..4u8 {
                pixels.extend_from_slice(&[x * 60, 0, 0, 255]);
            }
        }
        let source = Source::from_rgba(4, 2, pixels);
        // A square box keeps the middle two columns.
        let square = cover(&source, 2, 2);
        assert_eq!(square.get_pixel(0, 0).0[0], 60);
        assert_eq!(square.get_pixel(1, 0).0[0], 120);
    }
}
