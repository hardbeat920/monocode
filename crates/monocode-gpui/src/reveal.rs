//! Native port of wordFade.tsx: 90 cps floor, 220ms catch-up, 150ms
//! partial-word hold, 30ms commits and a 320ms opacity fade per word.
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, size,
};
use std::{
    collections::VecDeque,
    ops::Range,
    time::{Duration, Instant},
};

/// Measure the expanded child at the current width before interpolating height.
/// Estimates or heights from an earlier width cause a snap when motion finishes.
pub(crate) struct Disclosure {
    child: AnyElement,
    width: Pixels,
    progress: f32,
}
impl Disclosure {
    pub(crate) fn new(child: impl IntoElement, width: Pixels, progress: f32) -> Self {
        Self {
            child: child.into_any_element(),
            width,
            progress,
        }
    }
}
impl IntoElement for Disclosure {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Disclosure {
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
        let natural = self.child.layout_as_root(
            size(
                AvailableSpace::Definite(self.width),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let mut style = Style::default();
        style.size.width = self.width.into();
        style.size.height = (natural.height * self.progress).into();
        (window.request_layout(style, [], cx), ())
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
        self.child.prepaint_at(bounds.origin, window, cx);
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

pub(crate) const WORD_FADE: Duration = Duration::from_millis(320);
struct FadeGroup {
    words: Range<usize>,
    born: Instant,
    duration: Duration,
    opacity: f32,
}

/// Only the short, still-fading suffix needs timing data. Words committed
/// together share a timestamp and one easing calculation per frame.
#[derive(Default)]
pub(crate) struct WordFades {
    groups: VecDeque<FadeGroup>,
    words: usize,
}
impl WordFades {
    pub fn advance(&mut self, now: Instant) {
        self.groups
            .retain(|group| now.duration_since(group.born) < group.duration);
        for group in &mut self.groups {
            group.opacity = bezier(
                now.duration_since(group.born).as_secs_f32() / group.duration.as_secs_f32(),
                0.,
                0.,
                0.58,
                1.,
            );
        }
    }
    fn append(&mut self, count: usize, now: Instant, duration: Duration, animate: bool) {
        self.advance(now);
        if count < self.words {
            self.groups.retain(|group| group.words.start < count);
            if let Some(group) = self.groups.back_mut() {
                group.words.end = group.words.end.min(count);
            }
        } else if animate && count > self.words {
            self.groups.push_back(FadeGroup {
                words: self.words..count,
                born: now,
                duration,
                opacity: 0.,
            });
        }
        self.words = count;
    }
    pub fn opacity(&self, word: usize) -> f32 {
        self.groups
            .get(self.groups.partition_point(|group| group.words.end <= word))
            .filter(|group| group.words.contains(&word))
            .map_or(1., |group| group.opacity)
    }
    pub fn intersects(&self, words: Range<usize>) -> bool {
        words.start < words.end
            && self
                .groups
                .iter()
                .any(|group| group.words.start < words.end && group.words.end > words.start)
    }
    fn active(&self, now: Instant) -> bool {
        self.groups
            .iter()
            .any(|group| now.duration_since(group.born) < group.duration)
    }
}
pub(crate) struct PacedText {
    target: String,
    shown: usize,
    position: f64,
    last: Instant,
    burst: Instant,
    animate: bool,
    gap_ms: f64,
    skip_fade: bool,
    pub fades: WordFades,
}
impl PacedText {
    pub fn new(text: &str, animate: bool, now: Instant) -> Self {
        Self {
            target: text.into(),
            shown: if animate { 0 } else { text.len() },
            position: if animate { 0. } else { text.len() as f64 },
            last: now,
            burst: now,
            animate,
            gap_ms: 160.,
            skip_fade: false,
            fades: WordFades::default(),
        }
    }
    pub fn update(&mut self, text: &str, now: Instant) {
        if self.target == text {
            return;
        }
        if !text.starts_with(&self.target) {
            self.shown = 0;
            self.position = 0.;
            self.fades = WordFades::default();
            self.target.clear();
        } else {
            self.gap_ms = self.gap_ms * 0.7
                + (now.duration_since(self.burst).as_secs_f64() * 1000.).min(1000.) * 0.3;
        }
        self.target.push_str(&text[self.target.len()..]);
        self.burst = now;
        if !self.animate {
            self.shown = text.len();
            self.position = text.len() as f64;
        }
    }
    pub fn text(&self) -> &str {
        &self.target[..self.shown]
    }
    pub fn behind(&self) -> bool {
        self.shown < self.target.len()
    }
    pub fn step(&mut self, now: Instant, streaming: bool) -> bool {
        if !self.behind() || now.duration_since(self.last) < Duration::from_millis(30) {
            return false;
        }
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        let backlog = self.target.len() as f64 - self.position;
        self.position = (self.position + (90. * dt).max(backlog * (1. - (-dt / 0.22).exp())))
            .min(self.target.len() as f64);
        let mut at = (self.position.ceil() as usize).min(self.target.len());
        while at < self.target.len() && !self.target.is_char_boundary(at) {
            at += 1;
        }
        let end = self.target[at..]
            .char_indices()
            .find(|(_, c)| c.is_ascii_whitespace())
            .map(|(i, _)| at + i)
            .unwrap_or_else(|| {
                if !streaming || now.duration_since(self.burst) >= Duration::from_millis(150) {
                    self.target.len()
                } else {
                    self.target
                        .char_indices()
                        .rev()
                        .find(|(_, c)| c.is_ascii_whitespace())
                        .map_or(0, |(i, c)| i + c.len_utf8())
                }
            });
        if end > self.shown {
            self.shown = end;
            return true;
        }
        false
    }
    pub fn words(&mut self, count: usize, now: Instant) {
        self.fades.append(
            count,
            now,
            Duration::from_secs_f64((self.gap_ms * 0.003).clamp(0.12, WORD_FADE.as_secs_f64())),
            self.animate && !self.skip_fade,
        );
        self.skip_fade = false;
    }
    pub fn active(&self, now: Instant) -> bool {
        self.behind() || self.fades.active(now)
    }
    pub fn settle(&mut self, _now: Instant) {
        self.shown = self.target.len();
        self.position = self.shown as f64;
        self.fades.groups = VecDeque::new();
        self.skip_fade = true;
    }
}
pub(crate) fn bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if t <= 0. {
        return 0.;
    }
    if t >= 1. {
        return 1.;
    }
    let f = |s: f32, a: f32, b: f32| {
        3. * (1. - s) * (1. - s) * s * a + 3. * (1. - s) * s * s * b + s * s * s
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..12 {
        let middle = (low + high) * 0.5;
        if f(middle, x1, x2) < t {
            low = middle;
        } else {
            high = middle;
        }
    }
    f((low + high) * 0.5, y1, y2)
}

#[derive(Clone)]
pub(crate) struct Motion {
    pub start: Instant,
    pub opening: bool,
    pub duration: Duration,
    pub step: bool,
    pub group: Option<usize>,
    opacity_duration: Duration,
    rotation_duration: Duration,
    rotation_from: f32,
    from: (f32, f32),
}
impl Motion {
    pub fn new(opening: bool, step: bool, now: Instant) -> Self {
        Self {
            start: now,
            opening,
            step,
            group: None,
            from: if opening { (0., 0.) } else { (1., 1.) },
            duration: Duration::from_millis(if step { 480 } else { 340 }),
            opacity_duration: Duration::from_millis(340),
            rotation_duration: Duration::from_millis(200),
            rotation_from: if opening { 0. } else { 1. },
        }
    }
    /// .zen-phase-body: the whole body reveals over 320ms, with an
    /// independent 220ms CSS `ease` fade.
    pub fn phase(group: usize, opening: bool, now: Instant) -> Self {
        Self {
            group: Some(group),
            duration: Duration::from_millis(320),
            opacity_duration: Duration::from_millis(220),
            ..Self::new(opening, false, now)
        }
    }
    pub fn value(&self, now: Instant) -> (f32, f32) {
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0., 1.);
        if self.step {
            let height = bezier((t / 0.4).min(1.), 0.22, 1., 0.36, 1.);
            let opacity = bezier(((t - 0.5) / 0.5).clamp(0., 1.), 0.16, 1., 0.3, 1.);
            (height, opacity)
        } else {
            let value = bezier(t, 0.22, 1., 0.36, 1.);
            let fade = if self.group.is_some() {
                let t = (now.saturating_duration_since(self.start).as_secs_f32()
                    / self.opacity_duration.as_secs_f32())
                .clamp(0., 1.);
                bezier(t, 0.25, 0.1, 0.25, 1.)
            } else {
                value
            };
            let target = if self.opening { 1. } else { 0. };
            (
                self.from.0 + (target - self.from.0) * value,
                self.from.1 + (target - self.from.1) * fade,
            )
        }
    }
    pub fn rotation(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.rotation_duration.as_secs_f32())
        .clamp(0., 1.);
        let target = if self.opening { 1. } else { 0. };
        self.rotation_from + (target - self.rotation_from) * bezier(t, 0.4, 0., 0.2, 1.)
    }
    pub fn finished(&self, now: Instant) -> bool {
        let duration = if self.step {
            self.duration
        } else {
            self.duration
                .max(self.opacity_duration)
                .max(if self.group.is_some() {
                    self.rotation_duration
                } else {
                    Duration::ZERO
                })
        };
        now.saturating_duration_since(self.start) >= duration && now >= self.start
    }
    pub fn redirect(&self, opening: bool, now: Instant) -> Self {
        let from = self.value(now);
        let target = if opening { 1. } else { 0. };
        let height_seconds = if self.group.is_some() { 0.32 } else { 0.34 };
        let duration =
            Duration::from_secs_f32((height_seconds * (target - from.0).abs()).max(0.06));
        Self {
            start: now,
            opening,
            step: false,
            group: self.group,
            from,
            rotation_from: self.rotation(now),
            rotation_duration: Duration::from_secs_f32(
                (0.2 * (target - self.rotation(now)).abs()).max(0.06),
            ),
            duration,
            opacity_duration: if self.group.is_some() {
                Duration::from_secs_f32((0.22 * (target - from.1).abs()).max(0.06))
            } else {
                duration
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_state_tracks_active_groups_instead_of_all_words() {
        let now = Instant::now();
        let mut fades = WordFades::default();
        for n in 1..=500 {
            fades.append(
                n * 200,
                now + Duration::from_millis(n as u64 * 33),
                WORD_FADE,
                true,
            );
            assert!(fades.groups.len() <= 10);
        }
        assert_eq!(fades.words, 100_000);
        assert_eq!(fades.opacity(0), 1.);
        let end = now + Duration::from_secs(18);
        fades.advance(end);
        assert!(fades.groups.is_empty());
        fades.append(100_002, end, WORD_FADE, true);
        assert_eq!(fades.groups.len(), 1);
        assert_eq!(fades.groups[0].words, 100_000..100_002);
    }

    #[test]
    fn settling_never_replays_old_words_and_cadence_is_bounded() {
        let now = Instant::now();
        let mut text = PacedText::new("one two", true, now);
        text.words(2, now);
        text.settle(now);
        text.words(2, now);
        assert!(text.fades.groups.is_empty());
        text.update("one two three", now + Duration::from_millis(20));
        text.words(3, now + Duration::from_millis(20));
        assert_eq!(text.fades.groups[0].words, 2..3);
        let duration = text.fades.groups[0].duration;
        assert!((Duration::from_millis(120)..=WORD_FADE).contains(&duration));
        text.update("one two three four", now + Duration::from_secs(2));
        text.words(4, now + Duration::from_secs(2));
        assert_eq!(text.fades.groups[0].duration, WORD_FADE);
    }
    #[test]
    fn phase_matches_react_height_fade_and_chevron_timings_in_both_directions() {
        let start = Instant::now();
        for opening in [true, false] {
            let motion = Motion::phase(7, opening, start);
            let mid = motion.value(start + Duration::from_millis(110));
            let height = bezier(110. / 320., 0.22, 1., 0.36, 1.);
            let fade = bezier(0.5, 0.25, 0.1, 0.25, 1.);
            assert_eq!(
                mid,
                if opening {
                    (height, fade)
                } else {
                    (1. - height, 1. - fade)
                }
            );
            let target = if opening { 1. } else { 0. };
            assert_eq!(motion.rotation(start + Duration::from_millis(200)), target);
            assert_eq!(motion.value(start + Duration::from_millis(220)).1, target);
            assert!(!motion.finished(start + Duration::from_millis(319)));
            assert!(motion.finished(start + Duration::from_millis(320)));
            assert_eq!(
                motion.value(start + Duration::from_millis(320)),
                (target, target)
            );
            let now = start + Duration::from_millis(70);
            let reversed = motion.redirect(!opening, now);
            assert_eq!(reversed.value(now), motion.value(now));
            assert_eq!(reversed.rotation(now), motion.rotation(now));
            assert_eq!(reversed.group, Some(7));
        }
    }
    #[test]
    fn reversing_a_disclosure_keeps_its_current_height_and_opacity() {
        let start = Instant::now();
        let opening = Motion::new(true, false, start);
        let now = start + Duration::from_millis(70);
        let before = opening.value(now);
        let closing = opening.redirect(false, now);
        assert_eq!(closing.value(now), before);
        assert_eq!(closing.value(now + closing.duration), (0., 0.));
        let later = now + Duration::from_millis(25);
        let reopened = closing.redirect(true, later);
        assert_eq!(reopened.value(later), closing.value(later));
        assert_eq!(reopened.value(later + reopened.duration), (1., 1.));
    }
}
