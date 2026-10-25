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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::link::{CdpSessionId, Command, EventFilter, Link, Method};
use super::policy;
use super::protocol::TabId;

/// Tauri event that carries each picture to the panel.
pub const FRAME_EVENT: &str = "monocode-browser-live-frame";
const FRAME: &str = "Page.screencastFrame";
const POLL: Duration = Duration::from_millis(100);
/// JPEG quality of the pictures, 0 to 100.
const QUALITY: u32 = 70;
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
    /// JPEG, base64.
    pub data: String,
    /// Size of the page's viewport in CSS pixels; input coordinates are in
    /// this space.
    pub width: f64,
    pub height: f64,
    /// Counts the pictures of one view, from 1.
    pub seq: u64,
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

    fn picture(self) -> (u32, u32) {
        (
            (f64::from(self.width) * self.scale).round() as u32,
            (f64::from(self.height) * self.scale).round() as u32,
        )
    }
}

/// One running view, named by the pane that asked for it: [`run`] watches
/// it for a stop or a new panel size.
#[derive(Debug)]
pub struct LiveControl {
    pub view: String,
    stop: AtomicBool,
    finished: AtomicBool,
    resize: Mutex<Option<PanelSize>>,
}

impl LiveControl {
    pub fn new(view: String) -> LiveControl {
        LiveControl {
            view,
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            resize: Mutex::new(None),
        }
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
    let end = loop {
        if control.stopped() {
            break LiveEnd::Stopped;
        }
        if !alive() || events.is_closed() {
            break LiveEnd::TabGone;
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
        }
        let Some(event) = events.recv_timeout(POLL) else {
            continue;
        };
        if event.method != FRAME {
            continue;
        }
        if let Some(id) = event.params["sessionId"].as_i64() {
            let _ = send(
                link,
                &tab.main,
                Method::PageScreencastFrameAck,
                json!({ "sessionId": id }),
            );
        }
        let Some(data) = event.params["data"].as_str() else {
            continue;
        };
        seq += 1;
        let metadata = &event.params["metadata"];
        sink(LiveFrame {
            tab: tab.tab.0.clone(),
            data: data.to_string(),
            width: metadata["deviceWidth"].as_f64().unwrap_or(0.0),
            height: metadata["deviceHeight"].as_f64().unwrap_or(0.0),
            seq,
        });
    };
    restore(link, tab);
    Ok(end)
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
    send(link, &tab.main, method, params).map(|_| ())
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
    fn panel_size_stays_in_bounds() {
        let tiny = PanelSize::new(10, 10_000, f64::NAN);
        assert_eq!(
            (tiny.width, tiny.height, tiny.scale),
            (MIN_SIDE, MAX_SIDE, 1.0)
        );
        assert_eq!(PanelSize::new(800, 600, 2.0).picture(), (1600, 1200));
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
