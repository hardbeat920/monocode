//! Browser module: lets an agent use the user's own Chrome or Edge through a
//! thin, in-process CDP client, under the user's control at every moment.
//!
//! Architecture (implementation spec, chapters 3-7, 11, 13, 14):
//!
//! - [`discovery`]: finds `DevToolsActivePort`, probes the port, checks the
//!   version and administrator policies. Never launches a browser.
//! - [`link`]: the one WebSocket to the browser. Every command goes through
//!   [`Link::send`], which re-checks toggle, epochs and lease under the policy
//!   read lock immediately before the socket write. The method set is closed.
//! - [`policy`]: the authority: global toggle, epochs, per-session grants and
//!   tokens, per-tab leases, admission, limits, request-id dedup, URL and
//!   upload-path rules.
//! - [`targets`]: tab lifecycle (create, attach, marker, close, focus), popups
//!   and OOPIFs via session-scoped auto-attach, and the event pump that keeps
//!   the lease table current.
//! - [`audit`]: `browser_audit`, `browser_events`, `browser_leases` and the
//!   persisted settings in the app's SQLite database.
//! - [`protocol`]: the frozen contract shared with the semantic layer, the
//!   CLI/MCP adapters and the UI: action names and parameter records, the
//!   four outcome classes, reasons, tab and connection state, settings.
//! - [`semantic`]: work package 2; the core calls [`semantic::execute`] for
//!   every action it does not handle itself (`tabs.*`).
//!
//! # How the other work packages talk to this one
//!
//! **Agent side (CLI and MCP server, in the `monocode` binary).** A provider
//! process finds [`protocol::ENV_ENDPOINT`] and [`protocol::ENV_TOKEN`] in its
//! environment (injected by `control::configure_child` for every session).
//! It sends one JSON line to the loopback control endpoint:
//! `{"token": …, "namespace": "browser", "action": "<name>", "input": {…},
//! "requestId": "<id>"}` and reads one JSON line back:
//! `{"ok": true, "result": <ActionResult>}`. Policy refusals (`browser_off`,
//! `not_granted`, `turn_inactive`, …) are ordinary `blocked` results; only an
//! unknown token is a transport error (`{"ok": false, "error": …}`).
//!
//! **Frontend.** Tauri commands `browser_*` below, and the
//! [`protocol::STATE_EVENT`] event carrying a [`protocol::BrowserState`]
//! after every change. `control_authorize_turn` takes an optional
//! `browserAccess` flag that grants or drops `/browser` for the session.
//!
//! **Semantic layer.** [`semantic::Exec`] gives it the link, the admitted
//! [`policy::RequestCtx`], the resolved [`targets::TabHandle`] and the
//! settings; see that module's docs.

// The protocol records, lease accessors and CDP methods below are the shared
// contract for work packages 2-5 (semantic layer, CLI/MCP adapters, UI). Until
// those land, parts of it have no caller inside this crate.
#![allow(dead_code)]

pub mod agent;
pub mod audit;
pub mod discovery;
pub mod launch;
pub mod link;
pub mod policy;
pub mod protocol;
#[cfg(test)]
mod resilience;
pub mod semantic;
pub mod targets;
#[cfg(test)]
pub(crate) mod testing;

use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use audit::Actor;
use discovery::{Candidate, Located, CONSENT_TIMEOUT};
use link::{CdpSessionId, ClosedBy, Command, Link, LinkStatus, Method, OpenError, TargetId};
use policy::{audit_url, now_ms, Dedup, Policy, PolicyState, RequestCtx, BUSY_WAIT};
use protocol::{
    AccessMode, ActionKind, ActionRequest, ActionResult, AuditEntry, BrowserInfo, BrowserMode,
    BrowserSettings, BrowserState, CommonInput, ConnectionInfo, ConnectionState, Outcome, Reason,
    TabId, TabOverview, TabState, TabsCloseParams, TabsFocusParams, TabsListParams, TabsOpenParams,
    WaitUntil, MIN_BROWSER_MAJOR, STATE_EVENT,
};
use targets::TabEvent;

/// How long `OFF` waits for the browser to acknowledge our detaches (spec 5).
pub const OFF_DETACH_BUDGET: Duration = Duration::from_millis(500);
/// How long an agent action waits for the user to answer an access request.
/// Below the 60 s tool timeout some MCP clients use.
pub const ASK_WAIT: Duration = Duration::from_secs(45);
/// How long the MonoCode browser may take to close before it is killed.
const MANAGED_CLOSE_WAIT: Duration = Duration::from_secs(3);
/// How long the MonoCode browser stays up after an access request was
/// declined or expired while no thread may use it.
pub const UNUSED_CLOSE_GRACE: Duration = Duration::from_secs(10);

/// The MonoCode browser process this run started.
struct ManagedBrowser {
    child: Child,
    show_window: bool,
    profile: PathBuf,
    /// Private mode: the profile is deleted once the browser has exited.
    ephemeral: bool,
}

impl ManagedBrowser {
    fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// After exit: a private profile goes. Chrome's helper processes can hold
    /// files for a moment after the main one exits, hence the retries.
    fn forget(self) {
        if !self.ephemeral {
            return;
        }
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.profile).is_ok() || !self.profile.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Wait up to [`MANAGED_CLOSE_WAIT`] for the MonoCode browser to exit, kill
/// it otherwise, then forget it (a private profile is deleted).
fn stop_managed(mut managed: ManagedBrowser) {
    let deadline = Instant::now() + MANAGED_CLOSE_WAIT;
    while Instant::now() < deadline && !managed.exited() {
        thread::sleep(Duration::from_millis(100));
    }
    if !managed.exited() {
        let _ = managed.child.kill();
        let _ = managed.child.wait();
    }
    managed.forget();
}

static HOST: OnceLock<Arc<BrowserHost>> = OnceLock::new();

/// The process-wide host, once [`init`] ran.
pub fn host() -> Option<Arc<BrowserHost>> {
    HOST.get().cloned()
}

/// Where audit rows go: the app's session store, or an owned connection in tests.
enum Db {
    App(AppHandle),
    Own(Mutex<Connection>),
}

impl Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
        match self {
            Db::App(app) => {
                let store = app.try_state::<crate::session_store::SessionStore>()?;
                let conn = store.lock_conn().ok()?;
                f(&conn).ok()
            }
            Db::Own(conn) => {
                let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
                f(&conn).ok()
            }
        }
    }
}

/// The module's single instance: policy, link, connection state, audit.
pub struct BrowserHost {
    policy: Arc<Policy>,
    link: Link,
    connection: Mutex<ConnectionInfo>,
    /// `generation + 1` of the connection attempt running now, 0 when none.
    connecting: AtomicU64,
    /// Bumped by every toggle change. An attempt from an older generation
    /// (OFF, or OFF→ON, happened while it ran) neither reports its state nor
    /// keeps a socket it opened.
    generation: AtomicU64,
    /// How long the browser's consent dialog may stay unanswered, in ms.
    consent_timeout_ms: AtomicU64,
    /// [`ASK_WAIT`]; shorter in tests.
    ask_wait_ms: AtomicU64,
    /// [`UNUSED_CLOSE_GRACE`]; shorter in tests.
    unused_grace_ms: AtomicU64,
    /// Bumped by every scheduled "close when unused"; only the latest fires.
    unused_close: AtomicU64,
    /// A MonoCode browser shutting down in the background (OFF, mode change).
    closing: Mutex<Option<thread::JoinHandle<()>>>,
    /// Whether an agent request may still start a connection attempt that
    /// shows the browser's consent dialog. One per drop (spec 14.1 row 2,
    /// 14.2): restored when a connection is established or the toggle goes
    /// ON, spent by the first automatic attempt that reaches the browser.
    auto_dialog: AtomicBool,
    /// Profile folder of the MonoCode browser (managed mode).
    profile: Mutex<Option<PathBuf>>,
    managed: Mutex<Option<ManagedBrowser>>,
    /// "Open browser" asked for a visible window on the next start.
    force_window: AtomicBool,
    db: Db,
    app: Option<AppHandle>,
    run_id: u64,
}

/// Create the host, migrate the audit tables and register it. Called from
/// `lib.rs` setup after `session_store::init`.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let store = app
        .try_state::<crate::session_store::SessionStore>()
        .ok_or("Session store must be initialised before the browser module")?;
    let (run_id, settings) = {
        let conn = store.lock_conn()?;
        audit::migrate(&conn).map_err(|e| e.to_string())?;
        let _ = audit::prune(&conn, now_ms());
        let run_id = audit::next_run_id(&conn).map_err(|e| e.to_string())?;
        let _ = audit::close_stale_leases(&conn, run_id);
        let settings = audit::load_settings(&conn)
            .ok()
            .flatten()
            .unwrap_or_default();
        (run_id, settings)
    };
    let enabled = settings.enabled;
    let host = BrowserHost::create(run_id, settings, Db::App(app.clone()), Some(app.clone()));
    if let Ok(dir) = app.path().app_data_dir() {
        let profile = launch::profile_dir(&dir);
        launch::clear_private(&profile);
        host.set_profile_dir(profile);
    }
    if enabled {
        host.restore_toggle();
    }
    HOST.set(host)
        .map_err(|_| "Browser module initialised twice".to_string())
}

/// App exit: the MonoCode browser goes with the app.
pub fn shutdown() {
    if let Some(host) = host() {
        host.close_managed_browser();
    }
}

impl BrowserHost {
    fn create(
        run_id: u64,
        settings: BrowserSettings,
        db: Db,
        app: Option<AppHandle>,
    ) -> Arc<BrowserHost> {
        let policy = Arc::new(Policy::new(PolicyState::new(run_id, settings)));
        let link = Link::new(policy.clone());
        let host = Arc::new(BrowserHost {
            policy,
            link,
            connection: Mutex::new(ConnectionInfo {
                state: ConnectionState::Off,
                browser: None,
                detail: Some("Browser access is off.".into()),
                since_ms: now_ms(),
                last_error: None,
            }),
            connecting: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            consent_timeout_ms: AtomicU64::new(CONSENT_TIMEOUT.as_millis() as u64),
            ask_wait_ms: AtomicU64::new(ASK_WAIT.as_millis() as u64),
            unused_grace_ms: AtomicU64::new(UNUSED_CLOSE_GRACE.as_millis() as u64),
            unused_close: AtomicU64::new(0),
            closing: Mutex::new(None),
            auto_dialog: AtomicBool::new(true),
            profile: Mutex::new(None),
            managed: Mutex::new(None),
            force_window: AtomicBool::new(false),
            db,
            app,
            run_id,
        });
        let weak: Weak<BrowserHost> = Arc::downgrade(&host);
        host.link.set_on_closed(move |by| {
            if let Some(host) = weak.upgrade() {
                host.on_link_closed(by);
            }
        });
        host
    }

    /// Host for tests: in-memory audit, no Tauri.
    #[cfg(test)]
    pub(crate) fn for_tests(settings: BrowserSettings) -> Arc<BrowserHost> {
        let conn = Connection::open_in_memory().expect("in-memory db");
        audit::migrate(&conn).expect("migrate");
        BrowserHost::create(0x1234, settings, Db::Own(Mutex::new(conn)), None)
    }

    pub fn policy(&self) -> &Arc<Policy> {
        &self.policy
    }

    /// Shorter consent waits for tests (the browser's own dialog has no limit).
    #[cfg(test)]
    pub(crate) fn set_consent_timeout(&self, timeout: Duration) {
        self.consent_timeout_ms
            .store(timeout.as_millis() as u64, Ordering::Release);
    }

    /// Shorter waits for an answer to an access request, for tests.
    #[cfg(test)]
    pub(crate) fn set_ask_wait(&self, wait: Duration) {
        self.ask_wait_ms
            .store(wait.as_millis() as u64, Ordering::Release);
    }

    /// Shorter grace before an unused MonoCode browser closes, for tests.
    #[cfg(test)]
    pub(crate) fn set_unused_grace(&self, grace: Duration) {
        self.unused_grace_ms
            .store(grace.as_millis() as u64, Ordering::Release);
    }

    /// Treat `child` as the MonoCode browser this run started, for tests.
    #[cfg(test)]
    pub(crate) fn adopt_managed_for_tests(&self, child: Child, profile: PathBuf, ephemeral: bool) {
        *self.managed.lock().unwrap_or_else(|e| e.into_inner()) = Some(ManagedBrowser {
            child,
            show_window: true,
            profile,
            ephemeral,
        });
    }

    pub fn link(&self) -> &Link {
        &self.link
    }

    pub fn run_id(&self) -> u64 {
        self.run_id
    }

    // --- state for the UI ------------------------------------------------------

    pub fn connection(&self) -> ConnectionInfo {
        self.connection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn state(&self) -> BrowserState {
        let policy = self.policy.read();
        BrowserState {
            toggle_on: policy.toggle_on,
            connection: self.connection(),
            sessions: policy.session_access(),
            tabs: policy.overview(),
            settings: policy.settings.clone(),
            global_epoch: policy.global_epoch,
        }
    }

    fn emit_state(&self) {
        self.sync_lease_rows();
        if let Some(app) = &self.app {
            let _ = app.emit(STATE_EVENT, self.state());
        }
    }

    fn set_connection(
        &self,
        state: ConnectionState,
        detail: Option<String>,
        browser: Option<BrowserInfo>,
        last_error: Option<String>,
    ) {
        {
            let mut connection = self.connection.lock().unwrap_or_else(|e| e.into_inner());
            connection.state = state;
            connection.detail = detail;
            connection.browser = browser;
            connection.since_ms = now_ms();
            if state == ConnectionState::Connected {
                connection.last_error = None;
            } else if last_error.is_some() {
                connection.last_error = last_error;
            }
        }
        self.emit_state();
    }

    /// Close the `browser_leases` rows of this run whose tab is no longer
    /// leased (revoked, released, closed, or dropped with the connection), so
    /// the table only lists tabs some session actually holds.
    fn sync_lease_rows(&self) {
        let live: std::collections::HashSet<String> = self
            .policy
            .read()
            .leases()
            .filter(|l| l.is_live())
            .map(|l| l.tab.0.clone())
            .collect();
        self.db
            .with(|conn| audit::close_leases_except(conn, self.run_id, &live));
    }

    fn event(&self, kind: &str, actor: Actor, session: Option<&str>, detail: Option<&str>) {
        self.db
            .with(|conn| audit::event(conn, kind, actor, session, detail));
    }

    // --- toggle and connection (spec 4, 6, 13) ----------------------------------

    /// Main switch. ON starts discovery (asynchronously); OFF bumps the global
    /// epoch, revokes every lease, detaches and closes the link. Tabs stay.
    pub fn set_toggle(self: &Arc<Self>, on: bool) {
        let detach = {
            let mut policy = self.policy.write();
            if policy.toggle_on != on {
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
            policy.set_toggle(on)
        };
        self.persist_enabled(on);
        if on {
            self.auto_dialog.store(true, Ordering::Release);
            self.event("toggle_on", Actor::User, None, None);
            if self.mode() == BrowserMode::Attach {
                self.connect();
            } else {
                // The MonoCode browser starts on the first agent action or
                // "Open browser", not on the switch.
                self.set_idle();
            }
        } else {
            self.event("toggle_off", Actor::User, None, None);
            self.close_managed_pages();
            if self.link.status() != LinkStatus::Closed {
                self.link.close(detach, OFF_DETACH_BUDGET);
            }
            // A UI command: the process exit and a private profile's deletion
            // take seconds, so they finish in the background.
            self.close_managed_browser_later();
            self.set_connection(
                ConnectionState::Off,
                Some("Browser access is off.".into()),
                None,
                None,
            );
        }
        self.emit_state();
    }

    /// Startup with the switch remembered ON: nothing connects or starts
    /// until an agent needs the browser.
    fn restore_toggle(&self) {
        {
            let mut policy = self.policy.write();
            self.generation.fetch_add(1, Ordering::AcqRel);
            policy.set_toggle(true);
        }
        self.event("toggle_on", Actor::System, None, Some("remembered"));
        self.set_idle();
    }

    fn mode(&self) -> BrowserMode {
        self.policy.read().settings.mode
    }

    fn set_idle(&self) {
        let detail = match self.mode() {
            BrowserMode::Managed => "The MonoCode browser starts when an agent needs it.",
            BrowserMode::Private => "The private MonoCode browser starts when an agent needs it and forgets everything when it closes.",
            BrowserMode::Attach => "MonoCode connects to your browser when an agent needs it.",
        };
        self.set_connection(ConnectionState::Idle, Some(detail.into()), None, None);
    }

    fn persist_enabled(&self, on: bool) {
        let settings = {
            let mut policy = self.policy.write();
            if policy.settings.enabled == on {
                return;
            }
            policy.settings.enabled = on;
            policy.settings.clone()
        };
        self.db.with(|conn| audit::save_settings(conn, &settings));
    }

    pub fn set_profile_dir(&self, dir: PathBuf) {
        *self.profile.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
    }

    fn profile_dir(&self) -> Option<PathBuf> {
        self.profile
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn connected_to_managed(&self) -> bool {
        self.connection()
            .browser
            .as_ref()
            .is_some_and(|browser| browser.managed)
    }

    /// Close every page of the MonoCode browser while the link is still up,
    /// so the browser exits on its own and does not offer to restore them.
    /// Runs on OFF from a UI command, so all of it fits in
    /// [`OFF_DETACH_BUDGET`]; a browser that stops answering is left to
    /// [`Self::close_managed_browser`].
    fn close_managed_pages(&self) {
        if !self.link.is_open() || !self.connected_to_managed() {
            return;
        }
        let deadline = Instant::now() + OFF_DETACH_BUDGET;
        let Ok(reply) = self.link.send_system_within(
            Command::browser(Method::TargetGetTargets, json!({})),
            OFF_DETACH_BUDGET,
        ) else {
            return;
        };
        let pages: Vec<TargetId> = reply["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|info| info["type"] == "page")
            .filter_map(|info| info["targetId"].as_str())
            .map(|id| TargetId(id.to_string()))
            .collect();
        for page in pages {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero()
                || self
                    .link
                    .send_system_within(
                        Command::browser(Method::TargetCloseTarget, json!({ "targetId": page.0 })),
                        left,
                    )
                    .is_err_and(|error| {
                        matches!(
                            error.reason(),
                            Reason::CdpTimeout | Reason::LinkDown | Reason::BrowserDisconnected
                        )
                    })
            {
                return;
            }
        }
    }

    /// Stop the MonoCode browser this run started and wait until it is gone
    /// (app exit, failed start): it closes its pages first when the link is
    /// up, and is killed when it does not exit in time.
    pub fn close_managed_browser(&self) {
        self.close_managed_pages();
        let managed = self
            .managed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(managed) = managed {
            stop_managed(managed);
        }
        self.finish_closing();
    }

    /// [`Self::close_managed_browser`] for UI commands: the pages close now
    /// (within [`OFF_DETACH_BUDGET`]), the process exit and the profile
    /// cleanup run on a thread. The next start waits for it
    /// ([`Self::finish_closing`]), so two browsers never share a profile.
    fn close_managed_browser_later(&self) {
        self.close_managed_pages();
        let managed = self
            .managed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let Some(managed) = managed else { return };
        let mut closing = self.closing.lock().unwrap_or_else(|e| e.into_inner());
        let earlier = closing.take();
        let spawned = thread::Builder::new()
            .name("browser-close".into())
            .spawn(move || {
                if let Some(earlier) = earlier {
                    let _ = earlier.join();
                }
                stop_managed(managed);
            });
        *closing = spawned.ok();
    }

    /// Wait for a background shutdown of the MonoCode browser to end.
    fn finish_closing(&self) {
        let closing = self
            .closing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(closing) = closing {
            let _ = closing.join();
        }
    }

    /// After an access request was declined or expired: when no thread may
    /// use the MonoCode browser once [`UNUSED_CLOSE_GRACE`] has passed, close
    /// it instead of leaving it running until the switch goes off.
    fn close_when_unused(self: &Arc<Self>) {
        if !self.mode().launches() {
            return;
        }
        let ticket = self.unused_close.fetch_add(1, Ordering::AcqRel) + 1;
        let grace = Duration::from_millis(self.unused_grace_ms.load(Ordering::Acquire));
        let host = self.clone();
        let _ = thread::Builder::new()
            .name("browser-unused".into())
            .spawn(move || {
                thread::sleep(grace);
                // A start still running (the warm-up) ends first.
                let deadline = Instant::now() + launch::START_TIMEOUT;
                while host.connecting.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(100));
                }
                if host.unused_close.load(Ordering::Acquire) == ticket {
                    host.close_if_unused();
                }
            });
    }

    fn close_if_unused(self: &Arc<Self>) {
        {
            let policy = self.policy.read();
            if !policy.toggle_on || !policy.settings.mode.launches() || policy.anyone_has_access() {
                return;
            }
        }
        let running = self.link.status() != LinkStatus::Closed
            || self
                .managed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some();
        if !running {
            return;
        }
        self.event("managed_closed_unused", Actor::System, None, None);
        self.close_managed_pages();
        if self.link.status() != LinkStatus::Closed {
            self.link.close(Vec::new(), OFF_DETACH_BUDGET);
        }
        self.close_managed_browser_later();
        if self.policy.read().toggle_on {
            self.set_idle();
        }
        self.emit_state();
    }

    /// The link to the MonoCode browser dropped: when the window was closed
    /// or the browser crashed, its process exits shortly after. Forget it then
    /// (a private profile is deleted); a browser that keeps running stays.
    fn reap_managed_browser(self: &Arc<Self>) {
        let host = self.clone();
        let _ = thread::Builder::new()
            .name("browser-reap".into())
            .spawn(move || {
                for _ in 0..100 {
                    if host.take_exited_browser() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
    }

    /// True when there is nothing left to wait for.
    fn take_exited_browser(&self) -> bool {
        let exited = {
            let mut slot = self.managed.lock().unwrap_or_else(|e| e.into_inner());
            let Some(managed) = slot.as_mut() else {
                return true;
            };
            if !managed.exited() {
                return false;
            }
            slot.take()
        };
        if let Some(managed) = exited {
            managed.forget();
        }
        true
    }

    /// Settings "Open browser": the MonoCode browser with a visible window,
    /// for signing in to sites. A headless one is restarted.
    pub fn open_window(self: &Arc<Self>) -> Result<(), String> {
        if !self.mode().launches() {
            return Err("Open browser is only for the MonoCode browser.".into());
        }
        if !self.policy.read().toggle_on {
            return Err("Turn on browser access first.".into());
        }
        let headless = self
            .managed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|managed| !managed.show_window);
        if headless == Some(true) {
            self.close_managed_pages();
            self.disconnect();
            self.close_managed_browser_later();
        } else if self.link.is_open() {
            return Ok(());
        }
        self.force_window.store(true, Ordering::Release);
        self.connect();
        Ok(())
    }

    /// Start discovery and connection on a background thread, if ON and not
    /// already connected or connecting. Every connection means a new consent
    /// dialog in the browser; there is no automatic retry loop (spec 4).
    pub fn connect(self: &Arc<Self>) {
        self.start_connect(false);
    }

    /// A connection attempt started by an agent request (spec 4: the next
    /// request after a drop starts a new discovery). It may show the consent
    /// dialog at most once per drop; after that only the user's Connect does.
    fn connect_automatically(self: &Arc<Self>) {
        self.start_connect(true);
    }

    fn start_connect(self: &Arc<Self>, automatic: bool) {
        if !self.policy.read().toggle_on || self.link.is_open() {
            return;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let mine = generation + 1;
        let running = self.connecting.load(Ordering::Acquire);
        // One attempt per generation; a stale one (from before a toggle
        // change) does not hold up the new one.
        if running == mine
            || self
                .connecting
                .compare_exchange(running, mine, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let host = self.clone();
        let spawned = thread::Builder::new()
            .name("browser-connect".into())
            .spawn(move || {
                host.run_connect(automatic, generation);
                let _ =
                    host.connecting
                        .compare_exchange(mine, 0, Ordering::AcqRel, Ordering::Acquire);
            });
        if spawned.is_err() {
            let _ = self
                .connecting
                .compare_exchange(mine, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    /// [`Self::set_connection`] for a connection attempt: dropped when the
    /// toggle changed since the attempt started.
    fn attempt_state(
        &self,
        generation: u64,
        state: ConnectionState,
        detail: Option<String>,
        browser: Option<BrowserInfo>,
        last_error: Option<String>,
    ) {
        if self.current(generation) {
            self.set_connection(state, detail, browser, last_error);
        }
    }

    /// Discovery (spec 4 steps 1-4) then [`Self::connect_endpoint`].
    fn run_connect(self: &Arc<Self>, automatic: bool, generation: u64) {
        if self.mode().launches() {
            self.run_connect_managed(generation);
            return;
        }
        let before = self.connection();
        let choice = self.policy.read().settings.browser.clone();
        self.attempt_state(
            generation,
            ConnectionState::Discovering,
            Some("Looking for Chrome or Edge…".into()),
            None,
            None,
        );
        match discovery::locate(&choice) {
            Located::NoBrowser => self.attempt_state(
                generation,
                ConnectionState::NeedsToggle,
                Some(format!(
                    "No Chrome or Edge profile was found. Install Chrome or Edge, or choose the profile folder in Settings. {}",
                    discovery::setup_instruction(None)
                )),
                None,
                None,
            ),
            Located::NeedsToggle { candidates } => {
                let kind = candidates.first().map(|c| c.kind);
                match kind.and_then(discovery::policy_blocks_debugging) {
                    Some(policy) => self.attempt_state(
                        generation,
                        ConnectionState::PolicyBlocked,
                        Some(format!(
                            "An administrator policy blocks remote debugging ({policy}). Ask your administrator; MonoCode never works around policies."
                        )),
                        None,
                        None,
                    ),
                    None => self.attempt_state(
                        generation,
                        ConnectionState::NeedsToggle,
                        Some(discovery::setup_instruction(kind)),
                        None,
                        None,
                    ),
                }
            }
            Located::StalePort { candidate, port } => self.attempt_state(
                generation,
                ConnectionState::StalePort,
                Some(format!(
                    "{} left a DevToolsActivePort file (port {port}) but nothing answers there: the browser is closed or the file is stale. Start the browser and retry.",
                    candidate.channel
                )),
                None,
                None,
            ),
            Located::Found { .. }
                if automatic && !self.auto_dialog.swap(false, Ordering::AcqRel) =>
            {
                // Discovery is harmless; another consent dialog is not.
                self.event(
                    "auto_connect_skipped",
                    Actor::System,
                    None,
                    Some("consent dialog already shown once since the last drop"),
                );
                self.attempt_state(
                    generation,
                    before.state,
                    Some("The browser already asked for permission once since the connection ended; ask the user to click Connect in MonoCode Settings.".into()),
                    None,
                    None,
                );
            }
            Located::Found {
                candidate, ws_url, ..
            } => self.connect_endpoint(candidate, &ws_url, generation),
        }
    }

    /// Managed mode: reuse the MonoCode browser when it already runs, start
    /// it otherwise, then connect like to any other endpoint. No consent
    /// dialog: the browser was started by us with its own profile.
    fn run_connect_managed(self: &Arc<Self>, generation: u64) {
        // A browser still shutting down holds its profile.
        self.finish_closing();
        let private = self.mode() == BrowserMode::Private;
        let profile = if private {
            // The private browser still running keeps its profile; a new one
            // gets a fresh folder.
            let current = self
                .managed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .filter(|managed| managed.ephemeral)
                .map(|managed| managed.profile.clone());
            current
                .filter(|dir| launch::running(dir).is_some())
                .or_else(|| self.profile_dir().map(|dir| launch::private_dir(&dir)))
        } else {
            self.profile_dir()
        };
        let Some(profile) = profile else {
            self.attempt_state(
                generation,
                ConnectionState::StalePort,
                Some("MonoCode has no folder for its browser profile.".into()),
                None,
                None,
            );
            return;
        };
        self.attempt_state(
            generation,
            ConnectionState::Discovering,
            Some("Starting the MonoCode browser…".into()),
            None,
            None,
        );
        let choice = self.policy.read().settings.browser.clone();
        let exe = launch::find_executable(&choice);
        let kind = exe.as_ref().map(|exe| exe.kind);
        let active = match launch::running(&profile) {
            Some(active) => active,
            None => {
                let Some(exe) = exe else {
                    self.attempt_state(
                        generation,
                        ConnectionState::StalePort,
                        Some("Chrome or Edge was not found on this computer. Install one of them, or switch to your own browser in Settings.".into()),
                        None,
                        Some("no browser executable".into()),
                    );
                    return;
                };
                let show_window = self.policy.read().settings.show_window
                    || self.force_window.swap(false, Ordering::AcqRel);
                let mut child = match launch::start(&exe, &profile, show_window) {
                    Ok(child) => child,
                    Err(error) => {
                        self.attempt_state(
                            generation,
                            ConnectionState::StalePort,
                            Some(format!(
                                "The MonoCode browser could not start ({}).",
                                exe.path.display()
                            )),
                            None,
                            Some(error.to_string()),
                        );
                        return;
                    }
                };
                self.event(
                    "managed_started",
                    Actor::System,
                    None,
                    Some(&exe.path.display().to_string()),
                );
                let ready = launch::wait_for_endpoint(&mut child, &profile, launch::START_TIMEOUT);
                let previous = self
                    .managed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .replace(ManagedBrowser {
                        child,
                        show_window,
                        profile: profile.clone(),
                        ephemeral: private,
                    });
                if let Some(mut previous) = previous {
                    if previous.exited() {
                        previous.forget();
                    }
                }
                match ready {
                    Ok(active) => active,
                    Err(message) => {
                        self.close_managed_browser();
                        self.attempt_state(
                            generation,
                            ConnectionState::StalePort,
                            Some(format!("The MonoCode browser did not start. {message}")),
                            None,
                            Some(message),
                        );
                        return;
                    }
                }
            }
        };
        if !self.current(generation) {
            return;
        }
        let candidate = launch::candidate(&profile, kind.unwrap_or(protocol::BrowserKind::Chrome));
        self.connect_endpoint(candidate, &discovery::ws_url(&active), generation);
    }

    /// Spec 4 steps 5-8: open the socket (consent dialog), check the version,
    /// read the target list once, start the event pump.
    pub fn connect_endpoint(self: &Arc<Self>, candidate: Candidate, ws_url: &str, generation: u64) {
        let managed = candidate.channel == launch::CHANNEL;
        if managed {
            self.attempt_state(
                generation,
                ConnectionState::Discovering,
                Some("Connecting to the MonoCode browser…".into()),
                None,
                None,
            );
        } else {
            self.attempt_state(
                generation,
                ConnectionState::AwaitingConsent,
                Some("Confirm the remote debugging permission in your browser.".into()),
                None,
                None,
            );
            self.event(
                "consent_pending",
                Actor::System,
                None,
                Some(&format!(
                    "{} ({})",
                    candidate.channel,
                    candidate.user_data_dir.display()
                )),
            );
        }
        if !self.current(generation) {
            return;
        }
        let consent_timeout =
            Duration::from_millis(self.consent_timeout_ms.load(Ordering::Acquire));
        match self.link.open(ws_url, consent_timeout) {
            Ok(()) => {}
            Err(OpenError::AlreadyOpen) => return,
            Err(OpenError::ConsentTimeout) => {
                self.event("consent_timeout", Actor::Error, None, None);
                self.attempt_state(
                    generation,
                    ConnectionState::ConsentTimeout,
                    Some("The browser's permission dialog was not answered in time. Click Connect to try again.".into()),
                    None,
                    Some("consent timeout".into()),
                );
                return;
            }
            Err(OpenError::Refused(message)) => {
                self.event("consent_denied", Actor::User, None, Some(&message));
                self.attempt_state(
                    generation,
                    ConnectionState::ConsentDenied,
                    Some("The browser refused the connection. Turn browser access on again when you are ready.".into()),
                    None,
                    Some(message),
                );
                return;
            }
            Err(OpenError::Unreachable(message)) => {
                self.attempt_state(
                    generation,
                    ConnectionState::StalePort,
                    Some("The browser endpoint went away before the connection opened. Start the browser and retry.".into()),
                    None,
                    Some(message),
                );
                return;
            }
            Err(OpenError::Aborted) => {
                let state = if self.policy.read().toggle_on {
                    ConnectionState::Disconnected
                } else {
                    ConnectionState::Off
                };
                self.attempt_state(
                    generation,
                    state,
                    Some("Connection attempt cancelled.".into()),
                    None,
                    None,
                );
                return;
            }
        }
        if !self.current(generation) {
            // The toggle moved while the user was answering the dialog.
            self.link.close(Vec::new(), OFF_DETACH_BUDGET);
            return;
        }
        let version = self
            .link
            .send_system(Command::browser(Method::BrowserGetVersion, json!({})))
            .ok();
        let product = version
            .as_ref()
            .and_then(|v| v["product"].as_str())
            .unwrap_or("")
            .to_string();
        let major = discovery::parse_version(&product)
            .map(|(_, major)| major)
            .unwrap_or(0);
        if !discovery::version_supported(major) {
            self.link.close(Vec::new(), OFF_DETACH_BUDGET);
            self.event("unsupported_version", Actor::Error, None, Some(&product));
            self.attempt_state(
                generation,
                ConnectionState::UnsupportedVersion,
                Some(format!(
                    "{} is too old; Chrome or Edge {MIN_BROWSER_MAJOR} or newer is required.",
                    if product.is_empty() {
                        "This browser"
                    } else {
                        product.as_str()
                    }
                )),
                None,
                Some(format!("product {product:?}")),
            );
            return;
        }
        // Diagnostics only (spec 4 step 7): no attach, no adoption.
        let _ = self
            .link
            .send_system(Command::browser(Method::TargetGetTargets, json!({})));
        let info = BrowserInfo {
            kind: candidate.kind,
            channel: candidate.channel.clone(),
            product: product.clone(),
            major,
            user_data_dir: candidate.user_data_dir.display().to_string(),
            tabs_total: 0,
            managed,
        };
        let weak = Arc::downgrade(self);
        let _ = targets::spawn_pump(
            self.link.clone(),
            Arc::new(move |event| {
                if let Some(host) = weak.upgrade() {
                    host.on_tab_event(event);
                }
            }),
        );
        self.auto_dialog.store(true, Ordering::Release);
        self.event("connected", Actor::User, None, Some(&product));
        self.attempt_state(
            generation,
            ConnectionState::Connected,
            Some(product),
            Some(info),
            None,
        );
    }

    /// Settings "Disconnect": close the link while the toggle stays ON. The
    /// next agent request or Connect click starts a new discovery.
    pub fn disconnect(self: &Arc<Self>) {
        self.event("disconnect", Actor::User, None, None);
        if self.link.status() != LinkStatus::Closed {
            let detach: Vec<CdpSessionId> = self
                .policy
                .read()
                .leases()
                .filter(|l| l.is_live())
                .filter_map(|l| l.cdp_session.clone())
                .collect();
            // The on_closed hook moves the state to Disconnected.
            self.link.close(detach, OFF_DETACH_BUDGET);
        } else {
            self.on_link_closed(ClosedBy::Us);
        }
    }

    /// Link thread hook: socket gone (ours or the browser's). Epoch up,
    /// leases cleared; tabs in the browser stay (spec 4 "Disconnected").
    fn on_link_closed(self: &Arc<Self>, by: ClosedBy) {
        self.policy.write().connection_lost();
        let toggle_on = self.policy.read().toggle_on;
        let managed = self.mode().launches();
        if managed {
            self.reap_managed_browser();
        }
        let (state, detail) = if toggle_on && managed {
            (
                ConnectionState::Idle,
                "The MonoCode browser closed; it starts again when an agent needs it.",
            )
        } else if toggle_on {
            (
                ConnectionState::Disconnected,
                "Connection to the browser ended; the next request asks for a new permission in the browser.",
            )
        } else {
            (ConnectionState::Off, "Browser access is off.")
        };
        let actor = match by {
            ClosedBy::Us => Actor::System,
            ClosedBy::Peer => Actor::User,
            ClosedBy::Error => Actor::Error,
        };
        self.event("disconnected", actor, None, Some(&format!("{by:?}")));
        let current = self.connection().state;
        if current != ConnectionState::Off || toggle_on {
            self.set_connection(state, Some(detail.into()), None, None);
        }
    }

    fn on_tab_event(&self, event: TabEvent) {
        match &event {
            TabEvent::PopupOpened { tab, owner, opener } => {
                let lease = self.policy.read().lease(tab).cloned();
                if let Some(lease) = lease {
                    self.db.with(|conn| {
                        audit::lease_opened(
                            conn,
                            self.run_id,
                            tab,
                            &lease.target_id.0,
                            owner,
                            Some(opener),
                            &lease.url,
                        )
                    });
                }
            }
            TabEvent::PopupBlocked { owner, opener } => {
                self.event(
                    "popup_blocked",
                    Actor::System,
                    Some(owner),
                    Some(&format!("opener {}", opener.0)),
                );
            }
            TabEvent::Closed(tab) | TabEvent::Crashed(tab) => {
                self.db.with(|conn| audit::lease_closed(conn, tab));
            }
            TabEvent::Navigated(_) | TabEvent::DialogOpened(_) | TabEvent::DialogClosed(_) => {}
        }
        self.emit_state();
    }

    // --- permissions (spec 6, 13) ----------------------------------------------

    /// `/browser` on or off for a session (decision D2).
    pub fn set_grant(&self, session: &str, window: &str, enabled: bool) {
        let was_granted = self.policy.read().granted(session);
        let detach = self.policy.write().set_grant(session, window, enabled);
        if self.link.is_open() {
            targets::detach_sessions(&self.link, &detach);
        }
        // Recorded only when the grant actually changed.
        if enabled != was_granted {
            self.event(
                if enabled { "grant_on" } else { "grant_off" },
                Actor::User,
                Some(session),
                None,
            );
        }
        self.emit_state();
    }

    /// Settings "Revoke": the session loses its grant and leases; tabs stay.
    pub fn revoke(&self, session: &str) {
        let detach = self.policy.write().revoke_session(session);
        if self.link.is_open() {
            targets::detach_sessions(&self.link, &detach);
        }
        self.event("revoked", Actor::User, Some(session), None);
        self.emit_state();
    }

    /// Hand a tab to another session (spec 7 "Primopredaja").
    pub fn transfer(&self, tab: &TabId, from: &str, to: &str) -> Result<(), Reason> {
        self.policy.write().transfer(tab, from, to)?;
        // The new owner starts from its own snapshot (spec 7 "Primopredaja").
        semantic::forget_refs(&self.link, tab);
        self.db.with(|conn| audit::lease_transferred(conn, tab, to));
        self.event(
            "transfer",
            Actor::User,
            Some(from),
            Some(&format!("{} -> {to}", tab.0)),
        );
        self.emit_state();
        Ok(())
    }

    /// Settings "Take the tab back" (spec 13.1): the lease is released, the
    /// marker comes off, we detach, and the tab stays open for the user. The
    /// owning session's next action on it gets `no_lease`.
    pub fn release_tab(&self, tab: &TabId) -> Result<(), String> {
        let (owner, main_frame) = {
            let state = self.policy.read();
            let lease = state.lease(tab).ok_or("Unknown tab")?;
            (lease.owner.clone(), lease.main_frame_id.clone())
        };
        let detach = self
            .policy
            .write()
            .release_lease(tab)
            .map_err(|reason| reason.message().to_string())?;
        if let Some(session) = detach {
            if self.link.is_open() {
                if let Some(frame) = main_frame.as_deref() {
                    let _ = targets::release_marker(&self.link, &session, frame);
                }
                targets::detach_sessions(&self.link, std::slice::from_ref(&session));
            }
        }
        self.db.with(|conn| audit::lease_closed(conn, tab));
        self.event("tab_released", Actor::User, Some(&owner), Some(&tab.0));
        self.emit_state();
        Ok(())
    }

    /// Settings "Close tab" for any agent tab, whoever owns it.
    pub fn close_tab_by_user(&self, tab: &TabId) -> Result<(), String> {
        let target = {
            let state = self.policy.read();
            let lease = state.lease(tab).ok_or("Unknown tab")?;
            if lease.state == TabState::Closed {
                return Err("Tab is already closed".into());
            }
            lease.target_id.clone()
        };
        self.close_owned_targets(&[target]);
        if let Some(lease) = self.policy.write().lease_mut(tab) {
            lease.state = TabState::Closed;
            lease.busy = false;
        }
        self.db.with(|conn| audit::lease_closed(conn, tab));
        self.event("tab_closed", Actor::User, None, Some(&tab.0));
        self.emit_state();
        Ok(())
    }

    /// Settings "Close all agent tabs".
    pub fn close_all_tabs(&self) {
        let (targets, tabs): (Vec<TargetId>, Vec<TabId>) = {
            let state = self.policy.read();
            state
                .leases()
                .filter(|l| l.state != TabState::Closed)
                .map(|l| (l.target_id.clone(), l.tab.clone()))
                .unzip()
        };
        self.close_owned_targets(&targets);
        {
            let mut state = self.policy.write();
            for tab in &tabs {
                if let Some(lease) = state.lease_mut(tab) {
                    lease.state = TabState::Closed;
                    lease.busy = false;
                }
            }
        }
        for tab in &tabs {
            self.db.with(|conn| audit::lease_closed(conn, tab));
        }
        self.event("tabs_closed_all", Actor::User, None, None);
        self.emit_state();
    }

    fn close_owned_targets(&self, targets: &[TargetId]) {
        if self.link.is_open() {
            targets::close_targets(&self.link, targets);
        }
    }

    pub fn set_settings(self: &Arc<Self>, mut settings: BrowserSettings) {
        let (mode_changed, toggle_on) = {
            let mut policy = self.policy.write();
            // The switch has its own command; settings never flip it.
            settings.enabled = policy.toggle_on;
            let changed = policy.settings.mode != settings.mode;
            policy.settings = settings.clone();
            (changed, policy.toggle_on)
        };
        self.db.with(|conn| audit::save_settings(conn, &settings));
        if mode_changed {
            // The other kind of browser from now on: drop the current link
            // (and the MonoCode browser) and wait for the next agent action.
            self.event(
                "mode_changed",
                Actor::User,
                None,
                Some(&format!("{:?}", settings.mode)),
            );
            self.close_managed_pages();
            if self.link.status() != LinkStatus::Closed {
                self.disconnect();
            }
            self.close_managed_browser_later();
            if toggle_on {
                self.set_idle();
            }
        }
        self.emit_state();
    }

    // --- hooks from control.rs / session_store.rs ------------------------------

    /// Turn start (`control_authorize_turn`): the session gets or keeps its
    /// token; `browser_access` sets the grant when the frontend passes it.
    pub fn turn_started(
        &self,
        session: &str,
        window: &str,
        cwd: &str,
        browser_access: Option<bool>,
    ) {
        // The audit records transitions, not turns: a frontend may authorize
        // the same turn several times, and most turns keep the grant as is.
        let was_granted = self.policy.read().granted(session);
        let dropping = browser_access == Some(false) && was_granted;
        let detach: Vec<CdpSessionId> = if dropping {
            self.policy
                .read()
                .leases()
                .filter(|l| l.owner == session && l.is_live())
                .filter_map(|l| l.cdp_session.clone())
                .collect()
        } else {
            Vec::new()
        };
        self.policy
            .write()
            .turn_started(session, window, cwd, browser_access);
        if dropping {
            if self.link.is_open() {
                targets::detach_sessions(&self.link, &detach);
            }
            self.event(
                "grant_off",
                Actor::User,
                Some(session),
                Some("turn without browser access"),
            );
        } else if !was_granted && self.policy.read().granted(session) {
            self.event("grant_on", Actor::User, Some(session), None);
        }
        self.emit_state();
    }

    pub fn turn_finished(&self, session: &str) {
        self.policy.write().turn_finished(session);
        self.emit_state();
    }

    pub fn window_closed(&self, window: &str) {
        let detach = self.policy.write().window_closed(window);
        if self.link.is_open() {
            targets::detach_sessions(&self.link, &detach);
        }
        self.emit_state();
    }

    /// Session deleted: its grant goes and its tabs are closed.
    pub fn session_deleted(&self, session: &str) {
        let tabs: Vec<TabId> = self
            .policy
            .read()
            .leases()
            .filter(|l| l.owner == session && l.state != TabState::Closed)
            .map(|l| l.tab.clone())
            .collect();
        let close = self.policy.write().session_deleted(session);
        self.close_owned_targets(&close);
        for tab in &tabs {
            self.db.with(|conn| audit::lease_closed(conn, tab));
        }
        self.event("session_deleted", Actor::System, Some(session), None);
        self.emit_state();
    }

    /// The session's `MONOCODE_BROWSER_TOKEN`, once a turn was authorised.
    pub fn token_for(&self, session: &str) -> Option<String> {
        self.policy.read().token_for(session).map(str::to_string)
    }

    pub fn session_for_token(&self, token: &str) -> Option<String> {
        self.policy.read().session_for_token(token)
    }

    /// Ask mode: put the request to the user in the session's thread and
    /// wait for "Allow" (true) or "Not now", the end of the turn, OFF or
    /// [`ASK_WAIT`] (false).
    fn ask_user(self: &Arc<Self>, session: &str) -> bool {
        let asked = {
            let mut policy = self.policy.write();
            policy.toggle_on && !policy.requested(session) && policy.request_grant(session)
        };
        if !asked {
            // Another action of the session is already waiting, or the
            // session cannot ask now.
            return self.wait_for_answer(session, false);
        }
        self.event("grant_requested", Actor::System, Some(session), None);
        self.emit_state();
        // Start the MonoCode browser while the user decides, so an Allow near
        // the end of the wait does not add a cold start on top of it.
        if self.mode().launches() {
            self.connect_automatically();
        }
        self.wait_for_answer(session, true)
    }

    fn wait_for_answer(self: &Arc<Self>, session: &str, owner: bool) -> bool {
        let deadline =
            Instant::now() + Duration::from_millis(self.ask_wait_ms.load(Ordering::Acquire));
        loop {
            {
                let policy = self.policy.read();
                if policy.granted(session) {
                    return true;
                }
                if !policy.toggle_on || !policy.requested(session) {
                    return false;
                }
            }
            if Instant::now() >= deadline {
                if owner {
                    self.policy.write().withdraw_request(session);
                    self.event("grant_request_expired", Actor::System, Some(session), None);
                    self.emit_state();
                    self.close_when_unused();
                }
                return false;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// The user's answer to an access request (`browser_answer_request`).
    pub fn answer_request(self: &Arc<Self>, session: &str, window: &str, allow: bool) {
        if allow {
            self.set_grant(session, window, true);
            return;
        }
        self.policy.write().decline_request(session);
        self.event("grant_declined", Actor::User, Some(session), None);
        self.emit_state();
        self.close_when_unused();
    }

    /// Wait until the running connection attempt ends, then report where it
    /// left the connection.
    fn wait_for_connection(&self, budget: Duration) -> ConnectionInfo {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            let connection = self.connection();
            if connection.state == ConnectionState::Connected
                || self.connecting.load(Ordering::Acquire) == 0
            {
                return connection;
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.connection()
    }

    // --- the request path (spec 6 "Životni ciklus zahteva", 9.2, 11) -----------

    /// A request arriving over the control transport with a session token.
    /// `Err` only for an unknown token; everything else is an `ActionResult`.
    pub fn handle_token_request(
        self: &Arc<Self>,
        token: &str,
        request: ActionRequest,
    ) -> Result<ActionResult, String> {
        let session = self
            .session_for_token(token)
            .ok_or("Connection revoked or unauthorized")?;
        Ok(self.handle_request(&session, request))
    }

    /// Run one agent action for `session`. Always returns exactly one result.
    pub fn handle_request(self: &Arc<Self>, session: &str, request: ActionRequest) -> ActionResult {
        let started = Instant::now();
        let request_id = request.request_id.clone();
        if request_id.is_empty() || request_id.len() > 128 {
            return ActionResult::from_reason(&request_id, Reason::InvalidRequest)
                .with_message("request_id must be 1-128 characters.");
        }
        let Some(kind) = ActionKind::parse(&request.action) else {
            let result = ActionResult::from_reason(&request_id, Reason::InvalidRequest)
                .with_message(format!("Unknown action `{}`.", request.action));
            return self.finish(session, None, None, &request, started, 0, result);
        };
        let common: CommonInput = match serde_json::from_value(request.input.clone()) {
            Ok(common) => common,
            Err(error) => {
                let result = ActionResult::from_reason(&request_id, Reason::InvalidRequest)
                    .with_message(format!("Invalid input: {error}"));
                return self.finish(session, Some(kind), None, &request, started, 0, result);
            }
        };
        // Bind first: a guard living in a `match` scrutinee would still be
        // held while `finish` takes the lock again.
        let dedup = self
            .policy
            .write()
            .dedup_check(session, &request_id, &request.input);
        match dedup {
            Dedup::Cached(result) => return *result,
            Dedup::Conflict => {
                let result = ActionResult::from_reason(&request_id, Reason::InvalidRequest)
                    .with_message(
                        "request_id was already used with different input; use a new id.",
                    );
                return self.finish(session, Some(kind), None, &request, started, 0, result);
            }
            Dedup::Fresh => {}
        }
        let timeout = kind.effective_timeout(common.timeout_ms);
        let mut admitted =
            self.policy
                .read()
                .admit(session, &request_id, kind, common.tab.as_ref(), timeout);
        if admitted.as_ref().err() == Some(&Reason::NotGranted)
            && self.policy.read().settings.access == AccessMode::Ask
        {
            // Admitted again whatever the answer: OFF or a revocation during
            // the wait is reported as such, not as `not_granted`.
            self.ask_user(session);
            admitted =
                self.policy
                    .read()
                    .admit(session, &request_id, kind, common.tab.as_ref(), timeout);
        }
        let mut ctx = match admitted {
            Ok(ctx) => ctx,
            Err(reason) => {
                let result = ActionResult::from_reason(&request_id, reason);
                return self.finish(session, Some(kind), None, &request, started, 0, result);
            }
        };
        let mut connection = self.connection();
        if self.mode().launches() && connection.state != ConnectionState::Connected {
            // The MonoCode browser starts on demand; the action waits for it.
            self.connect_automatically();
            connection = self.wait_for_connection(launch::START_TIMEOUT + Duration::from_secs(5));
            if connection.state == ConnectionState::Connected {
                // A fresh deadline and epochs for the action itself.
                match self.policy.read().admit(
                    session,
                    &request_id,
                    kind,
                    common.tab.as_ref(),
                    timeout,
                ) {
                    Ok(fresh) => ctx = fresh,
                    Err(reason) => {
                        let result = ActionResult::from_reason(&request_id, reason);
                        return self.finish(
                            session,
                            Some(kind),
                            None,
                            &request,
                            started,
                            0,
                            result,
                        );
                    }
                }
            }
        }
        if let Some(reason) = connection.state.agent_reason() {
            if self.mode() == BrowserMode::Attach
                && matches!(
                    connection.state,
                    ConnectionState::Disconnected
                        | ConnectionState::Idle
                        | ConnectionState::StalePort
                        | ConnectionState::NeedsToggle
                        | ConnectionState::ConsentTimeout
                )
            {
                // Spec 4: a new discovery starts on the next agent request.
                self.connect_automatically();
            }
            let mut result = ActionResult::from_reason(&request_id, reason);
            if let Some(detail) = connection.detail {
                result.message = format!("{} {detail}", result.message);
            }
            return self.finish(
                session,
                Some(kind),
                Some(&ctx),
                &request,
                started,
                0,
                result,
            );
        }
        // One action per tab, N per connection (spec 9.6): wait briefly, then `busy`.
        let busy_deadline = Instant::now() + BUSY_WAIT.min(timeout);
        loop {
            if self.policy.write().try_begin_action(ctx.tab.as_ref()) {
                break;
            }
            if Instant::now() >= busy_deadline {
                let result = ActionResult::from_reason(&request_id, Reason::Busy);
                return self.finish(
                    session,
                    Some(kind),
                    Some(&ctx),
                    &request,
                    started,
                    0,
                    result,
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
        let commands_before = self.link.commands_sent();
        let result = self.run_action(&ctx, kind, &request.input, commands_before);
        self.policy.write().end_action(ctx.tab.as_ref());
        let commands_sent = self.link.commands_sent().saturating_sub(commands_before);
        self.finish(
            session,
            Some(kind),
            Some(&ctx),
            &request,
            started,
            commands_sent,
            result,
        )
    }

    fn run_action(
        &self,
        ctx: &RequestCtx,
        kind: ActionKind,
        input: &Value,
        commands_before: u64,
    ) -> ActionResult {
        let request_id = ctx.request_id.as_str();
        match kind {
            ActionKind::TabsList => {
                if let Err(result) = parse_params::<TabsListParams>(request_id, input) {
                    return *result;
                }
                let tabs = self.policy.read().tabs_for(&ctx.session);
                let count = tabs.len();
                ActionResult::done(
                    request_id,
                    match count {
                        0 => "No open tabs. Use tabs.open to open one.".to_string(),
                        1 => "1 open tab.".to_string(),
                        n => format!("{n} open tabs."),
                    },
                )
                .with_data(json!({ "tabs": tabs }))
            }
            ActionKind::TabsOpen => {
                let params = match parse_params::<TabsOpenParams>(request_id, input) {
                    Ok(params) => params,
                    Err(result) => return *result,
                };
                match targets::open_tab(&self.link, ctx, params.url.as_deref(), WaitUntil::Load) {
                    Ok(opened) => {
                        let tab = opened.handle.tab.clone();
                        let url = opened.observed.url.clone().unwrap_or_default();
                        self.db.with(|conn| {
                            audit::lease_opened(
                                conn,
                                self.run_id,
                                &tab,
                                &opened.handle.target.0,
                                &ctx.session,
                                None,
                                &url,
                            )
                        });
                        let data = json!({
                            "tab": tab,
                            "url": url,
                            "title": opened.observed.title.clone().unwrap_or_default(),
                        });
                        match opened.navigation {
                            Ok(()) => ActionResult::done(
                                request_id,
                                format!(
                                    "Opened tab {} at {url}. Use tab: \"{}\" for actions on it.",
                                    tab.0, tab.0
                                ),
                            ),
                            Err((reason, message)) => ActionResult::from_reason(request_id, reason)
                                .with_message(format!(
                                    "{message} The tab {} is open; navigate it again or close it.",
                                    tab.0
                                )),
                        }
                        .with_data(data)
                        .with_observed(opened.observed)
                    }
                    Err(error) => error.into_result(
                        request_id,
                        true,
                        self.link.commands_sent() > commands_before,
                    ),
                }
            }
            ActionKind::TabsClose => {
                let params = match parse_params::<TabsCloseParams>(request_id, input) {
                    Ok(params) => params,
                    Err(result) => return *result,
                };
                match targets::close_tab(&self.link, ctx, &params.tab) {
                    Ok(()) => {
                        self.db.with(|conn| audit::lease_closed(conn, &params.tab));
                        ActionResult::done(request_id, format!("Closed tab {}.", params.tab.0))
                    }
                    Err(error) => error.into_result(
                        request_id,
                        true,
                        self.link.commands_sent() > commands_before,
                    ),
                }
            }
            ActionKind::TabsFocus => {
                let params = match parse_params::<TabsFocusParams>(request_id, input) {
                    Ok(params) => params,
                    Err(result) => return *result,
                };
                if params.user_visible != Some(true) {
                    return ActionResult::from_reason(request_id, Reason::InvalidRequest).with_message(
                        "tabs.focus changes what the user sees; pass user_visible: true to confirm. Other actions work on background tabs without focusing them.",
                    );
                }
                match targets::focus_tab(&self.link, ctx, &params.tab) {
                    Ok(()) => ActionResult::done(
                        request_id,
                        format!("Tab {} is now in front.", params.tab.0),
                    ),
                    Err(error) => error.into_result(
                        request_id,
                        false,
                        self.link.commands_sent() > commands_before,
                    ),
                }
            }
            _ => {
                let handle = if kind.requires_lease() {
                    let Some(tab) = ctx.tab.as_ref() else {
                        return ActionResult::from_reason(request_id, Reason::InvalidRequest)
                            .with_message("This action needs a `tab`.");
                    };
                    match targets::handle_for(&self.policy.read(), &ctx.session, tab) {
                        Ok(handle) => Some(handle),
                        Err(reason) => return ActionResult::from_reason(request_id, reason),
                    }
                } else {
                    None
                };
                let settings = self.policy.read().settings.clone();
                let result = semantic::execute(semantic::Exec {
                    link: &self.link,
                    ctx,
                    kind,
                    input,
                    handle: handle.as_ref(),
                    settings: &settings,
                    commands_before,
                });
                self.closed_mid_action(ctx, kind, commands_before, result)
            }
        }
    }

    /// The tab closed or crashed while the action ran (spec 14.1 row 3, T15):
    /// the send gate refuses the next command as `no_lease`, but for the
    /// agent this is `target_closed`, failed (or `unknown` when a mutation
    /// had already written), not a tab it never owned.
    fn closed_mid_action(
        &self,
        ctx: &RequestCtx,
        kind: ActionKind,
        commands_before: u64,
        result: ActionResult,
    ) -> ActionResult {
        if result.reason != Some(Reason::NoLease) {
            return result;
        }
        let gone = ctx.tab.as_ref().is_some_and(|tab| {
            self.policy.read().lease(tab).is_some_and(|l| {
                l.owner == ctx.session && matches!(l.state, TabState::Closed | TabState::Crashed)
            })
        });
        if !gone {
            return result;
        }
        let wrote = self.link.commands_sent() > commands_before;
        let outcome = if kind.mutates() && wrote {
            Outcome::Unknown
        } else {
            Outcome::Failed
        };
        let mut closed =
            ActionResult::from_reason(&ctx.request_id, Reason::TargetClosed).with_outcome(outcome);
        closed.observed = result.observed;
        closed
    }

    /// Timing, audit row, dedup entry, lease cleanup, state emit.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        session: &str,
        kind: Option<ActionKind>,
        ctx: Option<&RequestCtx>,
        request: &ActionRequest,
        started: Instant,
        commands_sent: u64,
        mut result: ActionResult,
    ) -> ActionResult {
        result.timing_ms = started.elapsed().as_millis() as u64;
        let tab = ctx.and_then(|c| c.tab.clone()).or_else(|| {
            result
                .data
                .as_ref()
                .and_then(|d| d.get("tab"))
                .and_then(Value::as_str)
                .map(|t| TabId(t.to_string()))
        });
        let url = {
            let state = self.policy.read();
            tab.as_ref()
                .and_then(|t| state.lease(t))
                .map(|l| audit_url(&l.url))
                .or_else(|| result.observed.url.as_deref().map(audit_url))
        };
        let (global_epoch, session_epoch) = match ctx {
            Some(ctx) => (ctx.global_epoch, ctx.session_epoch),
            None => {
                let state = self.policy.read();
                (
                    state.global_epoch,
                    state.session_epoch(session).unwrap_or(protocol::Epoch {
                        run: self.run_id,
                        seq: 0,
                    }),
                )
            }
        };
        let params = kind
            .map(|k| audit::redact_params(k, &request.input))
            .unwrap_or_else(|| json!({}));
        let action_name = kind
            .map(|k| k.name().to_string())
            .unwrap_or_else(|| request.action.clone());
        self.db.with(|conn| {
            audit::record(
                conn,
                &audit::AuditRecord {
                    session_id: session,
                    request_id: &request.request_id,
                    global_epoch,
                    session_epoch,
                    action: &action_name,
                    tab: tab.as_ref(),
                    url: url.as_deref(),
                    params: &params,
                    outcome: result.outcome,
                    reason: result.reason,
                    duration_ms: result.timing_ms as i64,
                    commands_sent: commands_sent as i64,
                },
            )
        });
        // Dedup protects against executing a mutation twice (spec 11.4). A
        // `blocked` result executed nothing, so a retry with the same id after
        // the user acted (toggle, consent, grant) must run again.
        if result.outcome != Outcome::Blocked {
            self.policy
                .write()
                .dedup_store(session, &request.request_id, &request.input, &result);
        }
        // Closed tabs vanish from the list after the response that reported them (spec 7).
        if result.reason == Some(Reason::TargetClosed)
            || matches!(kind, Some(ActionKind::TabsList | ActionKind::TabsClose))
        {
            self.policy.write().forget_closed(session);
        }
        if matches!(
            kind,
            Some(
                ActionKind::TabsOpen
                    | ActionKind::TabsClose
                    | ActionKind::TabsList
                    | ActionKind::TabsFocus
            )
        ) || result.observed.new_tab.is_some()
        {
            self.emit_state();
        }
        result
    }

    pub fn audit_list(&self, session: Option<&str>, limit: u32) -> Vec<AuditEntry> {
        self.db
            .with(|conn| audit::list(conn, session, limit))
            .unwrap_or_default()
    }

    pub fn events_list(&self, limit: u32) -> Vec<audit::EventEntry> {
        self.db
            .with(|conn| audit::events(conn, limit))
            .unwrap_or_default()
    }

    pub fn audit_clear(&self) {
        self.db.with(audit::clear);
        self.event("audit_cleared", Actor::User, None, None);
    }
}

fn parse_params<T: DeserializeOwned>(
    request_id: &str,
    input: &Value,
) -> Result<T, Box<ActionResult>> {
    serde_json::from_value(input.clone()).map_err(|error| {
        Box::new(
            ActionResult::from_reason(request_id, Reason::InvalidRequest)
                .with_message(format!("Invalid input: {error}")),
        )
    })
}

// ---------------------------------------------------------------------------
// Hooks for the rest of the app (no-ops before `init`)
// ---------------------------------------------------------------------------

/// `control_authorize_turn`: prepare the session token and apply `browser_access`.
pub fn turn_started(session: &str, window: &str, cwd: &str, browser_access: Option<bool>) {
    if let Some(host) = host() {
        host.turn_started(session, window, cwd, browser_access);
    }
}

/// `control_turn_finished`: in-flight actions of the session end as `unknown`.
pub fn turn_finished(session: &str) {
    if let Some(host) = host() {
        host.turn_finished(session);
    }
}

pub fn window_closed(window: &str) {
    if let Some(host) = host() {
        host.window_closed(window);
    }
}

pub fn session_deleted(session: &str) {
    if let Some(host) = host() {
        host.session_deleted(session);
    }
}

/// The token to inject as [`protocol::ENV_TOKEN`] into a provider process.
pub fn child_token(session: &str) -> Option<String> {
    host().and_then(|host| host.token_for(session))
}

/// Control transport entry point for `namespace = "browser"`.
pub fn handle_token_request(token: &str, request: ActionRequest) -> Result<ActionResult, String> {
    let host = host().ok_or("Browser module is not available")?;
    host.handle_token_request(token, request)
}

// ---------------------------------------------------------------------------
// Tauri commands (spec 13.3)
// ---------------------------------------------------------------------------

fn require_host() -> Result<Arc<BrowserHost>, String> {
    host().ok_or_else(|| "Browser module is not initialised".to_string())
}

#[tauri::command]
pub fn browser_toggle(on: bool) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.set_toggle(on);
    Ok(host.state())
}

#[tauri::command]
pub fn browser_status() -> Result<BrowserState, String> {
    Ok(require_host()?.state())
}

#[tauri::command]
pub fn browser_tabs() -> Result<Vec<TabOverview>, String> {
    Ok(require_host()?.policy().read().overview())
}

#[tauri::command]
pub fn browser_revoke(session_id: String) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.revoke(&session_id);
    Ok(host.state())
}

#[tauri::command]
pub fn browser_transfer(
    tab: String,
    from_session: String,
    to_session: String,
) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.transfer(&TabId(tab), &from_session, &to_session)
        .map_err(|reason| reason.message().to_string())?;
    Ok(host.state())
}

#[tauri::command]
pub fn browser_release_tab(tab: String) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.release_tab(&TabId(tab))?;
    Ok(host.state())
}

#[tauri::command]
pub fn browser_connect() -> Result<BrowserState, String> {
    let host = require_host()?;
    host.connect();
    Ok(host.state())
}

#[tauri::command]
pub fn browser_disconnect() -> Result<BrowserState, String> {
    let host = require_host()?;
    host.disconnect();
    Ok(host.state())
}

#[tauri::command]
pub fn browser_set_session_grant(
    window: WebviewWindow,
    session_id: String,
    enabled: bool,
) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.set_grant(&session_id, window.label(), enabled);
    Ok(host.state())
}

#[tauri::command]
pub fn browser_answer_request(
    window: WebviewWindow,
    session_id: String,
    allow: bool,
) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.answer_request(&session_id, window.label(), allow);
    Ok(host.state())
}

#[tauri::command]
pub fn browser_open_window() -> Result<BrowserState, String> {
    let host = require_host()?;
    host.open_window()?;
    Ok(host.state())
}

#[tauri::command]
pub fn browser_close_tab(tab: String) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.close_tab_by_user(&TabId(tab))?;
    Ok(host.state())
}

#[tauri::command]
pub fn browser_close_all_tabs() -> Result<BrowserState, String> {
    let host = require_host()?;
    host.close_all_tabs();
    Ok(host.state())
}

#[tauri::command]
pub fn browser_audit_list(
    session_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<AuditEntry>, String> {
    Ok(require_host()?.audit_list(
        session_id.as_deref(),
        limit.unwrap_or(audit::DEFAULT_LIST_LIMIT),
    ))
}

#[tauri::command]
pub fn browser_events_list(limit: Option<u32>) -> Result<Vec<audit::EventEntry>, String> {
    Ok(require_host()?.events_list(limit.unwrap_or(audit::DEFAULT_LIST_LIMIT)))
}

#[tauri::command]
pub fn browser_audit_clear() -> Result<(), String> {
    require_host()?.audit_clear();
    Ok(())
}

#[tauri::command]
pub fn browser_set_settings(settings: BrowserSettings) -> Result<BrowserState, String> {
    let host = require_host()?;
    host.set_settings(settings);
    Ok(host.state())
}

#[cfg(test)]
mod tests {
    use super::protocol::{BrowserChoice, TabInfo, ISOLATED_WORLD};
    use super::testing::{FakeCdp, Response};
    use super::*;

    /// A profile directory whose `DevToolsActivePort` points at the fake.
    fn profile_for(server: &FakeCdp) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("monocode-host-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let rest = server.url.strip_prefix("ws://127.0.0.1:").unwrap();
        let (port, path) = rest.split_once('/').unwrap();
        std::fs::write(
            dir.join(discovery::ACTIVE_PORT_FILE),
            format!("{port}\n/{path}\n"),
        )
        .unwrap();
        dir
    }

    fn settings_for(dir: &std::path::Path) -> BrowserSettings {
        BrowserSettings {
            browser: BrowserChoice::UserDataDir {
                path: dir.display().to_string(),
            },
            mode: BrowserMode::Attach,
            access: AccessMode::Always,
            ..BrowserSettings::default()
        }
    }

    fn wait_state(host: &BrowserHost, state: ConnectionState) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if host.connection().state == state {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn request(id: &str, action: &str, input: Value) -> ActionRequest {
        ActionRequest {
            request_id: id.into(),
            action: action.into(),
            input,
        }
    }

    #[test]
    fn toggle_on_discovers_connects_and_off_detaches_and_blocks() {
        let server = FakeCdp::start();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        assert_eq!(host.connection().state, ConnectionState::Off);
        // Requests while OFF never reach anything.
        host.turn_started("s1", "main", "/repo", Some(true));
        let off = host.handle_request("s1", request("r0", "tabs.list", json!({})));
        assert_eq!(off.reason, Some(Reason::Off));
        assert_eq!(off.outcome, Outcome::Blocked);

        host.set_toggle(true);
        assert!(
            wait_state(&host, ConnectionState::Connected),
            "{:?}",
            host.connection()
        );
        let connection = host.connection();
        let info = connection.browser.unwrap();
        assert_eq!(info.major, 144);
        assert_eq!(info.channel, "custom");
        assert!(info.product.starts_with("Chrome/"));
        let methods: Vec<String> = server.received().into_iter().map(|r| r.method).collect();
        assert_eq!(methods, vec!["Browser.getVersion", "Target.getTargets"]);
        let token = host.token_for("s1").unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(host.session_for_token(&token).as_deref(), Some("s1"));
        assert!(host
            .handle_token_request("nope", request("x", "tabs.list", json!({})))
            .is_err());

        // tabs.open → tabs.list → tabs.close through the token path.
        let opened = host
            .handle_token_request(&token, request("r1", "tabs.open", json!({})))
            .unwrap();
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        let tab = opened.data.as_ref().unwrap()["tab"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(TabId(tab.clone()).is_well_formed());
        let listed = host.handle_request("s1", request("r2", "tabs.list", json!({})));
        let tabs: Vec<TabInfo> =
            serde_json::from_value(listed.data.unwrap()["tabs"].clone()).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].tab.0, tab);
        assert_eq!(tabs[0].state, TabState::Open);
        // Same request id, same input: cached, no second tab.
        let again = host.handle_request("s1", request("r1", "tabs.open", json!({})));
        assert_eq!(again.data.as_ref().unwrap()["tab"], json!(tab));
        assert_eq!(host.policy().read().tabs_for("s1").len(), 1);
        // Same request id, different input: invalid.
        let conflict = host.handle_request(
            "s1",
            request("r1", "tabs.open", json!({ "url": "https://x/" })),
        );
        assert_eq!(conflict.reason, Some(Reason::InvalidRequest));
        // Another session sees nothing and cannot touch the tab.
        host.turn_started("s2", "main", "/repo", Some(true));
        let foreign = host.handle_request("s2", request("f1", "tabs.close", json!({ "tab": tab })));
        assert_eq!(foreign.reason, Some(Reason::NoLease));
        let empty = host.handle_request("s2", request("f2", "tabs.list", json!({})));
        assert_eq!(empty.data.unwrap()["tabs"], json!([]));
        // Unknown action and unknown fields are invalid requests, audited.
        let unknown = host.handle_request("s1", request("u1", "teleport", json!({})));
        assert_eq!(unknown.reason, Some(Reason::InvalidRequest));
        let typo = host.handle_request("s1", request("u2", "tabs.open", json!({ "urll": "x" })));
        assert_eq!(typo.reason, Some(Reason::InvalidRequest));
        // Focus needs the explicit flag.
        let focus = host.handle_request("s1", request("fo1", "tabs.focus", json!({ "tab": tab })));
        assert_eq!(focus.reason, Some(Reason::InvalidRequest));
        let focus = host.handle_request(
            "s1",
            request(
                "fo2",
                "tabs.focus",
                json!({ "tab": tab, "user_visible": true }),
            ),
        );
        assert_eq!(focus.outcome, Outcome::Done, "{focus:?}");
        assert_eq!(
            server.received().last().unwrap().method,
            "Page.bringToFront"
        );
        // Semantic actions reach the semantic layer: a ref before any
        // snapshot is stale, and nothing is sent for it.
        let before = server.received().len();
        let click = host.handle_request(
            "s1",
            request("c1", "click", json!({ "tab": tab, "ref": "e1" })),
        );
        assert_eq!(click.reason, Some(Reason::RefStale));
        assert_eq!(server.received().len(), before);
        // Turn end: blocked as turn_inactive, nothing sent.
        let before = server.received().len();
        host.turn_finished("s1");
        let inactive = host.handle_request("s1", request("t1", "tabs.list", json!({})));
        assert_eq!(inactive.reason, Some(Reason::TurnInactive));
        assert_eq!(server.received().len(), before);
        host.turn_started("s1", "main", "/repo", None);

        // OFF: detach our session, close the link, tabs stay open in the browser.
        host.set_toggle(false);
        assert_eq!(host.connection().state, ConnectionState::Off);
        assert_eq!(host.link().status(), LinkStatus::Closed);
        let received = server.received();
        assert!(received
            .iter()
            .any(|r| r.method == "Target.detachFromTarget"));
        assert!(!received.iter().any(|r| r.method == "Target.closeTarget"));
        assert!(server.wait_for_close(Duration::from_secs(2)));
        let blocked = host.handle_request("s1", request("o1", "tabs.list", json!({})));
        assert_eq!(blocked.reason, Some(Reason::Off));
        assert!(host.policy().read().tabs_for("s1").is_empty());
        let state = host.state();
        assert!(!state.toggle_on);
        assert_eq!(state.connection.state, ConnectionState::Off);

        // Audit: one row per request, typed text never stored.
        let rows = host.audit_list(Some("s1"), 50);
        let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
        assert!(actions.contains(&"tabs.open"));
        assert!(actions.contains(&"teleport"));
        assert!(actions.iter().filter(|a| **a == "tabs.open").count() >= 1);
        // Newest first; the oldest r1 row is the real open, the newest the
        // refused re-use with different input.
        let open_row = rows.iter().rev().find(|r| r.request_id == "r1").unwrap();
        assert_eq!(open_row.outcome, Outcome::Done);
        let conflict_row = rows.iter().find(|r| r.request_id == "r1").unwrap();
        assert_eq!(conflict_row.outcome, Outcome::Blocked);
        assert_eq!(open_row.tab.as_deref(), Some(tab.as_str()));
        assert!(open_row.commands_sent >= 7);
        let events = host.events_list(50);
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"toggle_on"));
        assert!(kinds.contains(&"connected"));
        assert!(kinds.contains(&"toggle_off"));
        assert!(kinds.contains(&"disconnected"));
        host.audit_clear();
        assert!(host.audit_list(None, 50).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The audit records a grant when it changes, not on every turn: the
    /// frontend may authorize one turn several times (seen as three
    /// `grant_on` rows per turn in Ognjen's log).
    #[test]
    fn grant_events_record_transitions_only() {
        let host =
            BrowserHost::for_tests(settings_for(&std::env::temp_dir().join("monocode-grants")));
        let count = |kind: &str| {
            host.events_list(50)
                .iter()
                .filter(|e| e.kind == kind && e.session_id.as_deref() == Some("s1"))
                .count()
        };
        for _ in 0..3 {
            host.turn_started("s1", "main", "/repo", Some(true));
        }
        assert!(host.policy().read().granted("s1"));
        // A turn that says nothing about the browser keeps the grant quietly,
        // and granting again from Settings changes nothing.
        host.turn_started("s1", "main", "/repo", None);
        host.set_grant("s1", "main", true);
        assert_eq!(count("grant_on"), 1);
        assert_eq!(count("grant_off"), 0);
        // Dropping it is recorded once, however often it is dropped again.
        host.turn_started("s1", "main", "/repo", Some(false));
        host.turn_started("s1", "main", "/repo", Some(false));
        host.set_grant("s1", "main", false);
        assert!(!host.policy().read().granted("s1"));
        assert_eq!(count("grant_off"), 1);
        assert_eq!(count("grant_on"), 1);
        // A later grant is a new transition, from either path.
        host.set_grant("s1", "main", true);
        host.turn_started("s1", "main", "/repo", Some(true));
        assert_eq!(count("grant_on"), 2);
        host.turn_started("s1", "main", "/repo", Some(false));
        host.turn_started("s1", "main", "/repo", Some(true));
        assert_eq!(count("grant_on"), 3);
        assert_eq!(count("grant_off"), 2);
    }

    #[test]
    fn old_browsers_are_refused_and_missing_files_ask_for_the_toggle() {
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let server = FakeCdp::start_with(move |received| {
            if received.method == "Browser.getVersion" {
                return Response::Result(json!({ "product": "Chrome/143.0.1.2" }));
            }
            super::testing::default_response(received, &counter)
        });
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(
            wait_state(&host, ConnectionState::UnsupportedVersion),
            "{:?}",
            host.connection()
        );
        assert!(server.wait_for_close(Duration::from_secs(2)));
        // The link's own close ran its hook before the refusal was recorded:
        // the state stays, it is not turned into "Disconnected" a moment later.
        thread::sleep(Duration::from_millis(50));
        let connection = host.connection();
        assert_eq!(connection.state, ConnectionState::UnsupportedVersion);
        assert!(connection.detail.unwrap().contains("144"));
        host.turn_started("s1", "main", "/repo", Some(true));
        let blocked = host.handle_request("s1", request("r1", "tabs.open", json!({})));
        assert_eq!(blocked.reason, Some(Reason::BrowserUnsupported));
        assert_eq!(blocked.outcome, Outcome::Blocked);

        // No DevToolsActivePort: NeedsToggle with the instruction; the agent
        // request repeats the instruction and kicks off nothing harmful.
        std::fs::remove_file(dir.join(discovery::ACTIVE_PORT_FILE)).unwrap();
        host.set_toggle(false);
        host.set_toggle(true);
        assert!(
            wait_state(&host, ConnectionState::NeedsToggle),
            "{:?}",
            host.connection()
        );
        // OFF dropped every grant: without a new /browser the session is not granted.
        let ungranted = host.handle_request("s1", request("r2a", "tabs.open", json!({})));
        assert_eq!(ungranted.reason, Some(Reason::NotGranted));
        host.set_grant("s1", "main", true);
        let needs = host.handle_request("s1", request("r2", "tabs.open", json!({})));
        assert_eq!(needs.reason, Some(Reason::BrowserNeedsSetup));
        assert!(needs.message.contains("chrome://inspect"));
        // A stale file: the port does not answer.
        std::fs::write(
            dir.join(discovery::ACTIVE_PORT_FILE),
            "1\n/devtools/browser/x\n",
        )
        .unwrap();
        host.set_toggle(false);
        host.set_toggle(true);
        host.set_grant("s1", "main", true);
        assert!(
            wait_state(&host, ConnectionState::StalePort),
            "{:?}",
            host.connection()
        );
        let stale = host.handle_request("s1", request("r3", "tabs.open", json!({})));
        assert_eq!(stale.reason, Some(Reason::BrowserUnavailable));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revoke_transfer_and_session_deletion_act_on_the_browser() {
        let server = FakeCdp::start();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.turn_started("s1", "main", "/repo", Some(true));
        host.turn_started("s2", "main", "/repo", Some(true));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        let a = host.handle_request("s1", request("a", "tabs.open", json!({})));
        let b = host.handle_request("s1", request("b", "tabs.open", json!({})));
        let tab_a = TabId(a.data.unwrap()["tab"].as_str().unwrap().to_string());
        let tab_b = TabId(b.data.unwrap()["tab"].as_str().unwrap().to_string());
        assert_eq!(host.policy().read().tabs_for("s1").len(), 2);

        // Transfer: s2 now owns tab_a; s1's next action on it is no_lease.
        host.transfer(&tab_a, "s1", "s2").unwrap();
        assert_eq!(host.policy().read().tabs_for("s2").len(), 1);
        let lost = host.handle_request("s1", request("c", "tabs.close", json!({ "tab": tab_a.0 })));
        assert_eq!(lost.reason, Some(Reason::NoLease));
        assert_eq!(
            host.transfer(&tab_b, "s1", "nobody"),
            Err(Reason::NotGranted)
        );

        // User takes tab_a back: marker release in our world, detach, lease revoked, tab open.
        let before = server.received().len();
        host.release_tab(&tab_a).unwrap();
        let received = server.received();
        let tail: Vec<&str> = received[before..]
            .iter()
            .map(|r| r.method.as_str())
            .collect();
        assert_eq!(
            tail,
            vec![
                "Page.createIsolatedWorld",
                "Runtime.callFunctionOn",
                "Target.detachFromTarget"
            ]
        );
        assert_eq!(received[before].params["worldName"], json!(ISOLATED_WORLD));
        assert!(received[before + 1].params["functionDeclaration"]
            .as_str()
            .unwrap()
            .contains("__monocodeMarkerRelease"));
        assert_eq!(
            host.policy().read().lease(&tab_a).unwrap().state,
            TabState::Revoked
        );
        assert!(host.policy().read().tabs_for("s2").is_empty());
        let gone =
            host.handle_request("s2", request("c2", "tabs.close", json!({ "tab": tab_a.0 })));
        assert_eq!(gone.reason, Some(Reason::NoLease));
        // Releasing again is a no-op; unknown tabs are errors.
        host.release_tab(&tab_a).unwrap();
        assert!(host.release_tab(&TabId("t_deadbeef".into())).is_err());

        // Revoke s1: its remaining tab is detached, not closed.
        let before = server.received().len();
        host.revoke("s1");
        let received = server.received();
        assert_eq!(received.len(), before + 1);
        assert_eq!(received[before].method, "Target.detachFromTarget");
        let revoked = host.handle_request("s1", request("d", "tabs.list", json!({})));
        assert_eq!(revoked.reason, Some(Reason::NotGranted));
        assert_eq!(
            host.policy().read().lease(&tab_b).unwrap().state,
            TabState::Revoked
        );

        // User closes a tab from Settings, then deletes session s2.
        host.close_tab_by_user(&tab_b).unwrap();
        assert!(server
            .received()
            .iter()
            .any(
                |r| r.method == "Target.closeTarget" && r.params["targetId"] == json!("T1")
                    || r.method == "Target.closeTarget"
            ));
        assert!(host.close_tab_by_user(&tab_b).is_err());
        let closes_before = server
            .received()
            .iter()
            .filter(|r| r.method == "Target.closeTarget")
            .count();
        host.session_deleted("s2");
        let closes_after = server
            .received()
            .iter()
            .filter(|r| r.method == "Target.closeTarget")
            .count();
        assert_eq!(closes_after, closes_before + 1);
        assert!(host.policy().read().session("s2").is_none());
        assert!(host
            .state()
            .tabs
            .iter()
            .all(|t: &TabOverview| t.tab.state == TabState::Closed));

        // Dropping browser access on a turn start revokes too.
        host.turn_started("s3", "main", "/repo", Some(true));
        let c = host.handle_request("s3", request("e", "tabs.open", json!({})));
        assert_eq!(c.outcome, Outcome::Done);
        let detaches = server
            .received()
            .iter()
            .filter(|r| r.method == "Target.detachFromTarget")
            .count();
        host.turn_started("s3", "main", "/repo", Some(false));
        let detaches_after = server
            .received()
            .iter()
            .filter(|r| r.method == "Target.detachFromTarget")
            .count();
        assert_eq!(detaches_after, detaches + 1);
        assert!(!host.policy().read().granted("s3"));

        // Settings change persists through the audit db and shows in state.
        let mut settings = settings_for(&dir);
        settings.limits.tabs_per_session = 2;
        host.set_settings(settings.clone());
        // The remembered switch follows the toggle, not the settings call.
        settings.enabled = true;
        assert_eq!(host.state().settings, settings);
        host.disconnect();
        assert!(
            wait_state(&host, ConnectionState::Disconnected),
            "{:?}",
            host.connection()
        );
        assert!(host.state().toggle_on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_browser_moves_the_state_to_disconnected_and_clears_leases() {
        let server = FakeCdp::start();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.turn_started("s1", "main", "/repo", Some(true));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        let opened = host.handle_request("s1", request("a", "tabs.open", json!({})));
        assert_eq!(opened.outcome, Outcome::Done);
        let epoch = host.policy().read().global_epoch;
        server.close_current();
        assert!(
            wait_state(&host, ConnectionState::Disconnected),
            "{:?}",
            host.connection()
        );
        assert!(host.policy().read().tabs_for("s1").is_empty());
        assert_ne!(host.policy().read().global_epoch, epoch);
        // The next request reports the state and starts a new discovery
        // (which, with the fake still listening, reconnects).
        let after = host.handle_request("s1", request("b", "tabs.list", json!({})));
        assert_eq!(after.reason, Some(Reason::BrowserDisconnected));
        assert_eq!(after.outcome, Outcome::Blocked);
        assert!(
            wait_state(&host, ConnectionState::Connected),
            "{:?}",
            host.connection()
        );
        // Blocked results are not cached: the same id now runs.
        let again = host.handle_request("s1", request("b", "tabs.list", json!({})));
        assert_eq!(again.outcome, Outcome::Done, "{again:?}");
        host.set_toggle(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Managed mode with a MonoCode browser already listening on the fake.
    fn managed_host(server: &FakeCdp, access: AccessMode) -> (Arc<BrowserHost>, PathBuf) {
        let dir = profile_for(server);
        let host = BrowserHost::for_tests(BrowserSettings {
            access,
            ..BrowserSettings::default()
        });
        host.set_profile_dir(dir.clone());
        (host, dir)
    }

    #[test]
    fn managed_mode_starts_on_the_first_action_and_remembers_the_switch() {
        let server = FakeCdp::start();
        let (host, dir) = managed_host(&server, AccessMode::Always);
        host.set_toggle(true);
        // The switch alone starts nothing.
        assert_eq!(host.connection().state, ConnectionState::Idle);
        assert!(server.received().is_empty());
        assert!(host.policy().read().settings.enabled);
        host.turn_started("s1", "main", "/repo", Some(true));
        let listed = host.handle_request("s1", request("r1", "tabs.list", json!({})));
        assert_eq!(listed.outcome, Outcome::Done, "{listed:?}");
        let info = host.connection().browser.unwrap();
        assert!(info.managed);
        assert_eq!(info.channel, launch::CHANNEL);
        // No consent step for our own browser.
        assert!(!host
            .events_list(50)
            .iter()
            .any(|event| event.kind == "consent_pending"));
        host.set_toggle(false);
        assert!(!host.policy().read().settings.enabled);
        // A remembered switch comes back without connecting.
        let restored = BrowserHost::for_tests(BrowserSettings {
            enabled: true,
            ..BrowserSettings::default()
        });
        restored.restore_toggle();
        assert!(restored.policy().read().toggle_on);
        assert_eq!(restored.connection().state, ConnectionState::Idle);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn wait_requested(host: &BrowserHost, session: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if host
                .state()
                .sessions
                .iter()
                .any(|s| s.session_id == session && s.requested)
            {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn ask_mode_waits_for_the_users_answer() {
        let server = FakeCdp::start();
        let (host, dir) = managed_host(&server, AccessMode::Ask);
        host.set_toggle(true);

        // Allow: the waiting action runs.
        host.turn_started("s1", "main", "/repo", None);
        let waiting = {
            let host = host.clone();
            thread::spawn(move || host.handle_request("s1", request("a1", "tabs.list", json!({}))))
        };
        assert!(wait_requested(&host, "s1"));
        host.answer_request("s1", "main", true);
        let allowed = waiting.join().unwrap();
        assert_eq!(allowed.outcome, Outcome::Done, "{allowed:?}");
        assert!(host.policy().read().granted("s1"));
        assert!(!host.policy().read().requested("s1"));

        // Not now: not_granted, and no new prompt for the rest of the turn.
        host.turn_started("s2", "main", "/repo", None);
        let waiting = {
            let host = host.clone();
            thread::spawn(move || host.handle_request("s2", request("b1", "tabs.list", json!({}))))
        };
        assert!(wait_requested(&host, "s2"));
        host.answer_request("s2", "main", false);
        assert_eq!(waiting.join().unwrap().reason, Some(Reason::NotGranted));
        let again = host.handle_request("s2", request("b2", "tabs.list", json!({})));
        assert_eq!(again.reason, Some(Reason::NotGranted));
        assert!(!host.policy().read().requested("s2"));

        // The next turn may ask again; the turn ending withdraws the prompt.
        host.turn_started("s2", "main", "/repo", None);
        let waiting = {
            let host = host.clone();
            thread::spawn(move || host.handle_request("s2", request("b3", "tabs.list", json!({}))))
        };
        assert!(wait_requested(&host, "s2"));
        host.turn_finished("s2");
        assert_eq!(waiting.join().unwrap().reason, Some(Reason::NotGranted));
        // Without a turn nobody is asked.
        let idle = host.handle_request("s3", request("c1", "tabs.list", json!({})));
        assert_eq!(idle.reason, Some(Reason::NotGranted));
        assert!(!host.policy().read().requested("s3"));
        host.set_toggle(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole managed path against the Chrome or Edge on this machine:
    /// start, connect without consent, open a tab, read it, close the
    /// browser on OFF. `cargo test -- --ignored managed_mode_drives`.
    #[test]
    #[ignore = "starts the Chrome or Edge installed on this machine"]
    fn managed_mode_drives_a_real_browser() {
        let dir = std::env::temp_dir().join(format!("monocode-managed-{}", uuid::Uuid::new_v4()));
        let host = BrowserHost::for_tests(BrowserSettings {
            access: AccessMode::Always,
            show_window: false,
            ..BrowserSettings::default()
        });
        host.set_profile_dir(dir.clone());
        host.set_toggle(true);
        host.turn_started("s1", "main", "/repo", Some(true));
        let opened = host.handle_request(
            "s1",
            request("m1", "tabs.open", json!({ "url": "about:blank" })),
        );
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        let info = host.connection().browser.unwrap();
        assert!(info.managed, "{info:?}");
        let tab = opened.data.unwrap()["tab"].as_str().unwrap().to_string();
        let read = host.handle_request("s1", request("m2", "read", json!({ "tab": tab })));
        assert_eq!(read.outcome, Outcome::Done, "{read:?}");
        host.set_toggle(false);
        assert!(launch::running(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Private mode against the installed browser: every start gets a fresh
    /// profile, and OFF deletes it. `cargo test -- --ignored private_mode`.
    #[test]
    #[ignore = "starts the Chrome or Edge installed on this machine"]
    fn private_mode_deletes_its_profile() {
        let root = std::env::temp_dir().join(format!("monocode-private-{}", uuid::Uuid::new_v4()));
        let host = BrowserHost::for_tests(BrowserSettings {
            mode: BrowserMode::Private,
            access: AccessMode::Always,
            show_window: false,
            ..BrowserSettings::default()
        });
        host.set_profile_dir(root.join("browser-profile"));
        host.set_toggle(true);
        host.turn_started("s1", "main", "/repo", Some(true));
        let opened = host.handle_request("s1", request("p1", "tabs.open", json!({})));
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        let private = root.join("browser-private");
        let profiles: Vec<_> = std::fs::read_dir(&private).unwrap().collect();
        assert_eq!(profiles.len(), 1);
        assert!(!root.join("browser-profile").exists());
        host.set_toggle(false);
        let left = std::fs::read_dir(&private).map(|d| d.count()).unwrap_or(0);
        assert_eq!(left, 0, "the private profile is deleted");
        let _ = std::fs::remove_dir_all(&root);
    }
}
