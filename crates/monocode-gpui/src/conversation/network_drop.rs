//! A reply a dropped connection cut off carries on once the internet is back,
//! as if the person had typed "continue". Pairs with keep-awake's hotspot
//! switch, which is what usually brings the internet back.
use super::Conversation;
use gpui::{Context, Task};
use monocode_core::keep_awake::hotspot;
use std::time::Duration;

/// Automatic continues in a row before leaving it to the person.
const MAX_RESUMES: u8 = 3;

#[derive(Default)]
pub(super) struct NetworkDrop {
    waiting: Option<Task<()>>,
    resumes: u8,
}

impl Conversation {
    /// Called as each turn settles.
    pub(super) fn after_turn_for_network(&mut self, stopping: bool, cx: &mut Context<Self>) {
        let cut_off = !stopping
            && !self.is_remote()
            && self.settings.mono.is_none()
            && self.status != "Ready"
            && hotspot::network_error(&self.status);
        if !cut_off {
            if self.status == "Ready" {
                self.network_drop.resumes = 0;
            }
            self.network_drop.waiting = None;
            return;
        }
        // macOS only, like its switch in Settings.
        if cfg!(test)
            || !crate::keep_awake::KeepAwake::supported()
            || !crate::keep_awake::continue_after_drop()
            || self.network_drop.resumes >= MAX_RESUMES
        {
            return;
        }
        self.network_drop.waiting = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(hotspot::PROBE_EVERY).await;
                let online = cx
                    .background_executor()
                    .spawn(async { hotspot::online(Duration::from_secs(3)) })
                    .await;
                if online {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.network_drop.waiting = None;
                // Someone already sent something, or changed their mind.
                if this.busy || this.stopping || !crate::keep_awake::continue_after_drop() {
                    return;
                }
                this.network_drop.resumes += 1;
                this.launch_prompt("continue".into(), cx);
            });
        }));
    }
}
