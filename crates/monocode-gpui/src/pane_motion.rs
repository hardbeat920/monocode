//! Frame-driven pane motion. A critically damped spring gives a quick response
//! and a quiet finish, retaining velocity when an in-progress slide reverses.
use std::{cell::Cell, time::Instant};

pub(crate) struct PaneSlide {
    from: f32,
    pub(crate) to: f32,
    velocity: f32,
    frequency: f32,
    pixels_per_unit: f32,
    limits: (f32, f32),
    start: Cell<Option<Instant>>,
    #[cfg(test)]
    fixed_elapsed: Cell<Option<std::time::Duration>>,
}

impl PaneSlide {
    pub(crate) fn new(from: f32, to: f32, distance: f32) -> Self {
        let pace = (distance.abs() / 360.).sqrt().clamp(1., 1.35);
        let opening = to > from;
        Self {
            from,
            to,
            velocity: (to - from) * if opening { 12. } else { 13.5 } / pace,
            frequency: if opening { 34. } else { 40. } / pace,
            pixels_per_unit: if (to - from).abs() > f32::EPSILON {
                (distance / (to - from)).abs().max(1.)
            } else {
                1.
            },
            limits: (from.min(to), from.max(to)),
            start: Cell::new(None),
            #[cfg(test)]
            fixed_elapsed: Cell::new(None),
        }
    }

    pub(crate) fn opening(&self) -> bool {
        self.to > self.from
    }

    pub(crate) fn retarget(&self, to: f32, now: Instant) -> Self {
        let (from, velocity, _) = self.sample(now);
        let distance = (to - from) * self.pixels_per_unit;
        let mut next = Self::new(from, to, distance);
        next.velocity = if distance.abs() < 0.2 { 0. } else { velocity };
        next.pixels_per_unit = self.pixels_per_unit;
        next.limits = (self.limits.0.min(to), self.limits.1.max(to));
        next
    }

    pub(crate) fn at(&self, now: Instant) -> (f32, bool) {
        let (value, _, done) = self.sample(now);
        (value, done)
    }

    fn sample(&self, now: Instant) -> (f32, f32, bool) {
        // Starting at first paint prevents construction/loading time from
        // skipping the opening frames. No timer competes with display refresh.
        let start = self.start.get().unwrap_or(now);
        self.start.set(Some(start));
        let t = now.saturating_duration_since(start).as_secs_f32();
        #[cfg(test)]
        let t = self
            .fixed_elapsed
            .get()
            .map_or(t, |elapsed| elapsed.as_secs_f32());
        let displacement = self.from - self.to;
        let c = self.velocity + self.frequency * displacement;
        let decay = (-self.frequency * t).exp();
        let value = self.to + (displacement + c * t) * decay;
        let velocity = (self.velocity - self.frequency * c * t) * decay;
        let done = (value - self.to).abs() * self.pixels_per_unit < 0.2
            && velocity.abs() * self.pixels_per_unit < 5.;
        if done {
            return (self.to, 0., true);
        }
        if value < self.limits.0 {
            return (self.limits.0, velocity.max(0.), false);
        }
        if value > self.limits.1 {
            return (self.limits.1, velocity.min(0.), false);
        }
        (value, velocity, false)
    }

    #[cfg(test)]
    pub(crate) fn elapsed(&self, elapsed: std::time::Duration) {
        self.fixed_elapsed.set(None);
        self.start.set(Some(Instant::now() - elapsed));
    }
    #[cfg(test)]
    pub(crate) fn freeze_elapsed(&self, elapsed: std::time::Duration) {
        self.fixed_elapsed.set(Some(elapsed));
    }
}

#[derive(Default)]
pub(crate) struct PaneReveal {
    start: Cell<Option<Instant>>,
}
impl PaneReveal {
    pub(crate) fn opacity(&self, now: Instant) -> (f32, bool) {
        let start = self.start.get().unwrap_or(now);
        self.start.set(Some(start));
        let t = (now.saturating_duration_since(start).as_secs_f32() / 0.14).clamp(0., 1.);
        (1. - (1. - t).powi(3), t >= 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn starts_at_first_frame_and_settles_without_overshoot_at_any_refresh_rate() {
        for hz in [60, 90, 120, 144] {
            for distance in [220., 360., 640.] {
                for opening in [true, false] {
                    let (from, to) = if opening {
                        (0., distance)
                    } else {
                        (distance, 0.)
                    };
                    let slide = PaneSlide::new(from, to, distance);
                    let start = Instant::now() + Duration::from_secs(1);
                    assert_eq!(slide.at(start), (from, false));
                    let mut previous = from;
                    let mut finished = false;
                    for frame in 1..hz {
                        let (value, done) =
                            slide.at(start + Duration::from_secs_f32(frame as f32 / hz as f32));
                        assert!((0. ..=distance).contains(&value));
                        assert!(if opening {
                            value >= previous
                        } else {
                            value <= previous
                        });
                        previous = value;
                        if done {
                            assert_eq!(value, to);
                            finished = true;
                            break;
                        }
                    }
                    assert!(finished);
                }
            }
        }
    }

    #[test]
    fn reversal_keeps_position_and_velocity_then_settles() {
        let slide = PaneSlide::new(0., 360., 360.);
        let start = Instant::now();
        slide.at(start);
        let now = start + Duration::from_millis(70);
        let (position, velocity, _) = slide.sample(now);
        let reversed = slide.retarget(0., now);
        let (next, next_velocity, _) = reversed.sample(now);
        assert!((next - position).abs() < 0.001);
        assert!((next_velocity - velocity).abs() < 0.001);
        assert_eq!(reversed.at(now + Duration::from_secs(1)), (0., true));
        let reopened = reversed.retarget(360., now + Duration::from_millis(40));
        assert_eq!(
            reopened.at(now + Duration::from_secs(2)),
            (reopened.from, false)
        );
        assert_eq!(reopened.at(now + Duration::from_secs(3)), (360., true));
    }

    #[test]
    fn normalized_splits_have_the_same_pixel_motion_as_sidebars() {
        let pixels = PaneSlide::new(0., 480., 480.);
        let ratio = PaneSlide::new(0., 0.5, 480.);
        let start = Instant::now();
        for ms in [0, 16, 50, 100, 180, 300, 450] {
            let now = start + Duration::from_millis(ms);
            let (p, done) = pixels.at(now);
            let (r, ratio_done) = ratio.at(now);
            assert!((p - r * 960.).abs() < 0.001);
            assert_eq!(done, ratio_done);
        }
    }

    #[test]
    fn closing_before_first_paint_stays_closed_and_body_reveal_waits_for_paint() {
        let now = Instant::now();
        let slide = PaneSlide::new(0., 360., 360.);
        assert_eq!(slide.retarget(0., now).at(now), (0., true));
        let reveal = PaneReveal::default();
        let first = now + Duration::from_secs(1);
        assert_eq!(reveal.opacity(first), (0., false));
        assert_eq!(
            reveal.opacity(first + Duration::from_millis(140)),
            (1., true)
        );
    }
}
