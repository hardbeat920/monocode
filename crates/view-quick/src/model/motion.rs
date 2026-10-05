//! Port of src/features/quick-composer/ui/useQuickPickerMotion.ts: the card
//! itself animates, so the native blur and its shadow follow its height.
//!
//! When a picker opens, closes, or changes, the card grows or shrinks from
//! the height it had to its new natural height over 240 ms, and the picker
//! fades in. The picker keeps its own natural height the whole time, so its
//! lists and headers are revealed rather than squeezed. Every height the
//! card takes is reported once, rounded up, so the panel window's bottom
//! edge follows it while the top edge stays put.

use std::time::{Duration, Instant};

use monocode_ui::theme::CubicBezier;

/// `DURATION`.
pub const RESIZE_DURATION: Duration = Duration::from_millis(240);
/// `EASING`: `cubic-bezier(0.22, 1, 0.36, 1)`.
pub const RESIZE_EASING: CubicBezier = CubicBezier(0.22, 1.0, 0.36, 1.0);
/// The picker's fade.
pub const FADE_DURATION: Duration = Duration::from_millis(180);
pub const FADE_DELAY: Duration = Duration::from_millis(40);
/// CSS `ease-out`.
const FADE_EASING: CubicBezier = CubicBezier(0.0, 0.0, 0.58, 1.0);

/// Which list is open under the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Picker {
    Project,
    Model,
    Permissions,
    Attachments,
    Commands,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Resize {
    from: f32,
    to: f32,
    started: Instant,
}

/// The card's height animation and the fit reports.
#[derive(Debug, Clone)]
pub struct PickerMotion {
    picker: Option<Picker>,
    /// `lastHeight`: the card's last measured height.
    last_height: Option<f32>,
    /// `sentHeight`.
    sent: Option<u32>,
    resize: Option<Resize>,
    fade: Option<Instant>,
    reduced_motion: bool,
}

impl Default for PickerMotion {
    fn default() -> Self {
        Self::new(false)
    }
}

impl PickerMotion {
    pub fn new(reduced_motion: bool) -> Self {
        Self {
            picker: None,
            last_height: None,
            sent: None,
            resize: None,
            fade: None,
            reduced_motion,
        }
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.resize = None;
            self.fade = None;
        }
    }

    pub fn picker(&self) -> Option<Picker> {
        self.picker
    }

    /// `fit`: the card measured `height`. Returns the rounded height when
    /// the panel should resize.
    pub fn measured(&mut self, height: f32) -> Option<u32> {
        self.last_height = Some(height);
        let rounded = height.ceil().max(0.0) as u32;
        if self.sent == Some(rounded) {
            return None;
        }
        self.sent = Some(rounded);
        Some(rounded)
    }

    /// The last measured height.
    pub fn last_height(&self) -> Option<f32> {
        self.last_height
    }

    /// The picker changed and the card's natural height is now `to`. An
    /// animation in flight is cancelled and the new one starts from the
    /// height the card last had. Returns true when a resize started.
    pub fn picker_changed(&mut self, picker: Option<Picker>, to: f32, now: Instant) -> bool {
        if self.picker == picker {
            return false;
        }
        self.picker = picker;
        self.resize = None;
        self.fade = None;
        if self.reduced_motion {
            return false;
        }
        if picker.is_some() {
            self.fade = Some(now);
        }
        match self.last_height {
            Some(from) if (from - to).abs() > 0.5 => {
                self.resize = Some(Resize {
                    from,
                    to,
                    started: now,
                });
                true
            }
            _ => false,
        }
    }

    /// The card's natural height changed while a resize runs (content
    /// loaded). The resize retargets without restarting.
    pub fn retarget(&mut self, to: f32) {
        if let Some(resize) = self.resize.as_mut() {
            resize.to = to;
        }
    }

    /// The resize animation, if any: `(from, to)`.
    pub fn resize_span(&self) -> Option<(f32, f32)> {
        self.resize.map(|resize| (resize.from, resize.to))
    }

    /// The card's height at `now`, or `None` when it takes its natural
    /// height.
    pub fn height_at(&mut self, now: Instant) -> Option<f32> {
        let resize = self.resize?;
        let elapsed = now.saturating_duration_since(resize.started);
        if elapsed >= RESIZE_DURATION {
            // `onfinish`: release the picker's fixed height.
            self.resize = None;
            return None;
        }
        let t = elapsed.as_secs_f32() / RESIZE_DURATION.as_secs_f32();
        let eased = RESIZE_EASING.ease(t);
        Some(resize.from + (resize.to - resize.from) * eased)
    }

    /// The picker's opacity at `now`. `fill: backwards` keeps it hidden
    /// through the delay.
    pub fn opacity_at(&mut self, now: Instant) -> f32 {
        let Some(started) = self.fade else {
            return 1.0;
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed < FADE_DELAY {
            return 0.0;
        }
        let run = elapsed - FADE_DELAY;
        if run >= FADE_DURATION {
            self.fade = None;
            return 1.0;
        }
        FADE_EASING.ease(run.as_secs_f32() / FADE_DURATION.as_secs_f32())
    }

    /// Something still moves, so the view should draw another frame.
    pub fn animating(&self) -> bool {
        self.resize.is_some() || self.fade.is_some()
    }

    /// The picker keeps its natural height while the card resizes.
    pub fn holds_picker_height(&self) -> bool {
        self.resize.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The heights useQuickPickerMotion.test.ts gave each picker panel.
    fn natural(picker: Option<Picker>) -> f32 {
        100.0
            + match picker {
                Some(Picker::Model) => 400.0,
                Some(Picker::Project) => 180.0,
                Some(Picker::Commands) => 60.0,
                _ => 0.0,
            }
    }

    #[test]
    fn expands_and_fades_each_picker_while_resizing_the_native_window() {
        for picker in [Picker::Model, Picker::Project, Picker::Commands] {
            let start = Instant::now();
            let mut motion = PickerMotion::default();
            assert_eq!(motion.measured(natural(None)), Some(100));
            assert!(!motion.animating());
            assert!(motion.picker_changed(Some(picker), natural(Some(picker)), start));
            assert_eq!(motion.resize_span(), Some((100.0, natural(Some(picker)))));
            assert!(motion.holds_picker_height());
            assert_eq!(motion.opacity_at(start), 0.0);
            // A frame mid-way reports its own height to the window.
            assert_eq!(motion.measured(220.0), Some(220));
            assert_eq!(motion.measured(220.0), None);
            let mid = motion
                .height_at(start + Duration::from_millis(120))
                .unwrap();
            assert!(mid > 100.0 && mid < natural(Some(picker)));
            // `onfinish` releases the picker's height.
            assert_eq!(motion.height_at(start + RESIZE_DURATION), None);
            assert!(!motion.holds_picker_height());
            assert_eq!(motion.opacity_at(start + Duration::from_millis(400)), 1.0);
            assert!(!motion.animating());
        }
    }

    #[test]
    fn cancels_an_interrupted_expansion_and_starts_from_its_current_height() {
        let start = Instant::now();
        let mut motion = PickerMotion::default();
        motion.measured(100.0);
        motion.picker_changed(Some(Picker::Model), 500.0, start);
        motion.measured(320.0);
        let later = start + Duration::from_millis(60);
        assert!(motion.picker_changed(Some(Picker::Project), 280.0, later));
        assert_eq!(motion.resize_span(), Some((320.0, 280.0)));
        assert_eq!(motion.height_at(later), Some(320.0));
    }

    #[test]
    fn fits_immediately_without_animation_when_reduced_motion_is_enabled() {
        let start = Instant::now();
        let mut motion = PickerMotion::new(true);
        motion.measured(100.0);
        assert!(!motion.picker_changed(Some(Picker::Model), 500.0, start));
        assert!(!motion.animating());
        assert_eq!(motion.height_at(start), None);
        assert_eq!(motion.opacity_at(start), 1.0);
        assert_eq!(motion.measured(500.0), Some(500));
    }

    #[test]
    fn an_unchanged_picker_or_height_does_not_animate() {
        let start = Instant::now();
        let mut motion = PickerMotion::default();
        assert!(!motion.picker_changed(None, 100.0, start));
        motion.measured(100.0);
        assert!(!motion.picker_changed(Some(Picker::Attachments), 100.2, start));
        assert!(motion.animating(), "the picker still fades in");
        assert!(!motion.picker_changed(Some(Picker::Attachments), 300.0, start));
    }
}
