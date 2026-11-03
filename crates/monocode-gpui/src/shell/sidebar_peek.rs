// Sidebar.tsx's peek: in the inset layout a collapsed sidebar slides out over
// the workspace while the pointer rests on the window's left edge or on the
// sidebar toggle, and puts itself away once the pointer leaves it.
use super::Shell;
use crate::{reveal::bezier, theme::*};
use gpui::{
    AnyElement, BoxShadow, Context, DispatchPhase, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    StyleRefinement, Window, canvas, div, prelude::*, px, rgba,
};
use std::time::{Duration, Instant};

/// How close to the window's left edge the pointer peeks the sidebar out.
const PEEK_EDGE: f32 = 6.;
/// Grace before a peek closes, so the pointer can cross a gap to the panel.
const PEEK_CLOSE_DELAY: Duration = Duration::from_millis(180);
/// The panel's inset from the window's left and bottom edges (`left-1.5`).
const PEEK_INSET: f32 = 6.;
/// How far the panel drifts in from the edge.
const PEEK_DRIFT: f32 = 12.;

/// A fade between hidden (0) and shown (1). A reversal midway starts from
/// wherever the last one had got to.
pub(super) struct SidebarPeek {
    pub(super) open: bool,
    from: f32,
    start: Instant,
}

impl SidebarPeek {
    fn new(open: bool, from: f32, reduce_motion: bool) -> Self {
        Self {
            open,
            from: if reduce_motion {
                open as u8 as f32
            } else {
                from
            },
            start: Instant::now(),
        }
    }

    /// The progress now, and whether it has settled.
    fn at(&self, now: Instant) -> (f32, bool) {
        let to = if self.open { 1. } else { 0. };
        let (duration, curve) = if self.open {
            (240., (0.16, 1., 0.3, 1.))
        } else {
            (200., (0.4, 0., 0.2, 1.))
        };
        // Shorter the less of the way there is left to go.
        let duration = duration * (to - self.from).abs();
        if duration <= 0. {
            return (to, true);
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32() * 1000. / duration;
        if t >= 1. {
            return (to, true);
        }
        let eased = bezier(t, curve.0, curve.1, curve.2, curve.3);
        (self.from + (to - self.from) * eased, false)
    }
}

impl Shell {
    /// Only the inset layout's collapsed sidebar peeks.
    fn peek_available(&self) -> bool {
        !self.workspace.boxy_layout()
            && !self.workspace.sidebar_visible
            && self.sidebar_slide.is_none()
            && self.settings.is_none()
    }

    /// Whether the sidebar must paint uncached: inherited opacity is not part
    /// of a cached paint's key, so a fading sidebar repaints every frame.
    pub(super) fn sidebar_uncached(&self) -> bool {
        self.sidebar_slide.is_some() || self.peek_animating
    }

    pub(super) fn peek_open(&self) -> bool {
        self.sidebar_peek.as_ref().is_some_and(|peek| peek.open)
    }

    pub(super) fn set_peek(&mut self, open: bool, cx: &mut Context<Self>) {
        self.peek_close = None;
        if open && !self.peek_available() {
            return;
        }
        let current = match &self.sidebar_peek {
            Some(peek) if peek.open == open => return,
            Some(peek) => peek.at(Instant::now()).0,
            None if !open => return,
            None => 0.,
        };
        if !open {
            self.close_preview(cx);
        }
        self.sidebar_peek = Some(SidebarPeek::new(open, current, cx.reduce_motion()));
        self.peek_animating = true;
        cx.notify();
    }

    /// Drops the peek at once, as when the sidebar docks or the layout changes.
    pub(super) fn drop_peek(&mut self) {
        self.peek_close = None;
        self.sidebar_peek = None;
        self.peek_animating = false;
    }

    pub(super) fn hover_peek_toggle(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.set_peek(true, cx);
        }
    }

    fn schedule_peek_close(&mut self, cx: &mut Context<Self>) {
        if self.peek_close.is_some() {
            return;
        }
        self.peek_close = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PEEK_CLOSE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.peek_close = None;
                this.set_peek(false, cx);
            });
        }));
    }

    fn peek_contains(&self, position: Point<Pixels>) -> bool {
        let (x, y) = (position.x.as_f32(), position.y.as_f32());
        x >= PEEK_INSET
            && x <= PEEK_INSET + self.workspace.sidebar_width
            && y >= TITLE_BAR_HEIGHT
            && y <= self.viewport_height - PEEK_INSET
    }

    /// The panel, a menu opened from it or the session preview beside it keeps
    /// the peek out, as does a drag under way.
    fn peek_held(&self, position: Point<Pixels>) -> bool {
        self.peek_toggle_bounds
            .get()
            .is_some_and(|bounds| bounds.contains(&position))
            || self.peek_contains(position)
            || self.menu.is_some()
            || self.session_preview.as_ref().is_some_and(|preview| {
                preview
                    .surface
                    .get()
                    .is_some_and(|bounds| bounds.contains(&position))
            })
    }

    fn peek_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let edge =
            event.position.x.as_f32() <= PEEK_EDGE && event.position.y.as_f32() > TITLE_BAR_HEIGHT;
        if edge && event.pressed_button.is_none() {
            self.set_peek(true, cx);
            return;
        }
        if !self.peek_open() {
            return;
        }
        if self.peek_held(event.position) || event.pressed_button.is_some() {
            self.peek_close = None;
        } else {
            self.schedule_peek_close(cx);
        }
    }

    /// A click anywhere else puts the sidebar away at once.
    fn peek_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.peek_open() && !self.peek_held(event.position) {
            self.set_peek(false, cx);
        }
    }

    /// Watches the pointer across the whole window, ahead of any view that
    /// would stop its events, while the sidebar can peek.
    pub(super) fn peek_watch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.peek_available() {
            if self.sidebar_peek.is_some() {
                self.drop_peek();
            }
            return None;
        }
        let progress = match &self.sidebar_peek {
            Some(peek) => {
                let (progress, done) = peek.at(Instant::now());
                if done {
                    self.peek_animating = false;
                    if !peek.open {
                        self.sidebar_peek = None;
                    }
                } else {
                    window.request_animation_frame();
                }
                progress
            }
            None => 0.,
        };
        let shell = cx.entity().downgrade();
        let watch = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moves = shell.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moves.update(cx, |this, cx| this.peek_pointer(event, cx));
                    }
                });
                let presses = shell.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = presses.update(cx, |this, cx| this.peek_press(event, cx));
                    }
                });
            },
        )
        .absolute()
        .size_0()
        .into_any_element();
        if self.sidebar_peek.is_none() {
            return Some(watch);
        }
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(watch)
                .child(self.peek_backdrop(progress, cx))
                .child(self.peek_panel(progress, window))
                .into_any_element(),
        )
    }

    /// The rest of the window dims behind the panel. It takes no hitbox, so
    /// the pointer still reaches the toggle beneath it.
    fn peek_backdrop(&self, progress: f32, cx: &mut Context<Self>) -> gpui::Div {
        let p = &self.workspace.preferences;
        let light = p.theme == "light"
            || (p.theme == "system"
                && matches!(
                    cx.window_appearance(),
                    gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
                ));
        let mut dim: gpui::Hsla = rgba(0x000000ff).into();
        dim.a = if light { 0.18 } else { 0.4 } * progress;
        div().absolute().top_0().left_0().size_full().bg(dim)
    }

    fn peek_panel(&self, progress: f32, window: &mut Window) -> gpui::Stateful<gpui::Div> {
        // Glass blurs the dimmed workspace under a faint wash; otherwise the
        // panel takes the window's own surface.
        let glass = self.window_glass == Some(true) && !is_light();
        let scale = window.scale_factor();
        let sidebar = if self.sidebar_uncached() {
            self.sidebar_frame.clone().into_any_element()
        } else {
            self.sidebar_frame
                .clone()
                .cached(
                    StyleRefinement::default()
                        .w(px(self.workspace.sidebar_width))
                        .h_full(),
                )
                .into_any_element()
        };
        div()
            .id("sidebar-peek")
            .debug_selector(|| "sidebar-peek".into())
            // Closing, it lets the pointer through to the workspace.
            .when(self.peek_open(), |d| d.occlude())
            .absolute()
            .left(px(PEEK_INSET - PEEK_DRIFT * (1. - progress)))
            .top(px(TITLE_BAR_HEIGHT))
            .bottom(px(PEEK_INSET))
            .w(px(self.workspace.sidebar_width))
            .opacity(progress)
            .rounded(px(10.))
            .overflow_hidden()
            .shadow(vec![
                BoxShadow::new(px(0.), px(12.), rgba(0x00000047).into()).blur_radius(px(32.)),
                BoxShadow::new(px(0.), px(2.), rgba(0x00000029).into()).blur_radius(px(6.)),
            ])
            .when(!glass, |d| d.bg(shell_fill()))
            .when(glass, |d| {
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            #[cfg(any(target_os = "macos", target_os = "windows"))]
                            let quad = gpui::fill(bounds, content(0.05))
                                .border_color(gpui::backdrop_blur_marker(24. * scale));
                            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                            let quad = {
                                let _ = scale;
                                gpui::fill(bounds, shell_fill())
                            };
                            window.paint_quad(quad.corner_radii(px(10.)));
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
            })
            .child(sidebar)
            // The border rides above the contents, inside the rounding.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(content(0.08)),
            )
    }
}
