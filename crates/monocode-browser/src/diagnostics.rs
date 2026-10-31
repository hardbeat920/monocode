//! Developer signals of agent tabs: what the page logged to the console,
//! uncaught exceptions, the browser's own warnings (CSP, mixed content), and
//! requests that failed (HTTP 4xx/5xx, network and CORS errors).
//!
//! Only page sessions of our own tabs get `Network.enable` and `Log.enable`
//! ([`super::targets::setup_session`]), so nothing here ever comes from a tab
//! the agent does not hold. `Runtime.enable`, which console messages and
//! exceptions need, is visible to some bot-protection scripts, so it runs
//! only where it cannot cost the agent a page: on local development servers
//! ([`is_local_url`]), and on any page once the agent asked for the console of
//! that tab ([`sync_runtime`]). Network failures and the `Log` domain are
//! invisible to the page and always recorded.
//!
//! The link thread records every `Runtime.*`, `Network.*` and `Log.*` event
//! here before it routes the reply to any later command, and does not fan
//! them out to subscribers ([`Diagnostics::record`]). Nothing is stored
//! beyond the bounded per-tab rings below, nothing reaches the audit, and
//! request or response headers are never kept.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use serde_json::{json, Value};

use super::link::{CdpError, CdpEvent, CdpSessionId, Command, Link, Method};
use super::policy::{audit_url, now_ms};
use super::protocol::{ConsoleLevel, PageSignals, ISOLATED_WORLD};
use super::targets::{send, Sender};

/// Console entries kept per tab; older ones are dropped and counted.
const CONSOLE_CAP: usize = 300;
/// Failed requests kept per tab.
const REQUEST_CAP: usize = 200;
/// Requests in flight remembered for their method and URL.
const INFLIGHT_CAP: usize = 2_000;
/// Page sessions with a record; the oldest is forgotten beyond this.
const SESSION_CAP: usize = 64;
const TEXT_MAX: usize = 1_000;
const URL_MAX: usize = 300;
const STACK_FRAMES: usize = 5;
/// Response bodies are read only from local servers, and only this much.
pub const BODY_MAX: usize = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warning,
    Info,
    Debug,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    fn passes(self, filter: ConsoleLevel) -> bool {
        match filter {
            ConsoleLevel::Error => self == Level::Error,
            ConsoleLevel::Warning => matches!(self, Level::Error | Level::Warning),
            ConsoleLevel::All => true,
        }
    }
}

/// Where a console entry came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// `console.*` in the page.
    Console,
    /// An uncaught exception or unhandled promise rejection.
    Exception,
    /// The browser itself: CSP, mixed content, deprecations, interventions.
    Browser,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConsoleEntry {
    pub seq: u64,
    pub level: Level,
    pub source: Source,
    pub text: String,
    /// `url:line:column` of the first stack frame, 1-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Up to five frames, for errors only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<String>,
    /// The same message repeated back to back.
    pub count: u32,
    pub at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RequestEntry {
    pub seq: u64,
    pub method: String,
    /// Without query and fragment, except on a local server.
    pub url: String,
    /// CDP resource type in lower case: `fetch`, `xhr`, `document`, `script`...
    pub resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// `net::ERR_*`, blocked and CORS reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub at_ms: i64,
    /// For `Network.getResponseBody`, only while the browser keeps the body.
    #[serde(skip)]
    pub request_id: String,
    /// Served from this machine; only then is a body read.
    #[serde(skip)]
    pub local: bool,
}

struct Inflight {
    method: String,
    url: String,
    resource: String,
    /// `seq` of the entry a 4xx/5xx response already produced.
    reported: Option<u64>,
}

#[derive(Default)]
struct TabLog {
    console: VecDeque<ConsoleEntry>,
    requests: VecDeque<RequestEntry>,
    inflight: HashMap<String, Inflight>,
    /// Execution contexts of our own isolated world: never reported.
    own_contexts: HashSet<i64>,
    console_dropped: u64,
    requests_dropped: u64,
    errors: u64,
    errors_reported: u64,
    failures: u64,
    failures_reported: u64,
    runtime_on: bool,
    runtime_requested: bool,
    born: u64,
}

/// What [`Diagnostics::console`] returns.
#[derive(Clone, Debug, Default)]
pub struct ConsoleView {
    pub entries: Vec<ConsoleEntry>,
    /// Matching entries left out by `limit` (the oldest ones).
    pub omitted: usize,
    /// Entries the ring dropped since the tab was attached.
    pub dropped: u64,
    pub next_since: u64,
}

/// What [`Diagnostics::network`] returns.
#[derive(Clone, Debug, Default)]
pub struct NetworkView {
    pub entries: Vec<RequestEntry>,
    pub omitted: usize,
    pub dropped: u64,
    pub next_since: u64,
}

/// The record of every page session on one link.
#[derive(Default)]
pub struct Diagnostics {
    tabs: HashMap<CdpSessionId, TabLog>,
    next_seq: u64,
    born: u64,
}

impl Diagnostics {
    /// Take one event from the link thread. Returns whether it was a
    /// diagnostics event, which the link then keeps from subscribers.
    pub fn record(&mut self, event: &CdpEvent) -> bool {
        let method = event.method.as_str();
        if method == "Target.detachedFromTarget" {
            if let Some(session) = event.params["sessionId"].as_str() {
                self.tabs.remove(&CdpSessionId(session.to_string()));
            }
            return false;
        }
        let ours = method.starts_with("Runtime.")
            || method.starts_with("Network.")
            || method == "Log.entryAdded";
        if !ours {
            return false;
        }
        let Some(session) = &event.session else {
            return true;
        };
        let params = &event.params;
        match method {
            "Runtime.consoleAPICalled" => self.on_console_api(session, params),
            "Runtime.exceptionThrown" => self.on_exception(session, params),
            "Runtime.executionContextCreated" => {
                let context = &params["context"];
                if context["name"].as_str() == Some(ISOLATED_WORLD) {
                    if let Some(id) = context["id"].as_i64() {
                        self.tab_mut(session).own_contexts.insert(id);
                    }
                }
            }
            "Runtime.executionContextDestroyed" => {
                if let Some(id) = params["executionContextId"].as_i64() {
                    if let Some(log) = self.tabs.get_mut(session) {
                        log.own_contexts.remove(&id);
                    }
                }
            }
            "Runtime.executionContextsCleared" => {
                if let Some(log) = self.tabs.get_mut(session) {
                    log.own_contexts.clear();
                }
            }
            "Log.entryAdded" => self.on_log(session, &params["entry"]),
            "Network.requestWillBeSent" => self.on_request(session, params),
            "Network.responseReceived" => self.on_response(session, params),
            "Network.loadingFailed" => self.on_loading_failed(session, params),
            "Network.loadingFinished" => {
                if let (Some(log), Some(id)) =
                    (self.tabs.get_mut(session), params["requestId"].as_str())
                {
                    log.inflight.remove(id);
                }
            }
            _ => {}
        }
        true
    }

    fn tab_mut(&mut self, session: &CdpSessionId) -> &mut TabLog {
        if !self.tabs.contains_key(session) && self.tabs.len() >= SESSION_CAP {
            if let Some(oldest) = self
                .tabs
                .iter()
                .min_by_key(|(_, log)| log.born)
                .map(|(id, _)| id.clone())
            {
                self.tabs.remove(&oldest);
            }
        }
        self.born += 1;
        let born = self.born;
        self.tabs.entry(session.clone()).or_insert_with(|| TabLog {
            born,
            ..TabLog::default()
        })
    }

    fn seq(&mut self) -> u64 {
        self.next_seq += 1;
        self.next_seq
    }

    fn push_console(&mut self, session: &CdpSessionId, mut entry: ConsoleEntry) {
        let seq = self.seq();
        let log = self.tab_mut(session);
        if entry.level == Level::Error {
            log.errors += 1;
        }
        if let Some(last) = log.console.back_mut() {
            if last.level == entry.level
                && last.source == entry.source
                && last.text == entry.text
                && last.location == entry.location
            {
                last.count = last.count.saturating_add(1);
                last.at_ms = entry.at_ms;
                // A repeat is news to an agent reading with `since`.
                last.seq = seq;
                return;
            }
        }
        entry.seq = seq;
        if log.console.len() >= CONSOLE_CAP {
            log.console.pop_front();
            log.console_dropped += 1;
        }
        log.console.push_back(entry);
    }

    fn own_context(&self, session: &CdpSessionId, context: Option<i64>) -> bool {
        context.is_some_and(|id| {
            self.tabs
                .get(session)
                .is_some_and(|log| log.own_contexts.contains(&id))
        })
    }

    fn on_console_api(&mut self, session: &CdpSessionId, params: &Value) {
        if self.own_context(session, params["executionContextId"].as_i64()) {
            return;
        }
        let kind = params["type"].as_str().unwrap_or("log");
        let level = match kind {
            "error" | "assert" => Level::Error,
            "warning" => Level::Warning,
            "debug" => Level::Debug,
            "endGroup" => return,
            _ => Level::Info,
        };
        let mut text = params["args"]
            .as_array()
            .map(|args| {
                args.iter()
                    .map(remote_object_text)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        match kind {
            "assert" => text = format!("Assertion failed: {text}"),
            "clear" => text = "console.clear()".to_string(),
            _ => {}
        }
        let frames = params["stackTrace"]["callFrames"].as_array();
        let entry = ConsoleEntry {
            seq: 0,
            level,
            source: Source::Console,
            text: clip(text.trim_end(), TEXT_MAX),
            location: frames.and_then(|f| f.first()).and_then(frame_location),
            stack: if level == Level::Error {
                frames.map(|f| stack(f)).unwrap_or_default()
            } else {
                Vec::new()
            },
            count: 1,
            at_ms: now_ms(),
        };
        self.push_console(session, entry);
    }

    fn on_exception(&mut self, session: &CdpSessionId, params: &Value) {
        let details = &params["exceptionDetails"];
        if self.own_context(session, details["executionContextId"].as_i64()) {
            return;
        }
        let prefix = details["text"].as_str().unwrap_or("Uncaught");
        let exception = &details["exception"];
        let what = exception["description"]
            .as_str()
            .and_then(|d| d.lines().next())
            .map(str::to_string)
            .or_else(|| {
                let value = remote_object_text(exception);
                (!value.is_empty() && exception.is_object()).then_some(value)
            });
        let text = match what {
            Some(what) if !prefix.contains(&what) => format!("{prefix} {what}"),
            _ => prefix.to_string(),
        };
        let frames = details["stackTrace"]["callFrames"].as_array();
        let location = frames
            .and_then(|f| f.first())
            .and_then(frame_location)
            .or_else(|| {
                let url = details["url"].as_str().filter(|u| !u.is_empty())?;
                Some(format!(
                    "{}:{}:{}",
                    display_url(url),
                    details["lineNumber"].as_i64().unwrap_or(0) + 1,
                    details["columnNumber"].as_i64().unwrap_or(0) + 1
                ))
            });
        let entry = ConsoleEntry {
            seq: 0,
            level: Level::Error,
            source: Source::Exception,
            text: clip(&text, TEXT_MAX),
            location,
            stack: frames.map(|f| stack(f)).unwrap_or_default(),
            count: 1,
            at_ms: now_ms(),
        };
        self.push_console(session, entry);
    }

    fn on_log(&mut self, session: &CdpSessionId, entry: &Value) {
        let level = match entry["level"].as_str() {
            Some("error") => Level::Error,
            Some("warning") => Level::Warning,
            Some("info") => Level::Info,
            _ => return,
        };
        // "Failed to load resource": the network record has it already.
        if entry["source"].as_str() == Some("network") && entry["networkRequestId"].is_string() {
            return;
        }
        let frames = entry["stackTrace"]["callFrames"].as_array();
        let location = frames
            .and_then(|f| f.first())
            .and_then(frame_location)
            .or_else(|| {
                let url = entry["url"].as_str().filter(|u| !u.is_empty())?;
                Some(match entry["lineNumber"].as_i64() {
                    Some(line) => format!("{}:{}", display_url(url), line + 1),
                    None => display_url(url),
                })
            });
        let console = ConsoleEntry {
            seq: 0,
            level,
            source: Source::Browser,
            text: clip(entry["text"].as_str().unwrap_or("").trim_end(), TEXT_MAX),
            location,
            stack: Vec::new(),
            count: 1,
            at_ms: now_ms(),
        };
        self.push_console(session, console);
    }

    fn on_request(&mut self, session: &CdpSessionId, params: &Value) {
        let Some(id) = params["requestId"].as_str() else {
            return;
        };
        let request = &params["request"];
        let log = self.tab_mut(session);
        if log.inflight.len() >= INFLIGHT_CAP && !log.inflight.contains_key(id) {
            // A page that never finishes its requests (long polls, streams):
            // forget the lot rather than grow; their failures lose the URL.
            log.inflight.clear();
        }
        log.inflight.insert(
            id.to_string(),
            Inflight {
                method: request["method"].as_str().unwrap_or("GET").to_string(),
                url: request["url"].as_str().unwrap_or("").to_string(),
                resource: resource(params),
                reported: None,
            },
        );
    }

    fn on_response(&mut self, session: &CdpSessionId, params: &Value) {
        let response = &params["response"];
        let Some(status) = response["status"].as_u64().filter(|s| *s >= 400) else {
            return;
        };
        let Some(id) = params["requestId"].as_str() else {
            return;
        };
        let seq = self.seq();
        let log = self.tab_mut(session);
        let (method, url) = match log.inflight.get_mut(id) {
            Some(inflight) => {
                inflight.reported = Some(seq);
                (inflight.method.clone(), inflight.url.clone())
            }
            None => (
                "GET".to_string(),
                response["url"].as_str().unwrap_or("").to_string(),
            ),
        };
        let entry = RequestEntry {
            seq,
            method,
            local: is_local_url(&url),
            url: display_url(&url),
            resource: resource(params),
            status: Some(status.min(u16::MAX as u64) as u16),
            status_text: response["statusText"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            error: None,
            at_ms: now_ms(),
            request_id: id.to_string(),
        };
        log.failures += 1;
        push_request(log, entry);
    }

    fn on_loading_failed(&mut self, session: &CdpSessionId, params: &Value) {
        let Some(id) = params["requestId"].as_str() else {
            return;
        };
        let Some(log) = self.tabs.get_mut(session) else {
            return;
        };
        let Some(inflight) = log.inflight.remove(id) else {
            return;
        };
        if params["canceled"].as_bool() == Some(true) {
            return;
        }
        let mut error = params["errorText"]
            .as_str()
            .filter(|e| !e.is_empty())
            .unwrap_or("failed")
            .to_string();
        if let Some(blocked) = params["blockedReason"].as_str() {
            error.push_str(&format!(", blocked: {blocked}"));
        }
        if let Some(cors) = params["corsErrorStatus"]["corsError"].as_str() {
            error.push_str(&format!(", CORS: {cors}"));
        }
        if let Some(seq) = inflight.reported {
            if let Some(entry) = log.requests.iter_mut().find(|e| e.seq == seq) {
                entry.error = Some(error);
            }
            return;
        }
        let seq = self.seq();
        let log = self.tab_mut(session);
        let entry = RequestEntry {
            seq,
            method: inflight.method,
            local: is_local_url(&inflight.url),
            url: display_url(&inflight.url),
            resource: inflight.resource,
            status: None,
            status_text: None,
            error: Some(error),
            at_ms: now_ms(),
            request_id: id.to_string(),
        };
        log.failures += 1;
        push_request(log, entry);
    }

    // --- reads ---------------------------------------------------------------

    /// Console entries of a tab after `since`, the newest `limit` of them.
    /// Marks every console error as reported.
    pub fn console(
        &mut self,
        session: &CdpSessionId,
        level: ConsoleLevel,
        since: u64,
        limit: usize,
    ) -> ConsoleView {
        let next_since = self.next_seq.max(since);
        let Some(log) = self.tabs.get_mut(session) else {
            return ConsoleView {
                next_since,
                ..ConsoleView::default()
            };
        };
        log.errors_reported = log.errors;
        let matching: Vec<&ConsoleEntry> = log
            .console
            .iter()
            .filter(|e| e.seq > since && e.level.passes(level))
            .collect();
        let omitted = matching.len().saturating_sub(limit);
        let mut entries: Vec<ConsoleEntry> =
            matching[omitted..].iter().map(|e| (*e).clone()).collect();
        entries.sort_by_key(|e| e.seq);
        ConsoleView {
            entries,
            omitted,
            dropped: log.console_dropped,
            next_since,
        }
    }

    /// Failed requests of a tab after `since`, the newest `limit` of them.
    /// Marks every failure as reported.
    pub fn network(&mut self, session: &CdpSessionId, since: u64, limit: usize) -> NetworkView {
        let next_since = self.next_seq.max(since);
        let Some(log) = self.tabs.get_mut(session) else {
            return NetworkView {
                next_since,
                ..NetworkView::default()
            };
        };
        log.failures_reported = log.failures;
        let matching: Vec<&RequestEntry> = log.requests.iter().filter(|e| e.seq > since).collect();
        let omitted = matching.len().saturating_sub(limit);
        NetworkView {
            entries: matching[omitted..].iter().map(|e| (*e).clone()).collect(),
            omitted,
            dropped: log.requests_dropped,
            next_since,
        }
    }

    /// Ids of the tab's fetch and XHR requests still in flight.
    pub fn pending_fetches(&self, session: &CdpSessionId) -> HashSet<String> {
        self.tabs
            .get(session)
            .map(|log| {
                log.inflight
                    .iter()
                    .filter(|(_, r)| matches!(r.resource.as_str(), "xhr" | "fetch"))
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Console errors and failed requests not reported yet, and mark them
    /// reported. `None` when there are none.
    pub fn take_signals(&mut self, session: &CdpSessionId) -> Option<PageSignals> {
        let log = self.tabs.get_mut(session)?;
        let signals = PageSignals {
            console_errors: log
                .errors
                .saturating_sub(log.errors_reported)
                .min(u32::MAX as u64) as u32,
            failed_requests: log
                .failures
                .saturating_sub(log.failures_reported)
                .min(u32::MAX as u64) as u32,
        };
        log.errors_reported = log.errors;
        log.failures_reported = log.failures;
        (signals.console_errors > 0 || signals.failed_requests > 0).then_some(signals)
    }

    /// Whether console capture runs on this session.
    #[cfg(test)]
    pub fn capturing(&self, session: &CdpSessionId) -> bool {
        self.tabs.get(session).is_some_and(|log| log.runtime_on)
    }
}

fn push_request(log: &mut TabLog, entry: RequestEntry) {
    if log.requests.len() >= REQUEST_CAP {
        log.requests.pop_front();
        log.requests_dropped += 1;
    }
    log.requests.push_back(entry);
}

/// The sentence a result gets when its tab has new trouble.
pub fn signals_message(signals: PageSignals) -> String {
    let mut parts = Vec::new();
    match signals.console_errors {
        0 => {}
        1 => parts.push("1 new console error".to_string()),
        n => parts.push(format!("{n} new console errors")),
    }
    match signals.failed_requests {
        0 => {}
        1 => parts.push("1 failed request".to_string()),
        n => parts.push(format!("{n} failed requests")),
    }
    let tools = match (signals.console_errors > 0, signals.failed_requests > 0) {
        (true, true) => "console and network",
        (true, false) => "console",
        _ => "network",
    };
    format!(
        "The page has {} since the last report; read them with the {tools} action.",
        parts.join(" and ")
    )
}

// --- Runtime on and off -------------------------------------------------------

/// A local development server: `localhost`, `*.localhost`, loopback
/// addresses and `0.0.0.0`.
pub fn is_local_url(raw: &str) -> bool {
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    match parsed.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "localhost" || host.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        None => false,
    }
}

/// Turn console capture on or off for a page session that is about to show
/// (or shows) `url`: on for local servers and for tabs whose console the
/// agent asked for, off elsewhere. Idempotent; called when a tab is set up,
/// before an agent navigation, and when the main frame navigated.
pub fn sync_runtime(
    link: &Link,
    sender: Sender<'_>,
    session: &CdpSessionId,
    url: &str,
) -> Result<(), CdpError> {
    let wanted = {
        let mut diagnostics = link.diagnostics();
        let log = diagnostics.tab_mut(session);
        let wanted = log.runtime_requested || is_local_url(url);
        if wanted == log.runtime_on {
            return Ok(());
        }
        log.runtime_on = wanted;
        wanted
    };
    // The guard is released: the link thread takes it for every event, and
    // the reply to this command comes after the events it caused.
    let method = if wanted {
        Method::RuntimeEnable
    } else {
        Method::RuntimeDisable
    };
    let result = send(
        link,
        sender,
        Command::in_session(session, method, json!({})),
    );
    if result.is_err() {
        if let Some(log) = link.diagnostics().tabs.get_mut(session) {
            log.runtime_on = !wanted;
        }
    }
    result.map(|_| ())
}

/// The agent asked for this tab's console. Where capture was off (a remote
/// page), it starts and stays on for the rest of the tab's life; on a local
/// server, where it already runs, nothing changes. Returns whether capture
/// started just now; the browser replays the messages it kept, and they are
/// recorded before this returns.
pub fn request_runtime(
    link: &Link,
    sender: Sender<'_>,
    session: &CdpSessionId,
) -> Result<bool, CdpError> {
    {
        let mut diagnostics = link.diagnostics();
        let log = diagnostics.tab_mut(session);
        if log.runtime_on {
            return Ok(false);
        }
        log.runtime_requested = true;
    }
    sync_runtime(link, sender, session, "").map(|()| true)
}

// --- formatting ---------------------------------------------------------------

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max).collect();
    clipped.push('\u{2026}');
    clipped
}

/// A URL as the agent sees it in the record: complete on a local server,
/// without query and fragment elsewhere (they can carry tokens).
pub fn display_url(raw: &str) -> String {
    if raw.starts_with("data:") {
        return clip(raw, 60);
    }
    if is_local_url(raw) {
        clip(raw, URL_MAX)
    } else {
        clip(&audit_url(raw), URL_MAX)
    }
}

fn resource(params: &Value) -> String {
    params["type"]
        .as_str()
        .unwrap_or("other")
        .to_ascii_lowercase()
}

fn frame_location(frame: &Value) -> Option<String> {
    let url = frame["url"].as_str().filter(|u| !u.is_empty())?;
    Some(format!(
        "{}:{}:{}",
        display_url(url),
        frame["lineNumber"].as_i64().unwrap_or(0) + 1,
        frame["columnNumber"].as_i64().unwrap_or(0) + 1
    ))
}

fn stack(frames: &[Value]) -> Vec<String> {
    frames
        .iter()
        .take(STACK_FRAMES)
        .filter_map(|frame| {
            let location = frame_location(frame)?;
            Some(
                match frame["functionName"].as_str().filter(|f| !f.is_empty()) {
                    Some(name) => format!("{name} ({location})"),
                    None => location,
                },
            )
        })
        .collect()
}

/// One console argument as text, the way DevTools prints it on one line.
fn remote_object_text(object: &Value) -> String {
    match object["type"].as_str() {
        Some("string") => object["value"].as_str().unwrap_or("").to_string(),
        Some("undefined") => "undefined".to_string(),
        Some("number" | "boolean" | "bigint") => object["unserializableValue"]
            .as_str()
            .map(str::to_string)
            .or_else(|| object.get("value").map(Value::to_string))
            .or_else(|| object["description"].as_str().map(str::to_string))
            .unwrap_or_default(),
        Some("object") if object["subtype"].as_str() == Some("null") => "null".to_string(),
        Some("object") => match object.get("preview") {
            Some(preview) if object["subtype"].as_str() != Some("error") => render_preview(preview),
            _ => object["description"]
                .as_str()
                .or_else(|| object["className"].as_str())
                .unwrap_or("Object")
                .to_string(),
        },
        _ => object["description"]
            .as_str()
            .or_else(|| object["value"].as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn render_preview(preview: &Value) -> String {
    let array = preview["subtype"].as_str() == Some("array");
    let items: Vec<String> = preview["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|property| {
            let value = property["value"].as_str().unwrap_or("");
            let value = if property["type"].as_str() == Some("string") {
                format!("\"{value}\"")
            } else {
                value.to_string()
            };
            if array {
                value
            } else {
                format!("{}: {value}", property["name"].as_str().unwrap_or(""))
            }
        })
        .collect();
    let more = if preview["overflow"].as_bool() == Some(true) {
        ", \u{2026}"
    } else {
        ""
    };
    if array {
        format!("[{}{more}]", items.join(", "))
    } else {
        format!("{{{}{more}}}", items.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(method: &str, params: Value) -> CdpEvent {
        CdpEvent {
            method: method.to_string(),
            params,
            session: Some(CdpSessionId("S1".into())),
        }
    }

    fn s1() -> CdpSessionId {
        CdpSessionId("S1".into())
    }

    #[test]
    fn local_urls_are_development_servers_only() {
        for url in [
            "http://localhost:5173/",
            "http://LOCALHOST:3000/app",
            "https://app.localhost/",
            "http://127.0.0.1:8080/",
            "http://127.4.5.6/",
            "http://[::1]:4000/",
            "http://0.0.0.0:8000/",
        ] {
            assert!(is_local_url(url), "{url}");
        }
        for url in [
            "https://example.com/",
            "http://localhost.example.com/",
            "http://192.168.1.10/",
            "about:blank",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn console_calls_exceptions_and_browser_messages_are_recorded() {
        let mut d = Diagnostics::default();
        assert!(d.record(&event(
            "Runtime.consoleAPICalled",
            json!({
                "type": "log",
                "executionContextId": 1,
                "args": [
                    {"type": "string", "value": "user"},
                    {"type": "object", "className": "Object", "description": "Object",
                     "preview": {"type": "object", "overflow": false, "properties": [
                        {"name": "id", "type": "number", "value": "1"},
                        {"name": "name", "type": "string", "value": "Ana"}
                     ]}},
                    {"type": "number", "value": 3}
                ],
                "stackTrace": {"callFrames": [{"functionName": "load", "url": "http://localhost:5173/src/App.tsx?t=1", "lineNumber": 9, "columnNumber": 4}]}
            })
        )));
        assert!(d.record(&event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {
                "text": "Uncaught",
                "executionContextId": 1,
                "exception": {"type": "object", "subtype": "error", "description": "TypeError: x.map is not a function\n    at App (http://localhost:5173/src/App.tsx:12:5)"},
                "stackTrace": {"callFrames": [
                    {"functionName": "App", "url": "http://localhost:5173/src/App.tsx", "lineNumber": 11, "columnNumber": 4},
                    {"functionName": "", "url": "http://localhost:5173/src/main.tsx", "lineNumber": 2, "columnNumber": 0}
                ]}
            }})
        )));
        assert!(d.record(&event(
            "Log.entryAdded",
            json!({"entry": {"source": "security", "level": "error", "text": "Refused to load the script because it violates CSP.", "url": "https://example.com/page?token=secret"}})
        )));
        // Network-source log lines duplicate the network record.
        assert!(d.record(&event(
            "Log.entryAdded",
            json!({"entry": {"source": "network", "level": "error", "text": "Failed to load resource", "networkRequestId": "7"}})
        )));
        let view = d.console(&s1(), ConsoleLevel::All, 0, 50);
        assert_eq!(view.entries.len(), 3);
        let log = &view.entries[0];
        assert_eq!(log.level, Level::Info);
        assert_eq!(log.text, "user {id: 1, name: \"Ana\"} 3");
        assert_eq!(
            log.location.as_deref(),
            Some("http://localhost:5173/src/App.tsx?t=1:10:5")
        );
        assert!(log.stack.is_empty());
        let exception = &view.entries[1];
        assert_eq!(exception.source, Source::Exception);
        assert_eq!(
            exception.text,
            "Uncaught TypeError: x.map is not a function"
        );
        assert_eq!(
            exception.stack[0],
            "App (http://localhost:5173/src/App.tsx:12:5)"
        );
        assert_eq!(exception.stack[1], "http://localhost:5173/src/main.tsx:3:1");
        let csp = &view.entries[2];
        assert_eq!(csp.source, Source::Browser);
        assert_eq!(csp.location.as_deref(), Some("https://example.com/page"));
        let errors = d.console(&s1(), ConsoleLevel::Error, 0, 50);
        assert_eq!(errors.entries.len(), 2);
        let newer = d.console(&s1(), ConsoleLevel::All, exception.seq, 50);
        assert_eq!(newer.entries.len(), 1);
        let newest = d.console(&s1(), ConsoleLevel::All, 0, 1);
        assert_eq!((newest.entries.len(), newest.omitted), (1, 2));
    }

    #[test]
    fn repeats_fold_and_our_isolated_world_is_ignored() {
        let mut d = Diagnostics::default();
        d.record(&event(
            "Runtime.executionContextCreated",
            json!({"context": {"id": 9, "name": ISOLATED_WORLD}}),
        ));
        d.record(&event(
            "Runtime.consoleAPICalled",
            json!({"type": "error", "executionContextId": 9, "args": [{"type": "string", "value": "ours"}]}),
        ));
        for _ in 0..3 {
            d.record(&event(
                "Runtime.consoleAPICalled",
                json!({"type": "error", "executionContextId": 1, "args": [{"type": "string", "value": "boom"}]}),
            ));
        }
        let view = d.console(&s1(), ConsoleLevel::All, 0, 50);
        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].count, 3);
        assert_eq!(view.entries[0].text, "boom");
    }

    #[test]
    fn failed_requests_keep_method_and_hide_remote_queries() {
        let mut d = Diagnostics::default();
        let request = |id: &str, method: &str, url: &str, kind: &str| {
            event(
                "Network.requestWillBeSent",
                json!({"requestId": id, "type": kind, "request": {"method": method, "url": url}}),
            )
        };
        d.record(&request(
            "1",
            "GET",
            "http://localhost:5173/api/user?id=3",
            "Fetch",
        ));
        d.record(&event(
            "Network.responseReceived",
            json!({"requestId": "1", "type": "Fetch", "response": {"url": "http://localhost:5173/api/user?id=3", "status": 500, "statusText": "Internal Server Error"}}),
        ));
        d.record(&event("Network.loadingFinished", json!({"requestId": "1"})));
        d.record(&request(
            "2",
            "POST",
            "https://api.example.com/v1/save?token=abc",
            "XHR",
        ));
        d.record(&event(
            "Network.loadingFailed",
            json!({"requestId": "2", "type": "XHR", "errorText": "net::ERR_FAILED", "corsErrorStatus": {"corsError": "MissingAllowOriginHeader"}}),
        ));
        d.record(&request("3", "GET", "https://example.com/ok.js", "Script"));
        d.record(&event(
            "Network.responseReceived",
            json!({"requestId": "3", "type": "Script", "response": {"status": 200}}),
        ));
        d.record(&request("4", "GET", "https://example.com/aborted", "Fetch"));
        d.record(&event(
            "Network.loadingFailed",
            json!({"requestId": "4", "errorText": "net::ERR_ABORTED", "canceled": true}),
        ));
        assert_eq!(
            d.take_signals(&s1()),
            Some(PageSignals {
                console_errors: 0,
                failed_requests: 2
            })
        );
        assert_eq!(d.take_signals(&s1()), None);
        let view = d.network(&s1(), 0, 50);
        assert_eq!(view.entries.len(), 2);
        let server = &view.entries[0];
        assert_eq!(server.method, "GET");
        assert_eq!(server.url, "http://localhost:5173/api/user?id=3");
        assert_eq!(server.status, Some(500));
        assert!(server.local);
        assert_eq!(server.resource, "fetch");
        let cors = &view.entries[1];
        assert_eq!(cors.method, "POST");
        assert_eq!(cors.url, "https://api.example.com/v1/save");
        assert_eq!(
            cors.error.as_deref(),
            Some("net::ERR_FAILED, CORS: MissingAllowOriginHeader")
        );
        assert!(!cors.local);
    }

    #[test]
    fn signals_count_errors_once_and_reads_mark_them_reported() {
        let mut d = Diagnostics::default();
        let error = event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught Error: boom"}}),
        );
        d.record(&error);
        d.record(&error);
        let signals = d.take_signals(&s1()).unwrap();
        assert_eq!(signals.console_errors, 2);
        d.record(&error);
        let _ = d.console(&s1(), ConsoleLevel::All, 0, 10);
        assert_eq!(d.take_signals(&s1()), None);
        assert!(signals_message(signals).contains("2 new console errors"));
        assert!(signals_message(PageSignals {
            console_errors: 1,
            failed_requests: 1
        })
        .contains("console and network"));
    }

    #[test]
    fn non_diagnostic_events_pass_through_and_detach_forgets_the_session() {
        let mut d = Diagnostics::default();
        assert!(!d.record(&event("Page.frameNavigated", json!({}))));
        d.record(&event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught"}}),
        ));
        assert!(d.tabs.contains_key(&s1()));
        let detached = CdpEvent {
            method: "Target.detachedFromTarget".into(),
            params: json!({"sessionId": "S1"}),
            session: None,
        };
        assert!(!d.record(&detached));
        assert!(!d.tabs.contains_key(&s1()));
    }
}
