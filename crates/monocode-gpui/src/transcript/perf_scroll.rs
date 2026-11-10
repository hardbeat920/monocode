//! For profiling scrolling without a trackpad: with
//! `MONOCODE_PERF_SCROLL=<pixels per second>`, the first transcript to finish
//! streaming scrolls itself up and down between its ends on every display
//! frame, the way wheel scrolling moves its list and notifies it.
//! scripts/perf/frame-run.sh sets it for `MODE=scroll`.
use super::*;
use std::cell::Cell;

thread_local! {
    /// The transcript that scrolls, so other chats keep still.
    static OWNER: Cell<Option<gpui::EntityId>> = const { Cell::new(None) };
}

#[derive(Default)]
pub(super) struct PerfScroll {
    scheduled: bool,
    up: bool,
    last: Option<Instant>,
}

fn speed() -> Option<f32> {
    static SPEED: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *SPEED.get_or_init(|| {
        std::env::var("MONOCODE_PERF_SCROLL")
            .ok()?
            .parse::<f32>()
            .ok()
            .filter(|speed| *speed > 0.)
    })
}

impl TranscriptView {
    /// Called while rendering; schedules the next scroll step.
    pub(super) fn perf_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(speed) = speed() else {
            return;
        };
        if self.streaming || self.rows.is_empty() || self.perf_scroll.scheduled {
            return;
        }
        let id = cx.entity_id();
        if OWNER.get().is_some_and(|owner| owner != id) {
            return;
        }
        OWNER.set(Some(id));
        self.perf_scroll.scheduled = true;
        let view = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = view.update(cx, |this, cx| {
                let now = Instant::now();
                let state = &mut this.perf_scroll;
                state.scheduled = false;
                let elapsed = state
                    .last
                    .replace(now)
                    .map_or(0., |last| now.duration_since(last).as_secs_f32().min(0.05));
                let step = px(speed * elapsed);
                let before = this.list.logical_scroll_top();
                this.list
                    .scroll_by(if this.perf_scroll.up { -step } else { step });
                let after = this.list.logical_scroll_top();
                // At either end the offset stops changing: turn around.
                if step > px(0.)
                    && before.item_ix == after.item_ix
                    && before.offset_in_item == after.offset_in_item
                {
                    this.perf_scroll.up = !this.perf_scroll.up;
                }
                cx.notify();
            });
        });
    }
}
