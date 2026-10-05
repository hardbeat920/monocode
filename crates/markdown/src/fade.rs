//! Paced reveal and per-word fade for streaming replies.
//!
//! Port of `src/features/sessions/ui/wordFade.tsx`. Tokens land in uneven
//! bursts, so the text is let out a word at a time at a steady rate that
//! closes on whatever has arrived ([`Pacer`], `usePacedText`), and each word
//! fades in from transparent as it is let out ([`RevealTimeline`], the
//! `word-fade-in` keyframes in index.css: 320ms, `ease-out`).
//!
//! The React version gives every word a span whose CSS animation starts when
//! the span mounts. Here the timeline records, for each reveal step, the
//! source offset it reached and when. A character's opacity follows from the
//! step that revealed it, so the fade survives reparses that move text into
//! a different block. Links, inline code, and code blocks do not fade, as in
//! `UNFADED_TAGS`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long one word takes to fade in (`WORD_FADE_MS`).
pub const WORD_FADE_MS: u64 = 320;
/// The slowest the reveal goes, in characters a second (`REVEAL_MIN_CPS`).
const REVEAL_MIN_CPS: f64 = 90.0;
/// The reveal closes on what has arrived over about this long
/// (`REVEAL_CATCHUP_S`).
const REVEAL_CATCHUP_S: f64 = 0.22;
/// How long a word still being written is held back once the reveal has
/// caught up to it (`REVEAL_HOLD_MS`).
const REVEAL_HOLD: Duration = Duration::from_millis(150);
/// Largest frame step the reveal integrates (`Math.min(0.05, …)`).
const MAX_STEP: Duration = Duration::from_millis(50);

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\t' | b'\r')
}

/// Where to stop revealing `text` for a reveal that has reached `at`: the end
/// of the word `at` falls in, so a word never shows half written. A stream
/// still mid-word holds back at the last whole word; a finished one runs out.
/// Offsets are bytes; spaces are ASCII, so every result is a char boundary.
pub fn reveal_end(text: &str, at: f64, streaming: bool) -> usize {
    let bytes = text.as_bytes();
    let start = at.max(0.0).ceil() as usize;
    if let Some(ix) = bytes
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(ix, b)| is_space(*b).then_some(ix))
    {
        return ix;
    }
    if !streaming {
        return text.len();
    }
    let mut end = text.len();
    while end > 0 && !is_space(bytes[end - 1]) {
        end -= 1;
    }
    end
}

/// The inputs of the React effect: it restarts whenever one changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RunKey {
    len: usize,
    streaming: bool,
    behind: bool,
}

#[derive(Clone, Debug)]
struct Run {
    key: RunKey,
    position: f64,
    last: Instant,
    hold_until: Option<Instant>,
    done: bool,
}

/// The paced reveal (`usePacedText`). Text that is already there when the
/// pacer is created, or that changes while nothing is streaming, shows at
/// once; only what streams in is paced, and a stream that ends ahead of the
/// reveal is still let out at pace.
#[derive(Clone, Debug)]
pub struct Pacer {
    shown: usize,
    pacing: bool,
    run: Option<Run>,
}

impl Pacer {
    pub fn new(initial_len: usize, streaming: bool) -> Self {
        Self {
            shown: initial_len,
            pacing: streaming,
            run: None,
        }
    }

    /// Bytes of the text to show right now.
    pub fn shown(&self) -> usize {
        self.shown
    }

    /// Advance the reveal to `now` for the current `text`. Call it whenever
    /// the text changes and on every animation frame while
    /// [`Self::needs_frame`] is true. Returns the byte length to show.
    pub fn advance(&mut self, text: &str, streaming: bool, now: Instant) -> usize {
        if streaming {
            self.pacing = true;
        }
        if !self.pacing {
            self.shown = text.len();
            self.run = None;
            return self.shown;
        }
        self.shown = self.shown.min(text.len());
        while !text.is_char_boundary(self.shown) {
            self.shown -= 1;
        }
        let behind = self.shown < text.len();
        let key = RunKey {
            len: text.len(),
            streaming,
            behind,
        };
        if self.run.as_ref().is_none_or(|run| run.key != key) {
            if !behind {
                if !streaming {
                    self.pacing = false;
                }
                self.run = None;
                return self.shown;
            }
            self.run = Some(Run {
                key,
                position: self.shown as f64,
                last: now,
                hold_until: None,
                done: false,
            });
        }
        let Some(run) = self.run.as_mut() else {
            return self.shown;
        };
        if run.done {
            return self.shown;
        }
        if let Some(hold_until) = run.hold_until {
            if now >= hold_until {
                self.shown = text.len();
                run.done = true;
            }
            return self.shown;
        }
        let dt = now.saturating_duration_since(run.last).min(MAX_STEP);
        run.last = now;
        let len = text.len() as f64;
        let backlog = len - run.position;
        let speed = REVEAL_MIN_CPS.max(backlog / REVEAL_CATCHUP_S);
        run.position = len.min(run.position + speed * dt.as_secs_f64());
        let end = reveal_end(text, run.position, streaming);
        if end > self.shown {
            self.shown = end;
        }
        if run.position >= len {
            if self.shown < text.len() {
                run.hold_until = Some(now + REVEAL_HOLD);
            } else {
                run.done = true;
            }
        }
        self.shown
    }

    /// Whether the reveal is behind the text (`revealing`).
    pub fn is_revealing(&self, text_len: usize) -> bool {
        self.shown < text_len
    }

    /// Whether another frame would move the reveal.
    pub fn needs_frame(&self) -> bool {
        self.run.as_ref().is_some_and(|run| !run.done)
    }

    /// When a held word is due, if the reveal is waiting on one.
    pub fn hold_deadline(&self) -> Option<Instant> {
        self.run.as_ref().and_then(|run| run.hold_until)
    }
}

/// Whether a reply's words may fade (`useWordFading`): while it streams or is
/// being let out, and for one fade after, so the last word finishes.
#[derive(Clone, Debug, Default)]
pub struct FadeGate {
    last_active: Option<Instant>,
}

impl FadeGate {
    pub fn fading(&mut self, active: bool, now: Instant, fade: Duration) -> bool {
        if active {
            self.last_active = Some(now);
            return true;
        }
        self.last_active
            .is_some_and(|last| now.saturating_duration_since(last) < fade)
    }
}

/// CSS `ease-out`, `cubic-bezier(0, 0, 0.58, 1)`, at time fraction `x`.
pub fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let (x1, y1, x2, y2) = (0.0f32, 0.0f32, 0.58f32, 1.0f32);
    let bezier = |t: f32, p1: f32, p2: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    // Bisection on the x curve, which is monotonic.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..30 {
        let value = bezier(t, x1, x2);
        if (value - x).abs() < 1e-5 {
            break;
        }
        if value < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    bezier(t, y1, y2)
}

/// When each part of the source was revealed, for the per-word fade.
#[derive(Clone, Debug)]
pub struct RevealTimeline {
    /// Source text before this offset is fully opaque.
    baseline: usize,
    /// `(end, time)`: the reveal reached `end` at `time`. Ends increase.
    marks: VecDeque<(usize, Instant)>,
    fade: Duration,
}

impl RevealTimeline {
    pub fn new(baseline: usize, fade: Duration) -> Self {
        Self {
            baseline,
            marks: VecDeque::new(),
            fade,
        }
    }

    pub fn set_fade(&mut self, fade: Duration) {
        self.fade = fade;
    }

    /// The offset everything before which is fully opaque.
    pub fn baseline(&self) -> usize {
        self.baseline
    }

    /// The reveal end at the newest mark.
    pub fn end(&self) -> usize {
        self.marks.back().map_or(self.baseline, |(end, _)| *end)
    }

    /// Record that the reveal reached `shown` at `now`. A shorter `shown`
    /// means the text was replaced, so nothing fades.
    pub fn record(&mut self, shown: usize, now: Instant) {
        let end = self.end();
        if shown > end {
            self.marks.push_back((shown, now));
        } else if shown < end {
            self.marks.clear();
            self.baseline = shown;
        }
    }

    /// Settle everything and start over with `baseline` as the opaque end.
    pub fn reset(&mut self, baseline: usize) {
        self.baseline = baseline;
        self.marks.clear();
    }

    /// Treat everything revealed so far as settled.
    pub fn settle(&mut self) {
        self.baseline = self.end();
        self.marks.clear();
    }

    /// Drop marks whose fade is over.
    pub fn prune(&mut self, now: Instant) {
        while let Some(&(end, at)) = self.marks.front() {
            if now.saturating_duration_since(at) >= self.fade {
                self.baseline = end;
                self.marks.pop_front();
            } else {
                break;
            }
        }
    }

    /// Whether any word is still fading at `now`.
    pub fn is_fading(&self, now: Instant) -> bool {
        self.marks
            .back()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) < self.fade)
    }

    /// The source range whose characters may be partly transparent.
    pub fn fading_range(&self) -> std::ops::Range<usize> {
        self.baseline..self.end()
    }

    /// Opacity of the source character at `src`.
    pub fn opacity_at(&self, src: usize, now: Instant) -> f32 {
        if src < self.baseline {
            return 1.0;
        }
        let ix = self.marks.partition_point(|(end, _)| *end <= src);
        let Some(&(_, at)) = self.marks.get(ix) else {
            return 1.0;
        };
        let elapsed = now.saturating_duration_since(at).as_secs_f32();
        ease_out(elapsed / self.fade.as_secs_f32())
    }

    /// Split the source range `src..src + len` at reveal steps. Yields
    /// `(offset within the range, length, opacity)` for each piece whose
    /// opacity is below one.
    pub fn pieces(&self, src: usize, len: usize, now: Instant) -> Vec<(usize, usize, f32)> {
        let end = src + len;
        let mut out = Vec::new();
        if end <= self.baseline || self.marks.is_empty() {
            return out;
        }
        let mut start = src.max(self.baseline);
        let mut ix = self
            .marks
            .partition_point(|(mark_end, _)| *mark_end <= start);
        while start < end {
            let Some(&(mark_end, at)) = self.marks.get(ix) else {
                break;
            };
            let piece_end = mark_end.min(end);
            let elapsed = now.saturating_duration_since(at).as_secs_f32();
            let opacity = ease_out(elapsed / self.fade.as_secs_f32());
            if opacity < 1.0 && piece_end > start {
                out.push((start - src, piece_end - start, opacity));
            }
            start = piece_end;
            ix += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn reveal_end_stops_at_word_ends() {
        let text = "one two three";
        assert_eq!(reveal_end(text, 0.0, true), 3);
        assert_eq!(reveal_end(text, 2.2, true), 3);
        assert_eq!(reveal_end(text, 4.0, true), 7);
        // Mid last word: a stream holds back, a finished reply runs out.
        assert_eq!(reveal_end(text, 9.0, true), 8);
        assert_eq!(reveal_end(text, 9.0, false), 13);
        assert_eq!(reveal_end("", 0.0, true), 0);
    }

    #[test]
    fn text_present_at_start_shows_at_once() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(5, false);
        assert_eq!(pacer.advance("hello", false, t0), 5);
        assert_eq!(pacer.advance("hello world", false, t0), 11);
        assert!(!pacer.needs_frame());
    }

    #[test]
    fn streamed_text_is_let_out_word_by_word() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(0, true);
        let text = "alpha beta gamma delta ";
        // The reveal always shows the whole word its position falls in.
        assert_eq!(pacer.advance(text, true, t0), 5);
        assert!(pacer.needs_frame());
        let mut seen = vec![];
        let mut now = t0;
        for _ in 0..60 {
            now += Duration::from_millis(16);
            let shown = pacer.advance(text, true, now);
            if seen.last() != Some(&shown) {
                seen.push(shown);
            }
        }
        // Every step lands on a word end.
        for end in &seen {
            assert!(*end == text.len() || text.as_bytes()[*end] == b' ', "{end}");
        }
        assert_eq!(*seen.last().unwrap(), text.len());
        assert!(seen.len() >= 3, "{seen:?}");
    }

    #[test]
    fn a_word_still_being_written_shows_after_the_hold() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(0, true);
        let text = "hi ther";
        let mut now = t0;
        // Seven bytes at the 90 cps floor take about 80ms.
        for _ in 0..6 {
            now += Duration::from_millis(16);
            pacer.advance(text, true, now);
        }
        assert_eq!(pacer.shown(), 3);
        let deadline = pacer.hold_deadline().expect("holding");
        pacer.advance(text, true, deadline);
        assert_eq!(pacer.shown(), text.len());
        assert!(!pacer.needs_frame());
    }

    #[test]
    fn a_finished_stream_is_still_paced_then_stops_pacing() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(0, true);
        let text = "word ".repeat(40);
        pacer.advance(&text, true, t0);
        // The stream ends while the reveal is behind.
        let shown = pacer.advance(&text, false, ms(t0, 16));
        assert!(shown < text.len());
        let mut now = ms(t0, 16);
        while pacer.is_revealing(text.len()) {
            now += Duration::from_millis(16);
            pacer.advance(&text, false, now);
            assert!(now < ms(t0, 5000));
        }
        // Caught up and not streaming: later changes show at once.
        pacer.advance(&text, false, now);
        let more = format!("{text}tail");
        assert_eq!(pacer.advance(&more, false, now), more.len());
    }

    #[test]
    fn a_backlog_closes_with_the_catchup_time_constant() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(0, true);
        let text = "x ".repeat(500);
        let mut now = t0;
        pacer.advance(&text, true, now);
        // After one time constant (0.22s) about 63% of the backlog is shown.
        for _ in 0..14 {
            now += Duration::from_millis(16);
            pacer.advance(&text, true, now);
        }
        let shown = pacer.shown() as f64 / text.len() as f64;
        assert!((0.5..0.75).contains(&shown), "{shown}");
        while pacer.is_revealing(text.len()) {
            now += Duration::from_millis(16);
            pacer.advance(&text, true, now);
            assert!(now < t0 + Duration::from_millis(1500));
        }
    }

    #[test]
    fn gate_lingers_for_one_fade() {
        let t0 = Instant::now();
        let fade = Duration::from_millis(WORD_FADE_MS);
        let mut gate = FadeGate::default();
        assert!(!gate.fading(false, t0, fade));
        assert!(gate.fading(true, t0, fade));
        assert!(gate.fading(false, ms(t0, 300), fade));
        assert!(!gate.fading(false, ms(t0, 330), fade));
    }

    #[test]
    fn ease_out_matches_css_endpoints() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        let mid = ease_out(0.5);
        assert!(mid > 0.5 && mid < 0.9, "{mid}");
        assert!(ease_out(0.2) < ease_out(0.4));
    }

    #[test]
    fn timeline_fades_each_step_once() {
        let t0 = Instant::now();
        let fade = Duration::from_millis(WORD_FADE_MS);
        let mut timeline = RevealTimeline::new(4, fade);
        timeline.record(9, t0);
        timeline.record(15, ms(t0, 100));
        assert_eq!(timeline.opacity_at(2, t0), 1.0);
        assert_eq!(timeline.opacity_at(5, t0), 0.0);
        let older = timeline.opacity_at(5, ms(t0, 160));
        let newer = timeline.opacity_at(12, ms(t0, 160));
        assert!(older > newer && newer > 0.0);
        let pieces = timeline.pieces(0, 20, ms(t0, 160));
        assert_eq!(pieces.len(), 2);
        assert_eq!((pieces[0].0, pieces[0].1), (4, 5));
        assert_eq!((pieces[1].0, pieces[1].1), (9, 6));
        timeline.prune(ms(t0, 330));
        assert_eq!(timeline.baseline(), 9);
        assert!(timeline.is_fading(ms(t0, 330)));
        timeline.prune(ms(t0, 500));
        assert!(!timeline.is_fading(ms(t0, 500)));
        assert!(timeline.pieces(0, 20, ms(t0, 500)).is_empty());
    }

    #[test]
    fn timeline_resets_when_text_shrinks() {
        let t0 = Instant::now();
        let mut timeline = RevealTimeline::new(0, Duration::from_millis(WORD_FADE_MS));
        timeline.record(10, t0);
        timeline.record(3, t0);
        assert_eq!(timeline.baseline(), 3);
        assert!(!timeline.is_fading(t0));
    }
}
