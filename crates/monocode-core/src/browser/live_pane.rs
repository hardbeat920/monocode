//! A thread's tab shown live in a pane: the view that streams its pictures,
//! the pictures decoded on their own thread as they come (the pane draws
//! one while the next decodes), and the user's input, sent one at a time
//! in the order given.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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
    /// The newest decoded picture the pane has not taken yet.
    latest: Mutex<Option<Result<LivePicture, String>>>,
    fresh: Condvar,
    meter: Mutex<Meter>,
}

impl Shared {
    fn over(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
            || self
                .ended
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
    }

    /// Wake a pane waiting for a picture.
    fn wake(&self) {
        let _latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        self.fresh.notify_all();
    }
}

/// What a development build notes about the pane's side of the view every
/// two seconds, next to the browser's own `[live]` line.
struct Meter {
    since: Instant,
    decoded: u32,
    decoding: Duration,
    slowest: Duration,
    /// Decoded, then replaced by a newer one before the pane took it.
    passed_over: u32,
    taken: u32,
    drawn: u32,
}

impl Meter {
    fn new() -> Meter {
        Meter {
            since: Instant::now(),
            decoded: 0,
            decoding: Duration::ZERO,
            slowest: Duration::ZERO,
            passed_over: 0,
            taken: 0,
            drawn: 0,
        }
    }

    fn maybe_note(&mut self) {
        let elapsed = self.since.elapsed();
        if !cfg!(debug_assertions) || elapsed < Duration::from_secs(2) {
            return;
        }
        if self.decoded > 0 || self.drawn > 0 {
            let rate = |n: u32| f64::from(n) / elapsed.as_secs_f64();
            eprintln!(
                "[live] pane: decoded {:.0}/s (avg {} ms, max {} ms), {} passed over | taken {:.0}/s | drawn {:.0}/s",
                rate(self.decoded),
                (self.decoding / self.decoded.max(1)).as_millis(),
                self.slowest.as_millis(),
                self.passed_over,
                rate(self.taken),
                rate(self.drawn),
            );
        }
        *self = Meter::new();
    }
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
            latest: Mutex::new(None),
            fresh: Condvar::new(),
            meter: Mutex::new(Meter::new()),
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
                view.wake();
            })
            .map_err(|error| error.to_string())?;
        let decoder = shared.clone();
        std::thread::Builder::new()
            .name("browser-live-decode".into())
            .spawn(move || decode_pictures(&decoder))
            .map_err(|error| error.to_string())?;
        let sender = shared.clone();
        std::thread::Builder::new()
            .name("browser-live-input".into())
            .spawn(move || send_inputs(&sender))
            .map_err(|error| error.to_string())?;
        Ok(LivePane { shared })
    }

    /// The newest picture the pane has not drawn yet, waiting up to a
    /// second for one; `None` when none came, or the view is over
    /// ([`Self::ended`]). Blocks: call it off the UI thread.
    pub fn take(&self) -> Option<Result<LivePicture, String>> {
        let shared = &self.shared;
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut latest = shared.latest.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(picture) = latest.take() {
                drop(latest);
                shared.meter.lock().unwrap_or_else(|e| e.into_inner()).taken += 1;
                return Some(picture);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || shared.over() {
                return None;
            }
            latest = shared
                .fresh
                .wait_timeout(latest, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// The pane drew a picture: counted for a development build's notes.
    pub fn drawn(&self) {
        let mut meter = self.shared.meter.lock().unwrap_or_else(|e| e.into_inner());
        meter.drawn += 1;
        meter.maybe_note();
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
        self.shared.wake();
    }
}

/// Take each picture the browser sends and decode it, keeping only the
/// newest for the pane, until the view is over.
fn decode_pictures(shared: &Shared) {
    let mut after = 0;
    while !shared.over() {
        let packed = shared.host.live_take(&shared.view, after);
        if packed.len() < 8 {
            continue;
        }
        after = u64::from(u32::from_le_bytes(
            packed[4..8].try_into().unwrap_or_default(),
        ));
        let started = Instant::now();
        let picture = unpack_live(&packed);
        let took = started.elapsed();
        let passed_over = {
            let mut latest = shared.latest.lock().unwrap_or_else(|e| e.into_inner());
            let passed_over = latest.replace(picture).is_some();
            shared.fresh.notify_all();
            passed_over
        };
        let mut meter = shared.meter.lock().unwrap_or_else(|e| e.into_inner());
        meter.decoded += 1;
        meter.decoding += took;
        meter.slowest = meter.slowest.max(took);
        meter.passed_over += u32::from(passed_over);
        meter.maybe_note();
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
