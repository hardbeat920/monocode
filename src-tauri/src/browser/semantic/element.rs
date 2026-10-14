//! Isolated worlds, ref resolution and the shared element pipeline (spec 5
//! "Izolovani svet", 9.2 steps 2-6).
//!
//! Every page-side check runs in the `__monocode` isolated world of the
//! element's own frame, through `Runtime.callFunctionOn` on an object
//! resolved with `DOM.resolveNode {backendNodeId, executionContextId}`. The
//! page's main world never sees our functions and nothing the agent sends is
//! executed.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::link::Method;
use super::super::protocol::{Reason, ISOLATED_WORLD};
use super::refs::{self, FrameCtx, NodeRef};
use super::{Fail, Run};

pub const ACTIONABILITY_JS: &str = include_str!("../injected/actionability.js");
pub const HITTEST_JS: &str = include_str!("../injected/hittest.js");
pub const EDIT_JS: &str = include_str!("../injected/edit.js");
pub const SELECT_JS: &str = include_str!("../injected/select.js");
pub const PAGE_JS: &str = include_str!("../injected/page.js");
pub const READ_JS: &str = include_str!("../injected/read.js");

/// How long an element may keep moving before `not_stable`.
const STABLE_BUDGET: Duration = Duration::from_secs(2);
/// Gap between the two box samples of the stability check. Measured from
/// Rust, because timers and `requestAnimationFrame` are throttled or paused
/// in background tabs.
const STABLE_GAP: Duration = Duration::from_millis(50);
/// Hit-test attempts and their spacing (spec 9.2 step 6).
const HIT_ATTEMPTS: usize = 3;
const HIT_GAP: Duration = Duration::from_millis(100);

/// A ref resolved to a remote object in our isolated world.
#[derive(Clone, Debug)]
pub struct Element {
    pub node: NodeRef,
    pub object: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn close_to(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() < 0.5
            && (self.y - other.y).abs() < 0.5
            && (self.width - other.width).abs() < 0.5
            && (self.height - other.height).abs() < 0.5
    }
}

/// What `actionability.js` reports.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Facts {
    pub connected: bool,
    pub visible: bool,
    pub enabled: bool,
    pub editable: bool,
    pub text_like: bool,
    pub file_input: bool,
    pub multiple: bool,
    pub select: bool,
    pub tag: String,
    pub rect: Option<Rect>,
}

/// Which steps of the pipeline an action needs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Needs {
    pub visible: bool,
    pub enabled: bool,
    pub editable: bool,
    pub scroll: bool,
    pub stable: bool,
    /// Compute the input point (content quads inside the viewport).
    pub point: bool,
    pub hit: bool,
}

/// A point in the tab's viewport (for `Input.*`, always sent to the main
/// session) and the same point in the element's frame (for hit tests).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub local_x: f64,
    pub local_y: f64,
}

#[derive(Debug)]
pub struct Prepared {
    pub facts: Facts,
    pub point: Option<Point>,
}

/// The main frame as a [`FrameCtx`].
pub fn main_frame(run: &Run<'_>) -> Result<FrameCtx, Fail> {
    let frame_id = run.lease(|l| l.main_frame_id.clone())?;
    Ok(FrameCtx {
        session: run.main().clone(),
        frame_id: frame_id.or_else(|| Some(run.handle.target.0.clone())),
        owners: Vec::new(),
    })
}

enum WorldSlot {
    Main,
    Oopif(String),
    Subframe,
}

fn slot(run: &Run<'_>, frame: &FrameCtx) -> Result<WorldSlot, Fail> {
    if frame.session == *run.main() && !frame.is_oopif() {
        let main = run.lease(|l| l.main_frame_id.clone())?;
        if frame.frame_id.is_none() || frame.frame_id == main {
            return Ok(WorldSlot::Main);
        }
        return Ok(WorldSlot::Subframe);
    }
    let root = run.lease(|l| {
        l.frames
            .iter()
            .find(|(_, f)| f.cdp_session == frame.session)
            .map(|(id, _)| id.clone())
    })?;
    match root {
        Some(id) if frame.frame_id.as_deref() == Some(id.as_str()) => Ok(WorldSlot::Oopif(id)),
        Some(_) => Ok(WorldSlot::Subframe),
        None => Err(Fail::detail(
            Reason::RefStale,
            "The frame of this element is gone; take a new snapshot.",
        )),
    }
}

/// The `__monocode` world's execution context in `frame`, created once per
/// document and cached on the lease (main frame, OOPIF roots) or in the ref
/// table (same-process subframes).
pub fn world(run: &Run<'_>, frame: &FrameCtx) -> Result<i64, Fail> {
    let slot = slot(run, frame)?;
    let cached = match &slot {
        WorldSlot::Main => run.lease(|l| l.main_execution_context)?,
        WorldSlot::Oopif(id) => {
            run.lease(|l| l.frames.get(id).and_then(|f| f.execution_context_id))?
        }
        WorldSlot::Subframe => refs::world(&run.key(), frame),
    };
    if let Some(context) = cached {
        return Ok(context);
    }
    let Some(frame_id) = frame.frame_id.clone() else {
        return Err(Fail::reason(Reason::UnsupportedFrame));
    };
    let created = run.send(
        &frame.session,
        Method::PageCreateIsolatedWorld,
        json!({ "frameId": frame_id, "worldName": ISOLATED_WORLD, "grantUniveralAccess": false }),
    )?;
    let context = created["executionContextId"].as_i64().ok_or_else(|| {
        Fail::detail(
            Reason::UnsupportedFrame,
            "The browser did not create an isolated world for this frame.",
        )
    })?;
    store_world(run, frame, &slot, Some(context));
    Ok(context)
}

fn store_world(run: &Run<'_>, frame: &FrameCtx, slot: &WorldSlot, context: Option<i64>) {
    match slot {
        WorldSlot::Main => {
            if let Some(lease) = run.link.policy().write().lease_mut(run.tab()) {
                lease.main_execution_context = context;
            }
        }
        WorldSlot::Oopif(id) => {
            if let Some(lease) = run.link.policy().write().lease_mut(run.tab()) {
                if let Some(frame) = lease.frames.get_mut(id) {
                    frame.execution_context_id = context;
                }
            }
        }
        WorldSlot::Subframe => refs::set_world(&run.key(), frame, context),
    }
}

/// Drop a cached context the browser no longer knows (the frame navigated).
fn forget_world(run: &Run<'_>, frame: &FrameCtx) {
    if let Ok(slot) = slot(run, frame) {
        store_world(run, frame, &slot, None);
    }
}

fn stale_context(fail: &Fail) -> bool {
    fail.protocol_message()
        .is_some_and(|m| m.to_ascii_lowercase().contains("context"))
}

/// Ref string to a live element: table lookup (no command), then resolve.
pub fn locate(run: &Run<'_>, raw: &str) -> Result<Element, Fail> {
    let node = {
        let state = run.link.policy().read();
        let lease = state
            .lease(run.tab())
            .ok_or_else(|| Fail::reason(Reason::TargetClosed))?;
        refs::lookup(&run.key(), lease, raw)?
    };
    resolve(run, &node, raw)
}

/// `DOM.resolveNode` into our world (spec 9.2 step 2). A node that no longer
/// exists is `ref_stale`.
pub fn resolve(run: &Run<'_>, node: &NodeRef, raw: &str) -> Result<Element, Fail> {
    let object = resolve_backend(run, &node.frame, node.backend).map_err(|fail| {
        if fail.protocol_message().is_some() {
            Fail::detail(
                Reason::RefStale,
                format!("Element {raw} is no longer in the page; call browser_snapshot and use fresh refs."),
            )
        } else {
            fail
        }
    })?;
    Ok(Element {
        node: node.clone(),
        object,
    })
}

fn resolve_backend(run: &Run<'_>, frame: &FrameCtx, backend: i64) -> Result<String, Fail> {
    for attempt in 0..2 {
        let context = world(run, frame)?;
        match run.send(
            &frame.session,
            Method::DomResolveNode,
            json!({ "backendNodeId": backend, "executionContextId": context }),
        ) {
            Ok(value) => {
                return value["object"]["objectId"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| Fail::reason(Reason::Detached));
            }
            Err(fail) if attempt == 0 && stale_context(&fail) => forget_world(run, frame),
            Err(fail) => return Err(fail),
        }
    }
    Err(Fail::reason(Reason::Detached))
}

/// A call argument carrying a JSON value.
pub fn arg(value: Value) -> Value {
    json!({ "value": value })
}

fn call_params(function: &str, target: Value, args: &[Value]) -> Value {
    let mut params = json!({
        "functionDeclaration": function,
        "arguments": args,
        "returnByValue": true,
        "awaitPromise": false,
        "silent": true,
    });
    if let (Some(params), Some(target)) = (params.as_object_mut(), target.as_object()) {
        for (key, value) in target {
            params.insert(key.clone(), value.clone());
        }
    }
    params
}

/// Unwrap a `Runtime.callFunctionOn` answer. A thrown exception is reported
/// as a failure of the page script, never retried.
pub fn call_value(result: Value) -> Result<Value, Fail> {
    if let Some(details) = result.get("exceptionDetails") {
        let text = details["exception"]["description"]
            .as_str()
            .or_else(|| details["text"].as_str())
            .unwrap_or("exception");
        let first: String = text
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        return Err(Fail::detail(
            Reason::Detached,
            format!("The page changed under the action ({first}); take a new snapshot."),
        ));
    }
    Ok(result["result"]["value"].clone())
}

fn object_gone(fail: Fail) -> Fail {
    match fail.protocol_message() {
        Some(_) => Fail::detail(
            Reason::Detached,
            "The element left the document during the action; take a new snapshot.",
        ),
        None => fail,
    }
}

/// Parameters of a call on an element (for [`super::observe::Watch::input`]).
pub fn element_call(el: &Element, function: &str, args: &[Value]) -> Value {
    call_params(function, json!({ "objectId": el.object }), args)
}

/// Read-only call of one of our functions with `this` = the element.
pub fn call(run: &Run<'_>, el: &Element, function: &str, args: &[Value]) -> Result<Value, Fail> {
    let result = run
        .send(
            &el.node.frame.session,
            Method::RuntimeCallFunctionOn,
            element_call(el, function, args),
        )
        .map_err(object_gone)?;
    call_value(result)
}

/// Call one of our functions at document level in `frame`'s isolated world.
/// `mutating` marks the action as having changed the page (scroll fallback).
pub fn call_frame(
    run: &Run<'_>,
    frame: &FrameCtx,
    function: &str,
    args: &[Value],
    mutating: bool,
) -> Result<Value, Fail> {
    for attempt in 0..2 {
        let context = world(run, frame)?;
        let params = call_params(function, json!({ "executionContextId": context }), args);
        let sent = if mutating {
            run.send_mut(&frame.session, Method::RuntimeCallFunctionOn, params)
        } else {
            run.send(&frame.session, Method::RuntimeCallFunctionOn, params)
        };
        match sent {
            Ok(result) => return call_value(result),
            Err(fail) if attempt == 0 && stale_context(&fail) => forget_world(run, frame),
            Err(fail) => return Err(fail),
        }
    }
    Err(Fail::reason(Reason::Detached))
}

pub fn facts(run: &Run<'_>, el: &Element) -> Result<Facts, Fail> {
    let value = call(run, el, ACTIONABILITY_JS, &[arg(json!("full"))])?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

fn rect(run: &Run<'_>, el: &Element) -> Result<Option<Rect>, Fail> {
    let value = call(run, el, ACTIONABILITY_JS, &[arg(json!("rect"))])?;
    let facts: Facts = serde_json::from_value(value).unwrap_or_default();
    if !facts.connected {
        return Err(Fail::reason(Reason::Detached));
    }
    Ok(facts.rect)
}

/// Spec 9.2 steps 3-6 for an element: actionability, scroll into view,
/// stability, input point and hit test, as `needs` asks.
pub fn prepare(run: &Run<'_>, el: &Element, needs: Needs) -> Result<Prepared, Fail> {
    let facts = facts(run, el)?;
    if !facts.connected {
        return Err(Fail::reason(Reason::Detached));
    }
    if needs.visible && !facts.visible {
        return Err(Fail::reason(Reason::NotVisible));
    }
    if needs.enabled && !facts.enabled {
        return Err(Fail::reason(Reason::NotEnabled));
    }
    if needs.editable && !facts.editable {
        return Err(Fail::reason(Reason::NotEditable));
    }
    if needs.scroll {
        scroll_into_view(run, el)?;
    }
    if needs.stable {
        wait_stable(run, el)?;
    }
    let point = if needs.point || needs.hit {
        Some(point(run, el)?)
    } else {
        None
    };
    if let (true, Some(point)) = (needs.hit, point) {
        hit_test(run, el, point)?;
    }
    Ok(Prepared { facts, point })
}

/// Steps 5 and 6 again, after time passed since [`prepare`] (the visible
/// cursor's glide): the input point now, hit-tested when `hit`.
pub fn recheck(run: &Run<'_>, el: &Element, hit: bool) -> Result<Point, Fail> {
    let point = point(run, el)?;
    if hit {
        hit_test(run, el, point)?;
    }
    Ok(point)
}

/// `DOM.scrollIntoViewIfNeeded` (spec 9.2 step 4).
pub fn scroll_into_view(run: &Run<'_>, el: &Element) -> Result<(), Fail> {
    match run.send(
        &el.node.frame.session,
        Method::DomScrollIntoViewIfNeeded,
        json!({ "backendNodeId": el.node.backend }),
    ) {
        Ok(_) => Ok(()),
        Err(fail) => match fail.protocol_message() {
            Some(message) if message.to_ascii_lowercase().contains("detached") => {
                Err(Fail::reason(Reason::Detached))
            }
            Some(_) => Err(Fail::reason(Reason::NotVisible)),
            None => Err(fail),
        },
    }
}

fn wait_stable(run: &Run<'_>, el: &Element) -> Result<(), Fail> {
    let until = Instant::now() + STABLE_BUDGET;
    loop {
        let first = rect(run, el)?;
        run.pause(STABLE_GAP);
        let second = rect(run, el)?;
        match (first, second) {
            (Some(a), Some(b)) if a.close_to(&b) => return Ok(()),
            (None, None) => return Ok(()),
            _ => {}
        }
        if Instant::now() >= until || run.ctx.expired() {
            return Err(Fail::reason(Reason::NotStable));
        }
    }
}

/// Offset of an OOPIF's viewport inside the tab's viewport: the content
/// box of each `<iframe>` owner on the way down (spec 5, open question O4).
pub fn frame_offset(run: &Run<'_>, frame: &FrameCtx) -> Result<(f64, f64), Fail> {
    let mut x = 0.0;
    let mut y = 0.0;
    for (session, owner) in &frame.owners {
        let model = run
            .send(
                session,
                Method::DomGetBoxModel,
                json!({ "backendNodeId": owner }),
            )
            .map_err(|fail| match fail.protocol_message() {
                Some(_) => Fail::reason(Reason::UnsupportedFrame),
                None => fail,
            })?;
        let content = &model["model"]["content"];
        x += content[0].as_f64().unwrap_or(0.0);
        y += content[1].as_f64().unwrap_or(0.0);
    }
    Ok((x, y))
}

/// Layout viewport of the tab: `(page_x, page_y, width, height)`.
pub fn viewport(run: &Run<'_>) -> Result<(f64, f64, f64, f64), Fail> {
    let metrics = run.send(run.main(), Method::PageGetLayoutMetrics, json!({}))?;
    let layout = if metrics["cssLayoutViewport"].is_object() {
        &metrics["cssLayoutViewport"]
    } else {
        &metrics["layoutViewport"]
    };
    Ok((
        layout["pageX"].as_f64().unwrap_or(0.0),
        layout["pageY"].as_f64().unwrap_or(0.0),
        layout["clientWidth"].as_f64().unwrap_or(0.0),
        layout["clientHeight"].as_f64().unwrap_or(0.0),
    ))
}

/// Center of the first content quad that intersects the viewport (spec 9.2
/// step 5).
pub fn point(run: &Run<'_>, el: &Element) -> Result<Point, Fail> {
    let quads = run
        .send(
            &el.node.frame.session,
            Method::DomGetContentQuads,
            json!({ "backendNodeId": el.node.backend }),
        )
        .map_err(|fail| match fail.protocol_message() {
            Some(_) => Fail::reason(Reason::NotVisible),
            None => fail,
        })?;
    let (dx, dy) = frame_offset(run, &el.node.frame)?;
    let (_, _, width, height) = viewport(run)?;
    let quads: Vec<Vec<f64>> = serde_json::from_value(quads["quads"].clone()).unwrap_or_default();
    center_in_viewport(&quads, dx, dy, width, height)
        .map(|(x, y)| Point {
            x,
            y,
            local_x: x - dx,
            local_y: y - dy,
        })
        .ok_or_else(|| {
            Fail::detail(
                Reason::NotVisible,
                "Element is outside the visible part of the page even after scrolling.",
            )
        })
}

/// Pure part of [`point`]: shift each quad by the frame offset, clip its
/// bounding box to the viewport, return the center of the first one with an
/// area of at least one pixel.
pub fn center_in_viewport(
    quads: &[Vec<f64>],
    dx: f64,
    dy: f64,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    for quad in quads {
        if quad.len() < 8 {
            continue;
        }
        let xs = [quad[0], quad[2], quad[4], quad[6]].map(|v| v + dx);
        let ys = [quad[1], quad[3], quad[5], quad[7]].map(|v| v + dy);
        let left = xs.iter().copied().fold(f64::INFINITY, f64::min).max(0.0);
        let right = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let top = ys.iter().copied().fold(f64::INFINITY, f64::min).max(0.0);
        let bottom = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let right = if width > 0.0 { right.min(width) } else { right };
        let bottom = if height > 0.0 {
            bottom.min(height)
        } else {
            bottom
        };
        if (right - left) * (bottom - top) >= 1.0 && right > left && bottom > top {
            return Some(((left + right) / 2.0, (top + bottom) / 2.0));
        }
    }
    None
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Hit {
    status: String,
    by: String,
}

/// Spec 9.2 step 6: the point must land on the element or inside it.
fn hit_test(run: &Run<'_>, el: &Element, point: Point) -> Result<(), Fail> {
    let mut last = String::new();
    for attempt in 0..HIT_ATTEMPTS {
        if attempt > 0 {
            run.pause(HIT_GAP);
        }
        let value = call(
            run,
            el,
            HITTEST_JS,
            &[arg(json!(point.local_x)), arg(json!(point.local_y))],
        )?;
        let mut hit: Hit = serde_json::from_value(value).unwrap_or_default();
        if hit.status == "unknown" {
            hit = closed_root_hit(run, el, point);
        }
        match hit.status.as_str() {
            "obscured" => last = hit.by,
            _ => return Ok(()),
        }
    }
    Err(Fail::detail(
        Reason::Obscured,
        if last.is_empty() {
            Reason::Obscured.message().to_string()
        } else {
            format!("Element is covered by {last}; dismiss it or scroll.")
        },
    ))
}

/// Fallback for closed shadow roots (spec 9.2 step 6): ask the browser which
/// node is at the point and let the page script check composed containment.
/// Any failure here counts as a good hit rather than blocking the action.
fn closed_root_hit(run: &Run<'_>, el: &Element, point: Point) -> Hit {
    let ok = Hit {
        status: "ok".into(),
        by: String::new(),
    };
    let Ok(found) = run.send(
        &el.node.frame.session,
        Method::DomGetNodeForLocation,
        json!({
            "x": point.local_x.round() as i64,
            "y": point.local_y.round() as i64,
            "includeUserAgentShadowDOM": true,
            // `true` would hit-test `pointer-events: none` elements, which a
            // real click passes through (the visible cursor among them).
            "ignorePointerEventsNone": false,
        }),
    ) else {
        return ok;
    };
    let Some(backend) = found["backendNodeId"].as_i64() else {
        return ok;
    };
    if backend == el.node.backend {
        return ok;
    }
    let Ok(other) = resolve_backend(run, &el.node.frame, backend) else {
        return ok;
    };
    let args = [
        arg(json!(point.local_x)),
        arg(json!(point.local_y)),
        json!({ "objectId": other }),
    ];
    call(run, el, HITTEST_JS, &args)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_uses_the_first_quad_inside_the_viewport_and_frame_offsets() {
        let quad = vec![10.0, 10.0, 110.0, 10.0, 110.0, 40.0, 10.0, 40.0];
        assert_eq!(
            center_in_viewport(std::slice::from_ref(&quad), 0.0, 0.0, 800.0, 600.0),
            Some((60.0, 25.0))
        );
        // Shifted by an OOPIF offset.
        assert_eq!(
            center_in_viewport(std::slice::from_ref(&quad), 100.0, 200.0, 800.0, 600.0),
            Some((160.0, 225.0))
        );
        // Partly outside: the center of the visible part.
        let wide = vec![-100.0, 0.0, 100.0, 0.0, 100.0, 20.0, -100.0, 20.0];
        assert_eq!(
            center_in_viewport(&[wide], 0.0, 0.0, 800.0, 600.0),
            Some((50.0, 10.0))
        );
        // Entirely below the viewport, then a zero-size quad, then a good one.
        let below = vec![0.0, 700.0, 10.0, 700.0, 10.0, 710.0, 0.0, 710.0];
        let empty = vec![5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
        assert_eq!(
            center_in_viewport(&[below.clone(), empty, quad], 0.0, 0.0, 800.0, 600.0),
            Some((60.0, 25.0))
        );
        assert_eq!(center_in_viewport(&[below], 0.0, 0.0, 800.0, 600.0), None);
    }

    #[test]
    fn injected_scripts_are_named_functions() {
        for (script, name) in [
            (ACTIONABILITY_JS, "function monocodeActionability("),
            (HITTEST_JS, "function monocodeHitTest("),
            (EDIT_JS, "function monocodeSelectAll("),
            (SELECT_JS, "function monocodeSelect("),
            (PAGE_JS, "function monocodePage("),
            (READ_JS, "function monocodeRead("),
        ] {
            assert!(script.contains(name), "{name}");
            // Never anything that evaluates strings as code.
            assert!(!script.contains("eval("), "{name}");
            assert!(!script.contains("new Function"), "{name}");
        }
    }
}
