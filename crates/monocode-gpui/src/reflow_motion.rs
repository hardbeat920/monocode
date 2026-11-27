//! A pane slide rewraps the transcript every frame, and each paragraph that
//! gains or loses a line moves everything around it by a whole line at once.
//! Rows keep their painted position across a width change and ease into the
//! new layout, so the column flows while it wraps instead of ticking.
//! The chat's follow offset shares this prepaint scope, moving each row's
//! clipping and hitboxes together with its content.
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, point, px,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

type Key = (usize, usize);
/// Exponential time constant: steps arriving every frame blend into a glide
/// that trails the layout by roughly one line.
const SETTLE: f32 = 0.085;
/// Never trail further than this; an instant resize should not fly rows in.
const LIMIT: f32 = 160.;
/// A row unseen for longer was scrolled away; its old position means nothing.
const STALE: Duration = Duration::from_millis(120);

struct Track {
    top: Pixels,
    width: Pixels,
    offset: f32,
    seen: Instant,
}

#[derive(Clone, Default)]
pub(crate) struct ReflowMotion(Rc<RefCell<BTreeMap<Key, Track>>>);
impl ReflowMotion {
    pub(crate) fn wrap(&self, key: Key, child: AnyElement, follow: f32) -> AnyElement {
        Glide {
            motion: self.clone(),
            key,
            child,
            follow,
        }
        .into_any_element()
    }
    fn offset(&self, key: Key, bounds: Bounds<Pixels>, now: Instant, reduced: bool) -> f32 {
        let mut tracks = self.0.borrow_mut();
        if reduced {
            tracks.clear();
            return 0.;
        }
        let (top, width) = (bounds.top(), bounds.size.width);
        let fresh = |t: &Track| now.saturating_duration_since(t.seen) < STALE;
        let offset = match tracks.get(&key).filter(|t| fresh(t)) {
            Some(track) => {
                let dt = now.saturating_duration_since(track.seen).as_secs_f32();
                let mut offset = track.offset * (-dt / SETTLE).exp();
                if track.width != width {
                    offset += f32::from(track.top - top);
                }
                offset
            }
            // A row scrolled in mid-reflow travels with its painted neighbour
            // rather than opening a gap beside it.
            None => {
                tracks.retain(|_, t| fresh(t));
                tracks
                    .range(key..)
                    .next()
                    .or_else(|| tracks.range(..key).next_back())
                    .map_or(0., |(_, t)| t.offset)
            }
        }
        .clamp(-LIMIT, LIMIT);
        let offset = if offset.abs() < 0.25 { 0. } else { offset };
        tracks.insert(
            key,
            Track {
                top,
                width,
                offset,
                seen: now,
            },
        );
        offset
    }
}

struct Glide {
    motion: ReflowMotion,
    key: Key,
    child: AnyElement,
    /// Shift the complete row, including disclosure clipping and hitboxes.
    follow: f32,
}
impl IntoElement for Glide {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Glide {
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
        let reflow = self
            .motion
            .offset(self.key, bounds, Instant::now(), cx.reduce_motion());
        if reflow != 0. {
            // Notifies the transcript, so its cached view keeps painting.
            window.request_animation_frame();
        }
        // Whole device pixels keep glyphs from shimmering while they move.
        let scale = window.scale_factor();
        let offset = ((reflow + self.follow) * scale).round() / scale;
        window.with_element_offset(point(px(0.), px(offset)), |window| {
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

    fn at(top: f32, width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(width), px(48.)))
    }

    #[test]
    fn rewrap_keeps_the_painted_position_then_eases_into_layout() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        assert_eq!(motion.offset((0, 0), at(300., 600.), start, false), 0.);
        // Narrower: a paragraph above gained a line and pushed this row down.
        let frame = start + Duration::from_millis(16);
        assert_eq!(motion.offset((0, 0), at(324., 590.), frame, false), -24.);
        let mut previous = 24.;
        let mut now = frame;
        for _ in 0..40 {
            now += Duration::from_millis(16);
            let offset = motion.offset((0, 0), at(324., 590.), now, false);
            assert!(offset <= 0. && (-offset < previous || offset == 0.));
            previous = -offset;
        }
        assert_eq!(previous, 0.);
    }

    #[test]
    fn scrolling_at_a_fixed_width_is_never_smoothed() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        motion.offset((0, 0), at(300., 600.), start, false);
        let frame = start + Duration::from_millis(16);
        assert_eq!(motion.offset((0, 0), at(180., 600.), frame, false), 0.);
    }

    #[test]
    fn stale_rows_reduced_motion_and_scrolled_in_neighbours() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        motion.offset((0, 1), at(300., 600.), start, false);
        let late = start + STALE + Duration::from_millis(1);
        assert_eq!(motion.offset((0, 1), at(200., 590.), late, false), 0.);
        let frame = late + Duration::from_millis(16);
        let offset = motion.offset((0, 1), at(260., 580.), frame, false);
        assert_eq!(offset, -60.);
        // A row entering above joins the motion of the row it borders.
        assert_eq!(motion.offset((0, 0), at(200., 580.), frame, false), -60.);
        assert_eq!(motion.offset((0, 1), at(0., 300.), frame, true), 0.);
        assert_eq!(
            motion.offset(
                (0, 1),
                at(600., 600.),
                frame + Duration::from_millis(16),
                false
            ),
            0.
        );
    }
}
