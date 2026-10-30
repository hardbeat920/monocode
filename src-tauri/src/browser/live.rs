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
//! browser would.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::link::{CdpSessionId, Command, EventFilter, Link, Method};
use super::policy;
use super::protocol::TabId;

const FRAME: &str = "Page.screencastFrame";
const POLL: Duration = Duration::from_millis(100);
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
/// How long a picture waits for the panel to draw the one before it before
/// the browser may send the next one anyway.
const ACK_WAIT: Duration = Duration::from_millis(150);
/// How often a held-back acknowledgement looks at what the panel has drawn.
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

/// The tab the panel shows.
#[derive(Clone, Debug)]
pub struct LiveTab {
    pub tab: TabId,
    /// Main-frame session.
    pub main: CdpSessionId,
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

    /// The largest picture to ask for: never under the page's own device
    /// pixels, so the browser never scales a picture down (a pixel short
    /// blurs the whole frame).
    fn picture(self) -> (u32, u32) {
        (
            (f64::from(self.width) * self.scale).ceil() as u32 + 2,
            (f64::from(self.height) * self.scale).ceil() as u32 + 2,
        )
    }
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
    /// The last picture the panel has drawn, 0 until it says.
    drawn: AtomicU64,
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
            drawn: AtomicU64::new(0),
        }
    }

    /// Pause the pictures while the agent needs the tab's viewport to
    /// itself (a full-page screenshot): with the screencast running, the
    /// capture takes several times longer and can leave the viewport at
    /// the page's height. The view resumes at its own size when the hold
    /// is dropped.
    /// Waits, at most a moment, until the pictures have stopped.
    pub fn hold(self: &std::sync::Arc<Self>) -> LiveHold {
        self.held.fetch_add(1, Ordering::AcqRel);
        let deadline = std::time::Instant::now() + HOLD_WAIT;
        while !self.paused.load(Ordering::Acquire)
            && !self.finished.load(Ordering::Acquire)
            && std::time::Instant::now() < deadline
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
    }

    /// The view has handed the tab back (its viewport and scrollbars).
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }

    /// Wait, at most `limit`, for [`Self::finish`], so the next view of the
    /// same tab is not undone by this one's late clean-up.
    pub fn wait_finished(&self, limit: Duration) {
        let deadline = std::time::Instant::now() + limit;
        while !self.finished.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The panel has drawn picture `seq`. Once it says so, the browser
    /// sends a new picture only while the panel keeps up, so pictures never
    /// queue up in front of it and what it shows stays current.
    pub fn drawn(&self, seq: u64) {
        self.drawn.fetch_max(seq, Ordering::AcqRel);
    }

    /// Whether picture `seq` waits for the panel: it has not drawn the one
    /// before it yet.
    fn behind(&self, seq: u64) -> bool {
        let drawn = self.drawn.load(Ordering::Acquire);
        drawn > 0 && seq > drawn + 1
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

/// Give the tab the panel's viewport and start the pictures at that size.
fn show(link: &Link, tab: &LiveTab, size: PanelSize) -> Result<(), String> {
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
    let (width, height) = size.picture();
    send(
        link,
        &tab.main,
        Method::PageStartScreencast,
        json!({ "format": "jpeg", "quality": QUALITY, "maxWidth": width, "maxHeight": height }),
    )
    .map(|_| ())
}

/// Hand the tab back its window's viewport and scrollbars.
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
        methods: vec![FRAME],
    });
    let _ = send(
        link,
        &tab.main,
        Method::EmulationSetScrollbarsHidden,
        json!({ "hidden": true }),
    );
    if let Err(error) = show(link, tab, size) {
        restore(link, tab);
        return Err(error);
    }
    let mut current = size;
    let mut seq = 0;
    // The last JPEG, until the page has been still long enough for a PNG.
    let mut moving: Option<(std::time::Instant, f64, f64)> = None;
    // The data of the last JPEG: Chrome on Windows sends it again right
    // after a screenshot, and it must not cover the PNG.
    let mut last = String::new();
    // Since when the pictures have not fit the panel.
    let mut misfit: Option<std::time::Instant> = None;
    let mut healed: Option<std::time::Instant> = None;
    let mut paused = false;
    // A picture sent to the panel whose acknowledgement waits until the
    // panel has drawn the one before it.
    let mut waiting: Option<(super::link::CdpEvent, std::time::Instant)> = None;
    let mut meter = Meter::new();
    let end = loop {
        meter.drawn(control.drawn.load(Ordering::Acquire));
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
            if let Err(error) = show(link, tab, current) {
                restore(link, tab);
                return Err(error);
            }
        }
        if let Some(next) = control.take_resize().filter(|next| *next != current) {
            // A new size needs pictures of the new size, so the screencast
            // starts over at it.
            let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
            if let Err(error) = show(link, tab, next) {
                restore(link, tab);
                return Err(error);
            }
            current = next;
            last.clear();
        }
        let wait = moving.map_or(POLL, |(at, _, _)| {
            STILL_AFTER.saturating_sub(at.elapsed()).min(POLL)
        });
        let wait = if waiting.is_some() {
            wait.min(ACK_POLL)
        } else {
            wait
        };
        let Some(event) = events.recv_timeout(wait) else {
            if let Some((at, width, height)) = moving {
                if at.elapsed() >= STILL_AFTER {
                    moving = None;
                    if let Some(data) =
                        still(link, tab).filter(|data| fits(current, width, height, png_size(data)))
                    {
                        seq += 1;
                        sink(LiveFrame {
                            tab: tab.tab.0.clone(),
                            data,
                            format: "png",
                            width,
                            height,
                            seq,
                        });
                    }
                }
            }
            continue;
        };
        if event.method != FRAME {
            continue;
        }
        meter.arrived(event.params["metadata"]["timestamp"].as_f64());
        let sent = 'frame: {
            let Some(data) = event.params["data"].as_str() else {
                break 'frame false;
            };
            // The same picture again: nothing moved.
            if data == last {
                break 'frame false;
            }
            let metadata = &event.params["metadata"];
            let width = metadata["deviceWidth"].as_f64().unwrap_or(0.0);
            let height = metadata["deviceHeight"].as_f64().unwrap_or(0.0);
            if fits(current, width, height, jpeg_size(data)) {
                misfit = None;
            } else {
                let since = *misfit.get_or_insert_with(std::time::Instant::now);
                // Something else changed the tab's viewport (a full-page
                // screenshot does): the panel's is put back.
                if healed.is_none_or(|at: std::time::Instant| at.elapsed() >= HEAL_EVERY) {
                    healed = Some(std::time::Instant::now());
                    let _ = send(link, &tab.main, Method::PageStopScreencast, json!({}));
                    let _ = show(link, tab, current);
                }
                if since.elapsed() < MISFIT_FOR {
                    break 'frame false;
                }
            }
            last = data.to_string();
            seq += 1;
            moving = Some((std::time::Instant::now(), width, height));
            let handing = std::time::Instant::now();
            sink(LiveFrame {
                tab: tab.tab.0.clone(),
                data: last.clone(),
                format: "jpeg",
                width,
                height,
                seq,
            });
            meter.handed(seq, handing);
            true
        };
        if sent && control.behind(seq) {
            if let Some((older, _)) = waiting.replace((event, std::time::Instant::now())) {
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
/// were when they got here, how long handing one to the panel took, how
/// long until the panel had it on screen, and how long inputs took.
struct Meter {
    since: std::time::Instant,
    from_browser: u64,
    age: Span,
    hand_over: Span,
    on_screen: Span,
    /// Pictures sent and not yet drawn: seq and when.
    sent: std::collections::VecDeque<(u64, std::time::Instant)>,
    skipped: u64,
}

impl Meter {
    fn new() -> Meter {
        Meter {
            since: std::time::Instant::now(),
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

    fn handed(&mut self, seq: u64, started: std::time::Instant) {
        self.hand_over.add(started.elapsed().as_secs_f64() * 1000.0);
        self.sent.push_back((seq, std::time::Instant::now()));
        while self.sent.len() > 240 {
            self.sent.pop_front();
        }
    }

    fn drawn(&mut self, drawn: u64) {
        while let Some(&(seq, at)) = self.sent.front() {
            if seq > drawn {
                break;
            }
            self.sent.pop_front();
            if seq == drawn {
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
                "[live] browser {:.0}/s, age on arrival {} | hand-over {} | on screen after {} ({:.0}/s, {} skipped) | inputs {} ({})",
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

/// Whether a picture shows the page at the panel's size: its viewport is
/// the panel's, and its pixels have the panel's shape. While a page loads,
/// Chrome can send pictures of the tab's own viewport; shown in the panel,
/// each looks like a sudden zoom.
fn fits(size: PanelSize, width: f64, height: f64, pixels: Option<(u32, u32)>) -> bool {
    let (want_w, want_h) = (f64::from(size.width), f64::from(size.height));
    if (width - want_w).abs() > 1.0 || (height - want_h).abs() > 1.0 {
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

/// A lossless picture of what the tab shows now.
fn still(link: &Link, tab: &LiveTab) -> Option<String> {
    send(
        link,
        &tab.main,
        Method::PageCaptureScreenshot,
        json!({ "format": "png", "fromSurface": true }),
    )
    .ok()?["data"]
        .as_str()
        .map(str::to_string)
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
    let started = std::time::Instant::now();
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
    fn pictures_wait_only_once_the_panel_reports_what_it_drew() {
        let control = LiveControl::new("v".into(), TabId("t".into()));
        // Before the panel says anything, nothing waits.
        assert!(!control.behind(10));
        control.drawn(3);
        assert!(!control.behind(4));
        assert!(control.behind(5));
        // An older report never moves it back.
        control.drawn(2);
        assert!(control.behind(5));
        control.drawn(4);
        assert!(!control.behind(5));
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

        let panel = PanelSize::new(900, 600, 1.5);
        assert!(fits(panel, 900.0, 600.0, Some((900, 600))));
        assert!(fits(panel, 900.0, 600.0, Some((1350, 900))));
        assert!(fits(panel, 900.0, 600.0, None));
        // The tab's own window, or a picture of another shape.
        assert!(!fits(panel, 1200.0, 800.0, Some((900, 600))));
        assert!(!fits(panel, 900.0, 600.0, Some((900, 501))));
    }

    #[test]
    fn panel_size_stays_in_bounds() {
        let tiny = PanelSize::new(10, 10_000, f64::NAN);
        assert_eq!(
            (tiny.width, tiny.height, tiny.scale),
            (MIN_SIDE, MAX_SIDE, 1.0)
        );
        assert_eq!(PanelSize::new(800, 600, 2.0).picture(), (1602, 1202));
        assert_eq!(PanelSize::new(885, 600, 1.25).picture(), (1109, 752));
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
