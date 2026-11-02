//! Complete composer animation view. Host coordinates are confined to the
//! injected obstacle source; the library sees only a plain local obstacle.
//!
//! Call set_visible(false) before hiding/unmounting a retained entity. The
//! component pauses its own local timeline (including absolute effect ages),
//! stops renewing its clock lease, and remeasures on remount. Busy/appearance
//! configuration can still be applied while hidden, without stepping physics.
use super::{paint_rim_mask, AnimationClock, RimSprite};
use crate::{
    composer::{Composer, Facing, Input, Obstacle, COIN_FACE, STAR_FACE},
    MascotId,
};
use gpui::{
    canvas, div, prelude::*, App, Bounds, Context, Entity, Hsla, Pixels, Render, Subscription,
    Window,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

const TICK_MS: f64 = 1000. / 30.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Props {
    pub busy: bool,
    pub mascot: Option<MascotId>,
    /// Composer top edge measured from the top of this view's strip. The app
    /// retains placement policy; native's existing strip uses 64 logical px.
    pub rim_y: f32,
}

pub type ObstacleSource = Rc<dyn Fn(Bounds<Pixels>, f32, &mut App) -> Option<Obstacle>>;
#[derive(Clone)]
pub struct Services {
    pub clock: Rc<dyn AnimationClock>,
    /// Given measured strip window bounds and rim offset, return a local hurdle.
    /// Bind the host's geometry entity with watch_geometry, or explicitly notify
    /// this view when that source changes: window redraw alone can reuse a cache.
    pub obstacle: ObstacleSource,
}

pub struct ComposerView {
    simulation: Rc<RefCell<Composer>>,
    services: Services,
    props: Props,
    style: Style,
    visible: bool,
    bounds: Option<Bounds<Pixels>>,
    width: Option<f64>,
    obstacle: Option<Obstacle>,
    elapsed: Duration,
    base_tick: u64,
    last_clock: Option<Duration>,
    stepped: Option<u64>,
    frame_tick: Option<u64>,
    geometry_watch: Option<Subscription>,
}

impl ComposerView {
    pub fn new(seed: u32, props: Props, style: Style, services: Services) -> Self {
        Self {
            simulation: Rc::new(RefCell::new(Composer::new(seed))),
            services,
            props,
            style,
            visible: true,
            bounds: None,
            width: None,
            obstacle: None,
            elapsed: Duration::ZERO,
            base_tick: 0,
            last_clock: None,
            stepped: None,
            frame_tick: None,
            geometry_watch: None,
        }
    }
    /// A single opaque host model can publish obstacle changes even when this
    /// view is cached/reduced-motion. Observation doesn't retain the source or
    /// lease a timer, and ends when the view is dropped/rebound.
    pub fn watch_geometry<T: 'static>(&mut self, source: &Entity<T>, cx: &mut Context<Self>) {
        self.geometry_watch = Some(cx.observe(source, |_, _, cx| cx.notify()));
    }
    pub fn configure(&mut self, props: Props, style: Style, cx: &mut Context<Self>) {
        if self.props != props || self.style != style {
            self.props = props;
            self.style = style;
            self.sample_time(cx);
            self.prepare_frame(cx.reduce_motion(), false);
            cx.notify();
        }
    }
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        if !visible {
            // Freeze the last prepared frame, discarding the unpresented
            // fractional interval before hide as well as all hidden wall time.
            self.base_tick = self.frame_tick.unwrap_or(0);
            self.elapsed = Duration::ZERO;
            self.stepped = self.frame_tick;
            if self.frame_tick.is_some() {
                self.simulation
                    .borrow_mut()
                    .rebase_time(self.base_tick as f64 * TICK_MS);
            }
        }
        self.visible = visible;
        self.last_clock = if visible {
            Some(self.services.clock.elapsed(cx))
        } else {
            None
        };
        self.bounds = None;
        cx.notify();
    }
    fn sample_time(&mut self, cx: &mut App) {
        let now = self.services.clock.elapsed(cx);
        if self.visible {
            if let Some(last) = self.last_clock {
                self.elapsed += now.saturating_sub(last);
            }
            self.last_clock = Some(now);
        }
    }
    fn tick(&self) -> u64 {
        self.base_tick + (self.elapsed.as_secs_f64() * 1000. / TICK_MS) as u64
    }
    fn prepare_frame(&mut self, reduced: bool, advance: bool) {
        if let Some(width) = self.width {
            let tick = self.tick();
            let input = Input {
                now_ms: tick as f64 * TICK_MS,
                width,
                busy: self.props.busy,
                obstacle: self.obstacle,
                reduced_motion: reduced,
                mascot: self.props.mascot,
            };
            let mut simulation = self.simulation.borrow_mut();
            if advance {
                simulation.advance(input);
            } else {
                simulation.configure(input);
            }
            self.frame_tick = Some(tick);
        }
    }
    fn update_frame(&mut self, reduced: bool) {
        if self.bounds.is_none() {
            return;
        }
        let tick = self.tick();
        self.prepare_frame(reduced, self.stepped != Some(tick));
        self.stepped = Some(tick);
    }
}

impl Render for ComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        self.sample_time(cx);
        let reduced = cx.reduce_motion();
        if let Some(bounds) = self.bounds {
            self.obstacle = (self.services.obstacle)(bounds, self.props.rim_y, cx);
        }
        self.update_frame(reduced);
        if !reduced && (self.props.busy || self.simulation.borrow().frame().active) {
            self.services.clock.request(false, window, cx);
        }
        let weak = cx.entity().downgrade();
        let simulation = self.simulation.clone();
        let props = self.props;
        let color = self.style.color;
        div()
            .id("native-composer-mascot")
            .size_full()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let obstacle = (view.services.obstacle)(bounds, view.props.rim_y, cx);
                            if view.bounds != Some(bounds) || view.obstacle != obstacle {
                                view.bounds = Some(bounds);
                                view.obstacle = obstacle;
                                view.width = Some(f64::from(bounds.size.width.as_f32()));
                                // The paint closure borrows this prepared buffer in
                                // the same draw, even with no next motion tick.
                                view.update_frame(reduced);
                                cx.defer_in(window, |_, _, cx| cx.notify());
                            }
                        });
                    },
                    move |bounds, _, window, _| {
                        let simulation = simulation.borrow();
                        let frame = simulation.frame();
                        if !frame.active {
                            return;
                        }
                        let Some(pose) = frame.pose else {
                            return;
                        };
                        if let Some(art) = frame.artwork {
                            paint_rim_mask(
                                &art.rows,
                                bounds.origin,
                                RimSprite {
                                    center_x: pose.x as f32,
                                    feet_height: pose.y as f32,
                                    side: 16.,
                                    facing: pose.facing,
                                    rim_y: props.rim_y,
                                },
                                color,
                                window,
                            );
                        }
                        for coin in frame.coins {
                            let mut ink = color;
                            ink.a *= coin.alpha as f32;
                            paint_rim_mask(
                                &COIN_FACE,
                                bounds.origin,
                                RimSprite {
                                    center_x: coin.x as f32,
                                    feet_height: coin.y as f32 - 6.,
                                    side: 12.,
                                    facing: Facing::Right,
                                    rim_y: props.rim_y,
                                },
                                ink,
                                window,
                            );
                        }
                        for star in frame.stars {
                            let mut ink = color;
                            ink.a *= star.opacity as f32;
                            paint_rim_mask(
                                &STAR_FACE,
                                bounds.origin,
                                RimSprite {
                                    center_x: pose.x as f32 - 4. + star.dx as f32,
                                    feet_height: pose.y as f32 + 8. - star.dy as f32,
                                    side: 8.,
                                    facing: Facing::Right,
                                    rim_y: props.rim_y,
                                },
                                ink,
                                window,
                            );
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}
