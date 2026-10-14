//! `console` and `network`: the developer signals of the agent's own tab,
//! read from the record the link keeps ([`crate::browser::diagnostics`]).

use serde_json::{json, Value};

use super::{Fail, Run};
use crate::browser::diagnostics::{self, ConsoleEntry, RequestEntry, Source, BODY_MAX};
use crate::browser::link::Method;
use crate::browser::protocol::{ActionResult, ConsoleLevel, ConsoleParams, NetworkParams};
use crate::browser::targets::Sender;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;
/// Response bodies read per `network` call.
const BODIES: usize = 5;

fn limit(requested: Option<u32>) -> usize {
    requested.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize
}

pub(crate) fn console(run: &Run<'_>, params: ConsoleParams) -> Result<ActionResult, Fail> {
    run.check_time()?;
    // Asking is consent enough: from now on this tab's console is captured
    // on every page, and what the browser kept is replayed before the reply.
    let started = diagnostics::request_runtime(run.link, Sender::Agent(run.ctx), run.main())
        .map_err(|error| Fail::Cdp(error, Method::RuntimeEnable.name()))?;
    let level = params.level.unwrap_or_default();
    let since = params.since.unwrap_or(0);
    let view = run
        .link
        .diagnostics()
        .console(run.main(), level, since, limit(params.limit));
    let errors = view
        .entries
        .iter()
        .filter(|e| e.level == diagnostics::Level::Error)
        .count();
    let mut message = match view.entries.len() {
        0 if since > 0 => "No new console messages.".to_string(),
        0 => "No console messages on this tab.".to_string(),
        1 => "1 console message".to_string(),
        n => format!("{n} console messages"),
    };
    if !view.entries.is_empty() {
        message.push_str(&match errors {
            0 => ".".to_string(),
            1 => ", 1 of them an error.".to_string(),
            n => format!(", {n} of them errors."),
        });
    }
    if started {
        message.push_str(
            " Console capture started now; earlier messages are included where the browser kept them, and capture stays on for this tab.",
        );
    }
    let mut lines = Vec::new();
    if view.omitted > 0 {
        lines.push(format!(
            "({} older matching entries left out; raise limit or pass since.)",
            view.omitted
        ));
    }
    if view.dropped > 0 {
        lines.push(format!(
            "({} oldest entries of this tab were dropped to stay within the record size.)",
            view.dropped
        ));
    }
    lines.extend(view.entries.iter().map(console_line));
    lines.push(format!(
        "next_since: {} (pass it as since to get only newer entries)",
        view.next_since
    ));
    Ok(ActionResult::done(run.id(), message)
        .with_observed(run.observed_now())
        .with_data(json!({
            "text": lines.join("\n"),
            "count": view.entries.len(),
            "errors": errors,
            "next_since": view.next_since,
            "level": level_name(level),
        })))
}

pub(crate) fn network(run: &Run<'_>, params: NetworkParams) -> Result<ActionResult, Fail> {
    run.check_time()?;
    let since = params.since.unwrap_or(0);
    let view = run
        .link
        .diagnostics()
        .network(run.main(), since, limit(params.limit));
    // The newest few failures of a local server carry their error page: the
    // agent is debugging its own server, and the body is where it says why.
    let mut bodies: Vec<Option<String>> = vec![None; view.entries.len()];
    let wanted: Vec<usize> = view
        .entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, e)| e.local && e.status.is_some())
        .map(|(index, _)| index)
        .take(BODIES)
        .collect();
    for index in wanted {
        if run.ctx.expired() {
            break;
        }
        let entry = &view.entries[index];
        if let Ok(reply) = run.send(
            run.main(),
            Method::NetworkGetResponseBody,
            json!({ "requestId": entry.request_id }),
        ) {
            bodies[index] = body_text(&reply);
        }
    }
    let message = match view.entries.len() {
        0 if since > 0 => "No new failed requests.".to_string(),
        0 => "No failed requests on this tab since MonoCode attached to it.".to_string(),
        1 => "1 failed request.".to_string(),
        n => format!("{n} failed requests."),
    };
    let mut lines = Vec::new();
    if view.omitted > 0 {
        lines.push(format!(
            "({} older failures left out; raise limit or pass since.)",
            view.omitted
        ));
    }
    if view.dropped > 0 {
        lines.push(format!(
            "({} oldest failures of this tab were dropped to stay within the record size.)",
            view.dropped
        ));
    }
    for (entry, body) in view.entries.iter().zip(&bodies) {
        lines.push(request_line(entry));
        if let Some(body) = body {
            lines.push(format!("    body: {body}"));
        }
    }
    lines.push(format!(
        "next_since: {} (pass it as since to get only newer failures)",
        view.next_since
    ));
    Ok(ActionResult::done(run.id(), message)
        .with_observed(run.observed_now())
        .with_data(json!({
            "text": lines.join("\n"),
            "count": view.entries.len(),
            "next_since": view.next_since,
        })))
}

fn level_name(level: ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Error => "error",
        ConsoleLevel::Warning => "warning",
        ConsoleLevel::All => "all",
    }
}

/// `#14 error (uncaught exception): TypeError ... [url:line:col]`, then the stack.
fn console_line(entry: &ConsoleEntry) -> String {
    let source = match entry.source {
        Source::Console => "console",
        Source::Exception => "uncaught exception",
        Source::Browser => "browser",
    };
    let mut line = format!("#{} {} ({source})", entry.seq, entry.level.as_str());
    if entry.count > 1 {
        line.push_str(&format!(" x{}", entry.count));
    }
    line.push_str(": ");
    line.push_str(&entry.text);
    if entry.stack.is_empty() {
        if let Some(location) = &entry.location {
            line.push_str(&format!(" [{location}]"));
        }
    }
    for frame in &entry.stack {
        line.push_str(&format!("\n    at {frame}"));
    }
    line
}

/// `#18 GET http://localhost:5173/api/user: 500 Internal Server Error (fetch)`.
fn request_line(entry: &RequestEntry) -> String {
    let mut outcome = Vec::new();
    if let Some(status) = entry.status {
        outcome.push(match &entry.status_text {
            Some(text) => format!("{status} {text}"),
            None => status.to_string(),
        });
    }
    if let Some(error) = &entry.error {
        outcome.push(error.clone());
    }
    format!(
        "#{} {} {}: {} ({})",
        entry.seq,
        entry.method,
        entry.url,
        outcome.join(", "),
        entry.resource
    )
}

/// The start of a text body on one line; binary bodies are skipped.
fn body_text(reply: &Value) -> Option<String> {
    if reply["base64Encoded"].as_bool() == Some(true) {
        return None;
    }
    let body = reply["body"].as_str()?.trim();
    if body.is_empty() {
        return None;
    }
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clipped: String = flat.chars().take(BODY_MAX).collect();
    if flat.chars().count() > BODY_MAX {
        clipped.push('\u{2026}');
    }
    Some(clipped)
}
