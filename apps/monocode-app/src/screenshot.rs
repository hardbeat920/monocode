//! `--screenshot`: draw the window, wait for images and fonts to settle, and
//! write what GPUI rendered as a PNG. Built only with `--features screenshot`,
//! because `Window::render_to_image` needs GPUI's `test-support`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::{AnyWindowHandle, App, AsyncApp};
use image::{Rgba, RgbaImage};

/// How long to keep redrawing before the capture. SVG and image assets load
/// on a background thread and show up a frame or two after first paint.
const SETTLE: Duration = Duration::from_millis(900);
const FRAME: Duration = Duration::from_millis(60);

/// Captures `window` once it settles, writes `out`, and quits the app.
pub fn capture_and_quit(
    window: AnyWindowHandle,
    out: PathBuf,
    backdrop: Option<[u8; 3]>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = capture(window, &out, backdrop, cx).await;
        match &result {
            Ok(size) => eprintln!("wrote {} ({}x{})", out.display(), size.0, size.1),
            Err(err) => eprintln!("screenshot failed: {err:#}"),
        }
        let code = if result.is_ok() { 0 } else { 1 };
        cx.update(|cx| cx.quit());
        std::process::exit(code);
    })
    .detach();
}

async fn capture(
    window: AnyWindowHandle,
    out: &Path,
    backdrop: Option<[u8; 3]>,
    cx: &mut AsyncApp,
) -> Result<(u32, u32)> {
    let mut waited = Duration::ZERO;
    while waited < SETTLE {
        cx.background_executor().timer(FRAME).await;
        waited += FRAME;
        window.update(cx, |_, window, cx| {
            park_pointer(window, cx);
            window.refresh();
        })?;
    }
    let (image, scale) = window.update(cx, |_, window, cx| {
        park_pointer(window, cx);
        window.draw(cx).clear();
        let scale = window.scale_factor();
        window.render_to_image().map(|image| (image, scale))
    })??;
    let mut image = image;
    if let Some(color) = backdrop {
        paint_traffic_lights(&mut image, scale);
        composite_over(&mut image, color);
    }
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    image
        .save(out)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(image.dimensions())
}

/// Moves GPUI's idea of the pointer outside the window, so wherever the real
/// cursor sits, the capture shows no hover state.
fn park_pointer(window: &mut gpui::Window, cx: &mut App) {
    window.dispatch_event(
        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
            position: gpui::point(gpui::px(-1000.), gpui::px(-1000.)),
            ..Default::default()
        }),
        cx,
    );
}

/// Composites the (premultiplied) window pixels over a solid backdrop, the
/// way the compositor would over the blurred desktop.
fn composite_over(image: &mut RgbaImage, backdrop: [u8; 3]) {
    for pixel in image.pixels_mut() {
        let Rgba([r, g, b, a]) = *pixel;
        let keep = 255 - a as u32;
        let blend =
            |src: u8, dst: u8| (src as u32 + (dst as u32 * keep + 127) / 255).min(255) as u8;
        *pixel = Rgba([
            blend(r, backdrop[0]),
            blend(g, backdrop[1]),
            blend(b, backdrop[2]),
            255,
        ]);
    }
}

/// AppKit draws the traffic lights outside GPUI's scene, so the capture has
/// none. Paint stand-ins where `TitlebarOptions::traffic_light_position` puts
/// them (12pt in, 14pt buttons 6pt apart, centered in the 40pt bar).
fn paint_traffic_lights(image: &mut RgbaImage, scale: f32) {
    let colors = [[0xff, 0x5f, 0x57], [0xfe, 0xbc, 0x2e], [0x28, 0xc8, 0x40]];
    let radius = 6.0 * scale;
    for (i, color) in colors.iter().enumerate() {
        let cx = (12.0 + 7.0 + i as f32 * 20.0) * scale;
        let cy = 20.0 * scale;
        let (x0, x1) = ((cx - radius - 1.0) as u32, (cx + radius + 1.0) as u32);
        let (y0, y1) = ((cy - radius - 1.0) as u32, (cy + radius + 1.0) as u32);
        for y in y0..=y1.min(image.height() - 1) {
            for x in x0..=x1.min(image.width() - 1) {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let px = image.get_pixel_mut(x, y);
                let a = (coverage * 255.0) as u32;
                // Premultiplied OVER, so the backdrop pass leaves it opaque.
                for (channel, &paint) in px.0.iter_mut().zip(color.iter()) {
                    *channel = ((paint as u32 * a + *channel as u32 * (255 - a)) / 255) as u8;
                }
                px.0[3] = (a + px.0[3] as u32 * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}
