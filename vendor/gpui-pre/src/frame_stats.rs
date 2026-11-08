//! Opt-in frame counters for finding out what drives redraws.
//!
//! Collection is off until [`enable`] is called, so a normal run pays one
//! relaxed atomic load per draw. All counting happens on the main thread.

use crate::EntityId;
use collections::{FxHashMap, FxHashSet};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Start counting frames. There is no way to stop; it is meant for a whole run.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

#[inline]
pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// What the main thread drew since the previous [`take`].
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    /// Calls to `Window::draw`, across all windows.
    pub frames: u32,
    /// Frames where some view changed in a way that may affect layout.
    pub layout_frames: u32,
    /// Frames where only `notify_paint` (animation steps) changed views.
    pub paint_frames: u32,
    /// Frames that redrew the whole window, such as a resize or `refresh`.
    pub refresh_frames: u32,
    /// Time spent inside `Window::draw`.
    pub draw_time: Duration,
    /// The slowest single draw.
    pub max_draw: Duration,
    /// Entity views whose `render` ran.
    pub views_rendered: u32,
    /// Cached entity views reused without rendering.
    pub views_reused: u32,
    /// Views whose own `notify` caused a frame, by type, with how many frames
    /// each caused. A frame caused by two views counts for both.
    pub origins: Vec<(&'static str, u32)>,
    /// Named counts the app added with [`count`], largest first.
    pub counters: Vec<(&'static str, u32)>,
}

#[derive(Default)]
struct Accumulator {
    stats: FrameStats,
    origins: FxHashMap<&'static str, u32>,
    counters: FxHashMap<&'static str, u32>,
    /// The views notified directly for the frame being drawn.
    frame_origins: FxHashSet<EntityId>,
}

thread_local! {
    static STATS: RefCell<Accumulator> = RefCell::default();
}

/// Returns the counts since the previous call and starts a new interval.
/// Call it on the main thread, where the frames are drawn.
pub fn take() -> FrameStats {
    STATS.with_borrow_mut(|acc| {
        let mut stats = std::mem::take(&mut acc.stats);
        stats.origins = acc.origins.drain().collect();
        stats
            .origins
            .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        stats.counters = acc.counters.drain().collect();
        stats
            .counters
            .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        stats
    })
}

/// Adds one to a named count shown with the frame stats, for app code that
/// wants to see how often something expensive happens. A no-op until
/// [`enable`]; call it on the main thread.
pub fn count(name: &'static str) {
    add(name, 1);
}

/// Like [`count`], adding `amount`, such as microseconds spent.
pub fn add(name: &'static str, amount: u32) {
    if enabled() {
        STATS.with_borrow_mut(|acc| {
            let total = acc.counters.entry(name).or_default();
            *total = total.saturating_add(amount);
        });
    }
}

pub(crate) fn begin_frame(origins: impl IntoIterator<Item = EntityId>) {
    STATS.with_borrow_mut(|acc| {
        acc.frame_origins.clear();
        acc.frame_origins.extend(origins);
    });
}

pub(crate) fn end_frame(elapsed: Duration, layout: bool, refresh: bool) {
    STATS.with_borrow_mut(|acc| {
        let stats = &mut acc.stats;
        stats.frames += 1;
        if refresh {
            stats.refresh_frames += 1;
        } else if layout {
            stats.layout_frames += 1;
        } else {
            stats.paint_frames += 1;
        }
        stats.draw_time += elapsed;
        stats.max_draw = stats.max_draw.max(elapsed);
        acc.frame_origins.clear();
    });
}

pub(crate) fn view_rendered(entity: EntityId, type_name: &'static str) {
    STATS.with_borrow_mut(|acc| {
        acc.stats.views_rendered += 1;
        if acc.frame_origins.remove(&entity) {
            *acc.origins.entry(short_type_name(type_name)).or_default() += 1;
        }
    });
}

pub(crate) fn view_reused() {
    STATS.with_borrow_mut(|acc| acc.stats.views_reused += 1);
}

/// `monocode_gpui::transcript::TranscriptView` becomes `transcript::TranscriptView`.
fn short_type_name(name: &'static str) -> &'static str {
    let generic = name.find('<').unwrap_or(name.len());
    let path = &name[..generic];
    let mut cut = 0;
    let mut segments = 0;
    for (ix, _) in path.rmatch_indices("::") {
        segments += 1;
        cut = ix + 2;
        if segments == 2 {
            break;
        }
    }
    if segments < 2 { name } else { &name[cut..] }
}

#[cfg(test)]
mod tests {
    use super::short_type_name;

    #[test]
    fn keeps_module_and_type() {
        assert_eq!(
            short_type_name("monocode_gpui::transcript::TranscriptView"),
            "transcript::TranscriptView"
        );
        assert_eq!(short_type_name("shell::Shell"), "shell::Shell");
        assert_eq!(short_type_name("Shell"), "Shell");
    }
}
