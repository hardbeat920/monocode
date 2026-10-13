//! Semantic layer (work package 2 of the browser module, spec 8 and 9):
//! snapshot, refs, actionability, click/hover/type/press/select/scroll,
//! upload, dialog, wait, back/forward, screenshot and read.
//!
//! The core (`browser::mod`) owns admission, the lease, the busy flag, audit
//! and the `tabs.*` actions. For every other action it calls [`execute`]
//! exactly once with an admitted [`RequestCtx`]; everything this layer sends
//! to the browser goes through [`Link::send`] with that context, so the
//! OFF/revocation guarantees of the core hold here too.
//!
//! What the semantic layer may rely on:
//!
//! - [`Exec::handle`] already resolved the tab to its CDP ids
//!   ([`targets::TabHandle`]); the lease is the caller's and open.
//! - [`Exec::link`] for commands (`Method` is a closed set; nothing outside
//!   spec appendix 19.1 can be sent) and [`Link::subscribe`] for events of
//!   `handle.session`.
//! - [`Exec::link`]`.policy()` for the lease (`snapshot_version`,
//!   `frames`, `main_execution_context`, `pending_dialog`, `popups_blocked`),
//!   `settings` (upload roots, dotfiles) and `check_upload_path`.
//! - [`targets::navigate_and_wait`] and [`targets::wait_for_load`].
//! - Scripts under `browser/injected/` are embedded with `include_str!` and
//!   called with `Runtime.callFunctionOn` in the `__monocode` isolated world
//!   (`Page.createIsolatedWorld`); never `Runtime.evaluate`.
//!
//! Until work package 2 lands, [`execute`] answers every action with a
//! `blocked` / `invalid_request` result that says so.

use serde_json::Value;

use super::link::Link;
use super::policy::RequestCtx;
use super::protocol::{ActionKind, ActionResult, BrowserSettings, Reason};
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
    /// `link.commands_sent()` when the action started; compare to decide
    /// whether a failure after a write is `unknown` (spec 11.2).
    pub commands_before: u64,
}

/// Run one semantic action. Must return exactly one result and never panic.
pub fn execute(exec: Exec<'_>) -> ActionResult {
    ActionResult::from_reason(&exec.ctx.request_id, Reason::InvalidRequest).with_message(format!(
        "Action `{}` is not available in this build yet (semantic layer pending). `tabs.list`, `tabs.open`, `tabs.close` and `tabs.focus` work.",
        exec.kind.name()
    ))
}
