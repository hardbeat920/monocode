//! ParticleText.tsx: reveal the new title while the previous glyphs drift away.
use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, Font, Global, GlobalElementId, Hsla,
    InspectorElementId, LayoutId, Pixels, PlatformTextSystem, RenderGlyphParams, ShapedLine, Size,
    TextAlign, TextRun, Window, div, fill, point, prelude::*, px,
};
use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    sync::Arc,
    time::Instant,
};

const PAD: f32 = 20.;
const STEP: f32 = 1.5;
const DENSITY: f32 = 0.9;
const SWEEP: f32 = 1.1;
const EDGE: f32 = 18.;
const PARTICLE_LIFE: f32 = 0.85;

/// Share GPUI's actual font IDs and rasterizer, including fallback/emoji fonts.
pub(crate) struct GlyphRaster(pub Arc<dyn PlatformTextSystem>);
impl Global for GlyphRaster {}

pub(crate) fn particle_text(text: impl Into<String>) -> impl IntoElement {
    ParticleText {
        text: text.into(),
        space: div().size_full().into_any_element(),
    }
}

struct ParticleText {
    text: String,
    space: AnyElement,
}

#[derive(Clone, PartialEq)]
struct LabelStyle {
    width: Pixels,
    font: Font,
    font_size: Pixels,
    color: Hsla,
}

struct State {
    shown: String,
    style: LabelStyle,
    line: ShapedLine,
    transition: Option<Transition>,
}

struct Transition {
    start: Instant,
    previous: ShapedLine,
    size: Size<Pixels>,
    color: Hsla,
    particles: Vec<Particle>,
}

struct Particle {
    x: f32,
    y: f32,
    alpha: f32,
    vx: f32,
    vy: f32,
    life: f32,
    born: Option<f32>,
}

struct Frame {
    line: ShapedLine,
    previous: Option<(ShapedLine, Size<Pixels>, f32)>,
    particles: Vec<(Bounds<Pixels>, Hsla)>,
}

fn edge_at(elapsed: f32, width: f32) -> f32 {
    let t = (elapsed / SWEEP).clamp(0., 1.);
    let eased = if t < 0.5 {
        4. * t.powi(3)
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    };
    -EDGE + (width + EDGE * 2.) * eased
}

impl Particle {
    fn frame(&mut self, elapsed: f32, edge: f32) -> (bool, Option<(Bounds<Pixels>, f32)>) {
        let born = match self.born {
            Some(born) => born,
            None if self.x > edge => return (true, None),
            None => *self.born.insert(elapsed),
        };
        let t = ((elapsed - born) / self.life).clamp(0., 1.);
        if t >= 1. {
            return (false, None);
        }
        let drift = 1. - (1. - t).powi(3);
        let alpha = (self.alpha * 1.2).min(1.) * (1. - t).powf(1.2);
        let side = px(1.8 * (1. - t * 0.5));
        (
            true,
            Some((
                Bounds::new(
                    point(px(self.x + self.vx * drift), px(self.y + self.vy * drift)),
                    gpui::size(side, side),
                ),
                alpha,
            )),
        )
    }
}

fn fit_text(text: &str, style: &LabelStyle, window: &Window) -> ShapedLine {
    let shape = |text: &str| {
        window.text_system().shape_line(
            text.to_owned().into(),
            style.font_size,
            &[TextRun {
                len: text.len(),
                font: style.font.clone(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        )
    };
    let line = shape(text);
    if line.width() <= style.width {
        return line;
    }
    // Match the reference's measured ellipsis, retaining complete UTF-8 chars.
    let mut end = text.len();
    while end > 0 && shape(&format!("{}…", &text[..end])).width() > style.width {
        end = text[..end].char_indices().next_back().unwrap().0;
    }
    shape(&format!("{}…", text[..end].trim_end()))
}

/// Rasterization happens once per changed title, never on animation frames.
/// Composite glyph alpha before sampling the same 1.5 CSS-pixel grid as canvas.
fn sample_text(
    line: &ShapedLine,
    size: Size<Pixels>,
    scale: f32,
    raster: &dyn PlatformTextSystem,
) -> Vec<Particle> {
    let scale = scale.max(1.);
    let width = (f32::from(size.width) * scale).ceil() as usize;
    let height = (f32::from(size.height) * scale).ceil() as usize;
    let mut alpha = vec![0_f32; width * height];
    let baseline = (size.height - line.ascent - line.descent) / 2. + line.ascent;
    for run in &line.runs {
        for glyph in &run.glyphs {
            let origin =
                point(glyph.position.x, baseline + glyph.position.y).map(|p| f32::from(p) * scale);
            // Use the same quantization as Window::paint_glyph, carrying a
            // rounded fractional origin into the integer bitmap position.
            let quantize = |value: f32, variants: u8| {
                let value = value * f32::from(variants);
                (value.abs() - 0.5).ceil().copysign(value) / f32::from(variants)
            };
            let origin = point(
                quantize(origin.x, gpui::SUBPIXEL_VARIANTS_X),
                quantize(origin.y, gpui::SUBPIXEL_VARIANTS_Y),
            );
            let params = RenderGlyphParams {
                font_id: run.font_id,
                glyph_id: glyph.id,
                font_size: line.font_size,
                subpixel_variant: point(
                    (origin.x.fract() * f32::from(gpui::SUBPIXEL_VARIANTS_X)) as u8,
                    (origin.y.fract() * f32::from(gpui::SUBPIXEL_VARIANTS_Y)) as u8,
                ),
                scale_factor: scale,
                is_emoji: glyph.is_emoji,
                subpixel_rendering: false,
                dilation: 0,
            };
            let Ok(bounds) = raster.glyph_raster_bounds(&params) else {
                continue;
            };
            let Ok((bitmap_size, bytes)) = raster.rasterize_glyph(&params, bounds) else {
                continue;
            };
            let channels = if glyph.is_emoji { 4 } else { 1 };
            let glyph_width = bitmap_size.width.0 as usize;
            for (i, pixel) in bytes.chunks_exact(channels).enumerate() {
                let x = origin.x.floor() as i32 + bounds.origin.x.0 + (i % glyph_width) as i32;
                let y = origin.y.floor() as i32 + bounds.origin.y.0 + (i / glyph_width) as i32;
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let a = f32::from(pixel[channels - 1]) / 255.;
                    let dest = &mut alpha[y as usize * width + x as usize];
                    *dest = a + *dest * (1. - a);
                }
            }
        }
    }
    let mut random = RandomState::new().build_hasher();
    let mut next = || {
        random.write_u8(1);
        (random.finish() >> 40) as f32 / (1_u32 << 24) as f32
    };
    let mut particles = Vec::new();
    let mut y = 0.;
    while y < f32::from(size.height) {
        let mut x = 0.;
        while x < f32::from(size.width) {
            let a = alpha[(y * scale).floor() as usize * width + (x * scale).floor() as usize];
            if a >= 0.25 && next() <= DENSITY {
                let angle = -std::f32::consts::FRAC_PI_2 + (next() - 0.3) * std::f32::consts::PI;
                let speed = 6. + next() * 12.;
                particles.push(Particle {
                    x,
                    y,
                    alpha: a,
                    vx: angle.cos() * speed + 4.,
                    vy: angle.sin() * speed * 0.8,
                    life: PARTICLE_LIFE * (0.7 + next() * 0.5),
                    born: None,
                });
            }
            x += STEP;
        }
        y += STEP;
    }
    particles
}

impl State {
    fn prepare(
        &mut self,
        text: &str,
        style: LabelStyle,
        bounds: Bounds<Pixels>,
        now: Instant,
        window: &mut Window,
        cx: &App,
    ) -> Frame {
        let changed = self.shown != text;
        if changed || self.style != style {
            self.line = fit_text(text, &style, window);
        }
        if changed {
            let previous = std::mem::replace(&mut self.shown, text.to_owned());
            // A second change restarts from the last target, as shownRef does.
            self.transition = None;
            if !cx.reduce_motion() && bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                let previous = fit_text(&previous, &style, window);
                let particles = cx
                    .try_global::<GlyphRaster>()
                    .map_or_else(Vec::new, |raster| {
                        sample_text(
                            &previous,
                            bounds.size,
                            window.scale_factor(),
                            raster.0.as_ref(),
                        )
                    });
                self.transition = Some(Transition {
                    start: now,
                    previous,
                    size: bounds.size,
                    color: style.color,
                    particles,
                });
            }
        }
        self.style = style;
        if cx.reduce_motion() {
            self.transition = None;
        }
        let mut frame = Frame {
            line: self.line.clone(),
            previous: None,
            particles: Vec::new(),
        };
        if let Some(transition) = &mut self.transition {
            let elapsed = now
                .saturating_duration_since(transition.start)
                .as_secs_f32();
            let edge = edge_at(elapsed, transition.size.width.into());
            let mut alive = elapsed < SWEEP;
            for particle in &mut transition.particles {
                let (active, painted) = particle.frame(elapsed, edge);
                alive |= active;
                if let Some((bounds, alpha)) = painted {
                    frame
                        .particles
                        .push((bounds, transition.color.opacity(alpha)));
                }
            }
            if alive {
                frame.previous = Some((transition.previous.clone(), transition.size, edge));
                window.request_animation_frame();
            } else {
                self.transition = None;
            }
        }
        frame
    }
}

impl IntoElement for ParticleText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ParticleText {
    type RequestLayoutState = ();
    type PrepaintState = Frame;
    fn id(&self) -> Option<ElementId> {
        Some("particle-text".into())
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
        (self.space.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Frame {
        let text_style = window.text_style();
        let style = LabelStyle {
            width: bounds.size.width,
            font: text_style.font(),
            font_size: text_style.font_size.to_pixels(window.rem_size()),
            color: text_style.color,
        };
        window.with_element_state(id.unwrap(), |state: Option<State>, window| {
            let mut state = state.unwrap_or_else(|| State {
                shown: self.text.clone(),
                line: fit_text(&self.text, &style, window),
                style: style.clone(),
                transition: None,
            });
            let frame = state.prepare(&self.text, style, bounds, Instant::now(), window, cx);
            (frame, state)
        })
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some((previous, size, edge)) = &frame.previous {
            paint_text(&frame.line, bounds, Some((edge - EDGE, *edge)), window, cx);
            let ghost = Bounds::new(bounds.origin, *size);
            paint_text(
                previous,
                ghost,
                Some((edge + EDGE / 2., edge - EDGE / 2.)),
                window,
                cx,
            );
            // Canvas is a sibling of the masked labels, with 20px of room on
            // every side. Ancestor sidebar/scroll clipping still applies.
            let canvas = ghost.dilate(px(PAD));
            window.paint_layer(canvas, |window| {
                window.with_content_mask(Some(ContentMask { bounds: canvas }), |window| {
                    for (particle, color) in &frame.particles {
                        window.paint_quad(fill(
                            Bounds::new(bounds.origin + particle.origin, particle.size),
                            *color,
                        ));
                    }
                });
            });
        } else {
            paint_text(&frame.line, bounds, None, window, cx);
        }
    }
}

#[cfg(test)]
mod tests;

fn paint_text(
    line: &ShapedLine,
    bounds: Bounds<Pixels>,
    mask: Option<(f32, f32)>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        #[cfg(target_os = "macos")]
        {
            let marker = |begin, window: &mut Window| {
                if let Some((start, end)) = mask {
                    let scale = window.scale_factor();
                    window.paint_layer(bounds, |window| {
                        window.paint_quad(fill(bounds, gpui::black()).border_color(
                            gpui_apple::metal_renderer::text_reveal_marker(
                                begin,
                                start * scale,
                                end * scale,
                            ),
                        ));
                    });
                }
            };
            marker(true, window);
            window.paint_layer(bounds, |window| {
                let _ = line.paint(
                    bounds.origin,
                    bounds.size.height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            });
            marker(false, window);
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Device-pixel strips retain the soft edge on other renderers,
            // including inside a glyph (never recolor/re-shape font runs).
            if let Some((start, end)) = mask {
                let scale = window.scale_factor();
                let count = (f32::from(bounds.size.width) * scale).ceil() as usize;
                for x in 0..count {
                    let left = x as f32 / scale;
                    let alpha = ((end - left - 0.5 / scale) / (end - start)).clamp(0., 1.);
                    if alpha <= 0. {
                        continue;
                    }
                    let strip = Bounds::new(
                        bounds.origin + point(px(left), px(0.)),
                        gpui::size(px(1. / scale), bounds.size.height),
                    );
                    window.with_content_mask(Some(ContentMask { bounds: strip }), |window| {
                        gpui::Style {
                            opacity: Some(alpha),
                            ..Default::default()
                        }
                        .paint(strip, window, cx, |window, cx| {
                            let _ = line.paint(
                                bounds.origin,
                                bounds.size.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        });
                    });
                }
            } else {
                let _ = line.paint(
                    bounds.origin,
                    bounds.size.height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }
    });
}
