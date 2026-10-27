//! `fill`: several form fields in one action, so a form costs the agent one
//! call (and one model turn) instead of one per field. Each field goes
//! through the same pipeline as its single action: text through `type`
//! (replacing the content), options through `select`, a checkbox, radio
//! button or switch through `click` when its state differs. Fields are
//! filled in order; the first failure stops the rest, and so does a
//! navigation, a dialog or a new tab.

use serde_json::json;

use super::super::protocol::{
    ActionResult, ClickParams, FillField, FillParams, Observed, Reason, SelectParams, TabId,
    TypeParams,
};
use super::{actions, element, input, observe, Fail, Run};

/// Fields per call.
const MAX_FIELDS: usize = 50;

/// Checked state of a checkbox, radio button or ARIA switch/checkbox (for a
/// text node from a full snapshot, of its element).
const CHECKED_JS: &str = r#"function monocodeChecked() {
  const el = this.nodeType === 1 ? this : this.parentElement;
  if (!el) return { kind: 'none' };
  if (el instanceof HTMLInputElement && (el.type === 'checkbox' || el.type === 'radio')) {
    return { kind: el.type, checked: el.checked };
  }
  const aria = el.getAttribute('aria-checked') ?? el.getAttribute('aria-pressed');
  if (aria !== null) return { kind: 'aria', checked: aria === 'true' };
  return { kind: 'none' };
}"#;

enum Want<'a> {
    Text(&'a str),
    Values(&'a [String]),
    Checked(bool),
}

fn want(field: &FillField) -> Result<Want<'_>, Fail> {
    match (&field.text, &field.values, field.checked) {
        (Some(text), None, None) => Ok(Want::Text(text)),
        (None, Some(values), None) => Ok(Want::Values(values)),
        (None, None, Some(checked)) => Ok(Want::Checked(checked)),
        _ => Err(Fail::detail(
            Reason::InvalidRequest,
            format!(
                "Field {} needs exactly one of text, values or checked.",
                field.element.trim()
            ),
        )),
    }
}

/// The page went somewhere else or is blocked: the rest of the form is not
/// there any more (or not reachable).
fn interrupted(observed: &Observed) -> bool {
    observed.navigated || observed.dialog.is_some() || observed.new_tab.is_some()
}

fn list(refs: &[&str]) -> String {
    refs.join(", ")
}

pub fn fill(run: &Run<'_>, p: FillParams) -> Result<ActionResult, Fail> {
    if p.fields.is_empty() || p.fields.len() > MAX_FIELDS {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            format!("fields must hold 1 to {MAX_FIELDS} fields."),
        ));
    }
    // Every field is checked before the first one is touched.
    let wants = p.fields.iter().map(want).collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&str> = p.fields.iter().map(|f| f.element.trim()).collect();
    let mut observed = Observed::default();
    for (at, (field, want)) in p.fields.iter().zip(wants).enumerate() {
        let step = match want {
            Want::Text(text) => input::type_text(
                run,
                TypeParams {
                    tab: p.tab.clone(),
                    element: field.element.clone(),
                    text: text.to_string(),
                    clear: Some(true),
                    submit: None,
                    timeout_ms: None,
                    snapshot: Some(false),
                },
            ),
            Want::Values(values) => actions::select(
                run,
                SelectParams {
                    tab: p.tab.clone(),
                    element: field.element.clone(),
                    values: values.to_vec(),
                    timeout_ms: None,
                    snapshot: Some(false),
                },
            ),
            Want::Checked(checked) => set_checked(run, &p.tab, &field.element, checked),
        };
        match step {
            Ok(result) => observed = result.observed,
            Err(fail) => return Ok(stopped(run, fail, &refs, at)),
        }
        if interrupted(&observed) && at + 1 < refs.len() {
            let message = format!(
                "Filled {} ({}); stopped before {} because {}",
                count(at + 1),
                list(&refs[..=at]),
                list(&refs[at + 1..]),
                lower_first(&observe::summary(&observed)),
            );
            return Ok(ActionResult::done(run.id(), message)
                .with_data(json!({ "filled": &refs[..=at], "not_filled": &refs[at + 1..] }))
                .with_observed(observed));
        }
    }
    let mut what = format!("Filled {} ({})", count(refs.len()), list(&refs));
    if let Some(submit) = p.then_click.as_deref().filter(|_| !interrupted(&observed)) {
        let clicked = input::click(
            run,
            ClickParams {
                tab: p.tab.clone(),
                element: submit.to_string(),
                button: None,
                count: None,
                modifiers: None,
                timeout_ms: None,
                snapshot: Some(false),
            },
        );
        match clicked {
            Ok(result) => {
                observed = result.observed;
                what.push_str(&format!(", then clicked {}", submit.trim()));
            }
            Err(fail) => {
                let mut result = run.fail(fail, true);
                result.message = format!(
                    "Filled {} ({}), but clicking {} failed: {}",
                    count(refs.len()),
                    list(&refs),
                    submit.trim(),
                    result.message
                );
                return Ok(result.with_data(json!({ "filled": refs })));
            }
        }
    }
    what.push('.');
    let summary = observe::summary(&observed);
    let message = if summary.is_empty() {
        what
    } else {
        format!("{what} {summary}")
    };
    Ok(ActionResult::done(run.id(), message)
        .with_data(json!({ "filled": refs }))
        .with_observed(observed))
}

fn count(n: usize) -> String {
    format!("{n} field{}", if n == 1 { "" } else { "s" })
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The result when field `at` failed: its reason, plus what was filled.
fn stopped(run: &Run<'_>, fail: Fail, refs: &[&str], at: usize) -> ActionResult {
    let mut result = run.fail(fail, true);
    let filled = &refs[..at];
    let rest = &refs[at + 1..];
    let mut message = String::new();
    if !filled.is_empty() {
        message.push_str(&format!("Filled {} ({}). ", count(at), list(filled)));
    }
    message.push_str(&format!("Field {}: {}", refs[at], result.message));
    if !rest.is_empty() {
        message.push_str(&format!(" Not filled: {}.", list(rest)));
    }
    result.message = message;
    result.with_data(json!({ "filled": filled, "failed": refs[at], "not_filled": rest }))
}

/// Click a checkbox, radio button or switch when its state differs.
fn set_checked(run: &Run<'_>, tab: &TabId, raw: &str, want: bool) -> Result<ActionResult, Fail> {
    let el = element::locate(run, raw)?;
    let state = element::call(run, &el, CHECKED_JS, &[])?;
    let Some(checked) = state["checked"].as_bool() else {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "It is not a checkbox, radio button or switch; use text or values for it.",
        ));
    };
    if checked == want {
        return Ok(ActionResult::done(run.id(), "").with_observed(Observed::default()));
    }
    if !want && state["kind"] == "radio" {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "A radio button cannot be unchecked; check another one of its group.",
        ));
    }
    let result = input::click(
        run,
        ClickParams {
            tab: tab.clone(),
            element: raw.to_string(),
            button: None,
            count: None,
            modifiers: None,
            timeout_ms: None,
            snapshot: Some(false),
        },
    )?;
    if interrupted(&result.observed) {
        return Ok(result);
    }
    // A custom control may ignore clicks on the element the ref names.
    match element::call(run, &el, CHECKED_JS, &[]) {
        Ok(state) if state["checked"].as_bool() == Some(!want) => Err(Fail::detail(
            Reason::InvalidRequest,
            format!(
                "The click did not {} it; click the control the page shows instead.",
                if want { "check" } else { "uncheck" }
            ),
        )),
        _ => Ok(result),
    }
}
