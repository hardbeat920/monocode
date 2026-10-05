//! Port of `SessionListItem` and `SessionInsertMotion` in Sidebar.tsx: a
//! session created in the shown project while its list is on screen grows
//! open, pushing the rows below it down, and its card fades in.
//!
//! Sidebar.tsx moved the rows below with `Element.animate`. GPUI cannot
//! move siblings, so the new row's slot grows from nothing to a card's
//! height on the same curve instead, which moves everything below it the
//! same distance. The slot clips the card, so it shows from the top down
//! as the rows below slide away, as it did when they uncovered it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, IntoElement, ParentElement as _, Pixels, Styled as _,
    div, px,
};
use monocode_ui::theme::{CubicBezier, Motion};
use monocode_ui::u;

/// `SESSION_INSERT_WINDOW_MS`: rows created this recently slide in; older
/// ones are just being listed.
pub const SESSION_INSERT_WINDOW_MS: i64 = 15_000;
/// The push: 380ms on `cubic-bezier(0.32, 0.72, 0, 1)`.
pub const PUSH_DURATION: Duration = Duration::from_millis(380);
const PUSH_EASE: CubicBezier = CubicBezier(0.32, 0.72, 0.0, 1.0);
/// The card's fade: 220ms `ease-out`.
pub const FADE_DURATION: Duration = Duration::from_millis(220);

/// `SessionInsertMotion`: the project the list last drew and every session
/// it listed there, so only sessions that arrive later count as new.
#[derive(Debug, Default)]
pub struct SessionInsertMotion {
    cwd: Option<String>,
    seen: HashSet<String>,
    /// Rows growing in, with a count that keys each run's animation.
    entering: HashMap<String, u64>,
    runs: u64,
}

impl SessionInsertMotion {
    /// Run once per frame with the cards the list draws (`shown`, as id and
    /// `createdAt`) and every session it lists, drawn or not (`listed`).
    /// Returns the rows that start growing in this frame. A project's first
    /// frame never animates, and a row decides once: a reorder or a later
    /// frame does not replay it.
    pub fn sync<'a>(
        &mut self,
        cwd: &str,
        shown: impl IntoIterator<Item = (&'a str, i64)>,
        listed: impl IntoIterator<Item = &'a str>,
        now: i64,
        reduce_motion: bool,
    ) -> Vec<(String, u64)> {
        let same_project = self.cwd.as_deref() == Some(cwd);
        if !same_project {
            self.seen.clear();
            self.entering.clear();
        }
        let mut started = Vec::new();
        for (id, created_at) in shown {
            if !self.seen.insert(id.to_string()) {
                continue;
            }
            let fresh =
                same_project && (created_at == 0 || now - created_at < SESSION_INSERT_WINDOW_MS);
            if fresh && !reduce_motion {
                self.runs += 1;
                self.entering.insert(id.to_string(), self.runs);
                started.push((id.to_string(), self.runs));
            }
        }
        // Rows that show up later (a folder expanded) are not new.
        self.seen.extend(listed.into_iter().map(str::to_string));
        self.cwd = Some(cwd.to_string());
        started
    }

    /// The animation run for a row that is growing in.
    pub fn entering(&self, id: &str) -> Option<u64> {
        self.entering.get(id).copied()
    }

    /// The row's motion ended. A newer run of the same row keeps going.
    pub fn finish(&mut self, id: &str, run: u64) {
        if self.entering.get(id) == Some(&run) {
            self.entering.remove(id);
        }
    }
}

/// The growing slot around a new card. `height` is a card's measured
/// height, and `gap` the list's row gap in CSS px, which the slot also takes
/// up as it opens. Without a measured card there is nothing below to push, so the
/// card only fades in.
pub fn grow_in(
    id: &str,
    run: u64,
    card: AnyElement,
    height: Option<Pixels>,
    gap: f32,
) -> AnyElement {
    let fade = div().flex_none().child(card).with_animation(
        gpui::ElementId::Name(format!("session-fade:{id}:{run}").into()),
        Animation::new(FADE_DURATION).with_easing(Motion::EASE_OUT.easing()),
        |el, t| el.opacity(t),
    );
    let Some(height) = height else {
        return fade.into_any_element();
    };
    let height = f32::from(height);
    div()
        .flex()
        .flex_col()
        .flex_none()
        .overflow_hidden()
        .child(fade)
        .with_animation(
            gpui::ElementId::Name(format!("session-push:{id}:{run}").into()),
            Animation::new(PUSH_DURATION).with_easing(PUSH_EASE.easing()),
            move |el, t| el.h(px(height * t)).mb(u(-gap * (1.0 - t))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn sync(
        motion: &mut SessionInsertMotion,
        cwd: &str,
        shown: &[(&str, i64)],
        reduce_motion: bool,
    ) -> Vec<String> {
        let listed: Vec<&str> = shown.iter().map(|(id, _)| *id).collect();
        motion
            .sync(cwd, shown.iter().copied(), listed, NOW, reduce_motion)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// "grows in only for a session that arrives after the list has
    /// rendered" in SidebarRename.test.ts.
    #[test]
    fn grows_in_only_for_a_session_that_arrives_after_the_list_has_rendered() {
        let mut motion = SessionInsertMotion::default();
        assert!(sync(&mut motion, "/repo", &[("session-1", 1)], false).is_empty());
        // Later frames of the same list do not replay anything.
        assert!(sync(&mut motion, "/repo", &[("session-1", 1)], false).is_empty());

        let shown = [("session-2", NOW - 10), ("session-1", 1)];
        assert_eq!(sync(&mut motion, "/repo", &shown, false), ["session-2"]);
        assert!(motion.entering("session-2").is_some());
        assert!(motion.entering("session-1").is_none());
        // The next frame keeps the run going without starting another.
        assert!(sync(&mut motion, "/repo", &shown, false).is_empty());

        // An old session that appears (a load more) just lists.
        let shown = [
            ("session-old", 1),
            ("session-2", NOW - 10),
            ("session-1", 1),
        ];
        assert!(sync(&mut motion, "/repo", &shown, false).is_empty());

        // Reordering existing rows does not replay their entrance.
        let reversed: Vec<_> = shown.iter().rev().copied().collect();
        assert!(sync(&mut motion, "/repo", &reversed, false).is_empty());
    }

    #[test]
    fn a_projects_first_frame_and_rows_listed_before_they_show_do_not_animate() {
        let mut motion = SessionInsertMotion::default();
        sync(&mut motion, "/repo", &[("a", 1)], false);
        assert!(sync(&mut motion, "/other", &[("fresh", NOW)], false).is_empty());

        // A collapsed folder's session is listed without a card; expanding
        // the folder later is not an arrival.
        let started = motion.sync("/other", [("fresh", NOW)], ["fresh", "hidden"], NOW, false);
        assert!(started.is_empty());
        let started = motion.sync(
            "/other",
            [("fresh", NOW), ("hidden", NOW)],
            ["fresh", "hidden"],
            NOW,
            false,
        );
        assert!(started.is_empty());
    }

    #[test]
    fn reduced_motion_and_untimed_rows() {
        let mut motion = SessionInsertMotion::default();
        sync(&mut motion, "/repo", &[], false);
        assert!(sync(&mut motion, "/repo", &[("calm", NOW)], true).is_empty());
        // `createdAt` 0 is a row whose time is not known yet: it counts as new.
        assert_eq!(
            sync(
                &mut motion,
                "/repo",
                &[("calm", NOW), ("untimed", 0)],
                false
            ),
            ["untimed"]
        );
        // Past the window, a new id just lists.
        let stale = NOW - SESSION_INSERT_WINDOW_MS;
        assert!(sync(&mut motion, "/repo", &[("stale", stale)], false).is_empty());
    }

    #[test]
    fn finishing_an_older_run_keeps_the_newer_one() {
        let mut motion = SessionInsertMotion::default();
        sync(&mut motion, "/repo", &[], false);
        let run = motion.sync("/repo", [("a", NOW)], ["a"], NOW, false)[0].1;
        motion.finish("a", run + 1);
        assert_eq!(motion.entering("a"), Some(run));
        motion.finish("a", run);
        assert_eq!(motion.entering("a"), None);
    }
}
