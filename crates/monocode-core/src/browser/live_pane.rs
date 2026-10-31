//! A thread's tab shown live in a pane: the view that streams its pictures,
//! the pictures as the pane takes them, and the user's input, sent one at a
//! time in the order given.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use monocode_browser::live::{LiveEnd, LiveInput, LiveNav, MouseEvent, PanelSize};
use monocode_browser::protocol::TabId;
use monocode_browser::BrowserHost;

use super::{session_key, unpack_live, LivePicture};

/// How the view ended, once it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The pane stopped it, or another pane took the tab over.
    Stopped,
    /// The tab closed or left the thread.
    TabGone,
    /// It could not start or broke.
    Failed(String),
}

#[derive(Default)]
struct Inputs {
    queue: VecDeque<LiveInput>,
    closed: bool,
}

struct Shared {
    host: Arc<BrowserHost>,
    session: String,
    tab: TabId,
    view: String,
    ended: Mutex<Option<Ended>>,
    stopping: AtomicBool,
    inputs: Mutex<Inputs>,
    input_ready: Condvar,
    /// The last input that failed, for the pane to show.
    input_error: Mutex<Option<String>>,
}

/// One live view of a tab. Dropping it stops the view.
pub struct LivePane {
    shared: Arc<Shared>,
}

static NEXT_VIEW: AtomicU64 = AtomicU64::new(1);

impl LivePane {
    /// Start showing `tab`, one of `session`'s tabs, at `size`.
    pub fn start(session: i64, tab: TabId, size: PanelSize) -> Result<LivePane, String> {
        let host = super::host().ok_or("Browser access is not available.")?;
        let shared = Arc::new(Shared {
            host,
            session: session_key(session),
            tab,
            view: format!(
                "native-{}-{}",
                std::process::id(),
                NEXT_VIEW.fetch_add(1, Ordering::Relaxed)
            ),
            ended: Mutex::new(None),
            stopping: AtomicBool::new(false),
            inputs: Mutex::new(Inputs::default()),
            input_ready: Condvar::new(),
            input_error: Mutex::new(None),
        });
        let view = shared.clone();
        std::thread::Builder::new()
            .name("browser-live-view".into())
            .spawn(move || {
                let outcome =
                    view.host
                        .live_view(&view.session, &view.tab, &view.view, size, |_| {});
                let ended = match outcome {
                    Ok(LiveEnd::Stopped) => Ended::Stopped,
                    Ok(LiveEnd::TabGone) => Ended::TabGone,
                    Err(error) => Ended::Failed(error),
                };
                *view.ended.lock().unwrap_or_else(|e| e.into_inner()) = Some(ended);
            })
            .map_err(|error| error.to_string())?;
        let sender = shared.clone();
        std::thread::Builder::new()
            .name("browser-live-input".into())
            .spawn(move || send_inputs(&sender))
            .map_err(|error| error.to_string())?;
        Ok(LivePane { shared })
    }

    /// The next picture after `after`, waiting up to a second for it; `None`
    /// when none came, or the view is over ([`Self::ended`]). Blocks: call
    /// it off the UI thread.
    pub fn take(&self, after: u64) -> Option<Result<LivePicture, String>> {
        let packed = self.shared.host.live_take(&self.shared.view, after);
        (!packed.is_empty()).then(|| unpack_live(&packed))
    }

    /// How the view ended, once it has.
    pub fn ended(&self) -> Option<Ended> {
        self.shared
            .ended
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The pane changed size.
    pub fn resize(&self, size: PanelSize) {
        self.shared.host.resize_live(&self.shared.view, size);
    }

    /// Queue the user's input for the tab.
    pub fn input(&self, input: LiveInput) {
        let mut inputs = self.shared.inputs.lock().unwrap_or_else(|e| e.into_inner());
        queue_input(&mut inputs.queue, input);
        drop(inputs);
        self.shared.input_ready.notify_one();
    }

    /// The last input the tab refused, once.
    pub fn take_input_error(&self) -> Option<String> {
        self.shared
            .input_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Back, Forward, Reload or an address. Blocks: call it off the UI
    /// thread.
    pub fn navigate(&self, nav: &LiveNav) -> Result<(), String> {
        let shared = &self.shared;
        shared.host.live_navigate(&shared.session, &shared.tab, nav)
    }

    pub fn tab(&self) -> &TabId {
        &self.shared.tab
    }
}

impl Drop for LivePane {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared
            .inputs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closed = true;
        self.shared.input_ready.notify_all();
        self.shared.host.stop_live(Some(&self.shared.view));
    }
}

/// Send queued input until the pane goes. Each input waits until the page
/// has taken it, so moves and wheels that come meanwhile join the queue as
/// one ([`queue_input`]).
fn send_inputs(shared: &Shared) {
    loop {
        let input = {
            let mut inputs = shared.inputs.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if inputs.closed {
                    return;
                }
                if let Some(input) = inputs.queue.pop_front() {
                    break input;
                }
                inputs = shared
                    .input_ready
                    .wait(inputs)
                    .unwrap_or_else(|e| e.into_inner());
            }
        };
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = shared.host.live_input(&shared.session, &shared.tab, &input) {
            *shared.input_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
        }
    }
}

/// Add `input` to the inputs still waiting. Moves and scrolls that follow
/// one another become one: the pointer's latest place, and the scroll
/// distances added up, so a fast wheel or trackpad never builds a backlog
/// the page works through long after the hand has stopped.
pub fn queue_input(queue: &mut VecDeque<LiveInput>, input: LiveInput) {
    match (queue.back_mut(), &input) {
        (
            Some(LiveInput::Wheel {
                delta_x,
                delta_y,
                modifiers,
                x,
                y,
            }),
            LiveInput::Wheel {
                delta_x: dx,
                delta_y: dy,
                modifiers: m,
                x: nx,
                y: ny,
            },
        ) if modifiers == m => {
            *delta_x += dx;
            *delta_y += dy;
            *x = *nx;
            *y = *ny;
        }
        (
            Some(LiveInput::Mouse {
                event: MouseEvent::Moved,
                buttons,
                modifiers,
                ..
            }),
            LiveInput::Mouse {
                event: MouseEvent::Moved,
                buttons: b,
                modifiers: m,
                ..
            },
        ) if buttons == b && modifiers == m => {
            if let Some(last) = queue.back_mut() {
                *last = input;
            }
        }
        _ => queue.push_back(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_browser::live::MouseButton;

    fn moved(x: f64, buttons: u32) -> LiveInput {
        LiveInput::Mouse {
            event: MouseEvent::Moved,
            x,
            y: 0.0,
            button: MouseButton::None,
            buttons,
            click_count: 0,
            modifiers: 0,
        }
    }

    fn wheel(dy: f64, modifiers: u32) -> LiveInput {
        LiveInput::Wheel {
            x: 1.0,
            y: 2.0,
            delta_x: 0.0,
            delta_y: dy,
            modifiers,
        }
    }

    #[test]
    fn moves_and_wheels_that_follow_each_other_become_one() {
        let mut queue = VecDeque::new();
        queue_input(&mut queue, moved(1.0, 0));
        queue_input(&mut queue, moved(2.0, 0));
        assert_eq!(queue, [moved(2.0, 0)]);
        queue_input(&mut queue, moved(3.0, 1));
        assert_eq!(queue.len(), 2, "a drag is not a hover");
        queue_input(&mut queue, wheel(10.0, 0));
        queue_input(&mut queue, wheel(5.0, 0));
        assert_eq!(queue.back(), Some(&wheel(15.0, 0)));
        queue_input(&mut queue, wheel(5.0, 8));
        assert_eq!(queue.len(), 4, "a shifted wheel stays apart");
        queue_input(&mut queue, LiveInput::Text { text: "a".into() });
        queue_input(&mut queue, moved(4.0, 0));
        assert_eq!(queue.len(), 6);
    }
}
