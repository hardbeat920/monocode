//! Authority of the browser module: global toggle, epochs, per-session grants
//! bound to the active turn, per-tab leases, request-id dedup, limits, and the
//! URL and upload-path policies (spec 6, 7, 11.4, 15).
//!
//! `PolicyState` is the single source of truth. The `send()` gate in `link.rs`
//! consults it under a read lock right before every socket write; every
//! revocation (OFF, connection loss, turn end, session revoke) takes the write
//! lock and bumps an epoch, so nothing admitted under the old epoch can be
//! written afterwards (spec 6, "Garancije").
//!
//! The lease table maps the agent-facing `TabId` to CDP identifiers that the
//! agent never sees.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::link::{CdpSessionId, TargetId, TargetRef};
use super::protocol::{
    ActionKind, ActionResult, BrowserSettings, DialogInfo, Epoch, Reason, SessionAccess, TabId,
    TabInfo, TabOverview, TabState,
};

/// Dedup ring size per session and entry lifetime (spec 11.4).
pub const DEDUP_RING: usize = 500;
pub const DEDUP_TTL: Duration = Duration::from_secs(24 * 3600);
/// Largest file `upload` may hand to the browser (spec 10.2).
pub const UPLOAD_MAX_BYTES: u64 = 100 * 1024 * 1024;
/// How long a request waits for a busy tab or a free action slot (spec 9.6).
pub const BUSY_WAIT: Duration = Duration::from_secs(5);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Everything a request carries from admission to its last `send()`.
#[derive(Clone, Debug)]
pub struct RequestCtx {
    pub session: String,
    pub window: String,
    pub request_id: String,
    pub action: ActionKind,
    pub global_epoch: Epoch,
    pub session_epoch: Epoch,
    pub tab: Option<TabId>,
    pub started: Instant,
    pub deadline: Instant,
}

impl RequestCtx {
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Why the gate refused a command. `link.rs` maps this onto `CdpError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateError {
    Off,
    Revoked,
    TurnInactive,
    NoLease,
    NotAllowed,
}

#[derive(Clone, Debug)]
pub struct Turn {
    pub window: String,
}

#[derive(Clone, Debug)]
pub struct SessionAuth {
    pub window: String,
    pub cwd: String,
    /// 64 hex characters, injected into the provider process environment.
    pub token: String,
    pub epoch: Epoch,
    pub granted: bool,
    pub granted_since_ms: Option<i64>,
    pub turn: Option<Turn>,
}

#[derive(Clone, Debug)]
pub struct FrameSession {
    pub cdp_session: CdpSessionId,
    pub loader_id: Option<String>,
    /// Execution context of our isolated world in this frame, if created.
    pub execution_context_id: Option<i64>,
}

/// One row of the lease table (spec 7).
#[derive(Clone, Debug)]
pub struct Lease {
    pub tab: TabId,
    pub target_id: TargetId,
    /// CDP session of the main frame; `None` while attaching.
    pub cdp_session: Option<CdpSessionId>,
    pub owner: String,
    pub state: TabState,
    pub main_frame_id: Option<String>,
    pub main_loader_id: Option<String>,
    pub main_execution_context: Option<i64>,
    /// OOPIF frames: CDP `frameId` → child session.
    pub frames: HashMap<String, FrameSession>,
    pub busy: bool,
    pub snapshot_version: u64,
    pub pending_dialog: Option<DialogInfo>,
    pub opened_by: Option<TabId>,
    /// Popups of this tab closed because a tab limit was hit (spec 10.4);
    /// the action layer compares before/after to set `observed.popup_blocked`.
    pub popups_blocked: u32,
    pub url: String,
    pub title: String,
    pub created_at_ms: i64,
    pub last_action_at_ms: i64,
}

impl Lease {
    pub fn is_live(&self) -> bool {
        matches!(self.state, TabState::Attaching | TabState::Open)
    }

    pub fn holds_session(&self, session: &CdpSessionId) -> bool {
        self.cdp_session.as_ref() == Some(session)
            || self.frames.values().any(|f| &f.cdp_session == session)
    }

    pub fn info(&self) -> TabInfo {
        TabInfo {
            tab: self.tab.clone(),
            url: self.url.clone(),
            title: super::protocol::strip_marker(&self.title).to_string(),
            state: self.state,
            opened_by: self.opened_by.clone(),
            dialog_pending: self.pending_dialog.is_some(),
            busy: self.busy,
            created_at_ms: self.created_at_ms,
            last_action_at_ms: self.last_action_at_ms,
        }
    }
}

#[derive(Clone, Debug)]
struct DedupEntry {
    request_id: String,
    input_hash: u64,
    result: ActionResult,
    at: Instant,
}

pub enum Dedup {
    Fresh,
    Cached(Box<ActionResult>),
    Conflict,
}

/// The authority state. Mutate it only through the methods below so every
/// transition bumps the epochs the spec requires.
pub struct PolicyState {
    pub toggle_on: bool,
    pub run_id: u64,
    pub global_epoch: Epoch,
    pub settings: BrowserSettings,
    pub active_actions: u32,
    sessions: HashMap<String, SessionAuth>,
    leases: HashMap<TabId, Lease>,
    dedup: HashMap<String, VecDeque<DedupEntry>>,
    /// When the last revocation took effect; the race tests compare write
    /// timestamps against it.
    pub last_revocation: Option<Instant>,
}

impl PolicyState {
    pub fn new(run_id: u64, settings: BrowserSettings) -> Self {
        PolicyState {
            toggle_on: false,
            run_id,
            global_epoch: Epoch {
                run: run_id,
                seq: 0,
            },
            settings,
            active_actions: 0,
            sessions: HashMap::new(),
            leases: HashMap::new(),
            dedup: HashMap::new(),
            last_revocation: None,
        }
    }

    // --- identity -----------------------------------------------------------

    /// Create or refresh the session's auth record; the token is stable for
    /// the life of the session (spec 13.2: every session gets one at spawn).
    pub fn prepare_session(&mut self, session: &str, window: &str, cwd: &str) -> String {
        let entry = self
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionAuth {
                window: window.to_string(),
                cwd: cwd.to_string(),
                token: new_token(),
                epoch: Epoch {
                    run: self.run_id,
                    seq: 0,
                },
                granted: false,
                granted_since_ms: None,
                turn: None,
            });
        entry.window = window.to_string();
        entry.cwd = cwd.to_string();
        entry.token.clone()
    }

    pub fn token_for(&self, session: &str) -> Option<&str> {
        self.sessions.get(session).map(|auth| auth.token.as_str())
    }

    /// Constant-time token lookup; returns the session id.
    pub fn session_for_token(&self, token: &str) -> Option<String> {
        let mut found = None;
        for (id, auth) in &self.sessions {
            if constant_time_eq(auth.token.as_bytes(), token.as_bytes()) {
                found = Some(id.clone());
            }
        }
        found
    }

    pub fn session(&self, session: &str) -> Option<&SessionAuth> {
        self.sessions.get(session)
    }

    pub fn session_epoch(&self, session: &str) -> Option<Epoch> {
        self.sessions.get(session).map(|auth| auth.epoch)
    }

    pub fn turn_active(&self, session: &str) -> bool {
        self.sessions
            .get(session)
            .is_some_and(|auth| auth.turn.is_some())
    }

    pub fn granted(&self, session: &str) -> bool {
        self.sessions.get(session).is_some_and(|auth| auth.granted)
    }

    // --- transitions (spec 6, table "Toggle i epoch") --------------------------

    /// OFF→ON or ON→OFF. Returns the main CDP sessions whose tabs the caller
    /// must detach (OFF only). Tabs stay open (decision D4).
    pub fn set_toggle(&mut self, on: bool) -> Vec<CdpSessionId> {
        if self.toggle_on == on {
            return Vec::new();
        }
        self.toggle_on = on;
        self.global_epoch = self.global_epoch.next();
        self.last_revocation = Some(Instant::now());
        if on {
            return Vec::new();
        }
        // OFF revokes every permission (spec 6, 13.1): after the next ON each
        // session has to be granted `/browser` again.
        for auth in self.sessions.values_mut() {
            auth.granted = false;
            auth.granted_since_ms = None;
            auth.epoch = auth.epoch.next();
        }
        let mut detach = Vec::new();
        for lease in self.leases.values_mut() {
            if lease.is_live() {
                lease.state = TabState::Revoked;
                lease.busy = false;
                if let Some(session) = lease.cdp_session.clone() {
                    detach.push(session);
                }
            }
        }
        detach
    }

    /// Settings "Take the tab back" (spec 13.1): the lease ends, the tab stays
    /// with the user. Returns the main CDP session to detach, if attached.
    pub fn release_lease(&mut self, tab: &TabId) -> Result<Option<CdpSessionId>, Reason> {
        let lease = self.leases.get_mut(tab).ok_or(Reason::NoLease)?;
        match lease.state {
            TabState::Closed | TabState::Crashed => return Err(Reason::TargetClosed),
            TabState::Revoked => return Ok(None),
            TabState::Attaching | TabState::Open => {}
        }
        lease.state = TabState::Revoked;
        lease.busy = false;
        self.last_revocation = Some(Instant::now());
        Ok(lease.cdp_session.clone())
    }

    /// The socket closed while ON: epoch up, leases gone (spec 4, 6).
    pub fn connection_lost(&mut self) {
        self.global_epoch = self.global_epoch.next();
        self.last_revocation = Some(Instant::now());
        self.leases.clear();
        self.active_actions = 0;
    }

    /// Turn start: the session gets (or keeps) its token; `browser_access`
    /// sets the per-session grant when the frontend passes it.
    pub fn turn_started(
        &mut self,
        session: &str,
        window: &str,
        cwd: &str,
        browser_access: Option<bool>,
    ) {
        self.prepare_session(session, window, cwd);
        let auth = self.sessions.get_mut(session).expect("prepared above");
        auth.turn = Some(Turn {
            window: window.to_string(),
        });
        let revoke = match browser_access {
            Some(true) if !auth.granted => {
                auth.granted = true;
                auth.granted_since_ms = Some(now_ms());
                false
            }
            // Dropping the grant mid-session is a revocation (decision D2).
            Some(false) if auth.granted => true,
            _ => false,
        };
        if revoke {
            self.revoke_session(session);
        }
    }

    /// Turn end: in-flight actions of the session become `unknown`
    /// (`turn_inactive`), leases and tabs stay (spec 6).
    pub fn turn_finished(&mut self, session: &str) {
        if let Some(auth) = self.sessions.get_mut(session) {
            auth.turn = None;
            auth.epoch = auth.epoch.next();
            self.last_revocation = Some(Instant::now());
        }
    }

    /// `/browser` on or off for one session (decision D2).
    pub fn set_grant(&mut self, session: &str, window: &str, enabled: bool) -> Vec<CdpSessionId> {
        if !enabled {
            return self.revoke_session(session);
        }
        let auth = self
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionAuth {
                window: window.to_string(),
                cwd: String::new(),
                token: new_token(),
                epoch: Epoch {
                    run: self.run_id,
                    seq: 0,
                },
                granted: false,
                granted_since_ms: None,
                turn: None,
            });
        if !auth.granted {
            auth.granted = true;
            auth.granted_since_ms = Some(now_ms());
        }
        Vec::new()
    }

    /// "Oduzmi browser pristup": epoch up, grant gone, leases released, tabs
    /// stay open. Returns the CDP sessions to detach.
    pub fn revoke_session(&mut self, session: &str) -> Vec<CdpSessionId> {
        self.last_revocation = Some(Instant::now());
        if let Some(auth) = self.sessions.get_mut(session) {
            auth.epoch = auth.epoch.next();
            auth.granted = false;
            auth.granted_since_ms = None;
        }
        let mut detach = Vec::new();
        for lease in self.leases.values_mut() {
            if lease.owner == session && lease.is_live() {
                lease.state = TabState::Revoked;
                lease.busy = false;
                if let Some(cdp) = lease.cdp_session.clone() {
                    detach.push(cdp);
                }
            }
        }
        detach
    }

    /// Session deleted: like revoke, plus its tabs are closed. Returns the
    /// targets to close.
    pub fn session_deleted(&mut self, session: &str) -> Vec<TargetId> {
        self.revoke_session(session);
        self.sessions.remove(session);
        self.dedup.remove(session);
        let mut close = Vec::new();
        for lease in self.leases.values_mut() {
            if lease.owner == session && lease.state != TabState::Closed {
                lease.state = TabState::Closed;
                close.push(lease.target_id.clone());
            }
        }
        close
    }

    /// App window closed: its sessions lose grants and leases; tabs stay.
    pub fn window_closed(&mut self, window: &str) -> Vec<CdpSessionId> {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, auth)| auth.window == window)
            .map(|(id, _)| id.clone())
            .collect();
        let mut detach = Vec::new();
        for id in ids {
            detach.extend(self.revoke_session(&id));
            self.sessions.remove(&id);
            self.dedup.remove(&id);
        }
        detach
    }

    /// Hand a tab to another session (spec 7, "Primopredaja").
    pub fn transfer(&mut self, tab: &TabId, from: &str, to: &str) -> Result<(), Reason> {
        if !self.granted(to) {
            return Err(Reason::NotGranted);
        }
        let lease = self.leases.get_mut(tab).ok_or(Reason::NoLease)?;
        if lease.owner != from || !lease.is_live() {
            return Err(Reason::NoLease);
        }
        lease.owner = to.to_string();
        lease.snapshot_version += 1;
        lease.busy = false;
        if let Some(auth) = self.sessions.get_mut(from) {
            auth.epoch = auth.epoch.next();
        }
        self.last_revocation = Some(Instant::now());
        Ok(())
    }

    // --- admission (spec 6 "Životni ciklus zahteva", 9.2 step 1) --------------

    /// Admit a request: toggle, grant, turn, lease, dialog. The connection
    /// state and the busy flag are the host's concern.
    pub fn admit(
        &self,
        session: &str,
        request_id: &str,
        action: ActionKind,
        tab: Option<&TabId>,
        timeout: Duration,
    ) -> Result<RequestCtx, Reason> {
        if !self.toggle_on {
            return Err(Reason::Off);
        }
        let auth = self.sessions.get(session).ok_or(Reason::NotGranted)?;
        if !auth.granted {
            return Err(Reason::NotGranted);
        }
        let turn = auth.turn.as_ref().ok_or(Reason::TurnInactive)?;
        if action.requires_lease() {
            let tab = tab.ok_or(Reason::InvalidRequest)?;
            let lease = self.leases.get(tab).ok_or(Reason::NoLease)?;
            if lease.owner != session {
                return Err(Reason::NoLease);
            }
            match lease.state {
                TabState::Closed | TabState::Crashed => return Err(Reason::TargetClosed),
                TabState::Revoked => return Err(Reason::NoLease),
                TabState::Attaching => return Err(Reason::Busy),
                TabState::Open => {}
            }
            if lease.pending_dialog.is_some() && action != ActionKind::Dialog {
                return Err(Reason::DialogPending);
            }
        }
        let started = Instant::now();
        Ok(RequestCtx {
            session: session.to_string(),
            window: turn.window.clone(),
            request_id: request_id.to_string(),
            action,
            global_epoch: self.global_epoch,
            session_epoch: auth.epoch,
            tab: tab.cloned(),
            started,
            deadline: started + timeout,
        })
    }

    /// The `send()` gate (spec 5). Called under the read lock immediately
    /// before the socket write; a command that fails here never leaves the
    /// process.
    pub fn gate(
        &self,
        ctx: &RequestCtx,
        target: &TargetRef,
        browser_level_allowed: bool,
    ) -> Result<(), GateError> {
        if !self.toggle_on {
            return Err(GateError::Off);
        }
        let auth = self.sessions.get(&ctx.session).ok_or(GateError::Revoked)?;
        if ctx.global_epoch != self.global_epoch || ctx.session_epoch != auth.epoch {
            return Err(GateError::Revoked);
        }
        if auth.turn.is_none() {
            return Err(GateError::TurnInactive);
        }
        match target {
            TargetRef::Browser => {
                if browser_level_allowed {
                    Ok(())
                } else {
                    Err(GateError::NotAllowed)
                }
            }
            TargetRef::Session(session) => {
                let held = self
                    .leases
                    .values()
                    .any(|l| l.owner == ctx.session && l.is_live() && l.holds_session(session));
                if held {
                    Ok(())
                } else {
                    Err(GateError::NoLease)
                }
            }
            TargetRef::Target(target_id) => {
                let held = self.leases.values().any(|l| {
                    l.owner == ctx.session
                        && l.state != TabState::Closed
                        && &l.target_id == target_id
                });
                if held {
                    Ok(())
                } else {
                    Err(GateError::NoLease)
                }
            }
        }
    }

    // --- leases --------------------------------------------------------------

    pub fn lease(&self, tab: &TabId) -> Option<&Lease> {
        self.leases.get(tab)
    }

    pub fn lease_mut(&mut self, tab: &TabId) -> Option<&mut Lease> {
        self.leases.get_mut(tab)
    }

    pub fn lease_for_cdp_session(&self, session: &CdpSessionId) -> Option<&Lease> {
        self.leases.values().find(|l| l.holds_session(session))
    }

    pub fn lease_for_cdp_session_mut(&mut self, session: &CdpSessionId) -> Option<&mut Lease> {
        self.leases.values_mut().find(|l| l.holds_session(session))
    }

    pub fn lease_for_target(&self, target: &TargetId) -> Option<&Lease> {
        self.leases.values().find(|l| &l.target_id == target)
    }

    pub fn lease_for_target_mut(&mut self, target: &TargetId) -> Option<&mut Lease> {
        self.leases.values_mut().find(|l| &l.target_id == target)
    }

    pub fn leases(&self) -> impl Iterator<Item = &Lease> {
        self.leases.values()
    }

    pub fn live_tabs(&self, session: &str) -> u32 {
        self.leases
            .values()
            .filter(|l| l.owner == session && l.is_live())
            .count() as u32
    }

    pub fn live_tabs_total(&self) -> u32 {
        self.leases.values().filter(|l| l.is_live()).count() as u32
    }

    /// Limit check before `Target.createTarget` (decision D5).
    pub fn can_open_tab(&self, session: &str) -> Result<(), Reason> {
        let limits = self.settings.limits;
        if self.live_tabs(session) >= limits.tabs_per_session
            || self.live_tabs_total() >= limits.tabs_total
        {
            return Err(Reason::TabLimit);
        }
        Ok(())
    }

    /// Register a tab we created (or a popup of ours) in state `Attaching`.
    pub fn insert_lease(&mut self, lease: Lease) -> TabId {
        let tab = lease.tab.clone();
        self.leases.insert(tab.clone(), lease);
        tab
    }

    pub fn new_tab_id(&self) -> TabId {
        loop {
            let id = TabId(format!(
                "t_{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ));
            if !self.leases.contains_key(&id) {
                return id;
            }
        }
    }

    /// Drop closed leases the agent has already been told about.
    pub fn forget_closed(&mut self, session: &str) {
        self.leases.retain(|_, l| {
            !(l.owner == session && matches!(l.state, TabState::Closed | TabState::Crashed))
        });
    }

    /// Tabs this session may act on (spec 7: closed tabs vanish from the list
    /// after the response that reported them).
    pub fn tabs_for(&self, session: &str) -> Vec<TabInfo> {
        let mut tabs: Vec<TabInfo> = self
            .leases
            .values()
            .filter(|l| l.owner == session && l.is_live())
            .map(Lease::info)
            .collect();
        tabs.sort_by_key(|a| a.created_at_ms);
        tabs
    }

    pub fn overview(&self) -> Vec<TabOverview> {
        let mut tabs: Vec<TabOverview> = self
            .leases
            .values()
            .filter(|l| l.state != TabState::Closed)
            .map(|l| TabOverview {
                tab: l.info(),
                session_id: l.owner.clone(),
            })
            .collect();
        tabs.sort_by_key(|a| a.tab.created_at_ms);
        tabs
    }

    pub fn session_access(&self) -> Vec<SessionAccess> {
        let mut rows: Vec<SessionAccess> = self
            .sessions
            .iter()
            .map(|(id, auth)| SessionAccess {
                session_id: id.clone(),
                window: auth.window.clone(),
                granted: auth.granted,
                turn_active: auth.turn.is_some(),
                granted_since_ms: auth.granted_since_ms,
                tabs: self.live_tabs(id),
            })
            .collect();
        rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        rows
    }

    // --- one action at a time per tab, N per connection (spec 9.6) ------------

    pub fn try_begin_action(&mut self, tab: Option<&TabId>) -> bool {
        if self.active_actions >= self.settings.limits.concurrent_actions {
            return false;
        }
        if let Some(tab) = tab {
            match self.leases.get_mut(tab) {
                Some(lease) if !lease.busy => {
                    lease.busy = true;
                    lease.last_action_at_ms = now_ms();
                }
                _ => return false,
            }
        }
        self.active_actions += 1;
        true
    }

    pub fn end_action(&mut self, tab: Option<&TabId>) {
        self.active_actions = self.active_actions.saturating_sub(1);
        if let Some(lease) = tab.and_then(|t| self.leases.get_mut(t)) {
            lease.busy = false;
        }
    }

    // --- request-id dedup (spec 11.4) ----------------------------------------

    pub fn dedup_check(&mut self, session: &str, request_id: &str, input: &Value) -> Dedup {
        let hash = input_hash(input);
        let now = Instant::now();
        let Some(ring) = self.dedup.get_mut(session) else {
            return Dedup::Fresh;
        };
        ring.retain(|entry| now.duration_since(entry.at) < DEDUP_TTL);
        match ring.iter().find(|entry| entry.request_id == request_id) {
            None => Dedup::Fresh,
            Some(entry) if entry.input_hash == hash => {
                Dedup::Cached(Box::new(entry.result.clone()))
            }
            Some(_) => Dedup::Conflict,
        }
    }

    pub fn dedup_store(
        &mut self,
        session: &str,
        request_id: &str,
        input: &Value,
        result: &ActionResult,
    ) {
        let ring = self.dedup.entry(session.to_string()).or_default();
        ring.retain(|entry| entry.request_id != request_id);
        while ring.len() >= DEDUP_RING {
            ring.pop_front();
        }
        ring.push_back(DedupEntry {
            request_id: request_id.to_string(),
            input_hash: input_hash(input),
            result: result.clone(),
            at: Instant::now(),
        });
    }
}

/// Thread-safe wrapper. Readers (the gate) hold the lock only for a check and
/// one write; writers (revocations) hold it only to flip state.
pub struct Policy {
    state: RwLock<PolicyState>,
}

impl Policy {
    pub fn new(state: PolicyState) -> Self {
        Policy {
            state: RwLock::new(state),
        }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, PolicyState> {
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, PolicyState> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }
}

// ---------------------------------------------------------------------------
// URL policy (spec 9.1, 15.2)
// ---------------------------------------------------------------------------

/// Only `http`, `https` and exactly `about:blank` may be navigated to or
/// opened. Everything else (`chrome://`, `edge://`, `devtools://`, `file://`,
/// `view-source:`, `javascript:`, `data:`, `blob:`, ...) is `url_denied`
/// before any CDP command.
pub fn check_url(raw: &str) -> Result<String, Reason> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("about:blank") {
        return Ok("about:blank".to_string());
    }
    let parsed = url::Url::parse(trimmed).map_err(|_| Reason::UrlDenied)?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(Reason::UrlDenied),
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(Reason::UrlDenied);
    }
    Ok(parsed.to_string())
}

/// URL as stored in the audit: no query string, no fragment (spec 11.5).
pub fn audit_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw.split(['?', '#']).next().unwrap_or_default().to_string(),
    }
}

// ---------------------------------------------------------------------------
// Upload path policy (spec 10.2, 15.2, decision D8)
// ---------------------------------------------------------------------------

/// Directories that are denied regardless of the dot-file setting: their
/// contents are credentials, and a prompt-injected `upload` must never reach
/// them. The spec lists them with the `.env*` exception; this module keeps
/// the credential stores denied even when the user allows dot-files.
const ALWAYS_DENIED_DIRS: [&str; 4] = [".ssh", ".aws", ".gnupg", ".kube"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDenied {
    pub message: String,
}

/// Resolve and check a path the agent wants to upload. Returns the canonical
/// path on success. `roots` are canonicalised here; the first one is normally
/// the session cwd.
pub fn check_upload_path(
    raw: &str,
    roots: &[PathBuf],
    allow_dotfiles: bool,
) -> Result<PathBuf, PathDenied> {
    let path = Path::new(raw);
    if raw.is_empty() || !path.is_absolute() {
        return Err(denied("Upload paths must be absolute", roots));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| denied("File does not exist or cannot be resolved", roots))?;
    let meta = std::fs::metadata(&canonical)
        .map_err(|_| denied("File does not exist or cannot be resolved", roots))?;
    if !meta.is_file() {
        return Err(denied("Only regular files can be uploaded", roots));
    }
    if meta.len() > UPLOAD_MAX_BYTES {
        return Err(denied("File is larger than 100 MB", roots));
    }
    let mut inside: Option<PathBuf> = None;
    for root in roots {
        let Ok(root) = std::fs::canonicalize(root) else {
            continue;
        };
        if canonical.starts_with(&root) {
            inside = Some(root);
            break;
        }
    }
    let Some(root) = inside else {
        return Err(denied("File is outside the allowed directories", roots));
    };
    let relative = canonical.strip_prefix(&root).unwrap_or(&canonical);
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy();
        if ALWAYS_DENIED_DIRS.contains(&name.as_ref()) {
            return Err(denied(
                "Credential directories cannot be uploaded from",
                roots,
            ));
        }
        if name == ".docker" {
            if let Some(file) = canonical.file_name() {
                if file == "config.json" {
                    return Err(denied("Credential files cannot be uploaded", roots));
                }
            }
        }
        if name.starts_with('.') && !allow_dotfiles {
            return Err(denied(
                "Dot-files and dot-directories are not allowed unless the user enables them",
                roots,
            ));
        }
    }
    Ok(canonical)
}

fn denied(what: &str, roots: &[PathBuf]) -> PathDenied {
    let allowed: Vec<String> = roots
        .iter()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .collect();
    PathDenied {
        message: if allowed.is_empty() {
            format!("{what}; no upload directory is allowed for this session.")
        } else {
            format!("{what}; allowed roots: {}.", allowed.join(", "))
        },
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Hash of the canonical (key-sorted) JSON form, so object key order never
/// turns a retry into a conflict.
pub fn input_hash(value: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    canonical_json(value).hash(&mut hasher);
    hasher.finish()
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_default(),
                        canonical_json(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state() -> PolicyState {
        PolicyState::new(7, BrowserSettings::default())
    }

    fn lease(tab: &str, target: &str, cdp: &str, owner: &str) -> Lease {
        Lease {
            tab: TabId(tab.into()),
            target_id: TargetId(target.into()),
            cdp_session: Some(CdpSessionId(cdp.into())),
            owner: owner.into(),
            state: TabState::Open,
            main_frame_id: Some(target.into()),
            main_loader_id: None,
            main_execution_context: None,
            frames: HashMap::new(),
            busy: false,
            snapshot_version: 0,
            pending_dialog: None,
            opened_by: None,
            popups_blocked: 0,
            url: "about:blank".into(),
            title: String::new(),
            created_at_ms: 0,
            last_action_at_ms: 0,
        }
    }

    fn ready(state: &mut PolicyState) -> RequestCtx {
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        state.insert_lease(lease("t_00000001", "T1", "S1", "s1"));
        state
            .admit(
                "s1",
                "r1",
                ActionKind::Click,
                Some(&TabId("t_00000001".into())),
                Duration::from_secs(10),
            )
            .unwrap()
    }

    #[test]
    fn gate_refuses_every_branch_before_the_socket() {
        let mut state = self::state();
        let ctx = ready(&mut state);
        let s1 = TargetRef::Session(CdpSessionId("S1".into()));
        assert_eq!(state.gate(&ctx, &s1, false), Ok(()));
        assert_eq!(state.gate(&ctx, &TargetRef::Browser, true), Ok(()));
        // NotAllowed: a browser-level command outside the allowlist.
        assert_eq!(
            state.gate(&ctx, &TargetRef::Browser, false),
            Err(GateError::NotAllowed)
        );
        // NoLease: a session we do not hold, and a target we did not create.
        assert_eq!(
            state.gate(&ctx, &TargetRef::Session(CdpSessionId("S9".into())), false),
            Err(GateError::NoLease)
        );
        assert_eq!(
            state.gate(&ctx, &TargetRef::Target(TargetId("T9".into())), true),
            Err(GateError::NoLease)
        );
        // TurnInactive, then Revoked (session epoch) once the turn ended.
        let mut probe = state.clone_for_test();
        probe.sessions.get_mut("s1").unwrap().turn = None;
        assert_eq!(probe.gate(&ctx, &s1, false), Err(GateError::TurnInactive));
        state.turn_finished("s1");
        assert_eq!(state.gate(&ctx, &s1, false), Err(GateError::Revoked));
        // Revoked (global epoch) after OFF→ON, and Off while OFF.
        let mut state = self::state();
        let ctx = ready(&mut state);
        let detach = state.set_toggle(false);
        assert_eq!(detach, vec![CdpSessionId("S1".into())]);
        assert_eq!(state.gate(&ctx, &s1, false), Err(GateError::Off));
        state.set_toggle(true);
        assert_eq!(state.gate(&ctx, &s1, false), Err(GateError::Revoked));
        // OFF dropped the grant; once re-granted, the lease is still gone.
        state.turn_started("s1", "main", "/repo", None);
        let ungranted = state.admit(
            "s1",
            "r2",
            ActionKind::Click,
            Some(&TabId("t_00000001".into())),
            Duration::from_secs(1),
        );
        assert_eq!(ungranted.unwrap_err(), Reason::NotGranted);
        state.set_grant("s1", "main", true);
        let fresh = state.admit(
            "s1",
            "r3",
            ActionKind::Click,
            Some(&TabId("t_00000001".into())),
            Duration::from_secs(1),
        );
        assert_eq!(fresh.unwrap_err(), Reason::NoLease);
    }

    #[test]
    fn admission_reports_the_spec_reasons_in_order() {
        let mut state = self::state();
        let tab = TabId("t_00000001".into());
        let admit = |state: &PolicyState| {
            state.admit(
                "s1",
                "r",
                ActionKind::Click,
                Some(&tab),
                Duration::from_secs(1),
            )
        };
        assert_eq!(admit(&state).unwrap_err(), Reason::Off);
        state.set_toggle(true);
        assert_eq!(admit(&state).unwrap_err(), Reason::NotGranted);
        state.turn_started("s1", "main", "/repo", None);
        assert_eq!(admit(&state).unwrap_err(), Reason::NotGranted);
        state.set_grant("s1", "main", true);
        state.turn_finished("s1");
        assert_eq!(admit(&state).unwrap_err(), Reason::TurnInactive);
        state.turn_started("s1", "main", "/repo", None);
        assert_eq!(admit(&state).unwrap_err(), Reason::NoLease);
        state.insert_lease(lease("t_00000001", "T1", "S1", "s2"));
        assert_eq!(admit(&state).unwrap_err(), Reason::NoLease);
        state.lease_mut(&tab).unwrap().owner = "s1".into();
        state.lease_mut(&tab).unwrap().pending_dialog = Some(DialogInfo {
            kind: super::super::protocol::DialogType::Confirm,
            message: "Delete?".into(),
            default_prompt: None,
            url: "https://x".into(),
            opened_at_ms: 0,
        });
        assert_eq!(admit(&state).unwrap_err(), Reason::DialogPending);
        assert!(state
            .admit(
                "s1",
                "r",
                ActionKind::Dialog,
                Some(&tab),
                Duration::from_secs(1)
            )
            .is_ok());
        state.lease_mut(&tab).unwrap().pending_dialog = None;
        state.lease_mut(&tab).unwrap().state = TabState::Closed;
        assert_eq!(admit(&state).unwrap_err(), Reason::TargetClosed);
        state.lease_mut(&tab).unwrap().state = TabState::Open;
        let ctx = admit(&state).unwrap();
        assert_eq!(ctx.global_epoch, state.global_epoch);
        assert!(state
            .admit(
                "s1",
                "r",
                ActionKind::TabsList,
                None,
                Duration::from_secs(1)
            )
            .is_ok());
        assert_eq!(
            state
                .admit("s1", "r", ActionKind::Click, None, Duration::from_secs(1))
                .unwrap_err(),
            Reason::InvalidRequest
        );
    }

    #[test]
    fn revocations_bump_epochs_and_release_leases_but_keep_tabs() {
        let mut state = self::state();
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        state.turn_started("s2", "main", "/repo", Some(true));
        state.insert_lease(lease("t_00000001", "T1", "S1", "s1"));
        state.insert_lease(lease("t_00000002", "T2", "S2", "s2"));
        let e1 = state.session_epoch("s1").unwrap();
        let detach = state.revoke_session("s1");
        assert_eq!(detach, vec![CdpSessionId("S1".into())]);
        assert_eq!(state.session_epoch("s1").unwrap(), e1.next());
        assert!(!state.granted("s1"));
        assert_eq!(
            state.lease(&TabId("t_00000001".into())).unwrap().state,
            TabState::Revoked
        );
        assert_eq!(
            state.lease(&TabId("t_00000002".into())).unwrap().state,
            TabState::Open
        );
        assert!(state.tabs_for("s1").is_empty());
        assert_eq!(state.tabs_for("s2").len(), 1);
        // Connection loss clears everything and bumps the global epoch.
        let g = state.global_epoch;
        state.connection_lost();
        assert_eq!(state.global_epoch, g.next());
        assert_eq!(state.leases().count(), 0);
        // Session deletion closes its tabs.
        state.insert_lease(lease("t_00000003", "T3", "S3", "s2"));
        assert_eq!(state.session_deleted("s2"), vec![TargetId("T3".into())]);
        assert!(state.session("s2").is_none());
        // Window close drops its sessions.
        state.turn_started("s4", "w2", "/repo", Some(true));
        state.insert_lease(lease("t_00000004", "T4", "S4", "s4"));
        assert_eq!(state.window_closed("w2"), vec![CdpSessionId("S4".into())]);
        assert!(state.session("s4").is_none());
        // OFF revokes every grant (spec 6, 13.1): after ON the session must
        // ask for /browser again, and its epoch moved so in-flight work dies.
        state.turn_started("s5", "main", "/repo", Some(true));
        state.insert_lease(lease("t_00000005", "T5", "S5", "s5"));
        let e5 = state.session_epoch("s5").unwrap();
        assert_eq!(state.set_toggle(false), vec![CdpSessionId("S5".into())]);
        assert!(!state.granted("s5"));
        assert_eq!(state.session_epoch("s5").unwrap(), e5.next());
        assert!(
            state.turn_active("s5"),
            "the turn itself is not ended by OFF"
        );
        state.set_toggle(true);
        assert!(!state.granted("s5"));
        assert_eq!(
            state
                .admit(
                    "s5",
                    "r",
                    ActionKind::TabsList,
                    None,
                    Duration::from_secs(1)
                )
                .unwrap_err(),
            Reason::NotGranted
        );
        state.set_grant("s5", "main", true);
        assert!(state.granted("s5"));
        // Releasing a tab to the user: lease revoked, session to detach returned.
        state.insert_lease(lease("t_00000006", "T6", "S6", "s5"));
        let t6 = TabId("t_00000006".into());
        assert_eq!(
            state.release_lease(&t6),
            Ok(Some(CdpSessionId("S6".into())))
        );
        assert_eq!(state.lease(&t6).unwrap().state, TabState::Revoked);
        assert_eq!(state.release_lease(&t6), Ok(None));
        assert_eq!(
            state.release_lease(&TabId("t_00000099".into())),
            Err(Reason::NoLease)
        );
        state.lease_mut(&t6).unwrap().state = TabState::Closed;
        assert_eq!(state.release_lease(&t6), Err(Reason::TargetClosed));
    }

    #[test]
    fn transfer_moves_the_lease_and_invalidates_the_old_owner() {
        let mut state = self::state();
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        state.turn_started("s2", "main", "/repo", None);
        state.insert_lease(lease("t_00000001", "T1", "S1", "s1"));
        let tab = TabId("t_00000001".into());
        assert_eq!(state.transfer(&tab, "s1", "s2"), Err(Reason::NotGranted));
        state.set_grant("s2", "main", true);
        let ctx = state
            .admit(
                "s1",
                "r",
                ActionKind::Click,
                Some(&tab),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(state.transfer(&tab, "s1", "s2"), Ok(()));
        assert_eq!(state.lease(&tab).unwrap().owner, "s2");
        assert_eq!(
            state.gate(&ctx, &TargetRef::Session(CdpSessionId("S1".into())), false),
            Err(GateError::Revoked)
        );
        assert_eq!(state.transfer(&tab, "s1", "s2"), Err(Reason::NoLease));
    }

    #[test]
    fn limits_and_busy_flags() {
        let mut state = self::state();
        state.settings.limits.tabs_per_session = 2;
        state.settings.limits.tabs_total = 3;
        state.settings.limits.concurrent_actions = 1;
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        state.insert_lease(lease("t_00000001", "T1", "S1", "s1"));
        assert!(state.can_open_tab("s1").is_ok());
        state.insert_lease(lease("t_00000002", "T2", "S2", "s1"));
        assert_eq!(state.can_open_tab("s1"), Err(Reason::TabLimit));
        assert!(state.can_open_tab("s2").is_ok());
        state.insert_lease(lease("t_00000003", "T3", "S3", "s2"));
        assert_eq!(state.can_open_tab("s2"), Err(Reason::TabLimit));
        let t1 = TabId("t_00000001".into());
        let t2 = TabId("t_00000002".into());
        assert!(state.try_begin_action(Some(&t1)));
        assert!(!state.try_begin_action(Some(&t1)));
        assert!(!state.try_begin_action(Some(&t2)));
        state.end_action(Some(&t1));
        assert!(state.try_begin_action(Some(&t2)));
        assert!(state.new_tab_id().is_well_formed());
        assert!(state.lease_for_target(&TargetId("T2".into())).is_some());
    }

    #[test]
    fn dedup_returns_cached_results_and_detects_conflicts() {
        let mut state = self::state();
        let input = json!({"tab": "t_00000001", "ref": "e1"});
        let reordered = json!({"ref": "e1", "tab": "t_00000001"});
        assert!(matches!(
            state.dedup_check("s1", "r1", &input),
            Dedup::Fresh
        ));
        let result = ActionResult::done("r1", "Clicked.");
        state.dedup_store("s1", "r1", &input, &result);
        assert!(matches!(
            state.dedup_check("s1", "r1", &reordered),
            Dedup::Cached(r) if *r == result
        ));
        assert!(matches!(
            state.dedup_check("s1", "r1", &json!({"tab": "t_00000001", "ref": "e2"})),
            Dedup::Conflict
        ));
        assert!(matches!(
            state.dedup_check("s2", "r1", &input),
            Dedup::Fresh
        ));
        for i in 0..(DEDUP_RING + 10) {
            state.dedup_store("s1", &format!("x{i}"), &input, &result);
        }
        assert!(matches!(
            state.dedup_check("s1", "x0", &input),
            Dedup::Fresh
        ));
        let last = format!("x{}", DEDUP_RING + 9);
        assert!(matches!(
            state.dedup_check("s1", &last, &input),
            Dedup::Cached(_)
        ));
        state.dedup.get_mut("s1").unwrap().iter_mut().for_each(|e| {
            e.at = Instant::now() - DEDUP_TTL - Duration::from_secs(1);
        });
        assert!(matches!(
            state.dedup_check("s1", &last, &input),
            Dedup::Fresh
        ));
    }

    #[test]
    fn tokens_are_stable_per_session_and_looked_up_in_constant_time() {
        let mut state = self::state();
        let token = state.prepare_session("s1", "main", "/repo");
        assert_eq!(token.len(), 64);
        assert_eq!(state.prepare_session("s1", "main", "/other"), token);
        assert_eq!(state.session_for_token(&token), Some("s1".into()));
        assert_eq!(state.session_for_token("nope"), None);
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn url_policy_allows_only_web_pages() {
        assert_eq!(
            check_url("https://example.com/a?b=1").unwrap(),
            "https://example.com/a?b=1"
        );
        assert_eq!(
            check_url(" http://localhost:8080 ").unwrap(),
            "http://localhost:8080/"
        );
        assert_eq!(check_url("about:blank").unwrap(), "about:blank");
        for denied in [
            "chrome://version",
            "edge://settings",
            "devtools://devtools/bundled/inspector.html",
            "file:///etc/passwd",
            "view-source:https://example.com",
            "javascript:alert(1)",
            "data:text/html,hi",
            "blob:https://example.com/x",
            "about:config",
            "ftp://example.com",
            "example.com",
            "",
        ] {
            assert_eq!(check_url(denied), Err(Reason::UrlDenied), "{denied}");
        }
        assert_eq!(audit_url("https://a.com/p?x=1#f"), "https://a.com/p");
        assert_eq!(audit_url("garbage?x=1"), "garbage");
    }

    #[test]
    fn upload_paths_must_be_regular_files_inside_roots_and_not_secrets() {
        let root = std::env::temp_dir().join(format!("mc-browser-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".ssh")).unwrap();
        std::fs::create_dir_all(root.join(".docker")).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/ok.txt"), b"hi").unwrap();
        std::fs::write(root.join(".ssh/id_ed25519"), b"key").unwrap();
        std::fs::write(root.join(".docker/config.json"), b"{}").unwrap();
        std::fs::write(root.join(".env"), b"SECRET=1").unwrap();
        std::fs::write(root.join(".env.example"), b"X=").unwrap();
        let outside = std::env::temp_dir().join(format!("mc-outside-{}", uuid::Uuid::new_v4()));
        std::fs::write(&outside, b"x").unwrap();
        let roots = vec![root.clone()];
        let ok =
            check_upload_path(root.join("sub/ok.txt").to_str().unwrap(), &roots, false).unwrap();
        assert!(ok.ends_with("sub/ok.txt") || ok.ends_with("sub\\ok.txt"));
        assert!(check_upload_path("sub/ok.txt", &roots, false).is_err());
        assert!(check_upload_path(root.join("sub").to_str().unwrap(), &roots, false).is_err());
        assert!(check_upload_path(root.join("missing").to_str().unwrap(), &roots, false).is_err());
        let err = check_upload_path(outside.to_str().unwrap(), &roots, false).unwrap_err();
        assert!(err.message.contains("allowed roots"));
        assert!(check_upload_path(root.join(".env").to_str().unwrap(), &roots, false).is_err());
        assert!(
            check_upload_path(root.join(".env.example").to_str().unwrap(), &roots, true).is_ok()
        );
        assert!(
            check_upload_path(root.join(".ssh/id_ed25519").to_str().unwrap(), &roots, true)
                .is_err()
        );
        assert!(check_upload_path(
            root.join(".docker/config.json").to_str().unwrap(),
            &roots,
            true
        )
        .is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("sub/escape")).unwrap();
            assert!(
                check_upload_path(root.join("sub/escape").to_str().unwrap(), &roots, false)
                    .is_err()
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_file(&outside).unwrap();
    }

    impl PolicyState {
        fn clone_for_test(&self) -> PolicyState {
            PolicyState {
                toggle_on: self.toggle_on,
                run_id: self.run_id,
                global_epoch: self.global_epoch,
                settings: self.settings.clone(),
                active_actions: self.active_actions,
                sessions: self.sessions.clone(),
                leases: self.leases.clone(),
                dedup: HashMap::new(),
                last_revocation: self.last_revocation,
            }
        }
    }
}
