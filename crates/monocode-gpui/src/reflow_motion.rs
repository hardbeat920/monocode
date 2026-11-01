//! A pane slide rewraps the transcript every frame, and each paragraph that
//! gains or loses a line moves everything around it by a whole line at once.
//! Rows keep their painted position across a width change and ease into the
//! new layout, so the column flows while it wraps instead of ticking.
//! The chat's follow offset shares this prepaint scope, moving each row's
//! clipping and hitboxes together with its content.
//! A wheel scroll stretches the column the same way: the row under the
//! pointer tracks the gesture exactly while rows farther away trail it a
//! little and settle back, so the chat moves like one elastic sheet.
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
/// Share of a scroll step a row trails by at full distance from the pointer.
const STRETCH: f32 = 0.16;
/// Pointer distance at which a row trails by the full stretch.
const STRETCH_REACH: f32 = 720.;
/// A fast flick stretches the column no further than this.
const STRETCH_LIMIT: f32 = 48.;
/// Wheel and momentum events arrive every frame; a longer gap means layout,
/// not the user, moved the rows.
const SCROLLING: Duration = Duration::from_millis(100);

struct Track {
    top: Pixels,
    width: Pixels,
    offset: f32,
    seen: Instant,
}

#[derive(Default)]
struct State {
    tracks: BTreeMap<Key, Track>,
    scrolled: Option<Instant>,
}

#[derive(Clone, Default)]
pub(crate) struct ReflowMotion(Rc<RefCell<State>>);
impl ReflowMotion {
    /// The user scrolled the list; position changes at a steady width
    /// stretch rather than snap until the gesture goes quiet.
    pub(crate) fn scrolled(&self) {
        self.0.borrow_mut().scrolled = Some(Instant::now());
    }
    pub(crate) fn wrap(&self, key: Key, child: AnyElement, follow: f32) -> AnyElement {
        Glide {
            motion: self.clone(),
            key,
            child,
            follow,
        }
        .into_any_element()
    }
    /// `pointer` is the cursor's window y, the point the stretch pivots on.
    fn offset(
        &self,
        key: Key,
        bounds: Bounds<Pixels>,
        pointer: Pixels,
        now: Instant,
        reduced: bool,
    ) -> f32 {
        let mut state = self.0.borrow_mut();
        let scrolling = state
            .scrolled
            .is_some_and(|at| now.saturating_duration_since(at) < SCROLLING);
        let tracks = &mut state.tracks;
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
                let moved = f32::from(track.top - top);
                if track.width != width {
                    offset += moved;
                } else if scrolling && moved != 0. {
                    // The row under the pointer is held; the rest lag by
                    // their distance from it.
                    let distance = if pointer < bounds.top() {
                        bounds.top() - pointer
                    } else if pointer > bounds.bottom() {
                        pointer - bounds.bottom()
                    } else {
                        px(0.)
                    };
                    let lag = STRETCH * (f32::from(distance) / STRETCH_REACH).min(1.);
                    offset = (offset + moved * lag).clamp(-STRETCH_LIMIT, STRETCH_LIMIT);
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
        let reflow = self.motion.offset(
            self.key,
            bounds,
            window.mouse_position().y,
            Instant::now(),
            cx.reduce_motion(),
        );
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
        assert_eq!(
            motion.offset((0, 0), at(300., 600.), px(0.), start, false),
            0.
        );
        // Narrower: a paragraph above gained a line and pushed this row down.
        let frame = start + Duration::from_millis(16);
        assert_eq!(
            motion.offset((0, 0), at(324., 590.), px(0.), frame, false),
            -24.
        );
        let mut previous = 24.;
        let mut now = frame;
        for _ in 0..40 {
            now += Duration::from_millis(16);
            let offset = motion.offset((0, 0), at(324., 590.), px(0.), now, false);
            assert!(offset <= 0. && (-offset < previous || offset == 0.));
            previous = -offset;
        }
        assert_eq!(previous, 0.);
    }

    #[test]
    fn scrolling_at_a_fixed_width_is_never_smoothed() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        motion.offset((0, 0), at(300., 600.), px(0.), start, false);
        let frame = start + Duration::from_millis(16);
        assert_eq!(
            motion.offset((0, 0), at(180., 600.), px(0.), frame, false),
            0.
        );
    }

    #[test]
    fn a_wheel_scroll_stretches_rows_away_from_the_pointer() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        let pointer = px(320.);
        // One row under the pointer, one far below it.
        motion.offset((0, 0), at(300., 600.), pointer, start, false);
        motion.offset((0, 1), at(1100., 600.), pointer, start, false);
        motion.0.borrow_mut().scrolled = Some(start);
        let frame = start + Duration::from_millis(16);
        // Content moves up 40px.
        let held = motion.offset((0, 0), at(260., 600.), pointer, frame, false);
        let trailing = motion.offset((0, 1), at(1060., 600.), pointer, frame, false);
        assert_eq!(held, 0.);
        assert!((trailing - 40. * STRETCH).abs() < 0.01);
        // Once the gesture goes quiet the row settles into the layout.
        let mut now = frame;
        let mut offset = trailing;
        for _ in 0..40 {
            now += Duration::from_millis(16);
            offset = motion.offset((0, 1), at(1060., 600.), pointer, now, false);
        }
        assert_eq!(offset, 0.);
    }

    #[test]
    fn stale_rows_reduced_motion_and_scrolled_in_neighbours() {
        let motion = ReflowMotion::default();
        let start = Instant::now();
        motion.offset((0, 1), at(300., 600.), px(0.), start, false);
        let late = start + STALE + Duration::from_millis(1);
        assert_eq!(
            motion.offset((0, 1), at(200., 590.), px(0.), late, false),
            0.
        );
        let frame = late + Duration::from_millis(16);
        let offset = motion.offset((0, 1), at(260., 580.), px(0.), frame, false);
        assert_eq!(offset, -60.);
        // A row entering above joins the motion of the row it borders.
        assert_eq!(
            motion.offset((0, 0), at(200., 580.), px(0.), frame, false),
            -60.
        );
        assert_eq!(motion.offset((0, 1), at(0., 300.), px(0.), frame, true), 0.);
        assert_eq!(
            motion.offset(
                (0, 1),
                at(600., 600.),
                px(0.),
                frame + Duration::from_millis(16),
                false
            ),
            0.
        );
    }
}
