//! "Point at an element": the user picks an element in one of a thread's
//! tabs with the browser's own inspect highlight (CDP `Overlay`), and the
//! composer gets a line for the agent naming it, plus a picture of it.
//!
//! Everything here is sent as the system on the user's behalf, never for an
//! agent. The pick is remembered on the tab's ref table
//! ([`super::semantic::mark_picked`]), so the agent's next snapshot of that
//! document shows the element with a ref and `[picked by the user]`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::link::{CdpSessionId, Command, EventFilter, Link, Method};
use super::protocol::TabId;
use super::semantic::snapshot::{clip, PICKED_FLAG};

/// How long the browser waits for the user's click.
pub const PICK_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(100);
const INSPECT_EVENT: &str = "Overlay.inspectNodeRequested";
/// Room around the element in its picture, in CSS pixels.
const PAD: f64 = 8.0;
/// Longest side of the picture, in pixels.
const MAX_SIDE: f64 = 1200.0;
const MAX_ATTRIBUTE: usize = 60;

/// The tab the user picks in.
#[derive(Clone, Debug)]
pub struct PickTab {
    pub tab: TabId,
    /// Main-frame session.
    pub main: CdpSessionId,
    /// Out-of-process frames of the tab.
    pub frames: Vec<CdpSessionId>,
    pub url: String,
}

/// The node the user clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickedNode {
    pub session: CdpSessionId,
    pub backend: i64,
}

pub enum Outcome {
    Picked(PickedNode),
    Cancelled,
    TimedOut,
    TabGone,
}

/// What the composer gets.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PickedElement {
    pub tab: String,
    pub url: String,
    pub role: String,
    pub name: String,
    /// `<button id="save" class="primary">`.
    pub tag: String,
    /// `e12` when a snapshot of this page already gave the element a ref.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// PNG of the element, base64; elements of the main frame only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The line for the agent.
    pub text: String,
}

fn send(link: &Link, session: &CdpSessionId, method: Method, params: Value) -> Option<Value> {
    link.send_system(Command::in_session(session, method, params))
        .ok()
}

fn highlight() -> Value {
    json!({
        "showInfo": true,
        "showAccessibilityInfo": true,
        "contentColor": { "r": 45, "g": 212, "b": 191, "a": 0.30 },
        "paddingColor": { "r": 45, "g": 212, "b": 191, "a": 0.15 },
        "borderColor": { "r": 13, "g": 148, "b": 136, "a": 0.9 },
        "marginColor": { "r": 246, "g": 178, "b": 107, "a": 0.25 },
    })
}

/// Bring the tab forward, turn on inspect mode in it and wait for the
/// user's click. Inspect mode is off again when this returns, whatever the
/// outcome.
pub fn wait_for_pick(
    link: &Link,
    tab: &PickTab,
    cancel: &AtomicBool,
    timeout: Duration,
    alive: impl Fn() -> bool,
) -> Result<Outcome, String> {
    let events = link.subscribe(EventFilter {
        session: None,
        methods: vec![INSPECT_EVENT],
    });
    let sessions: Vec<CdpSessionId> = std::iter::once(tab.main.clone())
        .chain(tab.frames.iter().cloned())
        .collect();
    let _ = send(link, &tab.main, Method::PageBringToFront, json!({}));
    let mut started = false;
    for session in &sessions {
        // Overlay needs the DOM domain; a frame that refuses is skipped.
        let ok = send(link, session, Method::DomEnable, json!({})).is_some()
            && send(link, session, Method::OverlayEnable, json!({})).is_some()
            && send(
                link,
                session,
                Method::OverlaySetInspectMode,
                json!({ "mode": "searchForNode", "highlightConfig": highlight() }),
            )
            .is_some();
        started |= ok && session == &tab.main;
    }
    let outcome = if started {
        wait(&events, &sessions, cancel, timeout, alive)
    } else {
        Err("The browser did not start the element picker in that tab.".to_string())
    };
    for session in &sessions {
        let _ = send(
            link,
            session,
            Method::OverlaySetInspectMode,
            json!({ "mode": "none", "highlightConfig": {} }),
        );
        let _ = send(link, session, Method::OverlayHideHighlight, json!({}));
        let _ = send(link, session, Method::OverlayDisable, json!({}));
        let _ = send(link, session, Method::DomDisable, json!({}));
    }
    outcome
}

fn wait(
    events: &super::link::Subscription,
    sessions: &[CdpSessionId],
    cancel: &AtomicBool,
    timeout: Duration,
    alive: impl Fn() -> bool,
) -> Result<Outcome, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(Outcome::Cancelled);
        }
        if !alive() || events.is_closed() {
            return Ok(Outcome::TabGone);
        }
        if Instant::now() >= deadline {
            return Ok(Outcome::TimedOut);
        }
        let Some(event) = events.recv_timeout(POLL) else {
            continue;
        };
        let Some(session) = event.session.filter(|s| sessions.contains(s)) else {
            continue;
        };
        if let Some(backend) = event.params["backendNodeId"].as_i64() {
            return Ok(Outcome::Picked(PickedNode { session, backend }));
        }
    }
}

/// Role, name, tag and picture of the picked node. `reference` is the ref a
/// snapshot already gave it.
pub fn describe(
    link: &Link,
    tab: &PickTab,
    node: &PickedNode,
    reference: Option<u32>,
) -> PickedElement {
    let described = send(
        link,
        &node.session,
        Method::DomDescribeNode,
        json!({ "backendNodeId": node.backend }),
    )
    .unwrap_or(Value::Null);
    let tag = tag_of(&described["node"]);
    let ax = send(
        link,
        &node.session,
        Method::AccessibilityGetPartialAXTree,
        json!({ "backendNodeId": node.backend, "fetchRelatives": false }),
    )
    .unwrap_or(Value::Null);
    let (role, name) = role_and_name(&ax, node.backend);
    let image = (node.session == tab.main)
        .then(|| picture(link, &tab.main, node.backend))
        .flatten();
    let reference = reference.map(|number| format!("e{number}"));
    let text = message(
        &tab.tab.0,
        &tab.url,
        &role,
        &name,
        &tag,
        reference.as_deref(),
    );
    PickedElement {
        tab: tab.tab.0.clone(),
        url: tab.url.clone(),
        role,
        name,
        tag,
        reference,
        image,
        text,
    }
}

/// `<input type="email" name="email">` from a `DOM.describeNode` node.
pub fn tag_of(node: &Value) -> String {
    let local = node["localName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .or_else(|| node["nodeName"].as_str())
        .unwrap_or("element")
        .to_ascii_lowercase();
    let attributes: Vec<&str> = node["attributes"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    const SHOWN: [&str; 10] = [
        "id",
        "class",
        "type",
        "name",
        "role",
        "aria-label",
        "placeholder",
        "href",
        "data-testid",
        "for",
    ];
    let mut tag = format!("<{local}");
    for wanted in SHOWN {
        let Some(value) = attributes
            .chunks(2)
            .find(|pair| pair.len() == 2 && pair[0] == wanted)
            .map(|pair| pair[1])
        else {
            continue;
        };
        let value = if wanted == "class" {
            value
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            value.to_string()
        };
        let value = clip(&value, MAX_ATTRIBUTE);
        if value.is_empty() {
            continue;
        }
        tag.push_str(&format!(" {wanted}=\"{}\"", value.replace('"', "'")));
    }
    tag.push('>');
    tag
}

/// Role and accessible name of `backend` from `Accessibility.getPartialAXTree`.
pub fn role_and_name(ax: &Value, backend: i64) -> (String, String) {
    let nodes = ax["nodes"].as_array().cloned().unwrap_or_default();
    let Some(node) = nodes
        .iter()
        .find(|n| n["backendDOMNodeId"].as_i64() == Some(backend))
        .or_else(|| nodes.first())
    else {
        return ("generic".into(), String::new());
    };
    let role = match node["role"]["value"].as_str() {
        Some(role) if node["ignored"].as_bool() != Some(true) && !role.is_empty() => match role {
            "RootWebArea" => "document",
            "StaticText" => "text",
            "none" => "generic",
            other => other,
        },
        _ => "generic",
    };
    let name = clip(node["name"]["value"].as_str().unwrap_or(""), 120);
    (role.to_string(), name)
}

/// The line the composer gets.
pub fn message(
    tab: &str,
    url: &str,
    role: &str,
    name: &str,
    tag: &str,
    reference: Option<&str>,
) -> String {
    let what = if name.is_empty() {
        format!("{role} {tag}")
    } else {
        format!("{role} \"{name}\" {tag}")
    };
    match reference {
        Some(reference) => format!(
            "Element I picked in the browser: {what} in tab {tab} ({url}), ref {reference}."
        ),
        None => format!(
            "Element I picked in the browser: {what} in tab {tab} ({url}). A browser_snapshot of that tab marks it {PICKED_FLAG}."
        ),
    }
}

/// The element and a little room around it, cut to the visible part of the
/// viewport and scaled down when large.
fn picture(link: &Link, main: &CdpSessionId, backend: i64) -> Option<String> {
    let model = send(
        link,
        main,
        Method::DomGetBoxModel,
        json!({ "backendNodeId": backend }),
    )?;
    let border: Vec<f64> = serde_json::from_value(model["model"]["border"].clone()).ok()?;
    let metrics = send(link, main, Method::PageGetLayoutMetrics, json!({}))?;
    let layout = if metrics["cssLayoutViewport"].is_object() {
        &metrics["cssLayoutViewport"]
    } else {
        &metrics["layoutViewport"]
    };
    let viewport = (
        layout["pageX"].as_f64().unwrap_or(0.0),
        layout["pageY"].as_f64().unwrap_or(0.0),
        layout["clientWidth"].as_f64().unwrap_or(0.0),
        layout["clientHeight"].as_f64().unwrap_or(0.0),
    );
    let clip = clip_rect(&border, viewport)?;
    let shot = send(
        link,
        main,
        Method::PageCaptureScreenshot,
        json!({ "format": "png", "clip": clip }),
    )?;
    shot["data"]
        .as_str()
        .filter(|data| !data.is_empty())
        .map(str::to_string)
}

/// `Page.captureScreenshot` clip for a border quad: padded, inside the
/// viewport `(pageX, pageY, width, height)`, at most [`MAX_SIDE`] pixels.
pub fn clip_rect(border: &[f64], viewport: (f64, f64, f64, f64)) -> Option<Value> {
    if border.len() < 8 {
        return None;
    }
    let (page_x, page_y, width, height) = viewport;
    let xs = [border[0], border[2], border[4], border[6]];
    let ys = [border[1], border[3], border[5], border[7]];
    let left = (xs.iter().copied().fold(f64::INFINITY, f64::min) - PAD).max(0.0);
    let top = (ys.iter().copied().fold(f64::INFINITY, f64::min) - PAD).max(0.0);
    let mut right = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) + PAD;
    let mut bottom = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) + PAD;
    if width > 0.0 {
        right = right.min(width);
    }
    if height > 0.0 {
        bottom = bottom.min(height);
    }
    let (w, h) = (right - left, bottom - top);
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let scale = (MAX_SIDE / w.max(h)).min(1.0);
    Some(json!({
        "x": left + page_x,
        "y": top + page_y,
        "width": w,
        "height": h,
        "scale": scale,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_show_the_attributes_that_identify_an_element() {
        let node = json!({
            "localName": "button",
            "attributes": ["class", "btn  primary wide big extra", "id", "save", "style", "color: red", "data-testid", "save-\"btn\""],
        });
        assert_eq!(
            tag_of(&node),
            "<button id=\"save\" class=\"btn primary wide big\" data-testid=\"save-'btn'\">"
        );
        assert_eq!(tag_of(&json!({ "nodeName": "DIV" })), "<div>");
        assert_eq!(tag_of(&Value::Null), "<element>");
    }

    #[test]
    fn role_and_name_come_from_the_picked_node() {
        let ax = json!({ "nodes": [
            { "backendDOMNodeId": 7, "role": { "value": "generic" }, "name": { "value": "" } },
            { "backendDOMNodeId": 9, "role": { "value": "button" }, "name": { "value": "Save  draft" } },
        ]});
        assert_eq!(
            role_and_name(&ax, 9),
            ("button".into(), "Save draft".into())
        );
        let ignored = json!({ "nodes": [
            { "backendDOMNodeId": 3, "ignored": true, "role": { "value": "none" } },
        ]});
        assert_eq!(
            role_and_name(&ignored, 3),
            ("generic".into(), String::new())
        );
        assert_eq!(role_and_name(&Value::Null, 1).0, "generic");
    }

    #[test]
    fn the_message_names_the_tab_and_how_to_find_the_element() {
        let with_ref = message(
            "t_1",
            "http://localhost:5173/",
            "button",
            "Save",
            "<button id=\"save\">",
            Some("e12"),
        );
        assert_eq!(
            with_ref,
            "Element I picked in the browser: button \"Save\" <button id=\"save\"> in tab t_1 (http://localhost:5173/), ref e12."
        );
        let without = message(
            "t_1",
            "https://x/",
            "generic",
            "",
            "<div class=\"card\">",
            None,
        );
        assert!(without.starts_with(
            "Element I picked in the browser: generic <div class=\"card\"> in tab t_1"
        ));
        assert!(without.ends_with("marks it [picked by the user]."));
    }

    #[test]
    fn the_picture_is_padded_kept_in_the_viewport_and_scaled() {
        // A 100x20 button at (10, 4) on a page scrolled to y = 500.
        let border = [10.0, 4.0, 110.0, 4.0, 110.0, 24.0, 10.0, 24.0];
        let clip = clip_rect(&border, (0.0, 500.0, 800.0, 600.0)).unwrap();
        assert_eq!(clip["x"], 2.0);
        assert_eq!(clip["y"], 500.0);
        assert_eq!(clip["width"], 116.0);
        assert_eq!(clip["height"], 32.0);
        assert_eq!(clip["scale"], 1.0);
        // The whole page body: cut to the viewport, scaled to fit.
        let body = [0.0, -300.0, 2400.0, -300.0, 2400.0, 5000.0, 0.0, 5000.0];
        let clip = clip_rect(&body, (0.0, 300.0, 2400.0, 1200.0)).unwrap();
        assert_eq!(clip["width"], 2400.0);
        assert_eq!(clip["height"], 1200.0);
        assert_eq!(clip["scale"], 0.5);
        assert!(clip_rect(&border[..6], (0.0, 0.0, 800.0, 600.0)).is_none());
        // Entirely outside the viewport.
        let below = [0.0, 900.0, 10.0, 900.0, 10.0, 950.0, 0.0, 950.0];
        assert!(clip_rect(&below, (0.0, 0.0, 800.0, 600.0)).is_none());
    }
}
