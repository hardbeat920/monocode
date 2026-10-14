//! The agent's visible cursor: MonoCode's own pointer, drawn in the tabs the
//! agent controls so the user can follow where it moves, clicks and types.
//!
//! `injected/cursor.js` draws it in the main frame's `__monocode` isolated
//! world (an aria-hidden, `pointer-events: none` host with a closed shadow
//! root; see the script for what the page can and cannot see). Everything
//! here is best effort: a failed cursor call never fails or delays an action
//! beyond the glide itself, and nothing is sent that could change the page's
//! behavior, so these calls never make a later failure `unknown`.
//!
//! Before a click the cursor glides to the point and the action waits for
//! the glide (the page reports 0 ms for a hidden tab or reduced motion, so
//! background work is not slowed down); the caller then re-checks the point,
//! because the page had that long to move.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::super::policy::{Lease, PolicyState};
use super::element::{self, Point};
use super::refs::RefKey;
use super::Run;

pub const CURSOR_JS: &str = include_str!("../injected/cursor.js");

/// Longest glide the action waits for, whatever the page reports.
const MAX_GLIDE: Duration = Duration::from_millis(250);

/// Last cursor position per tab, so the cursor continues from there on the
/// next document instead of appearing out of nowhere.
static LAST: LazyLock<Mutex<HashMap<RefKey, (f64, f64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn last() -> std::sync::MutexGuard<'static, HashMap<RefKey, (f64, f64)>> {
    LAST.lock().unwrap_or_else(|e| e.into_inner())
}

/// Drop positions of this host's tabs that are gone.
pub fn prune(host: usize, state: &PolicyState) {
    last().retain(|key, _| key.host != host || state.lease(&key.tab).is_some_and(Lease::is_live));
}

fn call(run: &Run<'_>, args: Value) -> Option<Value> {
    let frame = element::main_frame(run).ok()?;
    let args: Vec<Value> = args.as_array()?.iter().cloned().map(element::arg).collect();
    element::call_frame(run, &frame, CURSOR_JS, &args, false).ok()
}

/// Move the cursor to `point` and wait for the glide. `true` when the
/// cursor actually glided, so the caller knows time passed.
pub fn glide(run: &Run<'_>, point: Point) -> bool {
    let from = last().get(&run.key()).copied();
    let (from_x, from_y) = match from {
        Some((x, y)) => (json!(x), json!(y)),
        None => (Value::Null, Value::Null),
    };
    let moved = call(
        run,
        json!(["move", point.x, point.y, from_x, from_y, false]),
    );
    if moved.is_none() {
        return false;
    }
    last().insert(run.key(), (point.x, point.y));
    let ms = moved.as_ref().and_then(|v| v["ms"].as_u64()).unwrap_or(0);
    if ms == 0 {
        return false;
    }
    run.pause(Duration::from_millis(ms).min(MAX_GLIDE));
    true
}

/// Put the cursor at `point` at once (the target moved during the glide).
pub fn jump(run: &Run<'_>, point: Point) {
    if call(run, json!(["move", point.x, point.y, null, null, true])).is_some() {
        last().insert(run.key(), (point.x, point.y));
    }
}

/// Show the press at the cursor's tip, right before the mouse goes down.
pub fn click(run: &Run<'_>) {
    call(run, json!(["click"]));
}

/// Hide the cursor for a screenshot, so the agent sees the page alone.
/// `true` when there was a cursor to hide (then [`reveal`] it afterwards).
/// Costs nothing on tabs where the cursor never appeared.
pub fn conceal(run: &Run<'_>) -> bool {
    if !last().contains_key(&run.key()) {
        return false;
    }
    call(run, json!(["conceal"])).is_some_and(|v| v["ok"] == json!(true))
}

pub fn reveal(run: &Run<'_>) {
    call(run, json!(["reveal"]));
}

#[cfg(test)]
mod tests {
    use super::CURSOR_JS;

    #[test]
    fn the_script_keeps_its_hooks_and_stays_out_of_the_page() {
        assert!(CURSOR_JS.contains("function monocodeCursor("));
        // `targets::release_marker` calls this when the user takes a tab back.
        assert!(CURSOR_JS.contains("globalThis.__monocodeCursorRemove = "));
        assert!(CURSOR_JS.contains(r#"setAttribute("aria-hidden", "true")"#));
        assert!(CURSOR_JS.contains(r#""pointer-events": "none""#));
        assert!(CURSOR_JS.contains(r#"attachShadow({ mode: "closed" })"#));
        // Nothing that a page's CSP or Trusted Types would block.
        for banned in ["innerHTML", "eval(", "<style", "insertRule"] {
            assert!(!CURSOR_JS.contains(banned), "{banned}");
        }
    }
}
