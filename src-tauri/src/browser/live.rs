//! "Show in MonoCode": the user watches one of a thread's agent tabs live in
//! a MonoCode panel and can click, scroll and type in it. Pictures come from
//! CDP `Page.startScreencast`; input goes back through `Input.*`.
//!
//! Everything here is sent as the system on the user's behalf, never for an
//! agent. Agent tabs already run with focus emulation
//! ([`super::targets`]), which keeps a tab painting while it is in the
//! background or its window is off-screen, so the panel works in every mode,
//! a headless MonoCode browser included. While the panel is open the tab's
//! viewport takes the panel's size, so the page fills it the way a built-in
//! browser would. In the hidden MonoCode browser each agent tab has a window
//! of its own, and that window is given the panel's size: the page then has
//! the size for real, so a navigation or an agent's action never shows it at
//! another size for a moment. A tab that shares its window with others (the
//! user's browser) has the panel's size emulated instead.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::link::{
    CdpSessionId, Command, EventFilter, Link, Method, Reply, TargetId, SCREENCAST_FRAME,
};
use super::policy;
use super::protocol::TabId;

const POLL: Duration = Duration::from_millis(100);
/// How often the view looks for the lossless picture's answer while the
/// browser makes it.
const STILL_POLL: Duration = Duration::from_millis(5);
/// How long the panel's request for the next picture waits for one before
/// it is answered empty (and the panel asks again).
pub const PULL_WAIT: Duration = Duration::from_secs(1);
/// A page this long without a new picture gets a lossless one: moving
/// pictures are JPEG, which softens text, a still one should be sharp.
const STILL_AFTER: Duration = Duration::from_millis(250);
/// How long pictures that don't fit the panel are left out. A browser that
/// never sends fitting ones still gets shown after this.
const MISFIT_FOR: Duration = Duration::from_secs(1);
/// How long a hold waits for the pictures to stop.
const HOLD_WAIT: Duration = Duration::from_millis(500);
/// How often the panel's viewport is put back while pictures don't fit.
const HEAL_EVERY: Duration = Duration::from_millis(300);
/// How long a picture waits for the panel to take the one before it before
/// the browser may send the next one anyway.
const ACK_WAIT: Duration = Duration::from_millis(150);
/// How often a held-back acknowledgement looks at what the panel has taken.
const ACK_POLL: Duration = Duration::from_millis(4);
/// Bytes in front of the picture in [`LiveFrame::packed`].
pub const PACKED_HEADER: usize = 24;
/// JPEG quality of the pictures, 0 to 100.
const QUALITY: u32 = 90;
/// How often a development build prints where the pictures' time goes.
const METER_EVERY: Duration = Duration::from_secs(2);
/// Bounds for the panel size, in CSS pixels.
const MIN_SIDE: u32 = 200;
const MAX_SIDE: u32 = 3840;
const MAX_SCALE: f64 = 4.0;
/// How many times the tab's window is resized until its page has the
/// panel's size (the window's frame is learnt from the first try).
const FIT_TRIES: usize = 3;
/// How long a resized window may take before its page has the new size.
const FIT_WAIT: Duration = Duration::from_millis(300);
/// How far, in CSS pixels, the page may be from the panel's size and still
/// count as having it (a display at 125 % has fractional CSS pixels).
const FIT_SLACK: f64 = 1.5;

/// The tab the panel shows.
#[derive(Clone, Debug)]
pub struct LiveTab {
    pub tab: TabId,
    pub target: TargetId,
    /// Main-frame session.
    pub main: CdpSessionId,
    /// The tab's own window in the hidden MonoCode browser, which may take
    /// the panel's size; `None` for a tab whose window is not MonoCode's to
    /// resize.
    pub window: Option<i64>,
}

/// One picture of the tab.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveFrame {
    pub tab: String,
    /// The picture, base64, in `format`.
    pub data: String,
    /// "jpeg" while the page moves; "png" for the lossless still once it
    /// has stopped.
    pub format: &'static str,
    /// Size of the page's viewport in CSS pixels; input coordinates are in
    /// this space.
    pub width: f64,
    pub height: f64,
    /// Counts the pictures of one view, from 1.
    pub seq: u64,
}

impl LiveFrame {
    /// The picture as the panel takes it: a fixed header (format byte, 3
    /// zero bytes, `seq` as u32, `width` and `height` as f64, all little
    /// endian) and then the picture's own bytes. Binary, so it reaches the
    /// panel without being turned into text and back.
    pub fn packed(&self) -> Option<Vec<u8>> {
        use base64::Engine as _;
        let picture = base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .ok()?;
        let mut bytes = Vec::with_capacity(PACKED_HEADER + picture.len());
        bytes.push(u8::from(self.format == "png"));
        bytes.extend_from_slice(&[0; 3]);
        bytes.extend_from_slice(&(self.seq as u32).to_le_bytes());
        bytes.extend_from_slice(&self.width.to_le_bytes());
        bytes.extend_from_slice(&self.height.to_le_bytes());
        bytes.extend_from_slice(&picture);
        Some(bytes)
    }
}

/// Why a view ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveEnd {
    Stopped,
    TabGone,
}

/// Input the user gives in the panel. Coordinates are CSS pixels of the
/// page's viewport.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LiveInput {
    #[serde(rename_all = "camelCase")]
    Mouse {
        /// `mousePressed`, `mouseReleased` or `mouseMoved`.
        event: MouseEvent,
        x: f64,
        y: f64,
        #[serde(default)]
        button: MouseButton,
        #[serde(default)]
        buttons: u32,
        #[serde(default)]
        click_count: u32,
        #[serde(default)]
        modifiers: u32,
    },
    #[serde(rename_all = "camelCase")]
    Wheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        #[serde(default)]
        modifiers: u32,
    },
    #[serde(rename_all = "camelCase")]
    Key {
        /// `keyDown` (with `text`, types it), `rawKeyDown` or `keyUp`.
        event: KeyEvent,
        key: String,
        code: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        key_code: u32,
        #[serde(default)]
        modifiers: u32,
    },
    /// Typed or pasted text, IME results included.
    Text { text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum MouseEvent {
    #[serde(rename = "mousePressed")]
    Pressed,
    #[serde(rename = "mouseReleased")]
    Released,
    #[serde(rename = "mouseMoved")]
    Moved,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    #[default]
    None,
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyEvent {
    KeyDown,
    RawKeyDown,
    KeyUp,
}

fn send(
    link: &Link,
    session: &CdpSessionId,
    method: Method,
    params: Value,
) -> Result<Value, String> {
    link.send_system(Command::in_session(session, method, params))
        .map_err(|e| format!("The browser refused {}: {e:?}", method.name()))
}

/// The panel the tab is shown in: CSS pixels and the display's pixel ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelSize {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl PanelSize {
    /// Kept within bounds a page and a picture can have.
    pub fn new(width: u32, height: u32, scale: f64) -> PanelSize {
        PanelSize {
            width: width.clamp(MIN_SIDE, MAX_SIDE),
            height: height.clamp(MIN_SIDE, MAX_SIDE),
            scale: if scale.is_finite() {
                scale.clamp(1.0, MAX_SCALE)
            } else {
                1.0
            },
        }
    }
}

/// The largest picture to ask for: never under the page's own device
/// pixels, so the browser never scales a picture down (a pixel short blurs
/// the whole frame).
fn picture_for(width: f64, height: f64, scale: f64) -> (u32, u32) {
    (
        (width * scale).ceil() as u32 + 2,
        (height * scale).ceil() as u32 + 2,
    )
}

/// How the tab shows the panel's size, as [`show`] set it up.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    /// The page's viewport in CSS pixels: the panel's, give or take
    /// [`FIT_SLACK`].
    width: f64,
    height: f64,
    /// The tab's own window has that size; otherwise it is emulated.
    window: bool,
}

/// One running view, named by the pane that asked for it: [`run`] watches
/// it for a stop or a new panel size.
#[derive(Debug)]
pub struct LiveControl {
    pub view: String,
    pub tab: TabId,
    stop: AtomicBool,
    finished: AtomicBool,
    resize: Mutex<Option<PanelSize>>,
    held: AtomicUsize,
    /// The view has stopped its pictures for a hold.
    paused: AtomicBool,
    /// The user gave the tab input since the view last looked.
    touched: AtomicBool,
    /// The newest picture for the panel, and what it took so far.
    mail: Mutex<Mailbox>,
    ready: Condvar,
}

/// The picture waiting for the panel. The panel asks for pictures
/// ([`LiveControl::take`]) rather than being sent each one: whatever it
/// gets is what the tab shows now, and a picture it was too slow for is
/// never drawn late.
#[derive(Debug, Default)]
struct Mailbox {
    /// The newest picture not taken yet, packed, with its `seq`.
    picture: Option<(u64, Vec<u8>)>,
    /// The `seq` of the last picture the panel took.
    taken: u64,
}

impl LiveControl {
    pub fn new(view: String, tab: TabId) -> LiveControl {
        LiveControl {
            view,
            tab,
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            resize: Mutex::new(None),
            held: AtomicUsize::new(0),
            paused: AtomicBool::new(false),
            touched: AtomicBool::new(false),
            mail: Mutex::new(Mailbox::default()),
            ready: Condvar::new(),
        }
    }

    fn mail(&self) -> std::sync::MutexGuard<'_, Mailbox> {
        self.mail.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Pause the pictures while the agent needs the tab's viewport to
    /// itself (a full-page screenshot): with the screencast running, the
    /// capture takes several times longer and can leave the viewport at
    /// the page's height. The view resumes at its own size when the hold
    /// is dropped.
    /// Waits, at most a moment, until the pictures have stopped.
    pub fn hold(self: &std::sync::Arc<Self>) -> LiveHold {
        self.held.fetch_add(1, Ordering::AcqRel);
        let deadline = Instant::now() + HOLD_WAIT;
        while !self.paused.load(Ordering::Acquire)
            && !self.finished.load(Ordering::Acquire)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        LiveHold(self.clone())
    }

    fn held(&self) -> bool {
        self.held.load(Ordering::Acquire) > 0
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        // A panel waiting for a picture learns there will be none.
        let _mail = self.mail();
        self.ready.notify_all();
    }

    /// The view has handed the tab back (its viewport and scrollbars).
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
        let _mail = self.mail();
        self.ready.notify_all();
    }

    /// Wait, at most `limit`, for [`Self::finish`], so the next view of the
    /// same tab is not undone by this one's late clean-up.
    pub fn wait_finished(&self, limit: Duration) {
        let deadline = Instant::now() + limit;
        while !self.finished.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A new picture for the panel. One before it that the panel has not
    /// taken is let go: the panel wants what the tab shows now.
    fn offer(&self, seq: u64, packed: Vec<u8>) {
        let mut mail = self.mail();
        mail.picture = Some((seq, packed));
        drop(mail);
        self.ready.notify_all();
    }

    /// The newest picture after `after`, waiting up to `wait` for one.
    /// `None` when none came in time, or the view is over.
    pub fn take(&self, after: u64, wait: Duration) -> Option<Vec<u8>> {
        let deadline = Instant::now() + wait;
        let mut mail = self.mail();
        loop {
            if mail.picture.as_ref().is_some_and(|(seq, _)| *seq > after) {
                let (seq, packed) = mail.picture.take()?;
                mail.taken = mail.taken.max(seq);
                return Some(packed);
            }
            if self.stopped() || self.finished.load(Ordering::Acquire) {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = self
                .ready
                .wait_timeout(mail, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            mail = guard;
        }
    }

    /// The `seq` of the last picture the panel took.
    fn taken(&self) -> u64 {
        self.mail().taken
    }

    /// Whether picture `seq` waits for the panel: it has not taken the one
    /// before it yet. The browser then sends a new picture only once the
    /// panel keeps up, so it never makes pictures nobody will see.
    fn behind(&self, seq: u64) -> bool {
        self.taken() + 1 < seq
    }

    /// The user scrolled, clicked or typed in the tab: a lossless picture
    /// asked for before may already show the page moved, under the scroll
    /// position of the picture before it.
    pub fn touch(&self) {
        self.touched.store(true, Ordering::Release);
    }

    pub fn resize(&self, size: PanelSize) {
        *self.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some(size);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn take_resize(&self) -> Option<PanelSize> {
        self.resize.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// A pause of a live view; see [`LiveControl::hold`].
pub struct LiveHold(std::sync::Arc<LiveControl>);

impl Drop for LiveHold {
    fn drop(&mut self) {
        self.0.held.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Give the tab the panel's viewport and start the pictures at that size:
/// its own window takes that size when it has one ([`fit_window`]), the
/// size is emulated otherwise, and when the window cannot get that small.
fn show(link: &Link, tab: &LiveTab, size: PanelSize) -> Result<Shown, String> {
    let fitted = tab
        .window
        .and_then(|window| match fit_window(link, tab, window, size) {
            Ok((width, height))
                if (width - f64::from(size.width)).abs() <= FIT_SLACK
                    && (height - f64::from(size.height)).abs() <= FIT_SLACK =>
            {
                Some(Shown {
                    width,
                    height,
                    window: true,
                })
            }
            Ok((width, height)) => {
                note(&format!(
                    "the window gives the page {width}x{height}, not the panel's {}x{}: emulated",
                    size.width, size.height
                ));
                None
            }
            Err(error) => {
                note(&format!("window not sized: {error}"));
                None
            }
        });
    let shown = match fitted {
        Some(shown) => shown,
        None => {
            send(
                link,
                &tab.main,
                Method::EmulationSetDeviceMetricsOverride,
                json!({
                    "width": size.width,
                    "height": size.height,
                    "deviceScaleFactor": size.scale,
                    "mobile": false,
                }),
            )?;
            Shown {
                width: f64::from(size.width),
                height: f64::from(size.height),
                window: false,
            }
        }
    };
    let (width, height) = picture_for(shown.width, shown.height, size.scale);
    send(
        link,
        &tab.main,
        Method::PageStartScreencast,
        json!({ "format": "jpeg", "quality": QUALITY, "maxWidth": width, "maxHeight": height }),
    )?;
    Ok(shown)
}

/// Resize the tab's own window until its page has the panel's size, at the
/// panel's pixel ratio. The window's frame (a title bar, borders) is not
/// known up front, so each try corrects by what the page still lacks.
/// Returns the page's viewport in CSS pixels.
fn fit_window(
    link: &Link,
    tab: &LiveTab,
    window: i64,
    size: PanelSize,
) -> Result<(f64, f64), String> {
    let mut seen = viewport(link, tab)?;
    // Only the pixel ratio is emulated, when the window's differs: the size
    // stays the window's (0 keeps it).
    if (seen.scale - size.scale).abs() > 0.01 {
        send(
            link,
            &tab.main,
            Method::EmulationSetDeviceMetricsOverride,
            json!({ "width": 0, "height": 0, "deviceScaleFactor": size.scale, "mobile": false }),
        )?;
        seen = viewport(link, tab)?;
    }
    let bounds = send_browser(
        link,
        Method::BrowserGetWindowBounds,
        json!({ "windowId": window }),
    )?;
    let bounds = &bounds["bounds"];
    if matches!(
        bounds["windowState"].as_str(),
        Some("maximized" | "fullscreen")
    ) {
        send_browser(
            link,
            Method::BrowserSetWindowBounds,
            json!({ "windowId": window, "bounds": { "windowState": "normal" } }),
        )?;
    }
    let mut outer = (
        bounds["width"].as_f64().unwrap_or(seen.width),
        bounds["height"].as_f64().unwrap_or(seen.height),
    );
    for _ in 0..FIT_TRIES {
        let lack = (
            f64::from(size.width) - seen.width,
            f64::from(size.height) - seen.height,
        );
        if lack.0.abs() < 1.0 && lack.1.abs() < 1.0 {
            break;
        }
        outer = (
            (outer.0 + lack.0.round()).max(1.0),
            (outer.1 + lack.1.round()).max(1.0),
        );
        send_browser(
            link,
            Method::BrowserSetWindowBounds,
            json!({ "windowId": window, "bounds": { "width": outer.0, "height": outer.1 } }),
        )?;
        let before = seen;
        let deadline = Instant::now() + FIT_WAIT;
        loop {
            seen = viewport(link, tab)?;
            if seen != before || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if seen == before {
            // The window would not change: it is as small as it gets.
            break;
        }
    }
    Ok((seen.width, seen.height))
}

/// The page's viewport as the browser lays it out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    width: f64,
    height: f64,
    /// Device pixels per CSS pixel.
    scale: f64,
}

fn viewport(link: &Link, tab: &LiveTab) -> Result<Viewport, String> {
    let metrics = send(link, &tab.main, Method::PageGetLayoutMetrics, json!({}))?;
    viewport_of(&metrics).ok_or_else(|| "The browser gave no layout metrics.".to_string())
}

/// The viewport in `Page.getLayoutMetrics`' answer: CSS pixels from
/// `cssVisualViewport`, the pixel ratio from the device-pixel
/// `layoutViewport` next to its CSS twin.
fn viewport_of(metrics: &Value) -> Option<Viewport> {
    let css = &metrics["cssVisualViewport"];
    let zoom = css["zoom"].as_f64().unwrap_or(1.0);
    let width = css["clientWidth"].as_f64()? * zoom;
    let height = css["clientHeight"].as_f64()? * zoom;
    let device = metrics["layoutViewport"]["clientWidth"].as_f64();
    let layout = metrics["cssLayoutViewport"]["clientWidth"].as_f64();
    let scale = match (device, layout) {
        (Some(device), Some(layout)) if layout > 0.0 => device / layout,
        _ => 1.0,
    };
    Some(Viewport {
        width,
        height,
        scale,
    })
}

fn send_browser(link: &Link, method: Method, params: Value) -> Result<Value, String> {
    link.send_system(Command::browser(method, params))
        .map_err(|e| format!("The browser refused {}: {e:?}", method.name()))
}

/// Hand the tab back its window's viewport and scrollbars. A window that was
/// given the panel's size keeps it: the agent goes on at the size the user
/// last saw.
fn restore(link: &Link, tab: &LiveTab) {
    let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
    let _ = send(
        link,
        &tab.main,
        Method::EmulationClearDeviceMetricsOverride,
        json!({}),
    );
    let _ = send(
        link,
        &tab.main,
        Method::EmulationSetScrollbarsHidden,
        json!({ "hidden": false }),
    );
}

/// Stream pictures of the tab to `sink` until `control` stops it or the tab
/// is gone. Each picture is acknowledged before the next one is asked for,
/// so the browser never runs ahead of the panel. The tab's viewport is the
/// panel's while this runs (it follows [`LiveControl::resize`]), and the
/// page's own scrollbars are hidden, the panel being the frame; both are
/// the window's again afterwards.
pub fn run(
    link: &Link,
    tab: &LiveTab,
    size: PanelSize,
    control: &LiveControl,
    alive: impl Fn() -> bool,
    mut sink: impl FnMut(LiveFrame),
) -> Result<LiveEnd, String> {
    let events = link.subscribe(EventFilter {
        session: Some(tab.main.clone()),
        methods: vec![SCREENCAST_FRAME],
    });
    let _ = send(
        link,
        &tab.main,
        Method::EmulationSetScrollbarsHidden,
        json!({ "hidden": true }),
    );
    let mut shown = match show(link, tab, size) {
        Ok(shown) => shown,
        Err(error) => {
            note(&format!("view not started: {error}"));
            restore(link, tab);
            return Err(error);
        }
    };
    note(&format!(
        "view started: panel {}x{} at {}, page {}x{} {}",
        size.width,
        size.height,
        size.scale,
        shown.width,
        shown.height,
        if shown.window {
            "in its own window"
        } else {
            "emulated"
        }
    ));
    let mut current = size;
    let mut seq = 0;
    // The last JPEG, until the page has been still long enough for a PNG.
    let mut moving: Option<(Instant, Picture)> = None;
    // The lossless picture the browser is making; dropped when the page
    // moves again meanwhile, since it would show the page as it was.
    let mut still: Option<Still> = None;
    // The data of the last JPEG: Chrome on Windows sends it again right
    // after a screenshot, and it must not cover the PNG.
    let mut last = String::new();
    // Since when the pictures have not fit the panel.
    let mut misfit: Option<Instant> = None;
    let mut healed: Option<Instant> = None;
    let mut paused = false;
    // A picture offered to the panel whose acknowledgement waits until the
    // panel has taken the one before it.
    let mut waiting: Option<(super::link::CdpEvent, Instant)> = None;
    let mut meter = Meter::new();
    let end = loop {
        meter.taken(control.taken());
        meter.report();
        if let Some((event, at)) = &waiting {
            if !control.behind(seq) || at.elapsed() >= ACK_WAIT || control.held() {
                ack(link, tab, event);
                waiting = None;
            }
        }
        if control.stopped() {
            break LiveEnd::Stopped;
        }
        if !alive() || events.is_closed() {
            break LiveEnd::TabGone;
        }
        if control.held() {
            if !paused {
                paused = true;
                moving = None;
                still = None;
                note("pictures paused: the agent needs the tab's viewport");
                let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
                // The capture is as fast as with no pane only on the tab's
                // own viewport; the pane's comes back on resume.
                let _ = send(
                    link,
                    &tab.main,
                    Method::EmulationClearDeviceMetricsOverride,
                    json!({}),
                );
                control.paused.store(true, Ordering::Release);
            }
            // Pictures already on the way are taken and let go.
            if let Some(event) = events.recv_timeout(POLL) {
                ack(link, tab, &event);
            }
            continue;
        }
        if paused {
            paused = false;
            control.paused.store(false, Ordering::Release);
            misfit = None;
            last.clear();
            note("pictures resumed");
            match show(link, tab, current) {
                Ok(again) => shown = again,
                Err(error) => {
                    restore(link, tab);
                    return Err(error);
                }
            }
        }
        if control.touched.swap(false, Ordering::AcqRel) {
            if still.take().is_some() {
                note("lossless picture let go: the user moved the page");
            }
            if let Some((at, _)) = &mut moving {
                *at = Instant::now();
            }
        }
        if let Some(next) = control.take_resize().filter(|next| *next != current) {
            // A new size needs pictures of the new size, so the screencast
            // starts over at it.
            let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
            // A window that could not take the old size may take this one.
            let _ = send(
                link,
                &tab.main,
                Method::EmulationClearDeviceMetricsOverride,
                json!({}),
            );
            match show(link, tab, next) {
                Ok(again) => shown = again,
                Err(error) => {
                    restore(link, tab);
                    return Err(error);
                }
            }
            note(&format!(
                "panel resized to {}x{} at {}: page {}x{} {}",
                next.width,
                next.height,
                next.scale,
                shown.width,
                shown.height,
                if shown.window {
                    "in its own window"
                } else {
                    "emulated"
                }
            ));
            current = next;
            last.clear();
        }
        if let Some(pending) = &still {
            match pending.answer.try_recv() {
                Ok(answer) => {
                    let Still { picture, .. } = still.take().expect("checked");
                    if let Some(data) = answer
                        .ok()
                        .and_then(|value| {
                            serde_json::from_str::<Value>(value.get()).ok()?["data"]
                                .as_str()
                                .map(str::to_string)
                        })
                        .filter(|data| fits(shown, picture.width, picture.height, png_size(data)))
                    {
                        seq += 1;
                        offer(
                            control,
                            &mut sink,
                            LiveFrame {
                                tab: tab.tab.0.clone(),
                                data,
                                format: "png",
                                width: picture.width,
                                height: picture.height,
                                seq,
                            },
                        );
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => still = None,
            }
        }
        if let Some((at, picture)) = moving {
            if at.elapsed() >= STILL_AFTER {
                // Asked for, not waited for: pictures and input keep flowing
                // while the browser makes it.
                moving = None;
                still = request_still(link, tab).map(|answer| Still { answer, picture });
                if still.is_none() {
                    note("lossless picture not asked for: the link refused");
                }
            }
        }
        let wait = moving.map_or(POLL, |(at, _)| {
            STILL_AFTER.saturating_sub(at.elapsed()).min(POLL)
        });
        let wait = if waiting.is_some() {
            wait.min(ACK_POLL)
        } else if still.is_some() {
            wait.min(STILL_POLL)
        } else {
            wait
        };
        let Some(event) = events.recv_timeout(wait) else {
            continue;
        };
        if event.method != SCREENCAST_FRAME {
            continue;
        }
        meter.arrived(event.params["metadata"]["timestamp"].as_f64());
        let sent =
            'frame: {
                let Some(data) = event.params["data"].as_str() else {
                    break 'frame false;
                };
                // The same picture again: nothing moved.
                if data == last {
                    note("picture dropped: the same as the last one");
                    break 'frame false;
                }
                let picture = Picture::of(&event.params["metadata"]).in_css(shown, current.scale);
                let (width, height) = (picture.width, picture.height);
                if fits(shown, width, height, jpeg_size(data)) {
                    misfit = None;
                } else {
                    let since = *misfit.get_or_insert_with(Instant::now);
                    // Something else changed the tab's viewport (a full-page
                    // screenshot does): the panel's is put back.
                    if healed.is_none_or(|at: Instant| at.elapsed() >= HEAL_EVERY) {
                        healed = Some(Instant::now());
                        let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
                        match show(link, tab, current) {
                            Ok(again) => {
                                shown = again;
                                note(&format!(
                                "viewport put back after a {width}x{height} picture: page {}x{} {}",
                                shown.width,
                                shown.height,
                                if shown.window { "in its own window" } else { "emulated" }
                            ));
                            }
                            Err(error) => note(&format!("viewport not put back: {error}")),
                        }
                    }
                    if since.elapsed() < MISFIT_FOR {
                        note(&format!(
                            "picture dropped: {width}x{height} css, {:?} px, page {}x{} at {}",
                            jpeg_size(data),
                            shown.width,
                            shown.height,
                            current.scale
                        ));
                        break 'frame false;
                    }
                }
                last = data.to_string();
                seq += 1;
                moving = Some((Instant::now(), picture));
                // The page moved on: a lossless picture still being made would
                // show it as it was.
                still = None;
                let handing = Instant::now();
                offer(
                    control,
                    &mut sink,
                    LiveFrame {
                        tab: tab.tab.0.clone(),
                        data: last.clone(),
                        format: "jpeg",
                        width,
                        height,
                        seq,
                    },
                );
                meter.handed(seq, handing);
                true
            };
        if sent && control.behind(seq) {
            if let Some((older, _)) = waiting.replace((event, Instant::now())) {
                ack(link, tab, &older);
            }
        } else {
            ack(link, tab, &event);
        }
    };
    if let Some((event, _)) = waiting {
        ack(link, tab, &event);
    }
    restore(link, tab);
    note(&format!("view ended: {end:?} after {seq} pictures"));
    Ok(end)
}

/// Inputs handed to tabs since the meter last printed: count, total and
/// longest round trip in microseconds.
static INPUTS: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Durations in milliseconds: how many, their sum and the longest.
#[derive(Default)]
struct Span {
    count: u64,
    sum: f64,
    max: f64,
}

impl Span {
    fn add(&mut self, ms: f64) {
        self.count += 1;
        self.sum += ms;
        self.max = self.max.max(ms);
    }

    fn show(&self) -> String {
        if self.count == 0 {
            return "-".into();
        }
        format!(
            "avg {:.0} ms, max {:.0}",
            self.sum / self.count as f64,
            self.max
        )
    }
}

/// Where a view's time goes, printed every [`METER_EVERY`] by development
/// builds while pictures flow: how many the browser sent, how old they
/// were when they got here, how long packing one for the panel took, how
/// long until the panel took it, and how long inputs took.
struct Meter {
    since: Instant,
    from_browser: u64,
    age: Span,
    hand_over: Span,
    on_screen: Span,
    /// Pictures offered and not yet taken: seq and when.
    sent: std::collections::VecDeque<(u64, Instant)>,
    skipped: u64,
}

impl Meter {
    fn new() -> Meter {
        Meter {
            since: Instant::now(),
            from_browser: 0,
            age: Span::default(),
            hand_over: Span::default(),
            on_screen: Span::default(),
            sent: std::collections::VecDeque::new(),
            skipped: 0,
        }
    }

    /// A picture from the browser, taken at `timestamp` (seconds since the
    /// epoch, the browser's clock).
    fn arrived(&mut self, timestamp: Option<f64>) {
        self.from_browser += 1;
        if let (Some(at), Ok(now)) = (
            timestamp,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH),
        ) {
            self.age.add((now.as_secs_f64() - at) * 1000.0);
        }
    }

    fn handed(&mut self, seq: u64, started: Instant) {
        self.hand_over.add(started.elapsed().as_secs_f64() * 1000.0);
        self.sent.push_back((seq, Instant::now()));
        while self.sent.len() > 240 {
            self.sent.pop_front();
        }
    }

    fn taken(&mut self, taken: u64) {
        while let Some(&(seq, at)) = self.sent.front() {
            if seq > taken {
                break;
            }
            self.sent.pop_front();
            if seq == taken {
                self.on_screen.add(at.elapsed().as_secs_f64() * 1000.0);
            } else {
                self.skipped += 1;
            }
        }
    }

    fn report(&mut self) {
        if !cfg!(debug_assertions) || self.since.elapsed() < METER_EVERY {
            return;
        }
        let secs = self.since.elapsed().as_secs_f64();
        let inputs: Vec<u64> = INPUTS.iter().map(|n| n.swap(0, Ordering::AcqRel)).collect();
        if self.from_browser > 0 || inputs[0] > 0 {
            eprintln!(
                "[live] browser {:.0}/s, age on arrival {} | packed in {} | taken by the panel after {} ({:.0}/s, {} skipped) | inputs {} ({})",
                self.from_browser as f64 / secs,
                self.age.show(),
                self.hand_over.show(),
                self.on_screen.show(),
                self.on_screen.count as f64 / secs,
                self.skipped,
                inputs[0],
                if inputs[0] == 0 {
                    "-".to_string()
                } else {
                    format!(
                        "avg {:.0} ms, max {:.0}",
                        inputs[1] as f64 / inputs[0] as f64 / 1000.0,
                        inputs[2] as f64 / 1000.0
                    )
                },
            );
        }
        let sent = std::mem::take(&mut self.sent);
        *self = Meter::new();
        self.sent = sent;
    }
}

/// A line in a development build's log about what the view did, next to
/// the meter's.
fn note(what: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[live] {what}");
    }
}

/// Let Chrome send the next picture.
fn ack(link: &Link, tab: &LiveTab, event: &super::link::CdpEvent) {
    if let Some(id) = event.params["sessionId"].as_i64() {
        let _ = send(
            link,
            &tab.main,
            Method::PageScreencastFrameAck,
            json!({ "sessionId": id }),
        );
    }
}

/// The page's viewport in a picture, in CSS pixels, from a screencast
/// picture's metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Picture {
    width: f64,
    height: f64,
}

impl Picture {
    fn of(metadata: &Value) -> Picture {
        let number = |key: &str| metadata[key].as_f64().unwrap_or(0.0);
        Picture {
            width: number("deviceWidth"),
            height: number("deviceHeight"),
        }
    }

    /// The picture in CSS pixels. Edge on Windows at a display scale other
    /// than 100 % sends some pictures' viewport in device pixels
    /// (a 1254 px page as 1379): taken as CSS pixels, the panel would draw
    /// them zoomed in and treat them as a page of another size.
    fn in_css(self, shown: Shown, scale: f64) -> Picture {
        let near = |a: f64, b: f64| (a - b).abs() <= FIT_SLACK;
        let at = |picture: &Picture| {
            near(picture.width, shown.width) && near(picture.height, shown.height)
        };
        if scale <= 1.0 || at(&self) {
            return self;
        }
        let css = Picture {
            width: self.width / scale,
            height: self.height / scale,
        };
        if at(&css) {
            css
        } else {
            self
        }
    }
}

/// Whether a picture shows the page at the size the view gave it: its
/// viewport is that size, and its pixels have that shape. While a page
/// loads with an emulated size, Chrome can send pictures of the tab's own
/// viewport; shown in the panel, each looks like a sudden zoom.
fn fits(shown: Shown, width: f64, height: f64, pixels: Option<(u32, u32)>) -> bool {
    let (want_w, want_h) = (shown.width, shown.height);
    if (width - want_w).abs() > FIT_SLACK || (height - want_h).abs() > FIT_SLACK {
        return false;
    }
    pixels.is_none_or(|(w, h)| {
        let (w, h) = (f64::from(w), f64::from(h));
        h > 0.0 && (w / h - want_w / want_h).abs() <= 0.02 * want_w / want_h
    })
}

/// The pixel size in a base64 JPEG's header, if it has one up front.
fn jpeg_size(data: &str) -> Option<(u32, u32)> {
    let head = head(data)?;
    let mut at = 2;
    while at + 9 < head.len() {
        if head[at] != 0xFF {
            return None;
        }
        let marker = head[at + 1];
        // Start of frame, baseline or progressive.
        if matches!(marker, 0xC0..=0xC2) {
            let height = u16::from_be_bytes([head[at + 5], head[at + 6]]);
            let width = u16::from_be_bytes([head[at + 7], head[at + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += 2 + usize::from(u16::from_be_bytes([head[at + 2], head[at + 3]]));
    }
    None
}

/// The pixel size in a base64 PNG's header.
fn png_size(data: &str) -> Option<(u32, u32)> {
    let head = head(data)?;
    if head.get(1..4) != Some(b"PNG".as_slice()) || head.len() < 24 {
        return None;
    }
    Some((
        u32::from_be_bytes(head[16..20].try_into().ok()?),
        u32::from_be_bytes(head[20..24].try_into().ok()?),
    ))
}

/// The first few KB of a base64 picture, decoded.
fn head(data: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let end = data.len().min(8192) / 4 * 4;
    base64::engine::general_purpose::STANDARD
        .decode(data.get(..end)?)
        .ok()
}

/// Hand a picture to the panel ([`LiveControl::offer`]) and to `sink`.
fn offer(control: &LiveControl, sink: &mut impl FnMut(LiveFrame), frame: LiveFrame) {
    if let Some(packed) = frame.packed() {
        control.offer(frame.seq, packed);
    }
    sink(frame);
}

/// A lossless picture on its way from the browser.
struct Still {
    answer: mpsc::Receiver<Reply>,
    /// The page as the last moving picture showed it, which a still one
    /// shows too.
    picture: Picture,
}

/// Ask for a lossless picture of what the tab shows now. Encoded for speed
/// rather than size: it is on its way to the screen, not to a file.
fn request_still(link: &Link, tab: &LiveTab) -> Option<mpsc::Receiver<Reply>> {
    link.request_system(Command::in_session(
        &tab.main,
        Method::PageCaptureScreenshot,
        json!({ "format": "png", "fromSurface": true, "optimizeForSpeed": true }),
    ))
    .ok()
}

/// The panel's Back, Forward, Reload and address field.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LiveNav {
    Back,
    Forward,
    Reload,
    Go { url: String },
}

/// Move the tab the way the user asked. Addresses go through the same check
/// as an agent's ([`policy::check_url`]): web pages only.
pub fn navigate(link: &Link, tab: &LiveTab, nav: &LiveNav) -> Result<(), String> {
    match nav {
        LiveNav::Reload => send(link, &tab.main, Method::PageReload, json!({})).map(|_| ()),
        LiveNav::Go { url } => {
            let url = policy::check_url(&address(url))
                .map_err(|_| "MonoCode opens only web addresses here.".to_string())?;
            send(link, &tab.main, Method::PageNavigate, json!({ "url": url })).map(|_| ())
        }
        LiveNav::Back | LiveNav::Forward => {
            let history = send(link, &tab.main, Method::PageGetNavigationHistory, json!({}))?;
            let current = history["currentIndex"].as_i64().unwrap_or(0);
            let target = current + if *nav == LiveNav::Back { -1 } else { 1 };
            let Some(entry) = usize::try_from(target)
                .ok()
                .and_then(|i| history["entries"].get(i))
            else {
                return Ok(());
            };
            send(
                link,
                &tab.main,
                Method::PageNavigateToHistoryEntry,
                json!({ "entryId": entry["id"] }),
            )
            .map(|_| ())
        }
    }
}

/// What the user typed into the address field, as an address: a bare host
/// (`example.com`, `localhost:5173`) gets a scheme, the way a browser's
/// address bar would.
fn address(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") || typed.eq_ignore_ascii_case("about:blank") {
        return typed.to_string();
    }
    let host = typed.split(['/', '?', '#']).next().unwrap_or("");
    let local = host.starts_with("localhost")
        || host.starts_with("127.")
        || host.starts_with("[::1]")
        || host.starts_with("0.0.0.0");
    format!("{}://{typed}", if local { "http" } else { "https" })
}

/// Hand one input event to the tab.
pub fn input(link: &Link, tab: &LiveTab, input: &LiveInput) -> Result<(), String> {
    let (method, params) = command_for(input);
    let started = Instant::now();
    // Scrolling and moving don't wait for the page: one wheel after another
    // would each wait a round trip, which caps scrolling at about 30 steps a
    // second. They still reach the page in order, behind what came before.
    let flowing = matches!(
        input,
        LiveInput::Wheel { .. }
            | LiveInput::Mouse {
                event: MouseEvent::Moved,
                ..
            }
    );
    let sent = if flowing {
        link.post_system(Command::in_session(&tab.main, method, params))
            .map_err(|e| format!("The browser refused {}: {e:?}", method.name()))
    } else {
        send(link, &tab.main, method, params).map(|_| ())
    };
    let took = started.elapsed().as_micros() as u64;
    INPUTS[0].fetch_add(1, Ordering::AcqRel);
    INPUTS[1].fetch_add(took, Ordering::AcqRel);
    INPUTS[2].fetch_max(took, Ordering::AcqRel);
    sent
}

fn command_for(input: &LiveInput) -> (Method, Value) {
    match input {
        LiveInput::Mouse {
            event,
            x,
            y,
            button,
            buttons,
            click_count,
            modifiers,
        } => (
            Method::InputDispatchMouseEvent,
            json!({
                "type": match event {
                    MouseEvent::Pressed => "mousePressed",
                    MouseEvent::Released => "mouseReleased",
                    MouseEvent::Moved => "mouseMoved",
                },
                "x": x,
                "y": y,
                "button": match button {
                    MouseButton::None => "none",
                    MouseButton::Left => "left",
                    MouseButton::Middle => "middle",
                    MouseButton::Right => "right",
                },
                "buttons": buttons,
                "clickCount": click_count,
                "modifiers": modifiers,
            }),
        ),
        LiveInput::Wheel {
            x,
            y,
            delta_x,
            delta_y,
            modifiers,
        } => (
            Method::InputDispatchMouseEvent,
            json!({
                "type": "mouseWheel",
                "x": x,
                "y": y,
                "deltaX": delta_x,
                "deltaY": delta_y,
                "modifiers": modifiers,
            }),
        ),
        LiveInput::Key {
            event,
            key,
            code,
            text,
            key_code,
            modifiers,
        } => {
            let mut params = json!({
                "type": match event {
                    KeyEvent::KeyDown => "keyDown",
                    KeyEvent::RawKeyDown => "rawKeyDown",
                    KeyEvent::KeyUp => "keyUp",
                },
                "key": key,
                "code": code,
                "windowsVirtualKeyCode": key_code,
                "nativeVirtualKeyCode": key_code,
                "modifiers": modifiers,
            });
            if let Some(text) = text.as_ref().filter(|_| *event == KeyEvent::KeyDown) {
                params["text"] = json!(text);
                params["unmodifiedText"] = json!(text);
            }
            (Method::InputDispatchKeyEvent, params)
        }
        LiveInput::Text { text } => (Method::InputInsertText, json!({ "text": text })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packed_picture_carries_its_header_and_bytes() {
        let frame = LiveFrame {
            tab: "t_1".into(),
            data: "/9j/AA==".into(),
            format: "png",
            width: 800.5,
            height: 600.0,
            seq: 7,
        };
        let bytes = frame.packed().unwrap();
        assert_eq!(bytes[0], 1);
        assert_eq!(&bytes[1..4], &[0, 0, 0]);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 7);
        assert_eq!(f64::from_le_bytes(bytes[8..16].try_into().unwrap()), 800.5);
        assert_eq!(f64::from_le_bytes(bytes[16..24].try_into().unwrap()), 600.0);
        assert_eq!(&bytes[PACKED_HEADER..], &[0xFF, 0xD8, 0xFF, 0x00]);
        let broken = LiveFrame {
            data: "not base64!".into(),
            ..frame
        };
        assert_eq!(broken.packed(), None);
    }

    #[test]
    fn the_panel_takes_the_newest_picture_and_the_browser_waits_for_it() {
        let control = LiveControl::new("v".into(), TabId("t".into()));
        // Nothing yet: the request waits, then comes back empty.
        assert_eq!(control.take(0, Duration::from_millis(20)), None);
        // The first picture never waits for the panel.
        assert!(!control.behind(1));
        control.offer(1, vec![1]);
        // The second does, until the panel has taken the first.
        assert!(control.behind(2));
        assert_eq!(control.take(0, Duration::ZERO), Some(vec![1]));
        assert!(!control.behind(2));
        // Two pictures the panel was too slow for: it gets only the newest,
        // and what it already has is never handed out again.
        control.offer(2, vec![2]);
        control.offer(3, vec![3]);
        assert_eq!(control.take(1, Duration::ZERO), Some(vec![3]));
        assert_eq!(control.take(3, Duration::ZERO), None);
        assert!(!control.behind(4));
        assert!(control.behind(5));
        // A waiting request wakes for a picture, and for the end.
        let waiter = std::sync::Arc::new(control);
        let (early, late) = (waiter.clone(), waiter.clone());
        let got = std::thread::spawn(move || early.take(3, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(20));
        waiter.offer(4, vec![4]);
        assert_eq!(got.join().unwrap(), Some(vec![4]));
        let none = std::thread::spawn(move || late.take(4, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(20));
        waiter.stop();
        assert_eq!(none.join().unwrap(), None);
    }

    #[test]
    fn panel_input_maps_to_cdp_input() {
        let click: LiveInput = serde_json::from_value(json!({
            "kind": "mouse", "event": "mousePressed", "x": 10.5, "y": 20.0,
            "button": "left", "buttons": 1, "clickCount": 1
        }))
        .unwrap();
        let (method, params) = command_for(&click);
        assert_eq!(method, Method::InputDispatchMouseEvent);
        assert_eq!(params["type"], "mousePressed");
        assert_eq!(params["button"], "left");
        assert_eq!(params["clickCount"], 1);

        let wheel: LiveInput = serde_json::from_value(json!({
            "kind": "wheel", "x": 1.0, "y": 2.0, "deltaX": 0.0, "deltaY": 120.0
        }))
        .unwrap();
        assert_eq!(command_for(&wheel).1["type"], "mouseWheel");

        let enter: LiveInput = serde_json::from_value(json!({
            "kind": "key", "event": "keyDown", "key": "Enter", "code": "Enter",
            "text": "\r", "keyCode": 13
        }))
        .unwrap();
        let (method, params) = command_for(&enter);
        assert_eq!(method, Method::InputDispatchKeyEvent);
        assert_eq!(params["windowsVirtualKeyCode"], 13);
        assert_eq!(params["text"], "\r");

        let up: LiveInput = serde_json::from_value(json!({
            "kind": "key", "event": "keyUp", "key": "a", "code": "KeyA", "text": "a"
        }))
        .unwrap();
        assert!(command_for(&up).1.get("text").is_none());

        let text: LiveInput =
            serde_json::from_value(json!({ "kind": "text", "text": "Ćao" })).unwrap();
        assert_eq!(
            command_for(&text),
            (Method::InputInsertText, json!({ "text": "Ćao" }))
        );

        assert!(serde_json::from_value::<LiveInput>(json!({
            "kind": "text", "text": "x", "extra": 1
        }))
        .is_err());
    }

    #[test]
    fn only_pictures_of_the_panels_viewport_fit() {
        use base64::Engine as _;
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        // SOI, an APP0 segment, then a baseline frame of 900x600.
        let jpeg = b64(&[
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02,
            0x58, 0x03, 0x84, 0x03, 0x00, 0x00, 0x00, 0x00,
        ]);
        assert_eq!(jpeg_size(&jpeg), Some((900, 600)));
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1350u32.to_be_bytes());
        png.extend_from_slice(&900u32.to_be_bytes());
        assert_eq!(png_size(&b64(&png)), Some((1350, 900)));

        let panel = Shown {
            width: 900.0,
            height: 600.0,
            window: false,
        };
        assert!(fits(panel, 900.0, 600.0, Some((900, 600))));
        assert!(fits(panel, 900.0, 600.0, Some((1350, 900))));
        assert!(fits(panel, 900.0, 600.0, None));
        // The tab's own window, or a picture of another shape.
        assert!(!fits(panel, 1200.0, 800.0, Some((900, 600))));
        assert!(!fits(panel, 900.0, 600.0, Some((900, 501))));
        // A window at 125 % has fractional CSS pixels.
        let window = Shown {
            width: 803.2,
            height: 559.2,
            window: true,
        };
        assert!(fits(window, 803.2, 559.2, Some((1004, 699))));
        assert!(!fits(window, 900.0, 600.0, None));
        let picture = Picture::of(&json!({
            "deviceWidth": 803.2, "deviceHeight": 559.2,
            "scrollOffsetX": 0, "scrollOffsetY": 4210.5, "pageScaleFactor": 1
        }));
        assert_eq!((picture.width, picture.height), (803.2, 559.2));
    }

    #[test]
    fn a_picture_in_device_pixels_is_read_in_css_pixels() {
        let page = Shown {
            width: 1254.0,
            height: 1125.0,
            window: true,
        };
        let of = |width: f64, height: f64| Picture { width, height };
        // Edge at 110 %: the same page, once in CSS and once in device pixels.
        let css = of(1254.0, 1125.0);
        assert_eq!(css.in_css(page, 1.1), css);
        let device = of(1379.4, 1237.5).in_css(page, 1.1);
        assert!((device.width - 1254.0).abs() < 0.01, "{device:?}");
        assert!((device.height - 1125.0).abs() < 0.01, "{device:?}");
        // A page of another size stays what it is, and so does all at 100 %.
        let other = of(1600.0, 900.0);
        assert_eq!(other.in_css(page, 1.1), other);
        assert_eq!(of(1379.4, 1237.5).in_css(page, 1.0).width, 1379.4);
    }

    #[test]
    fn layout_metrics_give_the_viewport_and_its_pixel_ratio() {
        let metrics = json!({
            "layoutViewport": { "clientWidth": 1004, "clientHeight": 699 },
            "cssLayoutViewport": { "clientWidth": 803, "clientHeight": 559 },
            "cssVisualViewport": { "clientWidth": 803.2, "clientHeight": 559.2, "scale": 1, "zoom": 1 }
        });
        let seen = viewport_of(&metrics).unwrap();
        assert_eq!((seen.width, seen.height), (803.2, 559.2));
        assert!((seen.scale - 1.25).abs() < 0.01, "{seen:?}");
        assert_eq!(viewport_of(&json!({})), None);
    }

    #[test]
    fn panel_size_stays_in_bounds() {
        let tiny = PanelSize::new(10, 10_000, f64::NAN);
        assert_eq!(
            (tiny.width, tiny.height, tiny.scale),
            (MIN_SIDE, MAX_SIDE, 1.0)
        );
        assert_eq!(picture_for(800.0, 600.0, 2.0), (1602, 1202));
        assert_eq!(picture_for(885.0, 600.0, 1.25), (1109, 752));
        assert_eq!(PanelSize::new(800, 600, 9.0).scale, MAX_SCALE);
    }

    #[test]
    fn the_address_field_reads_bare_hosts_like_a_browser() {
        assert_eq!(address(" example.com/a "), "https://example.com/a");
        assert_eq!(address("localhost:5173"), "http://localhost:5173");
        assert_eq!(address("127.0.0.1:8080/x"), "http://127.0.0.1:8080/x");
        assert_eq!(address("http://example.com"), "http://example.com");
        let nav: LiveNav = serde_json::from_value(json!({ "kind": "go", "url": "a.dev" })).unwrap();
        assert_eq!(
            nav,
            LiveNav::Go {
                url: "a.dev".into()
            }
        );
        assert_eq!(
            serde_json::from_value::<LiveNav>(json!({ "kind": "back" })).unwrap(),
            LiveNav::Back
        );
    }
}
