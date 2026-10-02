//! Port of src/app/model/harnessFlush.ts: when batched harness events apply.
//!
//! Tokens arrive many times per frame, so `Sessions` queues them and applies
//! each session's batch once. Visible output flushes on the next frame;
//! hidden output keeps advancing on a slower timer instead of driving the
//! whole UI at the display rate. The engine has no window of its own, so a
//! frame is a 16 ms timer.

use std::time::Duration;

use gpui::{Context, Task};

/// One display frame at 60 Hz, the TypeScript's `requestAnimationFrame`.
pub const FRAME_FLUSH: Duration = Duration::from_millis(16);

/// The background cadence, the TypeScript's `setTimeout(run, 100)`.
pub const BACKGROUND_FLUSH: Duration = Duration::from_millis(100);

/// `ScheduledFlush["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushKind {
    /// `raf`: the next frame.
    Frame,
    /// `timeout`: the background cadence.
    Timeout,
}

/// `ScheduledFlush`. Dropping it cancels the flush (`cancelScheduledFlush`).
pub struct ScheduledFlush {
    pub kind: FlushKind,
    _task: Task<()>,
}

/// The cadence for a flush: hidden or background output waits for the
/// timer, visible output takes the next frame.
pub fn flush_cadence(foreground: bool, hidden: bool) -> (FlushKind, Duration) {
    if hidden || !foreground {
        (FlushKind::Timeout, BACKGROUND_FLUSH)
    } else {
        (FlushKind::Frame, FRAME_FLUSH)
    }
}

/// `scheduleHarnessFlush`: run `run` on the entity once the cadence elapses.
pub fn schedule_harness_flush<T: 'static>(
    cx: &mut Context<T>,
    foreground: bool,
    hidden: bool,
    run: impl FnOnce(&mut T, &mut Context<T>) + 'static,
) -> ScheduledFlush {
    let (kind, delay) = flush_cadence(foreground, hidden);
    let timer = cx.background_executor().timer(delay);
    let task = cx.spawn(async move |this, cx| {
        timer.await;
        this.update(cx, run).ok();
    });
    ScheduledFlush { kind, _task: task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};

    #[derive(Default)]
    struct Counter {
        flushes: usize,
        handle: Option<ScheduledFlush>,
    }

    fn schedule(
        counter: &gpui::Entity<Counter>,
        cx: &mut TestAppContext,
        foreground: bool,
        hidden: bool,
    ) -> FlushKind {
        counter.update(cx, |counter, cx| {
            let handle =
                schedule_harness_flush(cx, foreground, hidden, |counter: &mut Counter, _| {
                    counter.flushes += 1
                });
            let kind = handle.kind;
            counter.handle = Some(handle);
            kind
        })
    }

    #[gpui::test]
    fn batches_background_only_work_while_allowing_it_to_progress(cx: &mut TestAppContext) {
        let counter = cx.new(|_| Counter::default());
        assert_eq!(schedule(&counter, cx, false, false), FlushKind::Timeout);
        cx.executor().advance_clock(Duration::from_millis(99));
        assert_eq!(counter.read_with(cx, |c, _| c.flushes), 0);
        cx.executor().advance_clock(Duration::from_millis(1));
        assert_eq!(counter.read_with(cx, |c, _| c.flushes), 1);
    }

    #[gpui::test]
    fn promotes_newly_visible_output_to_the_next_frame_without_a_duplicate_flush(
        cx: &mut TestAppContext,
    ) {
        let counter = cx.new(|_| Counter::default());
        schedule(&counter, cx, false, false);
        counter.update(cx, |counter, _| counter.handle = None);
        assert_eq!(schedule(&counter, cx, true, false), FlushKind::Frame);
        counter.update(cx, |counter, _| counter.handle = None);
        cx.executor().advance_clock(Duration::from_millis(200));
        assert_eq!(counter.read_with(cx, |c, _| c.flushes), 0);
    }

    #[gpui::test]
    fn does_not_depend_on_animation_frames_while_the_window_is_hidden(cx: &mut TestAppContext) {
        let counter = cx.new(|_| Counter::default());
        assert_eq!(schedule(&counter, cx, true, true), FlushKind::Timeout);
        cx.executor().advance_clock(Duration::from_millis(100));
        assert_eq!(counter.read_with(cx, |c, _| c.flushes), 1);
    }
}
