//! useComposerDockMotion.ts: move the first submitted composer's painted surface
//! from its empty-session slot to the dock, without moving the transcript layout.
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, Window, point, px,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

const DURATION: Duration = Duration::from_millis(480);
const LAUNCH_WINDOW: Duration = Duration::from_millis(1500);

struct Launch {
    from: Bounds<Pixels>,
    captured: Instant,
    animation: Option<(Instant, Point<Pixels>)>,
}
#[derive(Clone, Default)]
pub(crate) struct ComposerMotion(Rc<RefCell<Option<Launch>>>);
impl ComposerMotion {
    pub(crate) fn capture(&self, from: Option<Bounds<Pixels>>) {
        *self.0.borrow_mut() = from.map(|from| Launch {
            from,
            captured: Instant::now(),
            animation: None,
        });
    }
    pub(crate) fn wrap(&self, child: impl IntoElement) -> impl IntoElement {
        Slide {
            motion: self.clone(),
            child: child.into_any_element(),
        }
    }
    fn offset(&self, to: Bounds<Pixels>, now: Instant, reduced: bool) -> (Point<Pixels>, bool) {
        let mut state = self.0.borrow_mut();
        let Some(launch) = state.as_mut() else {
            return (Point::default(), false);
        };
        if reduced
            || (launch.animation.is_none() && now.duration_since(launch.captured) > LAUNCH_WINDOW)
        {
            *state = None;
            return (Point::default(), false);
        }
        let (started, delta) = *launch.animation.get_or_insert_with(|| {
            (
                now,
                point(
                    launch.from.center().x - to.center().x,
                    launch.from.top() - to.top(),
                ),
            )
        });
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= DURATION || (delta.x.abs() < px(1.) && delta.y.abs() < px(1.)) {
            *state = None;
            return (Point::default(), false);
        }
        let t = elapsed.as_secs_f32() / DURATION.as_secs_f32();
        let remaining = 1. - crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
        (delta * remaining, true)
    }
    #[cfg(test)]
    pub(crate) fn elapsed(&self, elapsed: Duration) {
        if let Some(launch) = self.0.borrow_mut().as_mut()
            && let Some((started, _)) = launch.animation.as_mut()
        {
            *started = Instant::now() - elapsed;
        }
    }
}

struct Slide {
    motion: ComposerMotion,
    child: AnyElement,
}
impl IntoElement for Slide {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Slide {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
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
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let (offset, active) = self
            .motion
            .offset(bounds, Instant::now(), cx.reduce_motion());
        if active {
            window.request_animation_frame();
        }
        // Prepaint moves paint bounds, hitboxes, the caret and IME together.
        window.with_element_offset(offset, |window| {
            self.child.prepaint(window, cx);
        });
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn launch_matches_reacts_translation_and_easing_then_settles() {
        let motion = ComposerMotion::default();
        let from = Bounds::new(point(px(100.), px(300.)), size(px(600.), px(150.)));
        let to = Bounds::new(point(px(50.), px(700.)), size(px(700.), px(100.)));
        motion.capture(Some(from));
        let start = Instant::now();
        assert_eq!(
            motion.offset(to, start, false),
            (point(px(0.), px(-400.)), true)
        );
        let (middle, active) = motion.offset(to, start + DURATION / 2, false);
        assert!(active && middle.y > px(-100.) && middle.y < px(0.));
        assert_eq!(
            motion.offset(to, start + DURATION, false),
            (Point::default(), false)
        );
    }
    #[test]
    fn history_reduced_motion_and_stale_launches_dock_without_motion() {
        let motion = ComposerMotion::default();
        let from = Bounds::new(point(px(0.), px(300.)), size(px(600.), px(100.)));
        let to = Bounds::new(point(px(0.), px(700.)), from.size);
        assert!(!motion.offset(to, Instant::now(), false).1);
        motion.capture(Some(from));
        assert!(!motion.offset(to, Instant::now(), true).1);
        motion.capture(Some(from));
        assert!(
            !motion
                .offset(
                    to,
                    Instant::now() + LAUNCH_WINDOW + Duration::from_millis(1),
                    false
                )
                .1
        );
        motion.capture(Some(to));
        assert!(!motion.offset(to, Instant::now(), false).1);
    }
}
