//! Semantic layer of the browser module: snapshot, refs, actionability,
//! click/hover/type/press/select/fill/scroll, upload, dialog, wait,
//! back/forward, screenshot, read, console and network.
//!
//! The core (`browser::mod`) owns admission, the lease, the busy flag, audit
//! and the `tabs.*` actions. For every other action it calls [`execute`]
//! exactly once with an admitted [`RequestCtx`]; everything this layer sends
//! to the browser goes through [`Link::send`] with that context, so the
//! OFF/revocation guarantees of the core hold here too.
//!
//! What the semantic layer relies on:
//!
//! - [`Exec::handle`] already resolved the tab to its CDP ids
//!   ([`targets::TabHandle`]); the lease is the caller's and open.
//! - [`Exec::link`] for commands (`Method` is a closed set; nothing else
//!   can be sent) and [`Link::subscribe`] for events of
//!   `handle.session`.
//! - [`Exec::link`]`.policy()` for the lease (`snapshot_version`,
//!   `frames`, `main_execution_context`, `pending_dialog`, `popups_blocked`),
//!   `settings` (upload roots, dotfiles) and `check_upload_path`.
//! - [`targets::navigate_and_wait`] and [`targets::wait_for_load`].
//! - Scripts under `browser/injected/` are embedded with `include_str!` and
//!   called with `Runtime.callFunctionOn` in the `__monocode` isolated world
//!   (`Page.createIsolatedWorld`); never `Runtime.evaluate`, and nothing the
//!   agent sends is ever executed as script.
//!
//! Layout:
//!
//! - [`refs`]: the ref table per tab (`e12`, `e12@4`), stable per document;
//!   a number is never reused within a tab.
//! - [`snapshot`]: the AX snapshot and its text format, and the snapshot (or
//!   its changes) attached to an action's result.
//! - [`element`]: isolated worlds, ref resolution and the shared element
//!   pipeline (actionability, scroll into view, stability, input point, hit
//!   test, frame offsets).
//! - [`observe`]: the post-action observation window (navigation, dialog,
//!   popup, tab closed) and input dispatch that never hangs on a dialog.
//! - [`input`]: `click`, `hover`, `type`, `press`, `scroll` and the key table.
//! - [`fill`]: several form fields in one action, through `type`, `select`
//!   and `click`.
//! - [`cursor`]: the visible MonoCode cursor that glides to the point of a
//!   click, hover or type, so the user can follow the agent.
//! - [`actions`]: `navigate`, `back`, `forward`, `select`, `upload`, `dialog`,
//!   `wait`, `screenshot`, `read`.
//! - [`devtools`]: `console` and `network`, read from the link's record of
//!   the tab ([`crate::browser::diagnostics`]).
//!
//! Result payloads (`ActionResult::data`), for the CLI/MCP adapters:
//!
//! - `snapshot`: `{snapshot, version, url, title, refs, truncated, picked}`;
//!   the snapshot is the text tree, refs look like `[ref=e12]`, `picked` is
//!   the ref of the element the user pointed at (or null).
//! - Actions that can change the page (`returns_snapshot`) add
//!   `{snapshot, snapshot_version}`: the page after them, or its changes.
//! - `screenshot`: `{mime_type, base64, bytes}`.
//! - `read`: `{text, offset, next_offset?, total}` (offsets in UTF-16 units,
//!   as the page counts them).
//! - `select`: `{selected}`; `upload`: `{files}` (base names);
//!   `scroll`: `{x, y}` (window scroll position); `dialog`: `{type, message,
//!   action}`; `wait`: `{for}`; `fill`: `{filled, failed?, not_filled?}`.
//! - `console`: `{text, count, errors, next_since, level}`; `network`:
//!   `{text, count, next_since}`. `text` is the readable listing.
//!
//! Outcome classes: a failure before the first command that can
//! change the page is `failed` (or `blocked`) with its reason; a CDP failure
//! after such a command is `unknown`. Read-only round trips (resolve,
//! actionability, quads) never make a later failure `unknown`.

mod actions;
mod cursor;
mod devtools;
mod element;
mod fill;
mod input;
mod observe;
mod refs;
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::link::{CdpError, CdpSessionId, Command, Link, Method};
use super::policy::{Lease, RequestCtx};
use super::protocol::{
    strip_marker, ActionKind, ActionResult, BrowserSettings, Observed, Outcome, Reason, TabId,
};
use super::targets::TabHandle;

/// Everything one action execution gets from the core.
pub struct Exec<'a> {
    pub link: &'a Link,
    pub ctx: &'a RequestCtx,
    pub kind: ActionKind,
    /// Raw `input` of the request; the layer parses it into the matching
    /// `*Params` record from `protocol.rs` (unknown fields are an
    /// `invalid_request`).
    pub input: &'a Value,
    /// Resolved tab; `None` only for actions that need no lease.
    pub handle: Option<&'a TabHandle>,
    pub settings: &'a BrowserSettings,
}

/// Run one semantic action. Returns exactly one result and never panics.
pub fn execute(exec: Exec<'_>) -> ActionResult {
    {
        let state = exec.link.policy().read();
        refs::prune(host_id(exec.link), &state);
        cursor::prune(host_id(exec.link), &state);
    }
    let request_id = exec.ctx.request_id.as_str();
    let Some(handle) = exec.handle else {
        return ActionResult::from_reason(request_id, Reason::InvalidRequest)
            .with_message("This action needs a `tab`.");
    };
    let run = Run {
        link: exec.link,
        ctx: exec.ctx,
        handle,
        settings: exec.settings,
        mutated: Cell::new(false),
    };
    let input = exec.input;
    let outcome = match exec.kind {
        ActionKind::Snapshot => params(input).and_then(|p| snapshot::snapshot(&run, p)),
        ActionKind::Navigate => params(input).and_then(|p| actions::navigate(&run, p)),
        ActionKind::Back => params(input).and_then(|p| actions::history(&run, p, -1)),
        ActionKind::Forward => params(input).and_then(|p| actions::history(&run, p, 1)),
        ActionKind::Click => params(input).and_then(|p| input::click(&run, p)),
        ActionKind::Hover => params(input).and_then(|p| input::hover(&run, p)),
        ActionKind::Type => params(input).and_then(|p| input::type_text(&run, p)),
        ActionKind::Press => params(input).and_then(|p| input::press(&run, p)),
        ActionKind::Scroll => params(input).and_then(|p| input::scroll(&run, p)),
        ActionKind::Select => params(input).and_then(|p| actions::select(&run, p)),
        ActionKind::Fill => params(input).and_then(|p| fill::fill(&run, p)),
        ActionKind::Upload => params(input).and_then(|p| actions::upload(&run, p)),
        ActionKind::Dialog => params(input).and_then(|p| actions::dialog(&run, p)),
        ActionKind::Wait => params(input).and_then(|p| actions::wait(&run, p)),
        ActionKind::Screenshot => params(input).and_then(|p| actions::screenshot(&run, p)),
        ActionKind::Read => params(input).and_then(|p| actions::read(&run, p)),
        ActionKind::Console => params(input).and_then(|p| devtools::console(&run, p)),
        ActionKind::Network => params(input).and_then(|p| devtools::network(&run, p)),
        ActionKind::TabsList
        | ActionKind::TabsOpen
        | ActionKind::TabsClose
        | ActionKind::TabsFocus
        | ActionKind::TabsAdopt => Err(Fail::detail(
            Reason::InvalidRequest,
            "tabs.* actions are handled by the browser core.",
        )),
    };
    let mut result = match outcome {
        Ok(result) => result,
        Err(fail) => run.fail(fail, exec.kind.mutates()),
    };
    if returns_snapshot(exec.kind) && wants_snapshot(input) {
        attach_snapshot(&run, &mut result);
    }
    result
}

/// Actions whose result carries the page snapshot after them, so the agent
/// sees the effect without another call (and another model turn).
fn returns_snapshot(kind: ActionKind) -> bool {
    matches!(
        kind,
        ActionKind::Navigate
            | ActionKind::Back
            | ActionKind::Forward
            | ActionKind::Click
            | ActionKind::Hover
            | ActionKind::Type
            | ActionKind::Press
            | ActionKind::Scroll
            | ActionKind::Select
            | ActionKind::Fill
            | ActionKind::Upload
            | ActionKind::Dialog
            | ActionKind::Wait
            | ActionKind::TabsOpen
    )
}

/// Unless the input says `snapshot: false`.
pub fn wants_snapshot(input: &Value) -> bool {
    input.get("snapshot") != Some(&Value::Bool(false))
}

/// Time a snapshot after an action may take, on top of the action's own.
pub(crate) const SNAPSHOT_BUDGET: Duration = Duration::from_secs(10);

/// Add the page snapshot after an action to a `done` result: `data.snapshot`
/// (the text: the whole page, or only the changes since the last snapshot of
/// this document) and `data.snapshot_version`. Best effort: when it cannot
/// be taken the result keeps telling the agent to take one. Nothing is taken
/// while a dialog blocks the page.
fn attach_snapshot(run: &Run<'_>, result: &mut ActionResult) {
    if result.outcome != Outcome::Done
        || result.observed.dialog.is_some()
        || run.lease(|l| l.pending_dialog.is_some()).unwrap_or(true)
    {
        return;
    }
    // The action may have used its whole time limit (a slow load).
    let mut ctx = run.ctx.clone();
    ctx.deadline = ctx
        .deadline
        .max(std::time::Instant::now() + SNAPSHOT_BUDGET);
    let after = Run {
        link: run.link,
        ctx: &ctx,
        handle: run.handle,
        settings: run.settings,
        mutated: Cell::new(false),
    };
    let Ok(inline) = snapshot::after_action(&after) else {
        return;
    };
    let mut data = match result.data.take() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            map
        }
        None => serde_json::Map::new(),
    };
    data.insert("snapshot".into(), Value::String(inline.text));
    data.insert("snapshot_version".into(), inline.version.into());
    result.data = Some(Value::Object(data));
    result.observed.snapshot_stale = false;
    let message = result.message.replace(observe::STALE_HINT, "");
    result.message = format!(
        "{} {}",
        message.trim_end(),
        match inline.kind {
            "unchanged" => "The page snapshot did not change.",
            "changes" => "The changes to the page snapshot follow.",
            _ => "The page snapshot follows.",
        }
    );
}

/// [`attach_snapshot`] for a tab the core just opened (`tabs.open`).
pub fn attach_snapshot_for(
    link: &Link,
    ctx: &RequestCtx,
    handle: &TabHandle,
    settings: &BrowserSettings,
    input: &Value,
    result: &mut ActionResult,
) {
    if !wants_snapshot(input) {
        return;
    }
    let mut ctx = ctx.clone();
    ctx.tab = Some(handle.tab.clone());
    let run = Run {
        link,
        ctx: &ctx,
        handle,
        settings,
        mutated: Cell::new(false),
    };
    attach_snapshot(&run, result);
}

/// Forget a tab's refs: after a transfer the new owner must take its own
/// snapshot, and refs the old owner saw are `ref_stale`.
pub fn forget_refs(link: &Link, tab: &TabId) {
    refs::clear(&refs::RefKey {
        host: host_id(link),
        tab: tab.clone(),
    });
}

/// The user pointed at `backend` in `tab` ([`crate::browser::pick`]): the
/// tab's next snapshot marks it. Returns its ref when a snapshot of this
/// document already gave it one.
pub fn mark_picked(
    link: &Link,
    tab: &TabId,
    loader: Option<&str>,
    session: &CdpSessionId,
    backend: i64,
) -> Option<u32> {
    refs::set_picked(
        &refs::RefKey {
            host: host_id(link),
            tab: tab.clone(),
        },
        loader,
        session,
        backend,
    )
}

/// Identity of the browser host owning `link` (its policy's address), which
/// keeps the ref tables of hosts apart (the app has one, tests many).
fn host_id(link: &Link) -> usize {
    std::sync::Arc::as_ptr(link.policy()) as usize
}

/// Parse an action's raw input into its parameter record.
fn params<T: DeserializeOwned>(input: &Value) -> Result<T, Fail> {
    serde_json::from_value(input.clone())
        .map_err(|error| Fail::detail(Reason::InvalidRequest, format!("Invalid input: {error}")))
}

/// Why an action did not complete.
#[derive(Debug)]
pub(crate) enum Fail {
    /// A known reason with an optional message for the agent.
    Reason(Reason, Option<String>),
    /// The link refused or lost a command; the method name is for messages.
    Cdp(CdpError, &'static str),
}

impl Fail {
    pub(crate) fn reason(reason: Reason) -> Fail {
        Fail::Reason(reason, None)
    }

    pub(crate) fn detail(reason: Reason, message: impl Into<String>) -> Fail {
        Fail::Reason(reason, Some(message.into()))
    }

    /// The browser's error message, when the browser rejected the command.
    pub(crate) fn protocol_message(&self) -> Option<&str> {
        match self {
            Fail::Cdp(CdpError::Protocol { message, .. }, _) => Some(message.as_str()),
            _ => None,
        }
    }
}

/// One action in flight: the admitted request, the tab, and whether a
/// command that can change the page has been written yet.
pub(crate) struct Run<'a> {
    pub link: &'a Link,
    pub ctx: &'a RequestCtx,
    pub handle: &'a TabHandle,
    pub settings: &'a BrowserSettings,
    /// Set right before the first page-changing command is written.
    pub mutated: Cell<bool>,
}

impl Run<'_> {
    pub fn id(&self) -> &str {
        &self.ctx.request_id
    }

    pub fn tab(&self) -> &TabId {
        &self.handle.tab
    }

    /// Key of this tab's ref table.
    pub fn key(&self) -> refs::RefKey {
        refs::RefKey {
            host: host_id(self.link),
            tab: self.tab().clone(),
        }
    }

    pub fn main(&self) -> &CdpSessionId {
        &self.handle.session
    }

    /// `timeout` once the action's own deadline passed.
    pub fn check_time(&self) -> Result<(), Fail> {
        if self.ctx.expired() {
            Err(Fail::detail(
                Reason::Timeout,
                format!(
                    "The action did not finish within its time limit ({} ms).",
                    self.ctx
                        .deadline
                        .duration_since(self.ctx.started)
                        .as_millis()
                ),
            ))
        } else {
            Ok(())
        }
    }

    /// A command that only reads (or scrolls) and never makes a later
    /// failure `unknown`.
    pub fn send(
        &self,
        session: &CdpSessionId,
        method: Method,
        params: Value,
    ) -> Result<Value, Fail> {
        self.check_time()?;
        self.link
            .send(self.ctx, Command::in_session(session, method, params))
            .map_err(|error| Fail::Cdp(error, method.name()))
    }

    /// [`Self::send`] that reads the answer straight into `T`.
    pub fn send_as<T: serde::de::DeserializeOwned>(
        &self,
        session: &CdpSessionId,
        method: Method,
        params: Value,
    ) -> Result<T, Fail> {
        self.check_time()?;
        self.link
            .send_as(self.ctx, Command::in_session(session, method, params))
            .map_err(|error| Fail::Cdp(error, method.name()))
    }

    /// A command that can change the page.
    pub fn send_mut(
        &self,
        session: &CdpSessionId,
        method: Method,
        params: Value,
    ) -> Result<Value, Fail> {
        self.check_time()?;
        self.mutated.set(true);
        self.link
            .send(self.ctx, Command::in_session(session, method, params))
            .map_err(|error| Fail::Cdp(error, method.name()))
    }

    /// Read the tab's lease; a lease that vanished means the tab is gone.
    pub fn lease<R>(&self, f: impl FnOnce(&Lease) -> R) -> Result<R, Fail> {
        let state = self.link.policy().read();
        state
            .lease(self.tab())
            .map(f)
            .ok_or_else(|| Fail::reason(Reason::TargetClosed))
    }

    /// `url` and `title` (without the marker) as the lease knows them.
    pub fn observed_now(&self) -> Observed {
        let (url, title) = self
            .lease(|l| (l.url.clone(), strip_marker(&l.title).to_string()))
            .unwrap_or_default();
        Observed {
            url: Some(url),
            title: Some(title),
            ..Observed::default()
        }
    }

    /// Sleep, but never past the action's deadline.
    pub fn pause(&self, duration: Duration) {
        let remaining = self.ctx.remaining();
        std::thread::sleep(duration.min(remaining));
    }

    /// The result for a failure: reasons keep their class, except
    /// a timeout after a page-changing write, which is `unknown`; CDP
    /// failures follow [`CdpError::into_result`], and a browser-side
    /// rejection is `failed` (the browser saw the command), or `unknown`
    /// after a write.
    fn fail(&self, fail: Fail, mutating: bool) -> ActionResult {
        let wrote = self.mutated.get();
        match fail {
            Fail::Reason(reason, message) => {
                let mut result = ActionResult::from_reason(self.id(), reason);
                if let Some(message) = message {
                    result = result.with_message(message);
                }
                if reason == Reason::Timeout && mutating && wrote {
                    result = result
                        .with_outcome(Outcome::Unknown)
                        .with_message("The action ran out of time after input reached the page; take a snapshot before retrying.");
                }
                result
            }
            Fail::Cdp(CdpError::Protocol { code, message }, method) => {
                ActionResult::from_reason(self.id(), Reason::InvalidRequest)
                    .with_message(format!("Browser rejected {method} ({code}): {message}"))
                    .with_outcome(if mutating && wrote {
                        Outcome::Unknown
                    } else {
                        Outcome::Failed
                    })
            }
            Fail::Cdp(error, _) => error.into_result(self.id(), mutating, wrote),
        }
    }
}

/// Short human label of a ref for result messages: `e12 (button "Save")`.
pub(crate) fn describe(raw: &str, role: &str, name: &str) -> String {
    let name: String = name.chars().take(60).collect();
    if name.is_empty() {
        format!("{raw} ({role})")
    } else {
        format!("{raw} ({role} \"{name}\")")
    }
}
