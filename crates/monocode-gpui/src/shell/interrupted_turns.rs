//! Continue the chats a quit, crash or update restart cut short. Each chat is
//! materialized on its own, even when its tab is in the background.
use super::*;

/// Several providers starting at once would stall the first frames.
const STAGGER: std::time::Duration = std::time::Duration::from_millis(400);

impl Shell {
    pub(super) fn resume_interrupted_turns(&mut self, cx: &mut Context<Self>) {
        let mut pending = crate::restart_resume::take();
        if pending.is_empty() {
            return;
        }
        let active = self
            .workspace
            .active
            .map(|pane| self.workspace.session_for_pane(pane));
        // The chat on screen first, then the order the turns started in.
        pending.sort_by_key(|turn| Some(turn.id) != active);
        cx.spawn(async move |this, cx| {
            // An orphan from the previous run could still drive the same
            // provider session or edit the same checkout.
            let started = std::time::Instant::now();
            while !crate::restart_resume::reaped()
                && started.elapsed() < std::time::Duration::from_secs(10)
            {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
            }
            for (ix, turn) in pending.into_iter().enumerate() {
                if ix > 0 {
                    cx.background_executor().timer(STAGGER).await;
                }
                let resumed = this.update(cx, |this, cx| {
                    let Some(view) = this.ensure_session(turn.id, cx) else {
                        return;
                    };
                    view.update(cx, |view, cx| {
                        view.resume_after_restart(turn.turn, turn.interrupted, cx)
                    });
                });
                if resumed.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}
