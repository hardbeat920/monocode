//! Port of src/shared/hooks/useAnimatedReorder.ts: direct manipulation for
//! a row or column of equal-sized items.
//!
//! The hook wrote CSS transforms straight onto DOM nodes and listened on the
//! window. Here [`AnimatedReorder`] is the same state machine without a DOM:
//! the view feeds it pointer positions, measured item spans, and a clock in
//! milliseconds, then reads each item's offset with [`AnimatedReorder::offset`].
//! [`OffsetTweens`] plays the CSS `transform` transition between offsets.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use monocode_ui::theme::CubicBezier;

use super::reorder::move_item;

/// The drag threshold in px, as the hook hard-coded it.
const THRESHOLD: f32 = 5.0;
/// How long a click stays suppressed after a drag, in ms.
const CLICK_SUPPRESS_MS: f64 = 400.0;

/// The axis items are laid out along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    X,
    Y,
}

/// An item's extent along the axis, as `getBoundingClientRect` measured it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemSpan {
    pub start: f32,
    pub size: f32,
}

impl ItemSpan {
    pub fn new(start: f32, size: f32) -> Self {
        Self { start, size }
    }

    pub fn end(&self) -> f32 {
        self.start + self.size
    }
}

/// Where a press lands: the pointer along the axis, and the strip's scroll
/// offset (`scrollLeft` or `scrollTop`) at that moment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PressPoint {
    pub position: f32,
    pub scroll: f32,
}

/// What the owner must do after a gesture step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorderEffect {
    /// `onReorder(ids, movedId)`.
    Reorder { ids: Vec<String>, moved_id: String },
    /// `externalDrop.onEnd(id)`: the gesture ended, whatever happened.
    End { id: String },
}

#[derive(Debug, Clone)]
struct Drag {
    id: String,
    items: Vec<String>,
    from: usize,
    spans: Vec<ItemSpan>,
    start_position: f32,
    pointer_position: f32,
    scroll_start: f32,
    scroll: f32,
    duration: f64,
    active: bool,
    settling: bool,
    destination: usize,
    over_external: bool,
    transforms: Vec<Option<f32>>,
    /// `transition: none` on the handle while it follows the pointer.
    handle_follows: bool,
    /// When the settle animation ends, and whether it commits a move.
    finish: Option<(f64, bool, usize)>,
}

impl Drag {
    fn preview(&mut self, to: usize) {
        let reordered = move_item(&self.items, self.from, to);
        for index in 0..self.items.len() {
            if index == self.from {
                continue;
            }
            let slot = reordered
                .iter()
                .position(|id| *id == self.items[index])
                .unwrap_or(index);
            self.transforms[index] = Some(self.spans[slot].start - self.spans[index].start);
        }
    }

    fn paint(&mut self, move_handle: bool) {
        // Scroll moves the original slots without changing their layout order.
        let scroll_offset = self.scroll - self.scroll_start;
        let first = self.spans[0];
        let last = self.spans[self.spans.len() - 1];
        let from = self.spans[self.from];
        let offset = (first.start - from.start).max(
            (self.pointer_position - self.start_position + scroll_offset)
                .min(last.end() - from.end()),
        );
        if move_handle {
            self.transforms[self.from] = Some(offset);
        }
        let center = from.start + from.size / 2.0 + offset;
        let distance = |span: &ItemSpan| (center - span.start - span.size / 2.0).abs();
        let mut next = self.from;
        for (index, span) in self.spans.iter().enumerate() {
            if distance(span) < distance(&self.spans[next]) {
                next = index;
            }
        }
        if next != self.destination {
            self.destination = next;
            self.preview(next);
        }
    }
}

/// The gesture state behind `useAnimatedReorder`.
#[derive(Debug, Clone, Default)]
pub struct AnimatedReorder {
    axis: Axis,
    drag: Option<Drag>,
    suppress_click_until: f64,
    dragging_id: Option<String>,
}

impl AnimatedReorder {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            ..Self::default()
        }
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// `draggingId`: set once the pointer crosses the threshold.
    pub fn dragging_id(&self) -> Option<&str> {
        self.dragging_id.as_deref()
    }

    /// A press is held or a drop is settling.
    pub fn is_pressed(&self) -> bool {
        self.drag.is_some()
    }

    /// The drag is past the threshold and still follows the pointer: the
    /// grabbing cursor and pointer capture are on.
    pub fn is_grabbing(&self) -> bool {
        self.drag
            .as_ref()
            .is_some_and(|drag| drag.active && !drag.settling)
    }

    /// The drop animation is running.
    pub fn is_settling(&self) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.settling)
    }

    /// The item under the pointer, or settling into its slot.
    pub fn handle_id(&self) -> Option<&str> {
        self.drag
            .as_ref()
            .filter(|drag| drag.active)
            .map(|drag| drag.id.as_str())
    }

    /// When the settle animation ends, in the clock the owner passes in.
    pub fn settle_deadline(&self) -> Option<f64> {
        self.drag
            .as_ref()
            .and_then(|drag| drag.finish)
            .map(|(at, _, _)| at)
    }

    /// The `translate3d` offset for `id`, or `None` when it has no
    /// transform.
    pub fn offset(&self, id: &str) -> Option<f32> {
        let drag = self.drag.as_ref()?;
        let index = drag.items.iter().position(|item| item == id)?;
        drag.transforms[index]
    }

    /// Whether a change of `id`'s offset should animate. Every item has the
    /// transition during a drag except the handle while it follows the
    /// pointer.
    pub fn animates(&self, id: &str) -> bool {
        let Some(drag) = self.drag.as_ref().filter(|drag| drag.active) else {
            return false;
        };
        !(drag.id == id && drag.handle_follows)
    }

    /// `onItemPointerDown`. `spans` lines up with `ids`; a `None` means the
    /// item was not measured and the press does nothing. `duration` is
    /// `--motion-reorder-duration` in ms, or 0 under reduced motion.
    pub fn press(
        &mut self,
        id: &str,
        ids: &[String],
        spans: &[Option<ItemSpan>],
        pointer: PressPoint,
        duration: f64,
        now: f64,
    ) -> Vec<ReorderEffect> {
        let PressPoint { position, scroll } = pointer;
        let effects = self.finish_now(now);
        if self.drag.is_some() {
            return effects;
        }
        // A new press is a click candidate, even immediately after a drag.
        self.suppress_click_until = 0.0;
        let Some(from) = ids.iter().position(|item| item == id) else {
            return effects;
        };
        if ids.len() < 2 || spans.len() != ids.len() || spans.iter().any(Option::is_none) {
            return effects;
        }
        let spans: Vec<ItemSpan> = spans.iter().flatten().copied().collect();
        self.drag = Some(Drag {
            id: id.to_string(),
            items: ids.to_vec(),
            from,
            spans,
            start_position: position,
            pointer_position: position,
            scroll_start: scroll,
            scroll,
            duration: duration.max(0.0),
            active: false,
            settling: false,
            destination: from,
            over_external: false,
            transforms: vec![None; ids.len()],
            handle_follows: false,
            finish: None,
        });
        effects
    }

    /// `onMove`. `over_external` is what `externalDrop.onMove` answered.
    /// Returns whether anything visible changed.
    pub fn pointer_move(&mut self, position: f32, over_external: bool) -> bool {
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        if drag.settling {
            return false;
        }
        drag.pointer_position = position;
        if !drag.active {
            if (position - drag.start_position).abs() < THRESHOLD {
                return false;
            }
            drag.active = true;
            drag.handle_follows = true;
            self.dragging_id = Some(drag.id.clone());
        }
        drag.over_external = over_external;
        if over_external {
            let from = drag.from;
            drag.preview(from);
            return true;
        }
        drag.paint(true);
        true
    }

    /// The strip scrolled to `scroll` (`scrollLeft` or `scrollTop`).
    pub fn scroll(&mut self, scroll: f32) -> bool {
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        drag.scroll = scroll;
        if drag.active && !drag.settling && !drag.over_external {
            drag.paint(true);
            return true;
        }
        false
    }

    /// `onUp`: the last move, then the drop. `over_external` and
    /// `dropped_external` are `externalDrop.onMove` and `onDrop` at the
    /// release point.
    pub fn release(
        &mut self,
        position: f32,
        over_external: bool,
        dropped_external: bool,
        now: f64,
    ) -> Vec<ReorderEffect> {
        if self.drag.is_none() {
            return Vec::new();
        }
        self.pointer_move(position, over_external);
        self.stop(true, dropped_external, now)
    }

    /// Escape, `pointercancel`, or window blur.
    pub fn cancel(&mut self, now: f64) -> Vec<ReorderEffect> {
        self.stop(false, false, now)
    }

    /// Ends the settle animation once its time is up.
    pub fn tick(&mut self, now: f64) -> Vec<ReorderEffect> {
        match self.settle_deadline() {
            Some(at) if now >= at => self.finish_now(now),
            _ => Vec::new(),
        }
    }

    /// `finishSettling`: commit a settling drop right away, because a new
    /// press may change the list.
    pub fn finish_now(&mut self, now: f64) -> Vec<ReorderEffect> {
        let Some((_, commit, to)) = self.drag.as_ref().and_then(|drag| drag.finish) else {
            return Vec::new();
        };
        let drag = self.drag.clone().expect("a settling drag");
        let mut effects = self.reset(now);
        if commit && to != drag.from {
            effects.push(ReorderEffect::Reorder {
                ids: move_item(&drag.items, drag.from, to),
                moved_id: drag.id,
            });
        }
        effects
    }

    /// `consumeClick`: true while a click right after a drag should be
    /// ignored.
    pub fn consume_click(&self, now: f64) -> bool {
        now < self.suppress_click_until
    }

    fn stop(&mut self, commit: bool, dropped_external: bool, now: f64) -> Vec<ReorderEffect> {
        let Some(drag) = self.drag.as_mut() else {
            return Vec::new();
        };
        if drag.settling {
            return Vec::new();
        }
        if !drag.active {
            return self.reset(now);
        }
        if commit && dropped_external {
            return self.reset(now);
        }
        drag.settling = true;
        if commit {
            drag.paint(false);
        }
        let duration = drag.duration;
        self.suppress_click_until = now + duration + CLICK_SUPPRESS_MS;
        let to = if commit { drag.destination } else { drag.from };
        drag.preview(to);
        drag.handle_follows = false;
        drag.transforms[drag.from] = Some(drag.spans[to].start - drag.spans[drag.from].start);
        drag.finish = Some((now + duration, commit, to));
        if duration == 0.0 {
            return self.finish_now(now);
        }
        Vec::new()
    }

    fn reset(&mut self, now: f64) -> Vec<ReorderEffect> {
        let Some(drag) = self.drag.take() else {
            return Vec::new();
        };
        if drag.active {
            self.suppress_click_until = now + CLICK_SUPPRESS_MS;
        }
        self.dragging_id = None;
        vec![ReorderEffect::End { id: drag.id }]
    }
}

/// `reorderMotion`: the reorder transition, or none under reduced motion.
pub fn reorder_duration(theme: &monocode_ui::Theme, reduce_motion: bool) -> f64 {
    if reduce_motion {
        0.0
    } else {
        theme.motion.reorder.as_secs_f64() * 1000.0
    }
}

#[derive(Debug, Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    easing: CubicBezier,
}

impl Tween {
    fn value(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return self.to;
        }
        let t =
            now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        if t >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * self.easing.ease(t)
    }

    fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration
    }
}

/// The CSS `transform` transition between item offsets.
#[derive(Debug, Clone, Default)]
pub struct OffsetTweens {
    tweens: HashMap<String, Tween>,
}

impl OffsetTweens {
    /// Points `id` at `to`. With `animate`, the offset eases there from its
    /// current value; otherwise it jumps.
    pub fn target(
        &mut self,
        id: &str,
        to: f32,
        animate: bool,
        duration: Duration,
        easing: CubicBezier,
        now: Instant,
    ) {
        let current = self.value(id, now);
        if let Some(tween) = self.tweens.get(id)
            && tween.to == to
            && (animate || tween.done(now))
        {
            return;
        }
        let duration = if animate { duration } else { Duration::ZERO };
        self.tweens.insert(
            id.to_string(),
            Tween {
                from: current,
                to,
                start: now,
                duration,
                easing,
            },
        );
    }

    /// Drops every offset, as `removeProperty("transform")` did.
    pub fn clear(&mut self) {
        self.tweens.clear();
    }

    pub fn value(&self, id: &str, now: Instant) -> f32 {
        self.tweens.get(id).map_or(0.0, |tween| tween.value(now))
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.tweens.values().any(|tween| !tween.done(now))
    }
}

#[cfg(test)]
mod tests {
    //! Port of useAnimatedReorder.test.ts. The concurrency test
    //! (useAnimatedReorderConcurrency.test.ts) covers React suspense and has
    //! no equivalent here: the owner always passes the committed ids.

    use super::*;

    const DURATION: f64 = 160.0;

    fn ids() -> Vec<String> {
        ["sessions", "changes", "explorer"]
            .map(str::to_string)
            .to_vec()
    }

    /// Three 100px tabs on x, or three 32px rows 33px apart on y.
    fn spans(axis: Axis) -> Vec<Option<ItemSpan>> {
        match axis {
            Axis::X => (0..3)
                .map(|i| Some(ItemSpan::new(i as f32 * 100.0, 100.0)))
                .collect(),
            Axis::Y => (0..3)
                .map(|i| Some(ItemSpan::new(i as f32 * 33.0, 32.0)))
                .collect(),
        }
    }

    struct Setup {
        reorder: AnimatedReorder,
        axis: Axis,
        duration: f64,
        now: f64,
        reordered: Vec<(Vec<String>, String)>,
    }

    impl Setup {
        fn new(reduced_motion: bool, axis: Axis) -> Self {
            Self {
                reorder: AnimatedReorder::new(axis),
                axis,
                duration: if reduced_motion { 0.0 } else { DURATION },
                now: 0.0,
                reordered: Vec::new(),
            }
        }

        fn take(&mut self, effects: Vec<ReorderEffect>) {
            for effect in effects {
                if let ReorderEffect::Reorder { ids, moved_id } = effect {
                    self.reordered.push((ids, moved_id));
                }
            }
        }

        fn press(&mut self, index: usize) {
            let position = match self.axis {
                Axis::X => index as f32 * 100.0 + 50.0,
                Axis::Y => index as f32 * 33.0 + 16.0,
            };
            let effects = self.reorder.press(
                &ids()[index],
                &ids(),
                &spans(self.axis),
                PressPoint {
                    position,
                    scroll: 0.0,
                },
                self.duration,
                self.now,
            );
            self.take(effects);
        }

        fn advance(&mut self, ms: f64) {
            self.now += ms;
            let effects = self.reorder.tick(self.now);
            self.take(effects);
        }

        fn run_all(&mut self) {
            if let Some(at) = self.reorder.settle_deadline() {
                self.advance((at - self.now).max(0.0));
            }
        }

        fn up(&mut self, position: f32) {
            let effects = self.reorder.release(position, false, false, self.now);
            self.take(effects);
        }

        fn offset(&self, index: usize) -> Option<f32> {
            self.reorder.offset(&ids()[index])
        }

        fn reordered_to(&self, order: [&str; 3], moved: &str) -> bool {
            self.reordered == vec![(order.map(str::to_string).to_vec(), moved.to_string())]
        }
    }

    #[test]
    fn keeps_the_dragged_item_under_the_pointer_when_scrolling() {
        for (axis, position, scroll, offset) in
            [(Axis::X, 150.0, 100.0, 200.0), (Axis::Y, 49.0, 33.0, 66.0)]
        {
            let mut setup = Setup::new(false, axis);
            setup.press(0);
            setup.reorder.pointer_move(position, false);
            setup.advance(16.0);
            setup.reorder.scroll(scroll);
            setup.advance(16.0);
            assert_eq!(setup.offset(0), Some(offset));
            setup.up(position);
            setup.run_all();
            assert!(setup.reordered_to(["changes", "explorer", "sessions"], "sessions"));
        }
    }

    #[test]
    fn releases_drag_feedback_immediately_while_the_drop_animation_finishes() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        setup.reorder.pointer_move(180.0, false);
        setup.advance(16.0);
        assert!(setup.reorder.is_grabbing());
        setup.up(180.0);
        assert!(!setup.reorder.is_grabbing());
        assert!(setup.reordered.is_empty());
        setup.run_all();
        assert!(setup.reordered_to(["changes", "sessions", "explorer"], "sessions"));
    }

    #[test]
    fn previews_and_reorders_vertical_project_rows() {
        let mut setup = Setup::new(false, Axis::Y);
        setup.press(0);
        setup.reorder.pointer_move(60.0, false);
        setup.advance(16.0);
        assert_eq!(setup.offset(0), Some(44.0));
        assert_eq!(setup.offset(1), Some(-33.0));
        assert!(setup.reordered.is_empty());
        setup.up(90.0);
        setup.run_all();
        assert!(setup.reordered_to(["changes", "explorer", "sessions"], "sessions"));
    }

    #[test]
    fn preserves_button_clicks_below_the_drag_threshold() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        setup.reorder.pointer_move(53.0, false);
        assert!(!setup.reorder.is_grabbing());
        setup.up(53.0);
        assert!(!setup.reorder.consume_click(setup.now));
        assert!(setup.reordered.is_empty());
    }

    #[test]
    fn reorders_on_drop_and_permits_a_fresh_click_immediately_afterwards() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        setup.reorder.pointer_move(160.0, false);
        assert!(setup.reorder.is_grabbing());
        assert!(setup.reordered.is_empty());
        setup.up(160.0);
        assert!(setup.reorder.consume_click(setup.now));
        setup.run_all();
        assert!(setup.reordered_to(["changes", "sessions", "explorer"], "sessions"));
        assert!(!setup.reorder.is_pressed());
        setup.press(2);
        setup.up(250.0);
        assert!(!setup.reorder.consume_click(setup.now));
    }

    #[test]
    fn accepts_a_fresh_click_started_during_settling() {
        for held in [20.0, 140.0] {
            let mut setup = Setup::new(false, Axis::X);
            setup.press(0);
            setup.reorder.pointer_move(160.0, false);
            setup.up(160.0);
            assert!(setup.reorder.consume_click(setup.now));
            setup.advance(60.0);
            setup.press(2);
            setup.advance(held);
            setup.up(250.0);
            assert!(!setup.reorder.consume_click(setup.now));
            setup.run_all();
            assert!(setup.reordered_to(["changes", "sessions", "explorer"], "sessions"));
        }
    }

    #[test]
    fn previews_the_latest_pointer_position_and_makes_room_before_committing() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        setup.reorder.pointer_move(100.0, false);
        setup.reorder.pointer_move(180.0, false);
        setup.advance(16.0);
        assert_eq!(setup.offset(0), Some(130.0));
        assert_eq!(setup.offset(1), Some(-100.0));
        assert_eq!(setup.offset(2), Some(0.0));
        assert!(setup.reordered.is_empty());
    }

    #[test]
    fn moves_the_last_tab_to_the_first_slot_when_released_beyond_the_left_edge() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(2);
        setup.up(-100.0);
        setup.run_all();
        assert!(setup.reordered_to(["explorer", "sessions", "changes"], "explorer"));
    }

    #[test]
    fn cancels_a_drag_on_escape_pointercancel_or_blur() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        setup.reorder.pointer_move(250.0, false);
        setup.advance(16.0);
        let effects = setup.reorder.cancel(setup.now);
        setup.take(effects);
        setup.up(250.0);
        setup.run_all();
        assert!(setup.reordered.is_empty());
        assert!((0..3).all(|index| setup.offset(index).is_none()));
        assert!(!setup.reorder.is_grabbing());
    }

    #[test]
    fn commits_a_fast_drop_without_waiting_for_animation_in_reduced_motion() {
        let mut setup = Setup::new(true, Axis::X);
        setup.press(0);
        setup.up(250.0);
        assert!(setup.reordered_to(["changes", "explorer", "sessions"], "sessions"));
        assert!((0..3).all(|index| setup.offset(index).is_none()));
        assert_eq!(setup.reorder.settle_deadline(), None);
    }

    #[test]
    fn hands_a_drag_to_an_external_target_without_reordering() {
        let mut setup = Setup::new(false, Axis::X);
        setup.press(0);
        // `onMove` and `onDrop` answer true below y = 100; the test drags to
        // (180, 180).
        assert!(setup.reorder.pointer_move(180.0, true));
        let effects = setup.reorder.release(180.0, true, true, setup.now);
        assert_eq!(
            effects,
            vec![ReorderEffect::End {
                id: "sessions".into()
            }]
        );
        setup.take(effects);
        setup.run_all();
        assert!(setup.reordered.is_empty());
        assert!((0..3).all(|index| setup.offset(index).is_none()));
        assert!(setup.reorder.consume_click(setup.now));
    }

    #[test]
    fn tweens_ease_toward_their_target() {
        let start = Instant::now();
        let mut tweens = OffsetTweens::default();
        let ease = CubicBezier(0.22, 1.0, 0.36, 1.0);
        tweens.target("a", 100.0, true, Duration::from_millis(160), ease, start);
        let mid = tweens.value("a", start + Duration::from_millis(80));
        assert!(mid > 50.0 && mid < 100.0, "{mid}");
        assert_eq!(tweens.value("a", start + Duration::from_millis(200)), 100.0);
        tweens.target("a", 0.0, false, Duration::from_millis(160), ease, start);
        assert_eq!(tweens.value("a", start), 0.0);
    }
}
