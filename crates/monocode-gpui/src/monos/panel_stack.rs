//! PanelStack.tsx's 280ms slide over a covered, fading overview.
use super::panels::Route;
use std::time::Instant;

const DURATION: f32 = 0.28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pose {
    pub x: f32,
    pub opacity: f32,
}
impl Pose {
    const SHOWN: Self = Self { x: 0., opacity: 1. };
    const COVERED: Self = Self {
        x: -0.18,
        opacity: 0.,
    };
    const OUTSIDE: Self = Self { x: 1., opacity: 1. };
}

struct Motion {
    from: Pose,
    to: Pose,
    start: Option<Instant>,
}
impl Motion {
    fn still(pose: Pose) -> Self {
        Self {
            from: pose,
            to: pose,
            start: None,
        }
    }
    fn sample(&mut self, now: Instant) -> (Pose, bool) {
        if self.from == self.to {
            return (self.to, true);
        }
        let start = *self.start.get_or_insert(now);
        let t = now.saturating_duration_since(start).as_secs_f32() / DURATION;
        if t >= 1. {
            return (self.to, true);
        }
        let p = crate::reveal::bezier(t, 0.2, 0.9, 0.3, 1.);
        (
            Pose {
                x: self.from.x + (self.to.x - self.from.x) * p,
                opacity: self.from.opacity + (self.to.opacity - self.from.opacity) * p,
            },
            false,
        )
    }
    fn retarget(&mut self, to: Pose, now: Instant) {
        if self.to == to {
            return;
        }
        let (from, _) = self.sample(now);
        *self = Self {
            from,
            to,
            start: None,
        };
    }
}

struct Layer {
    route: Route,
    leaving: bool,
    motion: Motion,
}
pub(super) struct Page {
    pub route: Route,
    pub pose: Pose,
    pub interactive: bool,
}
pub(super) struct Frame {
    pub base: Pose,
    pub covered: bool,
    pub pages: Vec<Page>,
    pub animating: bool,
}
pub(super) struct PanelStack {
    mono: Option<String>,
    route: Option<Route>,
    base: Motion,
    layers: Vec<Layer>,
}
impl Default for PanelStack {
    fn default() -> Self {
        Self {
            mono: None,
            route: None,
            base: Motion::still(Pose::SHOWN),
            layers: Vec::new(),
        }
    }
}
impl PanelStack {
    pub fn sync(&mut self, mono: &str, route: Option<Route>, now: Instant, reduced: bool) -> bool {
        let changed = self.mono.as_deref() != Some(mono) || self.route != route;
        if self.mono.as_deref() != Some(mono) {
            *self = Self {
                mono: Some(mono.into()),
                ..Self::default()
            };
        }
        let base = if route.is_some() {
            Pose::COVERED
        } else {
            Pose::SHOWN
        };
        if reduced {
            self.base = Motion::still(base);
            self.layers = route
                .iter()
                .map(|route| Layer {
                    route: route.clone(),
                    leaving: false,
                    motion: Motion::still(Pose::SHOWN),
                })
                .collect();
        } else if self.route != route {
            self.base.retarget(base, now);
            let mut previous = std::mem::take(&mut self.layers);
            if let Some(route) = &route {
                let mut page = previous
                    .iter()
                    .position(|p| &p.route == route)
                    .map(|ix| previous.remove(ix))
                    .unwrap_or_else(|| Layer {
                        route: route.clone(),
                        leaving: false,
                        motion: Motion::still(Pose::OUTSIDE),
                    });
                page.leaving = false;
                page.motion.retarget(Pose::SHOWN, now);
                self.layers.push(page);
            }
            for mut page in previous {
                page.leaving = true;
                page.motion.retarget(Pose::OUTSIDE, now);
                self.layers.push(page);
            }
        }
        self.route = route;
        changed
    }

    pub fn frame(&mut self, now: Instant) -> Frame {
        let (base, done) = self.base.sample(now);
        let mut frame = Frame {
            base,
            covered: self.route.is_some(),
            pages: Vec::new(),
            animating: !done,
        };
        self.layers.retain_mut(|page| {
            let (pose, done) = page.motion.sample(now);
            if page.leaving && done {
                return false;
            }
            frame.animating |= !done;
            frame.pages.push(Page {
                route: page.route.clone(),
                pose,
                interactive: !page.leaving,
            });
            true
        });
        frame
    }

    #[cfg(test)]
    pub(super) fn elapsed(&mut self, elapsed: std::time::Duration) {
        let start = Instant::now() - elapsed;
        self.base.start = Some(start);
        for page in &mut self.layers {
            page.motion.start = Some(start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn pages_slide_over_the_fading_overview_and_back_out() {
        let mut stack = PanelStack::default();
        let now = Instant::now();
        let document = Route::Document("doc".into());
        stack.sync("mono", Some(document.clone()), now, false);
        let first = stack.frame(now);
        assert_eq!(first.base, Pose::SHOWN);
        assert_eq!(first.pages[0].pose, Pose::OUTSIDE);
        let mid = stack.frame(now + Duration::from_millis(100));
        assert!(mid.pages[0].pose.x > 0. && mid.pages[0].pose.x < 1.);
        assert!(mid.base.x < 0. && mid.base.x > -0.18);
        assert!(mid.base.opacity > 0. && mid.base.opacity < 1.);
        let settled = stack.frame(now + Duration::from_millis(280));
        assert!(!settled.animating);
        assert_eq!(settled.base, Pose::COVERED);
        assert_eq!(settled.pages[0].pose, Pose::SHOWN);

        let back = now + Duration::from_secs(1);
        stack.sync("mono", None, back, false);
        let first = stack.frame(back);
        assert!(!first.covered);
        assert_eq!(first.pages[0].route, document);
        assert!(!first.pages[0].interactive);
        assert_eq!(first.pages[0].pose, Pose::SHOWN);
        let settled = stack.frame(back + Duration::from_millis(280));
        assert!(settled.pages.is_empty());
        assert_eq!(settled.base, Pose::SHOWN);
        assert!(!settled.animating);
    }

    #[test]
    fn updates_and_rapid_back_reopen_keep_the_current_position() {
        let mut stack = PanelStack::default();
        let now = Instant::now();
        let route = Route::Session(42);
        stack.sync("mono", Some(route.clone()), now, false);
        stack.frame(now);
        let mid = now + Duration::from_millis(90);
        let before = stack.frame(mid).pages[0].pose;
        stack.sync("mono", Some(route.clone()), mid, false);
        assert_eq!(stack.frame(mid).pages[0].pose, before);
        stack.sync("mono", None, mid, false);
        assert_eq!(stack.frame(mid).pages[0].pose, before);
        let reopen = mid + Duration::from_millis(40);
        let before = stack.frame(reopen).pages[0].pose;
        stack.sync("mono", Some(route), reopen, false);
        let frame = stack.frame(reopen);
        assert_eq!(frame.pages.len(), 1);
        assert_eq!(frame.pages[0].pose, before);
        assert!(frame.pages[0].interactive);
    }

    #[test]
    fn replacing_pages_keeps_the_departing_page_above_the_new_one() {
        let mut stack = PanelStack::default();
        let now = Instant::now();
        stack.sync("mono", Some(Route::Document("a".into())), now, true);
        stack.sync("mono", Some(Route::Session(1)), now, false);
        let frame = stack.frame(now);
        assert_eq!(frame.pages[0].route, Route::Session(1));
        assert!(frame.pages[0].interactive);
        assert_eq!(frame.pages[1].route, Route::Document("a".into()));
        assert!(!frame.pages[1].interactive);
    }

    #[test]
    fn reduced_motion_and_switching_monos_discard_old_pages() {
        let mut stack = PanelStack::default();
        let now = Instant::now();
        stack.sync("a", Some(Route::Session(1)), now, false);
        stack.frame(now);
        stack.sync("a", None, now, true);
        assert!(stack.frame(now).pages.is_empty());
        stack.sync("a", Some(Route::Session(1)), now, false);
        stack.sync("b", None, now, false);
        let frame = stack.frame(now);
        assert!(frame.pages.is_empty());
        assert_eq!(frame.base, Pose::SHOWN);
        assert!(!frame.animating);
    }
}
