//! What happened after the input (spec 9.2 steps 8-9, 10.1, 10.4).
//!
//! A [`Watch`] subscribes to the tab's events *before* the first input
//! command, sends input through [`Watch::input`] and then gives the page a
//! short window: a main-frame navigation turns into a wait for `load` within
//! the action's remaining budget, a JavaScript dialog ends the action as
//! `done` with `observed.dialog` (never touched, decision D6), a closed tab
//! is `target_closed`, and a popup of this tab is reported as
//! `observed.new_tab` (or `popup_blocked` when the tab limit closed it).
//!
//! Input commands can block in the browser while a dialog they triggered is
//! open (the `click` handler calls `confirm()`): the response only arrives
//! once the dialog is answered. [`Watch::input`] therefore waits for either
//! the response or `Page.javascriptDialogOpening`, whichever comes first.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::super::link::{CdpEvent, CdpSessionId, Command, EventFilter, Method, Subscription};
use super::super::policy::now_ms;
use super::super::protocol::{
    strip_marker, DialogInfo, DialogType, LoadState, NewTabInfo, Observed, Reason, TabId, TabState,
    WaitUntil,
};
use super::super::targets;
use super::element::{self, PAGE_JS};
use super::{refs, Fail, Run};

/// Observation window after input (spec 9.2 step 8: up to 500 ms).
pub const WINDOW: Duration = Duration::from_millis(500);
/// Shorter window for actions that rarely start navigations (hover, scroll).
pub const SHORT_WINDOW: Duration = Duration::from_millis(150);
/// Extra time for the event pump to register a popup we saw attach.
const POPUP_GRACE: Duration = Duration::from_millis(300);
/// DOM mutation records after which a snapshot is reported as probably stale.
const STALE_MUTATIONS: u64 = 20;

pub struct Watch {
    session: Subscription,
    browser: Subscription,
    backlog: VecDeque<CdpEvent>,
    popups_before: Vec<TabId>,
    blocked_before: u32,
    dialog: Option<DialogInfo>,
    count_mutations: bool,
}

pub fn dialog_info(params: &Value) -> DialogInfo {
    let kind = match params["type"].as_str() {
        Some("confirm") => DialogType::Confirm,
        Some("prompt") => DialogType::Prompt,
        Some("beforeunload") => DialogType::Beforeunload,
        _ => DialogType::Alert,
    };
    DialogInfo {
        kind,
        message: params["message"].as_str().unwrap_or("").to_string(),
        default_prompt: params["defaultPrompt"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        url: params["url"].as_str().unwrap_or("").to_string(),
        opened_at_ms: now_ms(),
    }
}

impl Watch {
    /// Subscribe before any input is sent.
    pub fn start(run: &Run<'_>) -> Watch {
        let session = run.link.subscribe(EventFilter::session(run.main()));
        let browser = run.link.subscribe(EventFilter {
            session: None,
            methods: vec!["Target.detachedFromTarget", "Target.targetDestroyed"],
        });
        let (popups_before, blocked_before) = {
            let state = run.link.policy().read();
            let popups = state
                .leases()
                .filter(|l| l.opened_by.as_ref() == Some(run.tab()))
                .map(|l| l.tab.clone())
                .collect();
            let blocked = state.lease(run.tab()).map_or(0, |l| l.popups_blocked);
            (popups, blocked)
        };
        Watch {
            session,
            browser,
            backlog: VecDeque::new(),
            popups_before,
            blocked_before,
            dialog: None,
            count_mutations: false,
        }
    }

    /// Reset the page's DOM mutation counter so [`Watch::finish`] can tell a
    /// large DOM change (spec 9.2 step 9, `snapshot_stale`). Best effort.
    pub fn count_mutations(&mut self, run: &Run<'_>) {
        if let Ok(frame) = element::main_frame(run) {
            self.count_mutations = element::call_frame(
                run,
                &frame,
                PAGE_JS,
                &[element::arg(json!("mutations"))],
                false,
            )
            .is_ok();
        }
    }

    /// A dialog opened while input was being sent.
    pub fn dialog(&self) -> Option<&DialogInfo> {
        self.dialog.as_ref()
    }

    /// Send one page-changing command to `session`. `Ok(None)` when a dialog
    /// opened before the browser answered (the page is frozen; nothing more
    /// is sent), or when a dialog was already open.
    pub fn input(
        &mut self,
        run: &Run<'_>,
        session: &CdpSessionId,
        method: Method,
        params: Value,
    ) -> Result<Option<Value>, Fail> {
        if self.dialog.is_some() {
            return Ok(None);
        }
        run.check_time()?;
        run.mutated.set(true);
        let (tx, rx) = mpsc::channel();
        let command = Command::in_session(session, method, params);
        {
            let link = run.link.clone();
            let ctx = run.ctx.clone();
            let for_thread = command.clone();
            let spawned = thread::Builder::new()
                .name("browser-input".into())
                .spawn(move || {
                    let _ = tx.send(link.send(&ctx, for_thread));
                });
            if spawned.is_err() {
                // No thread: send inline and accept that a dialog may delay it.
                return run
                    .link
                    .send(run.ctx, command)
                    .map(Some)
                    .map_err(|e| Fail::Cdp(e, method.name()));
            }
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => return result.map(Some).map_err(|e| Fail::Cdp(e, method.name())),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Fail::Cdp(
                        super::super::link::CdpError::LinkDown { sent: true },
                        method.name(),
                    ))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            while let Some(event) = self.session.try_recv() {
                let opened = event.method == "Page.javascriptDialogOpening";
                if opened {
                    self.dialog = Some(dialog_info(&event.params));
                }
                self.backlog.push_back(event);
                if opened {
                    return Ok(None);
                }
            }
        }
    }

    fn next_event(&mut self, until: Instant) -> Option<CdpEvent> {
        if let Some(event) = self.backlog.pop_front() {
            return Some(event);
        }
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        self.session
            .recv_timeout(remaining.min(Duration::from_millis(25)))
            .or_else(|| self.browser.try_recv())
    }

    /// Watch the page for up to `window` (bounded by the action deadline)
    /// and assemble `observed`.
    pub fn finish(mut self, run: &Run<'_>, window: Duration) -> Result<Observed, Fail> {
        let main_frame = run.lease(|l| l.main_frame_id.clone())?;
        let is_main = |frame: Option<&str>| match (frame, main_frame.as_deref()) {
            (Some(frame), Some(main)) => frame == main,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let until = (Instant::now() + window).min(run.ctx.deadline);
        let mut navigating = false;
        let mut navigated = false;
        let mut same_document = false;
        let mut loaded = false;
        let mut closed = false;
        let mut popup_seen = false;
        while self.dialog.is_none() && !closed {
            let deadline = if navigating && !loaded {
                run.ctx.deadline
            } else {
                until
            };
            let Some(event) = self.next_event(deadline) else {
                if Instant::now() >= deadline {
                    break;
                }
                if self.session.is_closed() {
                    break;
                }
                continue;
            };
            let params = &event.params;
            let main_frame_event = is_main(params["frameId"].as_str());
            match event.method.as_str() {
                "Page.frameRequestedNavigation"
                    if main_frame_event
                        && params["disposition"]
                            .as_str()
                            .is_none_or(|d| d == "currentTab") =>
                {
                    navigating = true;
                }
                "Page.frameStartedNavigating" | "Page.frameStartedLoading" if main_frame_event => {
                    navigating = true;
                }
                "Page.frameNavigated"
                    if params["frame"]["parentId"]
                        .as_str()
                        .is_none_or(str::is_empty) =>
                {
                    // Into the lease now, not when the pump gets to it: the
                    // snapshot after this action must see the new loader.
                    targets::note_frame_navigated(run.link, &event);
                    navigating = true;
                    navigated = true;
                }
                "Page.navigatedWithinDocument" if main_frame_event => {
                    navigated = true;
                    same_document = true;
                    if !navigating {
                        loaded = true;
                    }
                }
                "Page.lifecycleEvent"
                    if navigating && main_frame_event && params["name"] == "load" =>
                {
                    loaded = true;
                }
                "Page.frameStoppedLoading" if navigating && main_frame_event => loaded = true,
                "Page.loadEventFired" if navigating => loaded = true,
                "Page.javascriptDialogOpening" => self.dialog = Some(dialog_info(params)),
                "Inspector.targetCrashed" => closed = true,
                "Target.attachedToTarget" if params["targetInfo"]["type"] == "page" => {
                    popup_seen = true;
                }
                "Target.detachedFromTarget"
                    if params["sessionId"].as_str() == Some(run.main().0.as_str()) =>
                {
                    closed = true;
                }
                "Target.targetDestroyed"
                    if params["targetId"].as_str() == Some(run.handle.target.0.as_str()) =>
                {
                    closed = true;
                }
                _ => {}
            }
            if navigating && loaded && Instant::now() >= until {
                break;
            }
        }
        let mut load_state = None;
        if navigating {
            navigated = true;
            load_state = Some(if loaded {
                LoadState::Load
            } else if self.dialog.is_none() && !closed {
                let (state, dialog) =
                    targets::wait_for_load(&self.session, None, WaitUntil::Load, run.ctx.deadline);
                if dialog.is_some() {
                    self.dialog = dialog;
                }
                state
            } else {
                LoadState::Timeout
            });
        }
        let state_closed = run
            .lease(|l| matches!(l.state, TabState::Closed | TabState::Crashed))
            .unwrap_or(true);
        if closed || state_closed {
            return Err(Fail::reason(Reason::TargetClosed));
        }
        if popup_seen {
            let grace = Instant::now() + POPUP_GRACE;
            while Instant::now() < grace && self.new_tab(run).is_none() && !self.blocked(run) {
                thread::sleep(Duration::from_millis(10));
            }
        }
        if navigated && !same_document {
            refs::clear(&run.key());
        }
        let mut stale = navigated;
        if self.count_mutations && !navigated && self.dialog.is_none() {
            if let Ok(frame) = element::main_frame(run) {
                if let Ok(value) = element::call_frame(
                    run,
                    &frame,
                    PAGE_JS,
                    &[element::arg(json!("mutations"))],
                    false,
                ) {
                    stale = value["count"].as_u64().unwrap_or(0) >= STALE_MUTATIONS;
                }
            }
        }
        let dialog = self
            .dialog
            .clone()
            .or_else(|| run.lease(|l| l.pending_dialog.clone()).ok().flatten());
        let (url, title) = run.lease(|l| (l.url.clone(), strip_marker(&l.title).to_string()))?;
        Ok(Observed {
            url: Some(url),
            title: Some(title),
            navigated,
            load_state,
            snapshot_stale: stale,
            dialog,
            new_tab: self.new_tab(run),
            popup_blocked: self.blocked(run),
        })
    }

    fn new_tab(&self, run: &Run<'_>) -> Option<NewTabInfo> {
        let state = run.link.policy().read();
        state
            .leases()
            .filter(|l| {
                l.opened_by.as_ref() == Some(run.tab())
                    && l.is_live()
                    && !self.popups_before.contains(&l.tab)
            })
            .min_by_key(|l| l.created_at_ms)
            .map(|l| NewTabInfo {
                tab: l.tab.clone(),
                url: l.url.clone(),
            })
    }

    fn blocked(&self, run: &Run<'_>) -> bool {
        run.link
            .policy()
            .read()
            .lease(run.tab())
            .is_some_and(|l| l.popups_blocked > self.blocked_before)
    }
}

/// One-line summary of what followed, appended to `done` messages.
pub fn summary(observed: &Observed) -> String {
    let mut parts = Vec::new();
    if let Some(dialog) = &observed.dialog {
        parts.push(format!(
            "A {} dialog opened (\"{}\"); answer it with browser_dialog.",
            match dialog.kind {
                DialogType::Alert => "alert",
                DialogType::Confirm => "confirm",
                DialogType::Prompt => "prompt",
                DialogType::Beforeunload => "beforeunload",
            },
            dialog.message.chars().take(120).collect::<String>()
        ));
    }
    if observed.navigated {
        let url = observed.url.as_deref().unwrap_or("");
        match observed.load_state {
            Some(LoadState::Timeout) => {
                parts.push(format!("The page navigated to {url} and is still loading."))
            }
            _ => parts.push(format!("The page navigated to {url}.")),
        }
    }
    if let Some(tab) = &observed.new_tab {
        parts.push(format!(
            "It opened a new tab {} ({}); pass tab: \"{}\" to use it.",
            tab.tab.0, tab.url, tab.tab.0
        ));
    }
    if observed.popup_blocked {
        parts.push("A popup was closed because the tab limit was reached.".into());
    }
    if observed.snapshot_stale && observed.dialog.is_none() {
        parts.push("Refs may be stale; take a new snapshot.".into());
    }
    parts.join(" ")
}
