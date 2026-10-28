//! ComposerRunner's original physics, using the shared project pixel art.
use crate::{popover::AnchorSlot, transcript::TranscriptView};
use gpui::{Context, Entity, Render, Window, canvas, div, point, prelude::*, px, size};
use serde::Deserialize;
use std::time::Instant;
/// Matches the shared pulse clock's 30fps tick.
const TICK_MS: f64 = 1000. / 30.;
#[derive(Clone, Default, Deserialize)]
struct Frame {
    active: bool,
    x: f32,
    y: f32,
    facing: i32,
    rows: Vec<String>,
    coins: Vec<Coin>,
    stars: Vec<Star>,
}
#[derive(Clone, Deserialize)]
struct Coin {
    x: f32,
    y: f32,
    alpha: f32,
}
#[derive(Clone, Deserialize)]
struct Star {
    dx: f32,
    dy: f32,
    opacity: f32,
}
pub(crate) struct Runner {
    context: rquickjs::Context,
    _runtime: rquickjs::Runtime,
    anchor: AnchorSlot,
    started: Instant,
    busy: bool,
    name: &'static str,
    color: u32,
    transcript: Entity<TranscriptView>,
    frame: Frame,
    /// The physics tick `frame` was stepped at.
    stepped: Option<u64>,
}
impl Runner {
    pub fn new(transcript: Entity<TranscriptView>, cx: &mut Context<Self>) -> Self {
        let runtime = rquickjs::Runtime::new().expect("runner runtime");
        runtime.set_memory_limit(8 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).expect("runner context");
        context
            .with(|cx| cx.eval::<(), _>(include_str!("../assets/settings-arcade.js")))
            .expect("runner physics");
        let _ = cx;
        Self {
            context,
            _runtime: runtime,
            anchor: Default::default(),
            started: Instant::now(),
            busy: false,
            name: "invader",
            color: 0x459cf7,
            transcript,
            frame: Default::default(),
            stepped: None,
        }
    }
    pub fn configure(
        &mut self,
        busy: bool,
        name: &'static str,
        color: u32,
        cx: &mut Context<Self>,
    ) {
        if (self.busy, self.name, self.color) != (busy, name, color) {
            self.busy = busy;
            self.name = name;
            self.color = color;
            cx.notify();
        }
    }
}
impl Runner {
    /// The strip above the composer the runner plays in. Mounted cached, so
    /// the conversation's own re-renders don't step the physics again.
    pub fn placement() -> gpui::StyleRefinement {
        gpui::StyleRefinement::default()
            .absolute()
            .left_0()
            .right_0()
            .top(px(-64.))
            .h(px(80.))
    }
}
impl Render for Runner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.anchor.get().unwrap_or_default();
        let width = bounds.size.width.as_f32();
        let top = bounds.top().as_f32() + 64.;
        let obstacle=self.transcript.read(cx).jump_bounds().filter(|b|b.right()>bounds.left()&&b.left()<bounds.right()&&b.bottom().as_f32()>=top-48.&&b.top().as_f32()<=top+12.).map(|b|serde_json::json!({"left":(b.left()-bounds.left()).as_f32(),"right":(b.right()-bounds.left()).as_f32(),"height":(top-b.top().as_f32()+10.).max(28.)}));
        let reduced = cx.reduce_motion();
        // Reading the transcript re-renders this view on every streamed
        // token. Physics advance on a fixed tick instead, so those extra
        // renders repaint the same frame rather than nudging the runner
        // forward at uneven intervals.
        let tick = (self.started.elapsed().as_secs_f64() * 1000. / TICK_MS) as u64;
        let result = (self.stepped != Some(tick)).then(|| {
            self.context.with(|cx| {
                let o: rquickjs::Object = cx.globals().get("NativeArcade")?;
                let f: rquickjs::Function = o.get("runnerFrame")?;
                f.call::<_, String>((
                    tick as f64 * TICK_MS,
                    width,
                    self.busy,
                    obstacle
                        .map(|o| o.to_string())
                        .unwrap_or_else(|| "null".into()),
                    reduced,
                    self.name,
                ))
            })
        });
        if let Some(result) = result {
            self.stepped = Some(tick);
            if let Ok(json) = result
                && let Ok(frame) = serde_json::from_str(&json)
            {
                self.frame = frame;
            }
        }
        // It covers ground quickly, so it needs the full 30fps to read as
        // running rather than hopping.
        if self.busy || self.frame.active {
            crate::pulse::request(crate::pulse::Rate::Smooth, window, cx);
        }
        let frame = self.frame.clone();
        let color: gpui::Hsla = gpui::rgb(self.color).into();
        // Conversation places the cached view; see `Runner::placement`.
        div().size_full().child(self.anchor.probe()).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    if !frame.active {
                        return;
                    }
                    let sprite = |rows: &[String],
                                  x: f32,
                                  y: f32,
                                  side: f32,
                                  facing: i32,
                                  alpha: f32,
                                  window: &mut Window| {
                        let unit = side / 8.;
                        // Whole device pixels, so the art doesn't shimmer as
                        // it crosses pixel boundaries.
                        let scale = window.scale_factor();
                        let snap = |v: f32| (v * scale).round() / scale;
                        let left = snap(x - side / 2.);
                        let top = snap(64. - y - side);
                        let mut ink = color;
                        ink.a *= alpha;
                        for (iy, row) in rows.iter().enumerate() {
                            for (ix, pixel) in row.bytes().enumerate() {
                                if pixel != b'#' {
                                    continue;
                                }
                                let px_x = left
                                    + if facing < 0 {
                                        (7 - ix) as f32 * unit
                                    } else {
                                        ix as f32 * unit
                                    };
                                let px_y = top + iy as f32 * unit;
                                if px_y >= 64. {
                                    continue;
                                }
                                window.paint_quad(gpui::fill(
                                    gpui::Bounds::new(
                                        bounds.origin + point(px(px_x), px(px_y)),
                                        size(px(unit), px(unit.min(64. - px_y))),
                                    ),
                                    ink,
                                ));
                            }
                        }
                    };
                    sprite(&frame.rows, frame.x, frame.y, 16., frame.facing, 1., window);
                    let coin = [
                        "........", "..####..", ".######.", "########", "########", ".######.",
                        "..####..", "........",
                    ]
                    .map(str::to_owned);
                    for c in &frame.coins {
                        sprite(&coin, c.x, c.y - 6., 12., 1, c.alpha, window);
                    }
                    let star = [
                        "...##...", "...##...", "..####..", "########", "########", "..####..",
                        "...##...", "...##...",
                    ]
                    .map(str::to_owned);
                    for s in &frame.stars {
                        sprite(
                            &star,
                            frame.x - 4. + s.dx,
                            frame.y + 8. - s.dy,
                            8.,
                            1,
                            s.opacity,
                            window,
                        );
                    }
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_runner_physics_deserialize_and_exit() {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|cx| {
            cx.eval::<(), _>(include_str!("../assets/settings-arcade.js"))
                .unwrap();
            let o: rquickjs::Object = cx.globals().get("NativeArcade").unwrap();
            let f: rquickjs::Function = o.get("runnerFrame").unwrap();
            let json: String = f
                .call((1000f64, 600f32, true, "null", false, "invader"))
                .unwrap();
            let frame: super::Frame = serde_json::from_str(&json).unwrap();
            assert!(frame.active);
            assert_eq!(frame.rows.len(), 8);
            let _: String = f
                .call((1100f64, 600f32, false, "null", false, "invader"))
                .unwrap();
            let json: String = f
                .call((1800f64, 600f32, false, "null", false, "invader"))
                .unwrap();
            assert!(!serde_json::from_str::<super::Frame>(&json).unwrap().active);
        });
    }
}
