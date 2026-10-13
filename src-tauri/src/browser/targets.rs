//! Tab lifecycle (spec 7, 10.4): create and attach, session setup, marker,
//! close, focus, popups and OOPIF frames via auto-attach, and the event pump
//! that keeps the lease table current from CDP events.
//!
//! Everything here goes through [`Link::send`] with the caller's
//! [`RequestCtx`] (agent work) or [`Link::send_system`] (module housekeeping
//! such as resuming a paused popup or detaching a worker). The semantic layer
//! builds on [`TabHandle`], [`navigate_and_wait`] and [`wait_for_load`].

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::link::{
    CdpError, CdpEvent, CdpSessionId, Command, EventFilter, Link, Method, Subscription, TargetId,
};
use super::policy::{check_url, now_ms, FrameSession, Lease, Policy, PolicyState, RequestCtx};
use super::protocol::{
    strip_marker, ActionResult, DialogInfo, DialogType, LoadState, Observed, Outcome, Reason,
    TabId, TabState, WaitUntil, ISOLATED_WORLD,
};

/// The marker script (spec 7, decision D12), embedded at build time.
pub const MARKER_JS: &str = include_str!("injected/marker.js");

/// How long the module waits for a page to report `load` when the caller
/// gave no deadline of its own.
pub const DEFAULT_LOAD_WAIT: Duration = Duration::from_secs(30);

/// Resolved identifiers of a tab the caller holds a lease on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabHandle {
    pub tab: TabId,
    pub target: TargetId,
    /// Main-frame CDP session.
    pub session: CdpSessionId,
}

/// Why a tab operation failed.
#[derive(Debug)]
pub enum TabError {
    /// A policy or protocol-level refusal with an optional detail message.
    Reason(Reason, Option<String>),
    /// The link refused or lost the command.
    Cdp(CdpError),
}

impl TabError {
    pub fn reason(reason: Reason) -> TabError {
        TabError::Reason(reason, None)
    }

    pub fn detail(reason: Reason, message: impl Into<String>) -> TabError {
        TabError::Reason(reason, Some(message.into()))
    }

    /// Build the `ActionResult` for this failure. `mutating` and
    /// `wrote_before` follow [`CdpError::into_result`].
    pub fn into_result(self, request_id: &str, mutating: bool, wrote_before: bool) -> ActionResult {
        match self {
            TabError::Reason(reason, message) => {
                let mut result = ActionResult::from_reason(request_id, reason);
                if let Some(message) = message {
                    result = result.with_message(message);
                }
                if wrote_before && mutating && reason.outcome() == Outcome::Failed {
                    result = result.with_outcome(Outcome::Unknown);
                }
                result
            }
            TabError::Cdp(error) => error.into_result(request_id, mutating, wrote_before),
        }
    }
}

impl From<CdpError> for TabError {
    fn from(error: CdpError) -> TabError {
        TabError::Cdp(error)
    }
}

impl From<Reason> for TabError {
    fn from(reason: Reason) -> TabError {
        TabError::Reason(reason, None)
    }
}

/// Who is sending: an admitted agent request, or the module itself.
#[derive(Clone, Copy)]
pub enum Sender<'a> {
    Agent(&'a RequestCtx),
    System,
}

fn send(link: &Link, sender: Sender<'_>, command: Command) -> Result<Value, CdpError> {
    match sender {
        Sender::Agent(ctx) => link.send(ctx, command),
        Sender::System => link.send_system(command),
    }
}

/// Resolve the caller's tab to its CDP identifiers. The lease must be the
/// caller's and open (spec 7).
pub fn handle_for(state: &PolicyState, session: &str, tab: &TabId) -> Result<TabHandle, Reason> {
    let lease = state.lease(tab).ok_or(Reason::NoLease)?;
    if lease.owner != session {
        return Err(Reason::NoLease);
    }
    match lease.state {
        TabState::Open => {}
        TabState::Attaching => return Err(Reason::Busy),
        TabState::Closed | TabState::Crashed => return Err(Reason::TargetClosed),
        TabState::Revoked => return Err(Reason::NoLease),
    }
    let cdp = lease.cdp_session.clone().ok_or(Reason::Busy)?;
    Ok(TabHandle {
        tab: tab.clone(),
        target: lease.target_id.clone(),
        session: cdp,
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Spec 5 "Attach model", step 2: what every page session of ours gets once.
/// `Target.setAutoAttach` here is session-scoped, so popups and OOPIFs of
/// this tab arrive paused (`waitForDebuggerOnStart`) and nothing else does.
pub fn setup_session(
    link: &Link,
    sender: Sender<'_>,
    session: &CdpSessionId,
) -> Result<(), CdpError> {
    send(
        link,
        sender,
        Command::in_session(session, Method::PageEnable, json!({})),
    )?;
    send(
        link,
        sender,
        Command::in_session(
            session,
            Method::PageSetLifecycleEventsEnabled,
            json!({ "enabled": true }),
        ),
    )?;
    send(
        link,
        sender,
        Command::in_session(
            session,
            Method::TargetSetAutoAttach,
            json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }),
        ),
    )?;
    send(
        link,
        sender,
        Command::in_session(
            session,
            Method::EmulationSetFocusEmulationEnabled,
            json!({ "enabled": true }),
        ),
    )?;
    send(
        link,
        sender,
        Command::in_session(
            session,
            Method::PageAddScriptToEvaluateOnNewDocument,
            json!({ "source": MARKER_JS, "worldName": ISOLATED_WORLD, "runImmediately": true }),
        ),
    )?;
    Ok(())
}

fn new_lease(
    state: &PolicyState,
    target: &TargetId,
    cdp_session: Option<CdpSessionId>,
    owner: &str,
    opened_by: Option<TabId>,
    url: &str,
    title: &str,
) -> Lease {
    let now = now_ms();
    Lease {
        tab: state.new_tab_id(),
        target_id: target.clone(),
        cdp_session,
        owner: owner.to_string(),
        state: TabState::Attaching,
        main_frame_id: Some(target.0.clone()),
        main_loader_id: None,
        main_execution_context: None,
        frames: Default::default(),
        busy: false,
        snapshot_version: 0,
        pending_dialog: None,
        opened_by,
        popups_blocked: 0,
        url: url.to_string(),
        title: title.to_string(),
        created_at_ms: now,
        last_action_at_ms: now,
    }
}

fn mark_closed(policy: &Policy, tab: &TabId) {
    if let Some(lease) = policy.write().lease_mut(tab) {
        lease.state = TabState::Closed;
        lease.busy = false;
    }
}

/// Result of [`open_tab`]: the tab exists even when the first navigation
/// failed, so the agent can still close or reuse it.
#[derive(Debug)]
pub struct Opened {
    pub handle: TabHandle,
    pub observed: Observed,
    /// `Err` when the requested URL did not load (`navigation_failed`,
    /// `timeout`); the tab itself is open.
    pub navigation: Result<(), (Reason, String)>,
}

/// A tab slot held by [`open_tab`] from the limit check until its lease
/// exists; released on every early return.
struct Reservation<'a> {
    policy: &'a Policy,
    session: &'a str,
    held: bool,
}

impl Reservation<'_> {
    fn release(&mut self, state: &mut PolicyState) {
        if std::mem::take(&mut self.held) {
            state.unreserve_tab(self.session);
        }
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.held {
            self.policy.write().unreserve_tab(self.session);
        }
    }
}

/// `tabs.open` (spec 7 "Otvaranje"): create a background tab in the user's
/// window, attach, set the session up, then navigate.
pub fn open_tab(
    link: &Link,
    ctx: &RequestCtx,
    url: Option<&str>,
    wait_until: WaitUntil,
) -> Result<Opened, TabError> {
    let policy = link.policy();
    let (separate_window, target_url) = {
        let mut state = policy.write();
        let target_url = match url.map(str::trim) {
            Some(url) if !url.is_empty() => check_url(url)?,
            _ => "about:blank".to_string(),
        };
        state.reserve_tab(&ctx.session)?;
        (state.settings.separate_window, target_url)
    };
    let mut reservation = Reservation {
        policy,
        session: &ctx.session,
        held: true,
    };
    let created = link.send(
        ctx,
        Command::browser(
            Method::TargetCreateTarget,
            json!({ "url": "about:blank", "newWindow": separate_window, "background": true }),
        ),
    )?;
    let target = TargetId(string_field(&created, "targetId").ok_or_else(|| {
        TabError::detail(
            Reason::BrowserDisconnected,
            "Target.createTarget returned no targetId",
        )
    })?);
    let tab = {
        let mut state = policy.write();
        let lease = new_lease(&state, &target, None, &ctx.session, None, "about:blank", "");
        reservation.release(&mut state);
        state.insert_lease(lease)
    };
    let attached = match link.send(
        ctx,
        Command::browser(
            Method::TargetAttachToTarget,
            json!({ "targetId": target.0, "flatten": true }),
        ),
    ) {
        Ok(value) => value,
        Err(error) => {
            mark_closed(policy, &tab);
            let _ = link.send_system(Command::browser(
                Method::TargetCloseTarget,
                json!({ "targetId": target.0 }),
            ));
            return Err(match error {
                // `DeveloperToolsAvailability = 2`: the tab exists but no one
                // may attach to it (spec 4, "Politike i okruženje").
                CdpError::Protocol { message, .. } => TabError::detail(
                    Reason::BrowserPolicyBlocked,
                    format!("Target.attachToTarget: {message}"),
                ),
                other => other.into(),
            });
        }
    };
    let session = CdpSessionId(string_field(&attached, "sessionId").ok_or_else(|| {
        TabError::detail(
            Reason::BrowserDisconnected,
            "Target.attachToTarget returned no sessionId",
        )
    })?);
    if let Some(lease) = policy.write().lease_mut(&tab) {
        lease.cdp_session = Some(session.clone());
    }
    if let Err(error) = setup_session(link, Sender::Agent(ctx), &session) {
        mark_closed(policy, &tab);
        let _ = link.send_system(Command::browser(
            Method::TargetCloseTarget,
            json!({ "targetId": target.0 }),
        ));
        return Err(error.into());
    }
    if let Some(lease) = policy.write().lease_mut(&tab) {
        lease.state = TabState::Open;
    }
    let handle = TabHandle {
        tab,
        target,
        session,
    };
    let mut observed = Observed {
        url: Some("about:blank".into()),
        title: Some(String::new()),
        ..Observed::default()
    };
    let mut navigation = Ok(());
    if target_url != "about:blank" {
        match navigate_and_wait(link, ctx, &handle, &target_url, wait_until, ctx.deadline) {
            Ok(seen) => observed = seen,
            Err(TabError::Reason(reason, message)) => {
                navigation = Err((
                    reason,
                    message.unwrap_or_else(|| reason.message().to_string()),
                ));
            }
            Err(TabError::Cdp(error)) => return Err(error.into()),
        }
    }
    Ok(Opened {
        handle,
        observed,
        navigation,
    })
}

/// `Page.navigate` and wait for the requested load state (spec 9.1
/// `navigate`, 10.4). Returns what the module observed; a navigation error
/// (`errorText`, DNS failure, blocked scheme) is `navigation_failed`.
pub fn navigate_and_wait(
    link: &Link,
    ctx: &RequestCtx,
    handle: &TabHandle,
    url: &str,
    wait_until: WaitUntil,
    deadline: Instant,
) -> Result<Observed, TabError> {
    let url = check_url(url)?;
    let subscription = link.subscribe(EventFilter::session(&handle.session));
    let result = link.send(
        ctx,
        Command::in_session(&handle.session, Method::PageNavigate, json!({ "url": url })),
    )?;
    if let Some(error_text) = result.get("errorText").and_then(Value::as_str) {
        if !error_text.is_empty() {
            return Err(TabError::detail(
                Reason::NavigationFailed,
                format!("{url}: {error_text}"),
            ));
        }
    }
    let loader = string_field(&result, "loaderId");
    let (load_state, dialog) =
        wait_for_load(&subscription, loader.as_deref(), wait_until, deadline);
    let (seen_url, title) = {
        let state = link.policy().read();
        state
            .lease(&handle.tab)
            .map(|l| (l.url.clone(), strip_marker(&l.title).to_string()))
            .unwrap_or_else(|| (url.clone(), String::new()))
    };
    if let Some(lease) = link.policy().write().lease_mut(&handle.tab) {
        lease.snapshot_version += 1;
        if lease.url.is_empty() || lease.url == "about:blank" {
            lease.url = url.clone();
        }
    }
    Ok(Observed {
        url: Some(if seen_url.is_empty() || seen_url == "about:blank" {
            url
        } else {
            seen_url
        }),
        title: Some(title),
        navigated: true,
        load_state: Some(load_state),
        snapshot_stale: true,
        dialog,
        ..Observed::default()
    })
}

/// Wait on a session subscription for `load` (or `DOMContentLoaded`) of the
/// given loader. A dialog opening ends the wait early (spec 10.1: the module
/// never touches dialogs). Returns the load state reached and the dialog, if any.
pub fn wait_for_load(
    subscription: &Subscription,
    loader: Option<&str>,
    wait_until: WaitUntil,
    deadline: Instant,
) -> (LoadState, Option<DialogInfo>) {
    let mut dom_content_loaded = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return (
                if dom_content_loaded {
                    LoadState::Domcontentloaded
                } else {
                    LoadState::Timeout
                },
                None,
            );
        }
        let Some(event) = subscription.recv_timeout(remaining.min(Duration::from_millis(100)))
        else {
            if subscription.is_closed() {
                return (LoadState::Timeout, None);
            }
            continue;
        };
        match event.method.as_str() {
            "Page.lifecycleEvent" => {
                let same_loader =
                    loader.is_none_or(|l| event.params["loaderId"].as_str() == Some(l));
                if !same_loader {
                    continue;
                }
                match event.params["name"].as_str() {
                    Some("load") => return (LoadState::Load, None),
                    Some("DOMContentLoaded") => {
                        dom_content_loaded = true;
                        if wait_until == WaitUntil::Domcontentloaded {
                            return (LoadState::Domcontentloaded, None);
                        }
                    }
                    _ => {}
                }
            }
            "Page.loadEventFired" | "Page.navigatedWithinDocument" => {
                return (LoadState::Load, None);
            }
            "Page.javascriptDialogOpening" => {
                return (
                    if dom_content_loaded {
                        LoadState::Domcontentloaded
                    } else {
                        LoadState::Timeout
                    },
                    Some(dialog_info(&event.params)),
                );
            }
            _ => {}
        }
    }
}

/// `tabs.close` (spec 7): close our own tab.
pub fn close_tab(link: &Link, ctx: &RequestCtx, tab: &TabId) -> Result<(), TabError> {
    let target = {
        let state = link.policy().read();
        let lease = state.lease(tab).ok_or(Reason::NoLease)?;
        if lease.owner != ctx.session {
            return Err(Reason::NoLease.into());
        }
        match lease.state {
            TabState::Closed | TabState::Crashed => return Err(Reason::TargetClosed.into()),
            TabState::Revoked => return Err(Reason::NoLease.into()),
            TabState::Attaching | TabState::Open => {}
        }
        lease.target_id.clone()
    };
    link.send(
        ctx,
        Command::browser(Method::TargetCloseTarget, json!({ "targetId": target.0 })),
    )?;
    mark_closed(link.policy(), tab);
    Ok(())
}

/// `tabs.focus {user_visible: true}`: bring our tab to the front. The only
/// action that changes what the user sees, hence the explicit flag (spec 9.1).
pub fn focus_tab(link: &Link, ctx: &RequestCtx, tab: &TabId) -> Result<(), TabError> {
    let handle = handle_for(&link.policy().read(), &ctx.session, tab)?;
    link.send(
        ctx,
        Command::in_session(&handle.session, Method::PageBringToFront, json!({})),
    )?;
    Ok(())
}

/// Close tabs the module owns without an agent context (session deleted,
/// Settings "close all agent tabs"). Best effort; the lease is already Closed.
pub fn close_targets(link: &Link, targets: &[TargetId]) {
    for target in targets {
        let _ = link.send_system(Command::browser(
            Method::TargetCloseTarget,
            json!({ "targetId": target.0 }),
        ));
    }
}

/// Script run in our isolated world to take the marker off a page the user
/// takes back (spec 13.1 "Preuzmi tab"); `marker.js` defines the hook.
const MARKER_RELEASE_FN: &str =
    "function() { return typeof globalThis.__monocodeMarkerRelease === 'function' ? globalThis.__monocodeMarkerRelease() : false; }";

/// Strip the title prefix and restore the favicon in the current document of
/// `session` before detaching from it. Best effort: the isolated world of the
/// main frame is looked up by name, so this reaches the running marker script.
/// New documents get no marker once the session is detached, because
/// `Page.addScriptToEvaluateOnNewDocument` registrations die with the session.
pub fn release_marker(
    link: &Link,
    session: &CdpSessionId,
    main_frame_id: &str,
) -> Result<(), CdpError> {
    let world = link.send_system(Command::in_session(
        session,
        Method::PageCreateIsolatedWorld,
        json!({ "frameId": main_frame_id, "worldName": ISOLATED_WORLD, "grantUniveralAccess": false }),
    ))?;
    let Some(context) = world["executionContextId"].as_i64() else {
        return Ok(());
    };
    link.send_system(Command::in_session(
        session,
        Method::RuntimeCallFunctionOn,
        json!({
            "functionDeclaration": MARKER_RELEASE_FN,
            "executionContextId": context,
            "returnByValue": true,
        }),
    ))?;
    Ok(())
}

/// Detach from sessions whose leases were revoked (revoke, grant off,
/// window closed). Tabs stay open with the marker (spec 7).
pub fn detach_sessions(link: &Link, sessions: &[CdpSessionId]) {
    for session in sessions {
        let _ = link.send_system(Command::browser(
            Method::TargetDetachFromTarget,
            json!({ "sessionId": session.0 }),
        ));
    }
}

fn dialog_info(params: &Value) -> DialogInfo {
    let kind = match params["type"].as_str() {
        Some("confirm") => DialogType::Confirm,
        Some("prompt") => DialogType::Prompt,
        Some("beforeunload") => DialogType::Beforeunload,
        _ => DialogType::Alert,
    };
    DialogInfo {
        kind,
        message: string_field(params, "message").unwrap_or_default(),
        default_prompt: string_field(params, "defaultPrompt").filter(|s| !s.is_empty()),
        url: string_field(params, "url").unwrap_or_default(),
        opened_at_ms: now_ms(),
    }
}

// ---------------------------------------------------------------------------
// Event pump
// ---------------------------------------------------------------------------

/// What the pump tells the host about (audit rows, state emits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabEvent {
    /// A popup of `opener` became a tab of the same owner.
    PopupOpened {
        tab: TabId,
        opener: TabId,
        owner: String,
    },
    /// A popup was closed because of the tab limit (spec 10.4).
    PopupBlocked {
        opener: TabId,
        owner: String,
    },
    /// The browser closed (or the user closed) one of our tabs.
    Closed(TabId),
    Crashed(TabId),
    /// Main frame navigated (user or page initiated as well).
    Navigated(TabId),
    DialogOpened(TabId),
    DialogClosed(TabId),
}

/// Lease owning `session` as main frame or OOPIF frame.
fn tab_holding(state: &PolicyState, session: &CdpSessionId) -> Option<TabId> {
    state
        .leases()
        .find(|l| l.state != TabState::Closed && l.holds_session(session))
        .map(|l| l.tab.clone())
}

/// Run the pump on the given subscription until the link closes. Returns the
/// thread handle; the host keeps it only for tests.
pub fn spawn_pump(
    link: Link,
    notify: Arc<dyn Fn(TabEvent) + Send + Sync>,
) -> std::io::Result<thread::JoinHandle<()>> {
    let subscription = link.subscribe(EventFilter::all());
    thread::Builder::new()
        .name("browser-events".into())
        .spawn(move || pump_loop(&link, &subscription, notify.as_ref()))
}

fn pump_loop(link: &Link, subscription: &Subscription, notify: &dyn Fn(TabEvent)) {
    loop {
        match subscription.recv_timeout(Duration::from_millis(250)) {
            Some(event) => handle_event(link, &event, notify),
            None if subscription.is_closed() => return,
            None => {}
        }
    }
}

/// One event, one lease-table update (spec 5 "Događaji koje link prati").
pub fn handle_event(link: &Link, event: &CdpEvent, notify: &dyn Fn(TabEvent)) {
    let policy = link.policy();
    match event.method.as_str() {
        "Target.attachedToTarget" => {
            // Browser-level copies of our own `Target.attachToTarget` carry no
            // sessionId; only auto-attached children of a page session matter.
            let Some(parent) = &event.session else { return };
            on_attached(link, parent, &event.params, notify);
        }
        "Target.detachedFromTarget" => {
            let Some(child) = string_field(&event.params, "sessionId") else {
                return;
            };
            let child = CdpSessionId(child);
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, &child) else {
                return;
            };
            let Some(lease) = state.lease_mut(&tab) else {
                return;
            };
            if lease.cdp_session.as_ref() == Some(&child) {
                if lease.state == TabState::Revoked {
                    // Our own detach after a revocation: the lease already says so.
                    return;
                }
                lease.state = TabState::Closed;
                lease.busy = false;
                drop(state);
                notify(TabEvent::Closed(tab));
            } else {
                lease.frames.retain(|_, f| f.cdp_session != child);
            }
        }
        "Inspector.targetCrashed" => {
            let Some(session) = &event.session else {
                return;
            };
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            let Some(lease) = state.lease_mut(&tab) else {
                return;
            };
            if lease.cdp_session.as_ref() == Some(session) {
                lease.state = TabState::Crashed;
                lease.busy = false;
                drop(state);
                notify(TabEvent::Crashed(tab));
            } else {
                lease.frames.retain(|_, f| &f.cdp_session != session);
            }
        }
        "Inspector.detached" => {
            let Some(session) = &event.session else {
                return;
            };
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            let Some(lease) = state.lease_mut(&tab) else {
                return;
            };
            if lease.cdp_session.as_ref() == Some(session) && lease.state != TabState::Revoked {
                lease.state = TabState::Closed;
                lease.busy = false;
                drop(state);
                notify(TabEvent::Closed(tab));
            }
        }
        "Target.targetInfoChanged" => {
            let info = &event.params["targetInfo"];
            let Some(target) = string_field(info, "targetId") else {
                return;
            };
            let mut state = policy.write();
            if let Some(lease) = state.lease_for_target_mut(&TargetId(target)) {
                if let Some(url) = string_field(info, "url") {
                    lease.url = url;
                }
                if let Some(title) = string_field(info, "title") {
                    lease.title = title;
                }
            }
        }
        "Page.frameNavigated" => {
            let Some(session) = &event.session else {
                return;
            };
            let frame = &event.params["frame"];
            let Some(frame_id) = string_field(frame, "id") else {
                return;
            };
            let is_main = frame
                .get("parentId")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty);
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            let Some(lease) = state.lease_mut(&tab) else {
                return;
            };
            if lease.cdp_session.as_ref() == Some(session) && is_main {
                lease.url = string_field(frame, "url").unwrap_or_default();
                lease.main_frame_id = Some(frame_id);
                lease.main_loader_id = string_field(frame, "loaderId");
                lease.main_execution_context = None;
                lease.snapshot_version += 1;
                lease.pending_dialog = None;
                drop(state);
                notify(TabEvent::Navigated(tab));
            } else if let Some(frame_session) = lease.frames.get_mut(&frame_id) {
                frame_session.loader_id = string_field(frame, "loaderId");
                frame_session.execution_context_id = None;
            }
        }
        "Page.navigatedWithinDocument" => {
            let Some(session) = &event.session else {
                return;
            };
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            if let Some(lease) = state.lease_mut(&tab) {
                if lease.cdp_session.as_ref() == Some(session) {
                    if let Some(url) = string_field(&event.params, "url") {
                        lease.url = url;
                    }
                    lease.snapshot_version += 1;
                }
            }
        }
        "Page.javascriptDialogOpening" => {
            let Some(session) = &event.session else {
                return;
            };
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            if let Some(lease) = state.lease_mut(&tab) {
                lease.pending_dialog = Some(dialog_info(&event.params));
            }
            drop(state);
            notify(TabEvent::DialogOpened(tab));
        }
        "Page.javascriptDialogClosed" => {
            let Some(session) = &event.session else {
                return;
            };
            let mut state = policy.write();
            let Some(tab) = tab_holding(&state, session) else {
                return;
            };
            if let Some(lease) = state.lease_mut(&tab) {
                lease.pending_dialog = None;
            }
            drop(state);
            notify(TabEvent::DialogClosed(tab));
        }
        _ => {}
    }
}

fn on_attached(link: &Link, parent: &CdpSessionId, params: &Value, notify: &dyn Fn(TabEvent)) {
    let policy = link.policy();
    let Some(child) = string_field(params, "sessionId") else {
        return;
    };
    let child = CdpSessionId(child);
    let info = &params["targetInfo"];
    let Some(target) = string_field(info, "targetId") else {
        return;
    };
    let target = TargetId(target);
    let kind = info["type"].as_str().unwrap_or("");
    let waiting = params["waitingForDebugger"].as_bool().unwrap_or(false);
    let resume = |session: &CdpSessionId| {
        let _ = link.send_system(Command::in_session(
            session,
            Method::RuntimeRunIfWaitingForDebugger,
            json!({}),
        ));
    };
    let parent_tab = tab_holding(&policy.read(), parent);
    let Some(parent_tab) = parent_tab else {
        // Not ours (or already gone): never keep a session we did not ask for.
        detach_sessions(link, std::slice::from_ref(&child));
        return;
    };
    match kind {
        "iframe" => {
            {
                let mut state = policy.write();
                if let Some(lease) = state.lease_mut(&parent_tab) {
                    lease.frames.insert(
                        target.0.clone(),
                        FrameSession {
                            cdp_session: child.clone(),
                            loader_id: None,
                            execution_context_id: None,
                        },
                    );
                }
            }
            // Nested OOPIFs and popups opened from inside the frame.
            let _ = link.send_system(Command::in_session(
                &child,
                Method::TargetSetAutoAttach,
                json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true }),
            ));
            if waiting {
                resume(&child);
            }
        }
        "page" => {
            let owner = {
                let state = policy.read();
                state.lease(&parent_tab).map(|l| l.owner.clone())
            };
            let Some(owner) = owner else {
                detach_sessions(link, std::slice::from_ref(&child));
                return;
            };
            let allowed = policy.read().can_open_tab(&owner).is_ok();
            if !allowed {
                // Over the limit: close it before its first script (spec 10.4).
                // Count it first, so whoever observes the `closeTarget` frame
                // (the action layer, tests) already sees the counter moved.
                if let Some(lease) = policy.write().lease_mut(&parent_tab) {
                    lease.popups_blocked += 1;
                }
                let _ = link.send_system(Command::browser(
                    Method::TargetCloseTarget,
                    json!({ "targetId": target.0 }),
                ));
                notify(TabEvent::PopupBlocked {
                    opener: parent_tab,
                    owner,
                });
                return;
            }
            let tab = {
                let mut state = policy.write();
                let lease = new_lease(
                    &state,
                    &target,
                    Some(child.clone()),
                    &owner,
                    Some(parent_tab.clone()),
                    info["url"].as_str().unwrap_or(""),
                    info["title"].as_str().unwrap_or(""),
                );
                state.insert_lease(lease)
            };
            let ready = setup_session(link, Sender::System, &child).is_ok();
            {
                let mut state = policy.write();
                if let Some(lease) = state.lease_mut(&tab) {
                    lease.state = if ready {
                        TabState::Open
                    } else {
                        TabState::Closed
                    };
                }
            }
            // Report before resuming: once the popup runs, observers (the
            // action layer, the UI) must already see it as a tab of ours.
            if ready {
                notify(TabEvent::PopupOpened {
                    tab,
                    opener: parent_tab,
                    owner,
                });
            }
            if waiting {
                resume(&child);
            }
        }
        _ => {
            // Workers and anything else: never ours. Detaching also resumes a
            // paused target.
            detach_sessions(link, std::slice::from_ref(&child));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::PolicyState;
    use super::super::protocol::{ActionKind, BrowserSettings, Limits};
    use super::super::testing::{Event, FakeCdp, Response};
    use super::*;
    use std::sync::Mutex;

    fn policy() -> Arc<Policy> {
        let mut state = PolicyState::new(7, BrowserSettings::default());
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        Arc::new(Policy::new(state))
    }

    fn admit(policy: &Policy, action: ActionKind, tab: Option<&TabId>) -> RequestCtx {
        policy
            .read()
            .admit("s1", "r1", action, tab, Duration::from_secs(5))
            .unwrap()
    }

    /// A fake that answers `Page.navigate` and then fires the lifecycle
    /// events Chrome would.
    fn navigating_fake() -> FakeCdp {
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(1));
        FakeCdp::start_with(move |received| {
            if received.method == "Page.navigate" {
                let session = received.session.as_deref();
                let url = received.params["url"].as_str().unwrap_or("").to_string();
                if url.contains("unreachable") {
                    return Response::Result(
                        json!({ "frameId": "T1", "loaderId": "L2", "errorText": "net::ERR_NAME_NOT_RESOLVED" }),
                    );
                }
                return Response::ResultThen(
                    json!({ "frameId": "T1", "loaderId": "L2" }),
                    vec![
                        Event::new(
                            "Page.frameNavigated",
                            json!({ "frame": { "id": "T1", "loaderId": "L2", "url": url, "securityOrigin": "x", "mimeType": "text/html" }, "type": "Navigation" }),
                            session,
                        ),
                        Event::new(
                            "Page.lifecycleEvent",
                            json!({ "frameId": "T1", "loaderId": "L2", "name": "DOMContentLoaded", "timestamp": 1.0 }),
                            session,
                        ),
                        Event::new(
                            "Page.lifecycleEvent",
                            json!({ "frameId": "T1", "loaderId": "L2", "name": "load", "timestamp": 2.0 }),
                            session,
                        ),
                    ],
                );
            }
            super::super::testing::default_response(received, &counter)
        })
    }

    #[test]
    fn open_tab_creates_attaches_sets_up_and_navigates_in_order() {
        let server = navigating_fake();
        let policy = policy();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let _pump = spawn_pump(
            link.clone(),
            Arc::new(move |e| seen.lock().unwrap().push(e)),
        )
        .unwrap();
        let ctx = admit(&policy, ActionKind::TabsOpen, None);
        let opened = open_tab(
            &link,
            &ctx,
            Some("https://example.com/a?b=1"),
            WaitUntil::Load,
        )
        .unwrap();
        assert!(opened.navigation.is_ok());
        assert!(opened.handle.tab.is_well_formed());
        assert_eq!(opened.handle.target, TargetId("T1".into()));
        assert_eq!(opened.handle.session, CdpSessionId("S2".into()));
        assert_eq!(opened.observed.load_state, Some(LoadState::Load));
        assert!(opened.observed.navigated);
        assert_eq!(
            opened.observed.url.as_deref(),
            Some("https://example.com/a?b=1")
        );
        let methods: Vec<String> = server.received().into_iter().map(|r| r.method).collect();
        assert_eq!(
            methods,
            vec![
                "Target.createTarget",
                "Target.attachToTarget",
                "Page.enable",
                "Page.setLifecycleEventsEnabled",
                "Target.setAutoAttach",
                "Emulation.setFocusEmulationEnabled",
                "Page.addScriptToEvaluateOnNewDocument",
                "Page.navigate",
            ]
        );
        let received = server.received();
        assert_eq!(received[0].params["background"], json!(true));
        assert_eq!(received[0].params["newWindow"], json!(false));
        assert_eq!(received[0].params["url"], json!("about:blank"));
        assert_eq!(received[4].params["waitForDebuggerOnStart"], json!(true));
        assert_eq!(received[4].session.as_deref(), Some("S2"));
        assert_eq!(received[6].params["worldName"], json!(ISOLATED_WORLD));
        assert!(received[6].params["source"]
            .as_str()
            .unwrap()
            .contains("MonoCode"));
        // The lease is open, owned by s1, and the pump saw the navigation.
        {
            let state = policy.read();
            let lease = state.lease(&opened.handle.tab).unwrap();
            assert_eq!(lease.state, TabState::Open);
            assert_eq!(lease.owner, "s1");
            assert_eq!(lease.url, "https://example.com/a?b=1");
            assert_eq!(lease.main_loader_id.as_deref(), Some("L2"));
            assert_eq!(state.tabs_for("s1").len(), 1);
        }
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, TabEvent::Navigated(_))));
        // Tab limit: a second session with limit 1.
        {
            let mut state = policy.write();
            state.settings.limits = Limits {
                tabs_per_session: 1,
                tabs_total: 24,
                concurrent_actions: 4,
            };
        }
        let ctx2 = admit(&policy, ActionKind::TabsOpen, None);
        let err = open_tab(&link, &ctx2, None, WaitUntil::Load).unwrap_err();
        assert!(matches!(err, TabError::Reason(Reason::TabLimit, _)));
        // Denied URL never reaches the socket.
        let before = server.received().len();
        let err = open_tab(&link, &ctx2, Some("file:///etc/passwd"), WaitUntil::Load).unwrap_err();
        assert!(matches!(err, TabError::Reason(Reason::UrlDenied, _)));
        assert_eq!(server.received().len(), before);
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn failed_navigation_keeps_the_tab_and_close_marks_the_lease() {
        let server = navigating_fake();
        let policy = policy();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let ctx = admit(&policy, ActionKind::TabsOpen, None);
        let opened = open_tab(
            &link,
            &ctx,
            Some("https://unreachable.invalid/"),
            WaitUntil::Load,
        )
        .unwrap();
        let (reason, message) = opened.navigation.unwrap_err();
        assert_eq!(reason, Reason::NavigationFailed);
        assert!(message.contains("ERR_NAME_NOT_RESOLVED"));
        let tab = opened.handle.tab.clone();
        assert_eq!(policy.read().lease(&tab).unwrap().state, TabState::Open);
        // Another session cannot close it.
        policy
            .write()
            .turn_started("s2", "main", "/repo", Some(true));
        let foreign = policy.read().admit(
            "s2",
            "r9",
            ActionKind::TabsClose,
            Some(&tab),
            Duration::from_secs(5),
        );
        assert_eq!(foreign.unwrap_err(), Reason::NoLease);
        let close_ctx = admit(&policy, ActionKind::TabsClose, Some(&tab));
        close_tab(&link, &close_ctx, &tab).unwrap();
        assert_eq!(policy.read().lease(&tab).unwrap().state, TabState::Closed);
        assert!(policy.read().tabs_for("s1").is_empty());
        let last = server.received().last().unwrap().clone();
        assert_eq!(last.method, "Target.closeTarget");
        assert_eq!(last.params["targetId"], json!("T1"));
        assert!(matches!(
            close_tab(&link, &close_ctx, &tab),
            Err(TabError::Reason(Reason::TargetClosed, _))
        ));
        policy.write().forget_closed("s1");
        assert!(policy.read().lease(&tab).is_none());
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn attach_refused_by_policy_is_reported_and_the_target_closed() {
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let server = FakeCdp::start_with(move |received| {
            if received.method == "Target.attachToTarget" {
                return Response::Error {
                    code: -32000,
                    message: "Not allowed".into(),
                };
            }
            super::super::testing::default_response(received, &counter)
        });
        let policy = policy();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let ctx = admit(&policy, ActionKind::TabsOpen, None);
        let err = open_tab(&link, &ctx, None, WaitUntil::Load).unwrap_err();
        assert!(matches!(
            err,
            TabError::Reason(Reason::BrowserPolicyBlocked, Some(_))
        ));
        assert!(server.wait_for(
            |log| log.iter().any(|r| r.method == "Target.closeTarget"),
            Duration::from_secs(2)
        ));
        assert!(policy.read().tabs_for("s1").is_empty());
        let result = err.into_result("r1", true, true);
        assert_eq!(result.outcome, Outcome::Blocked);
        assert_eq!(result.reason, Some(Reason::BrowserPolicyBlocked));
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn pump_adopts_popups_detaches_workers_and_tracks_dialogs_and_closes() {
        let server = navigating_fake();
        let policy = policy();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let _pump = spawn_pump(
            link.clone(),
            Arc::new(move |e| seen.lock().unwrap().push(e)),
        )
        .unwrap();
        let ctx = admit(&policy, ActionKind::TabsOpen, None);
        let opened = open_tab(&link, &ctx, None, WaitUntil::Load).unwrap();
        let tab = opened.handle.tab.clone();
        let main = opened.handle.session.0.clone();
        let sent_before = server.received().len();

        // A worker auto-attached under our page: detached, never a lease.
        server.emit(
            "Target.attachedToTarget",
            json!({ "sessionId": "W1", "targetInfo": { "targetId": "TW", "type": "service_worker", "url": "https://x/sw.js", "title": "", "attached": true }, "waitingForDebugger": false }),
            Some(&main),
        );
        assert!(server.wait_for(
            |log| log
                .iter()
                .any(|r| r.method == "Target.detachFromTarget"
                    && r.params["sessionId"] == json!("W1")),
            Duration::from_secs(2)
        ));
        assert_eq!(policy.read().tabs_for("s1").len(), 1);

        // An OOPIF: recorded as a frame of our tab and resumed.
        server.emit(
            "Target.attachedToTarget",
            json!({ "sessionId": "F1", "targetInfo": { "targetId": "FRAME1", "type": "iframe", "url": "https://ads/", "title": "", "attached": true }, "waitingForDebugger": true }),
            Some(&main),
        );
        assert!(server.wait_for(
            |log| log
                .iter()
                .any(|r| r.method == "Runtime.runIfWaitingForDebugger"
                    && r.session.as_deref() == Some("F1")),
            Duration::from_secs(2)
        ));
        {
            let state = policy.read();
            let lease = state.lease(&tab).unwrap();
            assert_eq!(
                lease.frames["FRAME1"].cdp_session,
                CdpSessionId("F1".into())
            );
            assert!(lease.holds_session(&CdpSessionId("F1".into())));
        }

        // A popup of our tab becomes a tab of the same owner, paused until set up.
        server.emit(
            "Target.attachedToTarget",
            json!({ "sessionId": "P1", "targetInfo": { "targetId": "TP", "type": "page", "url": "https://popup/", "title": "Pop", "attached": true, "openerId": "T1" }, "waitingForDebugger": true }),
            Some(&main),
        );
        assert!(server.wait_for(
            |log| log
                .iter()
                .any(|r| r.method == "Runtime.runIfWaitingForDebugger"
                    && r.session.as_deref() == Some("P1")),
            Duration::from_secs(2)
        ));
        let popup = {
            let state = policy.read();
            let tabs = state.tabs_for("s1");
            assert_eq!(tabs.len(), 2);
            let popup = tabs.iter().find(|t| t.tab != tab).unwrap().clone();
            assert_eq!(popup.opened_by.as_ref(), Some(&tab));
            assert_eq!(popup.url, "https://popup/");
            assert_eq!(popup.state, TabState::Open);
            popup.tab
        };
        // Setup happened before the resume, inside the popup's session.
        let log = server.received();
        let setup_idx = log
            .iter()
            .position(|r| {
                r.method == "Page.addScriptToEvaluateOnNewDocument"
                    && r.session.as_deref() == Some("P1")
            })
            .unwrap();
        let resume_idx = log
            .iter()
            .position(|r| {
                r.method == "Runtime.runIfWaitingForDebugger" && r.session.as_deref() == Some("P1")
            })
            .unwrap();
        assert!(setup_idx < resume_idx);
        let saw_popup = || {
            events.lock().unwrap().iter().any(|e| {
                matches!(e, TabEvent::PopupOpened { opener, owner, .. } if *opener == tab && owner == "s1")
            })
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !saw_popup() {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(saw_popup(), "PopupOpened event for the popup of {tab:?}");

        // Over the limit: the next popup is closed before its first script.
        {
            let mut state = policy.write();
            state.settings.limits.tabs_per_session = 2;
        }
        server.emit(
            "Target.attachedToTarget",
            json!({ "sessionId": "P2", "targetInfo": { "targetId": "TP2", "type": "page", "url": "https://popup2/", "title": "", "attached": true, "openerId": "T1" }, "waitingForDebugger": true }),
            Some(&main),
        );
        assert!(server.wait_for(
            |log| log
                .iter()
                .any(|r| r.method == "Target.closeTarget" && r.params["targetId"] == json!("TP2")),
            Duration::from_secs(2)
        ));
        assert_eq!(policy.read().lease(&tab).unwrap().popups_blocked, 1);
        assert_eq!(policy.read().tabs_for("s1").len(), 2);

        // Dialog state follows the events.
        server.emit(
            "Page.javascriptDialogOpening",
            json!({ "url": "https://example.com/", "message": "Sure?", "type": "confirm", "hasBrowserHandler": false }),
            Some(&main),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && policy.read().lease(&tab).unwrap().pending_dialog.is_none()
        {
            thread::sleep(Duration::from_millis(5));
        }
        let dialog = policy
            .read()
            .lease(&tab)
            .unwrap()
            .pending_dialog
            .clone()
            .unwrap();
        assert_eq!(dialog.kind, DialogType::Confirm);
        assert_eq!(dialog.message, "Sure?");
        let blocked = policy.read().admit(
            "s1",
            "r2",
            ActionKind::Click,
            Some(&tab),
            Duration::from_secs(1),
        );
        assert_eq!(blocked.unwrap_err(), Reason::DialogPending);
        server.emit(
            "Page.javascriptDialogClosed",
            json!({ "result": true, "userInput": "" }),
            Some(&main),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && policy.read().lease(&tab).unwrap().pending_dialog.is_some()
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(policy.read().lease(&tab).unwrap().pending_dialog.is_none());

        // The user closes the popup: the lease is closed and reported once.
        server.emit(
            "Target.detachedFromTarget",
            json!({ "sessionId": "P1", "targetId": "TP" }),
            None,
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && policy.read().lease(&popup).unwrap().state != TabState::Closed
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(policy.read().lease(&popup).unwrap().state, TabState::Closed);
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|e| *e == TabEvent::Closed(popup.clone())));
        // A crash of the main tab.
        server.emit("Inspector.targetCrashed", json!({}), Some(&main));
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && policy.read().lease(&tab).unwrap().state != TabState::Crashed
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(policy.read().lease(&tab).unwrap().state, TabState::Crashed);
        assert!(server.received().len() > sent_before);
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn wait_for_load_honours_wait_until_and_dialogs() {
        let server = FakeCdp::start();
        let policy = policy();
        let link = Link::new(policy);
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let session = CdpSessionId("S1".into());
        let sub = link.subscribe(EventFilter::session(&session));
        server.emit(
            "Page.lifecycleEvent",
            json!({ "frameId": "F", "loaderId": "OLD", "name": "load" }),
            Some("S1"),
        );
        server.emit(
            "Page.lifecycleEvent",
            json!({ "frameId": "F", "loaderId": "NEW", "name": "DOMContentLoaded" }),
            Some("S1"),
        );
        let (state, dialog) = wait_for_load(
            &sub,
            Some("NEW"),
            WaitUntil::Domcontentloaded,
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(state, LoadState::Domcontentloaded);
        assert!(dialog.is_none());
        // Waiting for `load` with only DOMContentLoaded seen reports that on timeout.
        let sub = link.subscribe(EventFilter::session(&session));
        server.emit(
            "Page.lifecycleEvent",
            json!({ "frameId": "F", "loaderId": "NEW", "name": "DOMContentLoaded" }),
            Some("S1"),
        );
        let (state, _) = wait_for_load(
            &sub,
            Some("NEW"),
            WaitUntil::Load,
            Instant::now() + Duration::from_millis(300),
        );
        assert_eq!(state, LoadState::Domcontentloaded);
        let sub = link.subscribe(EventFilter::session(&session));
        server.emit(
            "Page.javascriptDialogOpening",
            json!({ "url": "https://x/", "message": "hi", "type": "alert" }),
            Some("S1"),
        );
        let (state, dialog) = wait_for_load(
            &sub,
            None,
            WaitUntil::Load,
            Instant::now() + Duration::from_secs(2),
        );
        assert_eq!(state, LoadState::Timeout);
        assert_eq!(dialog.unwrap().kind, DialogType::Alert);
        let sub = link.subscribe(EventFilter::session(&session));
        let (state, _) = wait_for_load(
            &sub,
            None,
            WaitUntil::Load,
            Instant::now() + Duration::from_millis(50),
        );
        assert_eq!(state, LoadState::Timeout);
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn handle_for_enforces_owner_and_state() {
        let policy = policy();
        let tab = {
            let mut state = policy.write();
            let lease = new_lease(
                &state,
                &TargetId("T1".into()),
                Some(CdpSessionId("S1".into())),
                "s1",
                None,
                "about:blank",
                "",
            );
            state.insert_lease(lease)
        };
        assert_eq!(
            handle_for(&policy.read(), "s1", &tab).unwrap_err(),
            Reason::Busy
        );
        policy.write().lease_mut(&tab).unwrap().state = TabState::Open;
        let handle = handle_for(&policy.read(), "s1", &tab).unwrap();
        assert_eq!(handle.session, CdpSessionId("S1".into()));
        assert_eq!(
            handle_for(&policy.read(), "s2", &tab).unwrap_err(),
            Reason::NoLease
        );
        policy.write().lease_mut(&tab).unwrap().state = TabState::Revoked;
        assert_eq!(
            handle_for(&policy.read(), "s1", &tab).unwrap_err(),
            Reason::NoLease
        );
        policy.write().lease_mut(&tab).unwrap().state = TabState::Crashed;
        assert_eq!(
            handle_for(&policy.read(), "s1", &tab).unwrap_err(),
            Reason::TargetClosed
        );
        assert!(MARKER_JS.contains("addScriptToEvaluateOnNewDocument"));
        assert!(MARKER_JS.contains("globalThis.__monocodeMarkerRelease = "));
        assert!(MARKER_RELEASE_FN.contains("__monocodeMarkerRelease"));
        assert!(MARKER_JS.contains(super::super::protocol::MARKER_PREFIX));
    }
}
