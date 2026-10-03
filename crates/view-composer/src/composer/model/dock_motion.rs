//! Port of src/features/sessions/ui/useComposerDockMotion.ts: slides the
//! composer from the centered empty-session slot down to the dock when the
//! first message is sent, instead of letting it jump there.
//!
//! The owner (the session pane) holds a [`DockMotion`], calls
//! [`DockMotion::capture_launch`] with the centered composer's bounds on
//! submit, and asks [`DockMotion::docked`] for the starting offset once the
//! docked composer has bounds. It animates that offset to zero over
//! [`DURATION`] with [`EASING`].

use std::time::{Duration, Instant};

use monocode_ui::theme::CubicBezier;

/// A submit only counts as the launch if the composer docks shortly after.
pub const LAUNCH_WINDOW: Duration = Duration::from_millis(1500);
pub const DURATION: Duration = Duration::from_millis(480);
pub const EASING: CubicBezier = CubicBezier(0.22, 1.0, 0.36, 1.0);

/// A rectangle's top, left, and width, as `getBoundingClientRect` gave them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockRect {
    pub top: f32,
    pub left: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
struct Launch {
    top: f32,
    center_x: f32,
    at: Instant,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DockMotion {
    launch: Option<Launch>,
}

impl DockMotion {
    /// `captureLaunch`: remember where the centered composer was.
    pub fn capture_launch(&mut self, centered: Option<DockRect>, now: Instant) {
        self.launch = centered.map(|rect| Launch {
            top: rect.top,
            center_x: rect.left + rect.width / 2.0,
            at: now,
        });
    }

    /// The `(dx, dy)` the docked composer starts from, or `None` when it
    /// should not animate: no launch, reduced motion, a stale launch, or
    /// no distance to cover. Consumes the launch.
    pub fn docked(
        &mut self,
        to: DockRect,
        reduced_motion: bool,
        now: Instant,
    ) -> Option<(f32, f32)> {
        let from = self.launch.take()?;
        if reduced_motion {
            return None;
        }
        if now.duration_since(from.at) > LAUNCH_WINDOW {
            return None;
        }
        // Align centers so the composer drops straight down.
        let dx = from.center_x - (to.left + to.width / 2.0);
        let dy = from.top - to.top;
        if dx.abs() < 1.0 && dy.abs() < 1.0 {
            return None;
        }
        Some((dx, dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: DockRect = DockRect {
        top: 300.0,
        left: 100.0,
        width: 600.0,
    };
    const DOCK: DockRect = DockRect {
        top: 700.0,
        left: 50.0,
        width: 700.0,
    };

    #[test]
    fn drops_the_composer_straight_down_to_the_dock_after_a_submit() {
        let now = Instant::now();
        let mut motion = DockMotion::default();
        motion.capture_launch(Some(CENTER), now);
        assert_eq!(motion.docked(DOCK, false, now), Some((0.0, -400.0)));
    }

    #[test]
    fn does_not_animate_when_the_composer_docks_without_a_submit() {
        let mut motion = DockMotion::default();
        assert_eq!(motion.docked(DOCK, false, Instant::now()), None);
    }

    #[test]
    fn respects_reduced_motion() {
        let now = Instant::now();
        let mut motion = DockMotion::default();
        motion.capture_launch(Some(CENTER), now);
        assert_eq!(motion.docked(DOCK, true, now), None);
    }

    #[test]
    fn ignores_a_launch_that_went_stale() {
        let now = Instant::now();
        let mut motion = DockMotion::default();
        motion.capture_launch(Some(CENTER), now);
        assert_eq!(
            motion.docked(DOCK, false, now + Duration::from_secs(2)),
            None
        );
    }
}
