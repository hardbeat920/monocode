//! Mouse and keyboard actions (spec 9.2 step 7, 9.3, 9.4 `scroll`):
//! `click`, `hover`, `type`, `press`, `scroll`, and the fixed key table.
//!
//! `Input.*` always goes to the tab's main session with coordinates in the
//! tab's viewport (spec 5); the element pipeline in [`super::element`] has
//! already turned the ref into such a point.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::super::link::Method;
use super::super::protocol::{
    ActionResult, ClickParams, HoverParams, Modifier, MouseButton, PressParams, Reason,
    ScrollDirection, ScrollParams, TypeParams,
};
use super::cursor;
use super::element::{self, Element, Needs, EDIT_JS, PAGE_JS};
use super::observe::{self, Watch};
use super::{describe, Fail, Run};

/// `Input.insertText` chunk size (spec 9.3).
const TYPE_CHUNK: usize = 4000;
/// Default wheel distance in pixels (spec 9.4).
const SCROLL_AMOUNT: u32 = 600;
/// How long the page gets to react to a wheel event before the fallback.
const WHEEL_SETTLE: Duration = Duration::from_millis(150);

fn done(run: &Run<'_>, what: String, observed: super::super::protocol::Observed) -> ActionResult {
    let summary = observe::summary(&observed);
    let message = if summary.is_empty() {
        what
    } else {
        format!("{what} {summary}")
    };
    ActionResult::done(run.id(), message).with_observed(observed)
}

fn label(el: &Element, raw: &str) -> String {
    describe(raw.trim(), &el.node.role, &el.node.name)
}

fn modifier_bits(modifiers: &[Modifier]) -> u32 {
    modifiers.iter().fold(0, |bits, m| bits | m.bit())
}

/// `click` (spec 9.2): the full pipeline, then moved, pressed, released.
pub fn click(run: &Run<'_>, p: ClickParams) -> Result<ActionResult, Fail> {
    let count = p.count.unwrap_or(1);
    if !(1..=2).contains(&count) {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "count must be 1 or 2.",
        ));
    }
    let el = element::locate(run, &p.element)?;
    element::prepare(
        run,
        &el,
        Needs {
            visible: true,
            enabled: true,
            scroll: true,
            ..Needs::default()
        },
    )?;
    // Stability (spec 9.2 step 5) is measured across the cursor's glide
    // to the element's box, instead of in a pause of its own; the input
    // point and hit test come after both.
    let measured = Instant::now();
    let first = element::rect(run, &el)?;
    let aim = match first {
        Some(rect) => {
            let (dx, dy) = element::frame_offset(run, &el.node.frame)?;
            let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let aim = element::Point {
                x: x + dx,
                y: y + dy,
                local_x: x,
                local_y: y,
            };
            Some((aim, cursor::start_glide(run, aim)))
        }
        None => None,
    };
    run.pause(element::stable_after(measured).saturating_duration_since(Instant::now()));
    element::wait_stable_since(run, &el, first)?;
    if let Some((_, Some(glided))) = aim {
        run.pause(glided.saturating_duration_since(Instant::now()));
    }
    let point = element::recheck(run, &el, true)?;
    if aim.is_some_and(|(aim, _)| (aim.x - point.x).abs() >= 1.0 || (aim.y - point.y).abs() >= 1.0)
    {
        cursor::jump(run, point);
    }
    let modifiers = modifier_bits(p.modifiers.as_deref().unwrap_or(&[]));
    let (button, buttons) = match p.button.unwrap_or_default() {
        MouseButton::Left => ("left", 1),
        MouseButton::Right => ("right", 2),
        MouseButton::Middle => ("middle", 4),
    };
    let mut watch = Watch::start(run);
    watch.count_mutations(run);
    cursor::click(run);
    let mut events = vec![json!({
        "type": "mouseMoved", "x": point.x, "y": point.y, "modifiers": modifiers,
    })];
    for click_count in 1..=count {
        events.push(json!({
            "type": "mousePressed", "x": point.x, "y": point.y, "modifiers": modifiers,
            "button": button, "buttons": buttons, "clickCount": click_count,
        }));
        events.push(json!({
            "type": "mouseReleased", "x": point.x, "y": point.y, "modifiers": modifiers,
            "button": button, "buttons": 0, "clickCount": click_count,
        }));
    }
    for event in events {
        if watch
            .input(run, run.main(), Method::InputDispatchMouseEvent, event)?
            .is_none()
        {
            break;
        }
    }
    let observed = watch.finish(run, observe::WINDOW)?;
    let verb = if count == 2 {
        "Double-clicked"
    } else {
        "Clicked"
    };
    Ok(done(
        run,
        format!("{verb} {}.", label(&el, &p.element)),
        observed,
    ))
}

/// `hover`: attached, visible, stable, then one `mouseMoved` (spec 9.1).
pub fn hover(run: &Run<'_>, p: HoverParams) -> Result<ActionResult, Fail> {
    let el = element::locate(run, &p.element)?;
    let prepared = element::prepare(
        run,
        &el,
        Needs {
            visible: true,
            scroll: true,
            stable: true,
            point: true,
            ..Needs::default()
        },
    )?;
    let mut point = prepared
        .point
        .ok_or_else(|| Fail::reason(Reason::NotVisible))?;
    if cursor::glide(run, point) {
        let now = element::recheck(run, &el, false)?;
        if now != point {
            cursor::jump(run, now);
            point = now;
        }
    }
    let mut watch = Watch::start(run);
    watch.input(
        run,
        run.main(),
        Method::InputDispatchMouseEvent,
        json!({ "type": "mouseMoved", "x": point.x, "y": point.y, "modifiers": 0 }),
    )?;
    let observed = watch.finish(run, observe::SHORT_WINDOW)?;
    Ok(done(
        run,
        format!("Moved the mouse over {}.", label(&el, &p.element)),
        observed,
    ))
}

/// Split text into chunks of at most `size` characters.
pub fn chunks(text: &str, size: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut count = 0;
    for (index, _) in text.char_indices() {
        if count == size {
            out.push(&text[start..index]);
            start = index;
            count = 0;
        }
        count += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// `type` (spec 9.3): focus, optionally select everything, insert the text
/// in chunks, optionally press Enter. The text itself never appears in the
/// result (the audit keeps only its length and hash).
pub fn type_text(run: &Run<'_>, p: TypeParams) -> Result<ActionResult, Fail> {
    let el = element::locate(run, &p.element)?;
    element::prepare(
        run,
        &el,
        Needs {
            visible: true,
            enabled: true,
            editable: true,
            scroll: true,
            ..Needs::default()
        },
    )?;
    // Show where the text goes; no hit test, `type` never needed one.
    if let Ok(point) = element::point(run, &el) {
        cursor::glide(run, point);
    }
    let clear = p.clear.unwrap_or(false);
    let submit = p.submit.unwrap_or(false);
    let mut watch = Watch::start(run);
    watch.count_mutations(run);
    let frame_session = el.node.frame.session.clone();
    watch.input(
        run,
        &frame_session,
        Method::DomFocus,
        json!({ "backendNodeId": el.node.backend }),
    )?;
    if clear && watch.dialog().is_none() {
        let selected = watch.input(
            run,
            &frame_session,
            Method::RuntimeCallFunctionOn,
            element::element_call(&el, EDIT_JS, &[]),
        )?;
        if let Some(selected) = selected {
            let value = element::call_value(selected)?;
            if value["ok"] != json!(true) {
                return Err(Fail::reason(Reason::NotEditable));
            }
            if p.text.is_empty() {
                send_key(run, &mut watch, &parse_chord("Delete").map_err(invalid)?)?;
            }
        }
    }
    for chunk in chunks(&p.text, TYPE_CHUNK) {
        if watch
            .input(
                run,
                run.main(),
                Method::InputInsertText,
                json!({ "text": chunk }),
            )?
            .is_none()
        {
            break;
        }
    }
    if submit {
        send_key(run, &mut watch, &parse_chord("Enter").map_err(invalid)?)?;
    }
    let observed = watch.finish(run, observe::WINDOW)?;
    let typed = p.text.chars().count();
    let mut what = format!(
        "Typed {typed} character{} into {}",
        if typed == 1 { "" } else { "s" },
        label(&el, &p.element)
    );
    if clear {
        what.push_str(", replacing its content");
    }
    if submit {
        what.push_str(", then pressed Enter");
    }
    what.push('.');
    Ok(done(run, what, observed))
}

/// `press` (spec 9.3): optionally focus a ref, then keyDown/keyUp.
pub fn press(run: &Run<'_>, p: PressParams) -> Result<ActionResult, Fail> {
    let chord = parse_chord(&p.key).map_err(invalid)?;
    let target = match p.element.as_deref() {
        Some(raw) => {
            let el = element::locate(run, raw)?;
            let facts = element::facts(run, &el)?;
            if !facts.connected {
                return Err(Fail::reason(Reason::Detached));
            }
            Some((el, raw))
        }
        None => None,
    };
    let mut watch = Watch::start(run);
    watch.count_mutations(run);
    if let Some((el, _)) = &target {
        watch.input(
            run,
            &el.node.frame.session.clone(),
            Method::DomFocus,
            json!({ "backendNodeId": el.node.backend }),
        )?;
    }
    send_key(run, &mut watch, &chord)?;
    let observed = watch.finish(run, observe::WINDOW)?;
    let what = match &target {
        Some((el, raw)) => format!("Pressed {} on {}.", p.key.trim(), label(el, raw)),
        None => format!("Pressed {}.", p.key.trim()),
    };
    Ok(done(run, what, observed))
}

fn invalid(message: String) -> Fail {
    Fail::detail(Reason::InvalidRequest, message)
}

fn send_key(run: &Run<'_>, watch: &mut Watch, chord: &Chord) -> Result<(), Fail> {
    let key = &chord.key;
    let typing = chord.modifiers
        & (Modifier::Alt.bit() | Modifier::Control.bit() | Modifier::Meta.bit())
        == 0;
    let text = key.text.as_deref().filter(|_| typing);
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "modifiers": chord.modifiers,
        "windowsVirtualKeyCode": key.vk,
        "nativeVirtualKeyCode": key.vk,
        "code": key.code,
        "key": key.key,
        "location": key.location,
        "autoRepeat": false,
        "isKeypad": false,
    });
    if let (Some(text), Some(object)) = (text, down.as_object_mut()) {
        object.insert("text".into(), Value::String(text.to_string()));
        object.insert("unmodifiedText".into(), Value::String(text.to_string()));
    }
    let up = json!({
        "type": "keyUp",
        "modifiers": chord.modifiers,
        "windowsVirtualKeyCode": key.vk,
        "nativeVirtualKeyCode": key.vk,
        "code": key.code,
        "key": key.key,
        "location": key.location,
    });
    if watch
        .input(run, run.main(), Method::InputDispatchKeyEvent, down)?
        .is_some()
    {
        watch.input(run, run.main(), Method::InputDispatchKeyEvent, up)?;
    }
    Ok(())
}

/// `scroll` (spec 9.4): with only a ref, bring it into view; with a
/// direction, a mouse wheel over the ref or the viewport center, falling
/// back to `window.scrollBy` in the isolated world when the page ignored it.
pub fn scroll(run: &Run<'_>, p: ScrollParams) -> Result<ActionResult, Fail> {
    let el = match p.element.as_deref() {
        Some(raw) => Some(element::locate(run, raw)?),
        None => None,
    };
    if let (Some(el), None) = (&el, p.direction) {
        element::prepare(
            run,
            el,
            Needs {
                visible: true,
                scroll: true,
                ..Needs::default()
            },
        )?;
        let raw = p.element.as_deref().unwrap_or_default();
        return Ok(
            ActionResult::done(run.id(), format!("Scrolled {} into view.", label(el, raw)))
                .with_observed(run.observed_now()),
        );
    }
    let direction = p.direction.unwrap_or(ScrollDirection::Down);
    let amount = f64::from(p.amount.unwrap_or(SCROLL_AMOUNT).clamp(1, 100_000));
    let (dx, dy) = match direction {
        ScrollDirection::Down => (0.0, amount),
        ScrollDirection::Up => (0.0, -amount),
        ScrollDirection::Right => (amount, 0.0),
        ScrollDirection::Left => (-amount, 0.0),
    };
    let (x, y) = match &el {
        Some(el) => {
            let prepared = element::prepare(
                run,
                el,
                Needs {
                    visible: true,
                    scroll: true,
                    point: true,
                    ..Needs::default()
                },
            )?;
            let point = prepared
                .point
                .ok_or_else(|| Fail::reason(Reason::NotVisible))?;
            (point.x, point.y)
        }
        None => {
            let (_, _, width, height) = element::viewport(run)?;
            if width > 0.0 && height > 0.0 {
                (width / 2.0, height / 2.0)
            } else {
                (400.0, 300.0)
            }
        }
    };
    let main = element::main_frame(run)?;
    let position = |run: &Run<'_>| {
        element::call_frame(run, &main, PAGE_JS, &[element::arg(json!("scroll"))], false)
            .ok()
            .map(|v| {
                (
                    v["x"].as_f64().unwrap_or(0.0),
                    v["y"].as_f64().unwrap_or(0.0),
                )
            })
    };
    let before = position(run);
    let mut watch = Watch::start(run);
    watch.input(
        run,
        run.main(),
        Method::InputDispatchMouseEvent,
        json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy, "modifiers": 0 }),
    )?;
    let mut after = None;
    if watch.dialog().is_none() {
        run.pause(WHEEL_SETTLE);
        after = position(run);
        if el.is_none() && before.is_some() && after == before {
            let moved = element::call_frame(
                run,
                &main,
                PAGE_JS,
                &[
                    element::arg(json!("scroll")),
                    element::arg(json!({ "dx": dx, "dy": dy })),
                ],
                true,
            )?;
            after = Some((
                moved["x"].as_f64().unwrap_or(0.0),
                moved["y"].as_f64().unwrap_or(0.0),
            ));
        }
    }
    let observed = watch.finish(run, observe::SHORT_WINDOW)?;
    let dir = match direction {
        ScrollDirection::Up => "up",
        ScrollDirection::Down => "down",
        ScrollDirection::Left => "left",
        ScrollDirection::Right => "right",
    };
    let what = match (&el, p.element.as_deref()) {
        (Some(el), Some(raw)) => format!("Scrolled {dir} by {amount} px over {}.", label(el, raw)),
        _ => format!("Scrolled {dir} by {amount} px."),
    };
    let mut result = done(run, what, observed);
    if let Some((x, y)) = after {
        result = result.with_data(json!({ "x": x, "y": y }));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Key table (spec 9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub key: String,
    pub code: String,
    pub vk: i64,
    pub text: Option<String>,
    pub location: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub modifiers: u32,
    pub key: Key,
}

fn named(key: &str, code: &str, vk: i64, text: Option<&str>) -> Key {
    Key {
        key: key.into(),
        code: code.into(),
        vk,
        text: text.map(str::to_string),
        location: 0,
    }
}

/// Named keys of the fixed table, case-insensitive.
fn named_key(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" => named("Enter", "Enter", 13, Some("\r")),
        "tab" => named("Tab", "Tab", 9, None),
        "escape" | "esc" => named("Escape", "Escape", 27, None),
        "backspace" => named("Backspace", "Backspace", 8, None),
        "delete" => named("Delete", "Delete", 46, None),
        "arrowup" => named("ArrowUp", "ArrowUp", 38, None),
        "arrowdown" => named("ArrowDown", "ArrowDown", 40, None),
        "arrowleft" => named("ArrowLeft", "ArrowLeft", 37, None),
        "arrowright" => named("ArrowRight", "ArrowRight", 39, None),
        "home" => named("Home", "Home", 36, None),
        "end" => named("End", "End", 35, None),
        "pageup" => named("PageUp", "PageUp", 33, None),
        "pagedown" => named("PageDown", "PageDown", 34, None),
        "space" => named(" ", "Space", 32, Some(" ")),
        _ => {
            let n: i64 = lower.strip_prefix('f')?.parse().ok()?;
            if !(1..=12).contains(&n) {
                return None;
            }
            let name = format!("F{n}");
            return Some(named(&name, &name, 111 + n, None));
        }
    };
    Some(key)
}

/// A printable ASCII character on a US layout: `(code, vk, needs_shift)`.
fn char_key(c: char) -> Option<(String, i64, bool)> {
    if c.is_ascii_lowercase() {
        return Some((
            format!("Key{}", c.to_ascii_uppercase()),
            c.to_ascii_uppercase() as i64,
            false,
        ));
    }
    if c.is_ascii_uppercase() {
        // `Control+A` is select-all, not Control+Shift+A: letters never
        // imply Shift.
        return Some((format!("Key{c}"), c as i64, false));
    }
    if c.is_ascii_digit() {
        return Some((format!("Digit{c}"), c as i64, false));
    }
    let (code, vk, shift) = match c {
        ' ' => ("Space", 32, false),
        '-' => ("Minus", 189, false),
        '=' => ("Equal", 187, false),
        '[' => ("BracketLeft", 219, false),
        ']' => ("BracketRight", 221, false),
        '\\' => ("Backslash", 220, false),
        ';' => ("Semicolon", 186, false),
        '\'' => ("Quote", 222, false),
        ',' => ("Comma", 188, false),
        '.' => ("Period", 190, false),
        '/' => ("Slash", 191, false),
        '`' => ("Backquote", 192, false),
        '!' => ("Digit1", 49, true),
        '@' => ("Digit2", 50, true),
        '#' => ("Digit3", 51, true),
        '$' => ("Digit4", 52, true),
        '%' => ("Digit5", 53, true),
        '^' => ("Digit6", 54, true),
        '&' => ("Digit7", 55, true),
        '*' => ("Digit8", 56, true),
        '(' => ("Digit9", 57, true),
        ')' => ("Digit0", 48, true),
        '_' => ("Minus", 189, true),
        '+' => ("Equal", 187, true),
        '{' => ("BracketLeft", 219, true),
        '}' => ("BracketRight", 221, true),
        '|' => ("Backslash", 220, true),
        ':' => ("Semicolon", 186, true),
        '"' => ("Quote", 222, true),
        '<' => ("Comma", 188, true),
        '>' => ("Period", 190, true),
        '?' => ("Slash", 191, true),
        '~' => ("Backquote", 192, true),
        _ => return None,
    };
    Some((code.to_string(), vk, shift))
}

fn modifier(name: &str) -> Option<Modifier> {
    match name.to_ascii_lowercase().as_str() {
        "alt" | "option" => Some(Modifier::Alt),
        "control" | "ctrl" => Some(Modifier::Control),
        "meta" | "cmd" | "command" => Some(Modifier::Meta),
        "shift" => Some(Modifier::Shift),
        _ => None,
    }
}

/// Parse `Modifier+...+Key` (spec 9.3). Unknown keys are an error, never a
/// closest match.
pub fn parse_chord(raw: &str) -> Result<Chord, String> {
    let raw = raw.trim();
    let unknown = || {
        format!(
            "Unknown key `{raw}`. Use Enter, Tab, Escape, Backspace, Delete, ArrowUp/Down/Left/Right, Home, End, PageUp, PageDown, F1-F12, Space or one printable ASCII character, optionally prefixed by Alt+, Control+, Meta+ or Shift+."
        )
    };
    if raw.is_empty() {
        return Err(unknown());
    }
    let (prefix, key_name) = if raw == "+" {
        ("", "+")
    } else if let Some(prefix) = raw.strip_suffix("++") {
        (prefix, "+")
    } else {
        match raw.rsplit_once('+') {
            Some((prefix, key)) => (prefix, key),
            None => ("", raw),
        }
    };
    let mut modifiers = 0;
    if !prefix.is_empty() {
        for part in prefix.split('+') {
            let m = modifier(part.trim()).ok_or_else(unknown)?;
            modifiers |= m.bit();
        }
    }
    if key_name.is_empty() {
        return Err(unknown());
    }
    let shift = modifiers & Modifier::Shift.bit() != 0;
    let key = if let Some(key) = named_key(key_name) {
        key
    } else {
        let mut chars = key_name.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(unknown());
        };
        let (code, vk, needs_shift) = char_key(c).ok_or_else(unknown)?;
        let c = if shift && c.is_ascii_lowercase() {
            c.to_ascii_uppercase()
        } else {
            c
        };
        if needs_shift {
            modifiers |= Modifier::Shift.bit();
        }
        Key {
            key: c.to_string(),
            code,
            vk,
            text: Some(c.to_string()),
            location: 0,
        }
    };
    Ok(Chord { modifiers, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_from_the_fixed_table_only() {
        let enter = parse_chord("Enter").unwrap();
        assert_eq!(enter.modifiers, 0);
        assert_eq!(enter.key.vk, 13);
        assert_eq!(enter.key.text.as_deref(), Some("\r"));
        let select_all = parse_chord("Control+A").unwrap();
        assert_eq!(select_all.modifiers, 2);
        assert_eq!(select_all.key.code, "KeyA");
        assert_eq!(select_all.key.vk, 65);
        let shift_tab = parse_chord("shift+tab").unwrap();
        assert_eq!(shift_tab.modifiers, 8);
        assert_eq!(shift_tab.key.key, "Tab");
        let all = parse_chord("Ctrl+Alt+Meta+Shift+F12").unwrap();
        assert_eq!(all.modifiers, 15);
        assert_eq!(all.key.vk, 123);
        let plus = parse_chord("Control++").unwrap();
        assert_eq!(plus.key.key, "+");
        assert_eq!(plus.modifiers, 2 | 8);
        assert_eq!(parse_chord("+").unwrap().key.code, "Equal");
        assert_eq!(parse_chord("a").unwrap().key.text.as_deref(), Some("a"));
        assert_eq!(parse_chord("Shift+a").unwrap().key.key, "A");
        assert_eq!(parse_chord("Space").unwrap().key.text.as_deref(), Some(" "));
        assert_eq!(parse_chord("?").unwrap().modifiers, 8);
        for bad in [
            "",
            "Enterr",
            "Hyper+A",
            "Control+",
            "F13",
            "ab",
            "č",
            "Control+Shift",
        ] {
            assert!(parse_chord(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn text_is_chunked_on_character_boundaries() {
        assert!(chunks("", 3).is_empty());
        assert_eq!(chunks("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(chunks("ččč", 2), vec!["čč", "č"]);
        let long = "x".repeat(TYPE_CHUNK * 2 + 1);
        assert_eq!(chunks(&long, TYPE_CHUNK).len(), 3);
    }
}
