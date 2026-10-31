//! Browser module: lets an agent use a Chromium browser (Chrome, Edge, Brave,
//! Vivaldi, Opera or Arc) through a thin, in-process CDP client, under the
//! user's control at every moment. The browser is either the MonoCode
//! browser, which MonoCode starts on a profile of its own (kept, or private
//! and deleted when it closes), or the user's own browser, attached to after
//! the browser's consent dialog.
//!
//! Architecture:
//!
//! - [`discovery`]: finds `DevToolsActivePort`, probes the port, checks the
//!   version and administrator policies. Never launches a browser.
//! - [`launch`]: finds an installed browser and starts the MonoCode browser
//!   on its profile, then hands its endpoint to the same connect path.
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
//! - [`semantic`]: the page actions (snapshot, click, type, …); the core
//!   calls [`semantic::execute`] for every action it does not handle itself
//!   (`tabs.*`).
//! - [`diagnostics`]: console messages, uncaught exceptions and failed
//!   requests of agent tabs, recorded on the link thread; read by the
//!   `console` and `network` actions and summarised in `observed.signals`.
//! - [`pick`]: "Point at an element": the user picks an element of a thread's
//!   tab with the browser's inspect highlight; the composer gets a line
//!   naming it and the agent's next snapshot marks it.
//! - [`live`]: "Show in MonoCode": a thread's tab shown live in a MonoCode
//!   panel (CDP screencast), with the user's clicks and typing sent back.
//! - [`agent`]: the agent side, in the same `monocode` binary: the
//!   `monocode browser` CLI, the `monocode browser-mcp` server, their shared
//!   transport client and the provider registration.
//!
//! # How the rest of the app talks to this module
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

pub mod agent;
pub mod audit;
pub mod diagnostics;
pub mod discovery;
pub mod launch;
pub mod link;
pub mod live;
pub mod pick;
pub mod policy;
pub mod protocol;
#[cfg(test)]
mod resilience;
pub mod semantic;
pub mod targets;
#[cfg(test)]
pub(crate) mod testing;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
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
    TabId, TabOverview, TabState, TabsAdoptParams, TabsCloseParams, TabsFocusParams,
    TabsListParams, TabsOpenParams, WaitUntil, MIN_BROWSER_MAJOR, STATE_EVENT,
};
use targets::TabEvent;

/// How long `OFF` waits for the browser to acknowledge our detaches.
pub const OFF_DETACH_BUDGET: Duration = Duration::from_millis(500);
/// How long an agent action waits for the user to answer an access request.
/// It comes on top of the action's own time limit; see [`HOST_WAIT`].
pub const ASK_WAIT: Duration = Duration::from_secs(45);
/// How long an agent action waits for the MonoCode browser to start.
pub const MANAGED_START_WAIT: Duration = Duration::from_secs(launch::START_TIMEOUT.as_secs() + 5);
/// The longest a request can be held beyond its action's own time limit: an
/// access request, a cold start of the MonoCode browser, a busy tab, and the
/// snapshot taken after the action. The agent's client waits at least this
/// long on top of the action's limit, so it never gives up on a request the
/// host is still working on.
pub const HOST_WAIT: Duration = ASK_WAIT
    .saturating_add(MANAGED_START_WAIT)
    .saturating_add(BUSY_WAIT)
    .saturating_add(semantic::SNAPSHOT_BUDGET);
/// How long the MonoCode browser may take to close before it is killed.
const MANAGED_CLOSE_WAIT: Duration = Duration::from_secs(3);
/// How long the MonoCode browser stays up after an access request was
/// declined or expired while no thread may use it.
pub const UNUSED_CLOSE_GRACE: Duration = Duration::from_secs(10);

/// The MonoCode browser process this run started.
struct ManagedBrowser {
    child: Child,
    show_window: bool,
    /// The user opened its window ("Open browser"), so it is theirs to close.
    user_opened: bool,
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
/// it otherwise, then forget it (a private profile is deleted). The browser
/// was asked to close over the link first when the link was up
/// ([`BrowserHost::close_managed_pages`]).
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

/// A request the host is handling now. A retry with the same id (the
/// agent's client lost the answer) waits for this one's result instead of
/// running the action a second time; finished results are in the policy's
/// dedup ring.
struct InFlight {
    input_hash: u64,
    state: Mutex<InFlightState>,
    done: Condvar,
}

enum InFlightState {
    Running,
    Finished(Box<ActionResult>),
    /// The handler ended without a result (it panicked).
    Lost,
}

/// Owner of an [`InFlight`] entry: dropping it hands the result to the
/// waiting retries and removes the entry.
struct InFlightGuard<'a> {
    host: &'a BrowserHost,
    key: (String, String),
    entry: Arc<InFlight>,
    result: Option<Box<ActionResult>>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        // The dedup ring already holds the result (stored by `finish`), so
        // a retry arriving after the removal finds it there.
        {
            let mut inflight = self.host.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if inflight
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(entry, &self.entry))
            {
                inflight.remove(&self.key);
            }
        }
        *self.entry.state.lock().unwrap_or_else(|e| e.into_inner()) = match self.result.take() {
            Some(result) => InFlightState::Finished(result),
            None => InFlightState::Lost,
        };
        self.entry.done.notify_all();
    }
}

/// What a request id means when a request arrives.
enum Claim<'a> {
    /// New: this request runs; the guard announces its result.
    Fresh(InFlightGuard<'a>),
    /// Finished earlier with the same input.
    Cached(Box<ActionResult>),
    /// Running now with the same input.
    Running(Arc<InFlight>),
    /// Used before with different input.
    Conflict,
}

static HOST: OnceLock<Arc<BrowserHost>> = OnceLock::new();

/// The process-wide host, once [`init`] ran.
pub fn host() -> Option<Arc<BrowserHost>> {
    HOST.get().cloned()
}

/// Where audit rows go: the app's session store, or an owned connection in tests.
enum Db {
    App(AppHandle),
    #[cfg(test)]
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
            #[cfg(test)]
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
    /// shows the browser's consent dialog. One per drop: restored when a connection is established or the toggle goes
    /// ON, spent by the first automatic attempt that reaches the browser.
    auto_dialog: AtomicBool,
    /// Profile folder of the MonoCode browser (managed mode).
    profile: Mutex<Option<PathBuf>>,
    managed: Mutex<Option<ManagedBrowser>>,
    /// "Open browser" asked for a visible window on the next start.
    force_window: AtomicBool,
    /// The blank page the MonoCode browser starts with, until an agent tab
    /// replaces it.
    startup_pages: Mutex<Vec<TargetId>>,
    /// Cancels the element pick running now, if any (one at a time).
    picking: Mutex<Option<Arc<AtomicBool>>>,
    /// Stops the live view running now, if any (one at a time).
    live: Mutex<Option<Arc<live::LiveControl>>>,
    /// Tests: the browser counts as a hidden MonoCode browser, whose windows
    /// the live view may resize.
    #[cfg(test)]
    hidden_for_tests: AtomicBool,
    /// Requests being handled now, by session and request id.
    inflight: Mutex<HashMap<(String, String), Arc<InFlight>>>,
    /// Serialises the Settings and thread-menu commands, which run off the
    /// main thread but must not interleave (a quick ON then OFF).
    commands: Mutex<()>,
    /// The browser executable the MonoCode browser starts from, for tests.
    #[cfg(test)]
    test_executable: Mutex<Option<launch::Executable>>,
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
            #[cfg(test)]
            hidden_for_tests: AtomicBool::new(false),
            startup_pages: Mutex::new(Vec::new()),
            picking: Mutex::new(None),
            live: Mutex::new(None),
            inflight: Mutex::new(HashMap::new()),
            commands: Mutex::new(()),
            #[cfg(test)]
            test_executable: Mutex::new(None),
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
            user_opened: false,
            profile,
            ephemeral,
        });
    }

    #[cfg(test)]
    pub fn link(&self) -> &Link {
        &self.link
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

    // --- toggle and connection -------------------------------------------------

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

    /// The first agent tab in the MonoCode browser closes the blank page it
    /// started with, unless the user has since used that page.
    fn close_startup_pages(&self) {
        let startup =
            std::mem::take(&mut *self.startup_pages.lock().unwrap_or_else(|e| e.into_inner()));
        if startup.is_empty() || !self.link.is_open() || !self.connected_to_managed() {
            return;
        }
        let Ok(reply) = self
            .link
            .send_system(Command::browser(Method::TargetGetTargets, json!({})))
        else {
            return;
        };
        let still_blank: Vec<TargetId> = blank_pages(&reply)
            .into_iter()
            .filter(|target| startup.contains(target))
            .collect();
        targets::close_targets(&self.link, &still_blank);
    }

    /// Close every page of the MonoCode browser while the link is still up,
    /// so it does not offer to restore them, then the browser itself
    /// (`Browser.close`: a headless one stays up without pages). All of it
    /// fits in [`OFF_DETACH_BUDGET`]; a browser that stops answering is left
    /// to [`Self::close_managed_browser`], which kills it.
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
        let gone = |error: &link::CdpError| {
            matches!(
                error.reason(),
                Reason::LinkDown | Reason::BrowserDisconnected
            )
        };
        for page in pages {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.link.send_system_within(
                Command::browser(Method::TargetCloseTarget, json!({ "targetId": page.0 })),
                left,
            ) {
                Err(error) if gone(&error) => return,
                Err(error) if error.reason() == Reason::CdpTimeout => break,
                _ => {}
            }
        }
        // The browser may close the socket before it answers; the write is
        // what counts, and the process exit is awaited by the caller.
        let left = deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(100));
        if self.link.is_open() {
            let _ = self
                .link
                .send_system_within(Command::browser(Method::BrowserClose, json!({})), left);
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
        let (started, user_opened) = {
            let managed = self.managed.lock().unwrap_or_else(|e| e.into_inner());
            (
                managed.is_some(),
                managed.as_ref().is_some_and(|managed| managed.user_opened),
            )
        };
        if !started && self.link.status() == LinkStatus::Closed {
            return;
        }
        // A window the user opened, or pages they browse in it, are theirs:
        // only a browser started for the agent alone closes by itself.
        if user_opened || self.user_tabs().is_ok_and(|tabs| !tabs.is_empty()) {
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

    fn find_executable(&self, choice: &protocol::BrowserChoice) -> Option<launch::Executable> {
        #[cfg(test)]
        if let Some(exe) = self
            .test_executable
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Some(exe);
        }
        launch::find_executable(choice)
    }

    /// A start that failed or lost its generation: stop the browser `pid` it
    /// started, unless something else already took it out of the slot.
    fn stop_started(&self, pid: u32) {
        let mine = {
            let mut slot = self.managed.lock().unwrap_or_else(|e| e.into_inner());
            if slot
                .as_ref()
                .is_some_and(|managed| managed.child.id() == pid)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(managed) = mine {
            stop_managed(managed);
        }
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
        let headless = {
            let mut managed = self.managed.lock().unwrap_or_else(|e| e.into_inner());
            managed.as_mut().map(|managed| {
                // Now the user's window: it no longer closes when unused.
                managed.user_opened = true;
                !managed.show_window
            })
        };
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
    /// dialog in the browser; there is no automatic retry loop.
    pub fn connect(self: &Arc<Self>) {
        self.start_connect(false);
    }

    /// A connection attempt started by an agent request (the next request
    /// after a drop starts a new discovery). It may show the consent
    /// dialog at most once per drop; after that only the user's Connect does.
    fn connect_automatically(self: &Arc<Self>) {
        self.start_connect(true);
    }

    fn start_connect(self: &Arc<Self>, automatic: bool) {
        // The generation is bumped under the policy write lock, so read under
        // the same guard as the switch it belongs to.
        if self.link.is_open() {
            return;
        }
        let generation = {
            let policy = self.policy.read();
            if !policy.toggle_on {
                return;
            }
            self.generation.load(Ordering::Acquire)
        };
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

    /// Discovery (candidates, port file, probe) then [`Self::connect_endpoint`].
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
            Some("Looking for your browser…".into()),
            None,
            None,
        );
        match discovery::locate(&choice) {
            Located::NoBrowser => self.attempt_state(
                generation,
                ConnectionState::NeedsToggle,
                Some(format!(
                    "No browser profile was found. Install Chrome, Edge, Brave, Vivaldi, Opera or Arc, or choose the profile folder in Settings. {}",
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
        let exe = self.find_executable(&choice);
        let kind = exe.as_ref().map(|exe| exe.kind);
        let active = match launch::running(&profile) {
            Some(active) => active,
            None => {
                let Some(exe) = exe else {
                    self.attempt_state(
                        generation,
                        ConnectionState::StalePort,
                        Some("No supported browser was found on this computer. Install Chrome, Edge, Brave, Vivaldi, Opera or Arc, or switch to your own browser in Settings.".into()),
                        None,
                        Some("no browser executable".into()),
                    );
                    return;
                };
                // A browser of an earlier start that no longer serves its
                // profile (hung, or exited and not reaped yet) goes first: a
                // second start on a profile in use only hands over to it.
                let previous = self
                    .managed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take();
                if let Some(previous) = previous {
                    stop_managed(previous);
                }
                let user_opened = self.force_window.swap(false, Ordering::AcqRel);
                // Agent tabs shown in MonoCode need no browser window of their own.
                let show_window = {
                    let settings = &self.policy.read().settings;
                    (settings.show_window && !settings.tabs_in_monocode) || user_opened
                };
                // Started and registered under the lock, so OFF or a mode
                // change (generation first, then the slot) either stops this
                // browser or comes before it is started at all.
                let started = {
                    let mut slot = self.managed.lock().unwrap_or_else(|e| e.into_inner());
                    if !self.current(generation) {
                        return;
                    }
                    let raced = slot.take();
                    let started = launch::start(&exe, &profile, show_window, self.display_scale())
                        .map(|child| {
                            let pid = child.id();
                            *slot = Some(ManagedBrowser {
                                child,
                                show_window,
                                user_opened,
                                profile: profile.clone(),
                                ephemeral: private,
                            });
                            pid
                        });
                    drop(slot);
                    // Only a stale attempt can have registered one since.
                    if let Some(raced) = raced {
                        stop_managed(raced);
                    }
                    started
                };
                let pid = match started {
                    Ok(pid) => pid,
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
                let ready = launch::wait_for_endpoint(&profile, launch::START_TIMEOUT, || {
                    if !self.current(generation) {
                        return launch::Started::Gone;
                    }
                    let mut slot = self.managed.lock().unwrap_or_else(|e| e.into_inner());
                    match slot.as_mut() {
                        Some(managed) if managed.child.id() == pid => {
                            match managed.child.try_wait() {
                                Ok(Some(status)) => launch::Started::Exited(status.to_string()),
                                _ => launch::Started::Running,
                            }
                        }
                        // OFF or a mode change took it.
                        _ => launch::Started::Gone,
                    }
                });
                if !self.current(generation) {
                    self.stop_started(pid);
                    return;
                }
                match ready {
                    Ok(active) => active,
                    Err(message) => {
                        self.stop_started(pid);
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

    /// Open the socket (consent dialog), check the version,
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
                    "{} is too old; a browser built on Chromium {MIN_BROWSER_MAJOR} or newer is required.",
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
        // Diagnostics only: no attach, no adoption. The
        // MonoCode browser's start page is remembered so the first agent tab
        // can replace it.
        let targets = self
            .link
            .send_system(Command::browser(Method::TargetGetTargets, json!({})));
        *self.startup_pages.lock().unwrap_or_else(|e| e.into_inner()) = match targets {
            Ok(reply) if managed => blank_pages(&reply),
            _ => Vec::new(),
        };
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
    /// leases cleared; tabs in the browser stay.
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

    // --- permissions -----------------------------------------------------------

    /// `/browser` on or off for a session.
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
        let detach = self.policy.write().turn_off(session);
        if self.link.is_open() {
            targets::detach_sessions(&self.link, &detach);
        }
        self.event("revoked", Actor::User, Some(session), None);
        self.emit_state();
    }

    /// Hand a tab to another session.
    pub fn transfer(&self, tab: &TabId, from: &str, to: &str) -> Result<(), Reason> {
        self.policy.write().transfer(tab, from, to)?;
        // The new owner starts from its own snapshot.
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

    /// Settings "Take the tab back": the lease is released, the
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

    /// Pages no thread holds, for the user's "Give to a thread" list. In the
    /// user's own browser these are their own tabs; only the UI sees them.
    pub fn user_tabs(&self) -> Result<Vec<targets::FreeTab>, String> {
        if !self.link.is_open() {
            return Err("MonoCode is not connected to the browser.".into());
        }
        let mut tabs = targets::free_tabs(&self.link).map_err(|e| format!("{e:?}"))?;
        // Pages of the browser itself (settings, new tab page) are no use.
        tabs.retain(|tab| tab.url.starts_with("http://") || tab.url.starts_with("https://"));
        Ok(tabs)
    }

    /// The user hands one of the browser's tabs to a thread, which gets
    /// browser access with it (the hand-over is the user's consent).
    pub fn give_tab(&self, free_id: &str, session: &str, window: &str) -> Result<TabId, String> {
        let free = self
            .user_tabs()?
            .into_iter()
            .find(|tab| tab.id == free_id)
            .ok_or("That tab is gone or another thread already holds it.")?;
        let granted_now = self.grant_for_hand_over(session, window);
        let handle = match targets::adopt_tab(&self.link, targets::Sender::System, session, &free) {
            Ok(handle) => handle,
            Err(error) => {
                self.undo_hand_over_grant(session, window, granted_now);
                return Err(tab_error_message(error));
            }
        };
        self.adopted(&handle, session, &free, Actor::User);
        Ok(handle.tab)
    }

    /// A hand-over (give a tab, open the app) is the thread's browser
    /// access. True when the grant is new, so a failed hand-over takes it
    /// back ([`Self::undo_hand_over_grant`]) and one the session already
    /// had stays.
    fn grant_for_hand_over(&self, session: &str, window: &str) -> bool {
        if self.policy.read().granted(session) {
            return false;
        }
        self.set_grant(session, window, true);
        true
    }

    fn undo_hand_over_grant(&self, session: &str, window: &str, granted_now: bool) {
        if granted_now {
            self.set_grant(session, window, false);
        }
    }

    /// "Open this branch's app" in the thread's browser menu: a new tab at
    /// `url` (a local dev server), held by `session`. Like handing over a
    /// tab, the user's click is the thread's browser access. Waits for the
    /// MonoCode browser to start when it is not running yet.
    pub fn open_app(
        self: &Arc<Self>,
        session: &str,
        window: &str,
        url: &str,
    ) -> Result<TabId, String> {
        let url = policy::check_url(url).map_err(|reason| reason.message().to_string())?;
        if !diagnostics::is_local_url(&url) {
            return Err("Only an app on this computer can be opened from here.".into());
        }
        self.open_for_thread(session, window, &url)
    }

    /// "New tab" in a thread's browser pane: a tab at `url`, or a blank one
    /// for the user to type an address into, held by `session` like any tab
    /// MonoCode opened for it. The user's click is the thread's access.
    pub fn new_tab(
        self: &Arc<Self>,
        session: &str,
        window: &str,
        url: Option<&str>,
    ) -> Result<TabId, String> {
        let url = match url.map(str::trim).filter(|url| !url.is_empty()) {
            Some(url) => policy::check_url(url).map_err(|reason| reason.message().to_string())?,
            None => "about:blank".to_string(),
        };
        self.open_for_thread(session, window, &url)
    }

    /// A tab at `url` (already checked) that `session` holds, opened by the
    /// user. Waits for the MonoCode browser to start when it is not running.
    fn open_for_thread(
        self: &Arc<Self>,
        session: &str,
        window: &str,
        url: &str,
    ) -> Result<TabId, String> {
        let url = url.to_string();
        if !self.policy.read().toggle_on {
            return Err("Turn on browser access first.".into());
        }
        if !self.link.is_open() {
            if !self.mode().launches() {
                return Err("MonoCode is not connected to your browser.".into());
            }
            self.connect_automatically();
            let connection = self.wait_for_connection(MANAGED_START_WAIT);
            if connection.state != ConnectionState::Connected {
                return Err(connection
                    .detail
                    .unwrap_or_else(|| "The MonoCode browser did not start.".into()));
            }
        }
        // Blank first, so the tab is ours (marker, console) before the
        // app's first script runs; then the app.
        let managed = self.mode().launches();
        let in_monocode = self.policy.read().settings.tabs_in_monocode;
        // In the hidden MonoCode browser each tab gets a window of its own,
        // which the pane sizes to itself (as agent tabs, see `open_tab`).
        let created = self
            .link
            .send_system(Command::browser(
                Method::TargetCreateTarget,
                json!({
                    "url": "about:blank",
                    "background": !managed || in_monocode,
                    "newWindow": managed && in_monocode && self.browser_hidden(),
                }),
            ))
            .map_err(|error| format!("{error:?}"))?;
        let target = created["targetId"]
            .as_str()
            .map(|id| TargetId(id.to_string()))
            .ok_or("The browser opened no tab.")?;
        let free = targets::FreeTab {
            id: targets::free_id(&target),
            target: target.clone(),
            url: url.clone(),
            title: String::new(),
        };
        let granted_now = self.grant_for_hand_over(session, window);
        let handle = match targets::adopt_tab(&self.link, targets::Sender::System, session, &free) {
            Ok(handle) => handle,
            Err(error) => {
                self.undo_hand_over_grant(session, window, granted_now);
                targets::close_targets(&self.link, &[target]);
                return Err(tab_error_message(error));
            }
        };
        // MonoCode opened this tab for the thread, so ending the thread
        // closes it like any agent tab instead of handing it back.
        if let Some(lease) = self.policy.write().lease_mut(&handle.tab) {
            lease.from_user = false;
        }
        self.adopted(&handle, session, &free, Actor::User);
        self.close_startup_pages();
        if url != "about:blank" {
            self.link
                .send_system(Command::in_session(
                    &handle.session,
                    Method::PageNavigate,
                    json!({ "url": url }),
                ))
                .map_err(|error| format!("The tab opened, but not the page: {error:?}"))?;
        }
        Ok(handle.tab)
    }

    fn adopted(
        &self,
        handle: &targets::TabHandle,
        owner: &str,
        free: &targets::FreeTab,
        actor: Actor,
    ) {
        self.db.with(|conn| {
            audit::lease_opened(
                conn,
                self.run_id,
                &handle.tab,
                &handle.target.0,
                owner,
                None,
                &free.url,
            )
        });
        self.event(
            "tab_adopted",
            actor,
            Some(owner),
            Some(&audit_url(&free.url)),
        );
        self.emit_state();
    }

    /// "Point at an element" in the thread indicator: the user picks an
    /// element in `tab`, one of `session`'s tabs. `None` when they cancelled.
    /// Blocks until the click, a cancel or [`pick::PICK_TIMEOUT`].
    pub fn pick_element(
        &self,
        session: &str,
        tab: &TabId,
        in_pane: bool,
    ) -> Result<Option<pick::PickedElement>, String> {
        if !self.link.is_open() {
            return Err("MonoCode is not connected to the browser.".into());
        }
        let headless = self
            .managed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|managed| !managed.show_window);
        // In a MonoCode pane the user clicks the tab's picture; a window is
        // only needed to pick in the browser itself.
        if headless && !in_pane {
            return Err("The MonoCode browser runs without a window. Turn its window on in Settings to point at elements.".into());
        }
        let target = {
            let state = self.policy.read();
            let lease = state.lease(tab).ok_or("That tab is gone.")?;
            if lease.owner != session {
                return Err("That tab belongs to another thread.".into());
            }
            if lease.state != TabState::Open {
                return Err("That tab is not open.".into());
            }
            pick::PickTab {
                tab: tab.clone(),
                main: lease
                    .cdp_session
                    .clone()
                    .ok_or("That tab is still opening.")?,
                frames: lease
                    .frames
                    .values()
                    .map(|frame| frame.cdp_session.clone())
                    .collect(),
                url: lease.url.clone(),
                bring_to_front: !in_pane,
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let previous = self
            .picking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(cancel.clone());
        if let Some(previous) = previous {
            previous.store(true, Ordering::Release);
        }
        let alive = || {
            self.policy
                .read()
                .lease(tab)
                .is_some_and(|lease| lease.state == TabState::Open && lease.owner == session)
        };
        let outcome = pick::wait_for_pick(&self.link, &target, &cancel, pick::PICK_TIMEOUT, alive);
        {
            let mut picking = self.picking.lock().unwrap_or_else(|e| e.into_inner());
            if picking.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                *picking = None;
            }
        }
        match outcome? {
            pick::Outcome::Picked(node) => {
                let loader = self
                    .policy
                    .read()
                    .lease(tab)
                    .and_then(|lease| lease.main_loader_id.clone());
                let number = semantic::mark_picked(
                    &self.link,
                    tab,
                    loader.as_deref(),
                    &node.session,
                    node.backend,
                );
                let picked = pick::describe(&self.link, &target, &node, number);
                self.event(
                    "element_picked",
                    Actor::User,
                    Some(session),
                    Some(&audit_url(&target.url)),
                );
                Ok(Some(picked))
            }
            pick::Outcome::Cancelled => Ok(None),
            pick::Outcome::TimedOut => Err("No element was picked within two minutes.".into()),
            pick::Outcome::TabGone => {
                Err("The tab closed or changed hands while you were picking.".into())
            }
        }
    }

    /// Stop the element pick running now.
    pub fn cancel_pick(&self) {
        let running = self
            .picking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(cancel) = running {
            cancel.store(true, Ordering::Release);
        }
    }

    /// The pixel ratio of the display MonoCode's main window is on.
    fn display_scale(&self) -> Option<f64> {
        let app = self.app.as_ref()?;
        app.get_webview_window("main")
            .and_then(|window| window.scale_factor().ok())
            .or_else(|| {
                app.primary_monitor()
                    .ok()
                    .flatten()
                    .map(|monitor| monitor.scale_factor())
            })
    }

    /// The main-frame session of `tab` when `session` holds it open.
    fn held_tab(&self, session: &str, tab: &TabId) -> Result<live::LiveTab, String> {
        let state = self.policy.read();
        let lease = state.lease(tab).ok_or("That tab is gone.")?;
        if lease.owner != session {
            return Err("That tab belongs to another thread.".into());
        }
        if lease.state != TabState::Open {
            return Err("That tab is not open.".into());
        }
        Ok(live::LiveTab {
            tab: tab.clone(),
            target: lease.target_id.clone(),
            main: lease
                .cdp_session
                .clone()
                .ok_or("That tab is still opening.")?,
            window: None,
        })
    }

    /// "Show in MonoCode": stream pictures of `tab`, one of `session`'s tabs,
    /// to `sink` until [`Self::stop_live`], another view, or the tab is gone.
    /// Blocks while the view runs.
    pub fn live_view(
        &self,
        session: &str,
        tab: &TabId,
        view: &str,
        size: live::PanelSize,
        sink: impl FnMut(live::LiveFrame),
    ) -> Result<live::LiveEnd, String> {
        if !self.link.is_open() {
            return Err("MonoCode is not connected to the browser.".into());
        }
        let mut target = self.held_tab(session, tab)?;
        target.window = self.own_window(&target.target);
        let control = Arc::new(live::LiveControl::new(view.to_string(), tab.clone()));
        let previous = self
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(control.clone());
        if let Some(previous) = previous {
            previous.stop();
            previous.wait_finished(Duration::from_secs(2));
        }
        let alive = || {
            self.policy
                .read()
                .lease(tab)
                .is_some_and(|lease| lease.state == TabState::Open && lease.owner == session)
        };
        let outcome = live::run(&self.link, &target, size, &control, alive, sink);
        control.finish();
        {
            let mut running = self.live.lock().unwrap_or_else(|e| e.into_inner());
            if running.as_ref().is_some_and(|c| Arc::ptr_eq(c, &control)) {
                *running = None;
            }
        }
        outcome
    }

    /// The MonoCode browser runs without a window anyone sees.
    fn browser_hidden(&self) -> bool {
        let hidden = self
            .managed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|managed| !managed.show_window);
        #[cfg(test)]
        let hidden = hidden || self.hidden_for_tests.load(Ordering::Acquire);
        hidden
    }

    /// The window of `target` when the live view may give it the panel's
    /// size: the MonoCode browser runs hidden (no one sees its windows) and
    /// no other tab shares that window, so resizing it changes only this
    /// tab's page.
    fn own_window(&self, target: &TargetId) -> Option<i64> {
        if !self.browser_hidden() {
            return None;
        }
        let window_of = |target: &str| {
            self.link
                .send_system(Command::browser(
                    Method::BrowserGetWindowForTarget,
                    json!({ "targetId": target }),
                ))
                .ok()?["windowId"]
                .as_i64()
        };
        let own = window_of(&target.0)?;
        let targets = self
            .link
            .send_system(Command::browser(Method::TargetGetTargets, json!({})))
            .ok()?;
        let shared = targets["targetInfos"]
            .as_array()?
            .iter()
            .filter(|info| info["type"] == "page" && info["targetId"] != target.0.as_str())
            .filter_map(|info| info["targetId"].as_str())
            .any(|other| window_of(other) == Some(own));
        (!shared).then_some(own)
    }

    /// Pause the live view of `tab`, if one runs, until the hold is dropped.
    fn hold_live(&self, tab: &TabId) -> Option<live::LiveHold> {
        let control = self
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|control| control.tab == *tab)?;
        Some(control.hold())
    }

    /// Stop the live view running now, if it is `view` (any view without
    /// one). A pane that goes away stops only its own view, never the one a
    /// newer pane started, even for the same tab (a pane moved in the
    /// layout is a new pane while the old one animates out).
    pub fn stop_live(&self, view: Option<&str>) {
        let mut running = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if view.is_some_and(|view| running.as_ref().is_some_and(|c| c.view != view)) {
            return;
        }
        if let Some(control) = running.take() {
            control.stop();
        }
    }

    /// The next picture for the pane of `view`: the newest one after
    /// `after`, waiting up to [`live::PULL_WAIT`] for it. Empty when none
    /// came in time or the view is over; the pane asks again while it shows.
    pub fn live_take(&self, view: &str, after: u64) -> Vec<u8> {
        let control = self
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|c| c.view == view);
        if control.is_none() && cfg!(debug_assertions) {
            eprintln!("[live] the panel asked for a picture of a view that is not running");
        }
        control
            .and_then(|control| control.take(after, live::PULL_WAIT))
            .unwrap_or_default()
    }

    /// The pane of `view` changed size: its view follows it.
    pub fn resize_live(&self, view: &str, size: live::PanelSize) {
        let running = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(control) = running.as_ref().filter(|c| c.view == view) {
            control.resize(size);
        }
    }

    /// The panel's Back, Forward, Reload or address field, in `tab`.
    pub fn live_navigate(
        &self,
        session: &str,
        tab: &TabId,
        nav: &live::LiveNav,
    ) -> Result<(), String> {
        let target = self.held_tab(session, tab)?;
        live::navigate(&self.link, &target, nav)
    }

    /// The user's click, scroll or key in the live view of `tab`.
    pub fn live_input(
        &self,
        session: &str,
        tab: &TabId,
        input: &live::LiveInput,
    ) -> Result<(), String> {
        let target = self.held_tab(session, tab)?;
        if let Some(control) = self
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|control| control.tab == *tab)
        {
            control.touch();
            if matches!(input, live::LiveInput::Wheel { .. }) {
                control.scroll();
            }
        }
        live::input(&self.link, &target, input)
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

    /// Settings "Close all agent tabs". Tabs the user handed over are given
    /// back, not closed.
    pub fn close_all_tabs(&self) {
        let ended = self.policy.write().close_all();
        self.end_leases(&ended);
        self.event("tabs_closed_all", Actor::User, None, None);
        self.emit_state();
    }

    /// Close or give back the tabs of ended leases, and close their rows.
    fn end_leases(&self, ended: &policy::Ended) {
        if self.link.is_open() {
            targets::give_back(&self.link, &ended.give_back);
        }
        self.close_owned_targets(&ended.close);
        for tab in &ended.tabs {
            self.db.with(|conn| audit::lease_closed(conn, tab));
        }
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
            if changed {
                // An attempt for the old kind of browser neither reports
                // nor keeps what it started.
                self.generation.fetch_add(1, Ordering::AcqRel);
            }
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

    /// Session deleted: its grant goes, the tabs opened for it are closed and
    /// the ones the user handed it are given back.
    pub fn session_deleted(&self, session: &str) {
        let ended = self.policy.write().session_deleted(session);
        self.end_leases(&ended);
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

    // --- the request path ------------------------------------------------------

    /// A request arriving over the control transport with a session token.
    /// `Err` only for an unknown token; everything else is an `ActionResult`.
    pub fn handle_token_request(
        self: &Arc<Self>,
        token: &str,
        request: ActionRequest,
    ) -> Result<ActionResult, String> {
        let session = self
            .session_for_token(token)
            .ok_or(crate::control::UNAUTHORIZED)?;
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
        // A full-page screenshot gets the tab's viewport to itself.
        let _live = (kind == ActionKind::Screenshot && request.input["full_page"] == true)
            .then(|| common.tab.as_ref().and_then(|tab| self.hold_live(tab)))
            .flatten();
        let mut guard = match self.claim(session, &request_id, &request.input) {
            Claim::Fresh(guard) => guard,
            Claim::Cached(result) => return *result,
            Claim::Running(entry) => {
                let budget = self.host_wait() + kind.effective_timeout(common.timeout_ms);
                return wait_in_flight(&entry, &request_id, budget);
            }
            Claim::Conflict => {
                let result = ActionResult::from_reason(&request_id, Reason::InvalidRequest)
                    .with_message(
                        "request_id was already used with different input; use a new id.",
                    );
                return self.finish(session, Some(kind), None, &request, started, 0, result);
            }
        };
        let result = self.run_request(session, &request, kind, &common, started);
        guard.result = Some(Box::new(result.clone()));
        result
    }

    /// Look `request_id` up among finished and running requests, and claim
    /// it for this request when it is new. Under the in-flight lock, so a
    /// retry sees either the running entry or, once that is gone, the
    /// stored result (`finish` stores it before the entry goes).
    fn claim(&self, session: &str, request_id: &str, input: &Value) -> Claim<'_> {
        let mut inflight = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        // Bind first: a guard living in a `match` scrutinee would still be
        // held while `finish` takes the lock again.
        let dedup = self.policy.write().dedup_check(session, request_id, input);
        match dedup {
            Dedup::Cached(result) => return Claim::Cached(result),
            Dedup::Conflict => return Claim::Conflict,
            Dedup::Fresh => {}
        }
        let key = (session.to_string(), request_id.to_string());
        let hash = policy::input_hash(input);
        if let Some(entry) = inflight.get(&key) {
            return if entry.input_hash == hash {
                Claim::Running(entry.clone())
            } else {
                Claim::Conflict
            };
        }
        let entry = Arc::new(InFlight {
            input_hash: hash,
            state: Mutex::new(InFlightState::Running),
            done: Condvar::new(),
        });
        inflight.insert(key.clone(), entry.clone());
        Claim::Fresh(InFlightGuard {
            host: self,
            key,
            entry,
            result: None,
        })
    }

    /// The longest a request is held beyond its action's limit
    /// ([`HOST_WAIT`], with this host's access-request wait).
    fn host_wait(&self) -> Duration {
        Duration::from_millis(self.ask_wait_ms.load(Ordering::Acquire))
            + MANAGED_START_WAIT
            + BUSY_WAIT
            + semantic::SNAPSHOT_BUDGET
    }

    /// [`Self::handle_request`] once the request id is claimed.
    fn run_request(
        self: &Arc<Self>,
        session: &str,
        request: &ActionRequest,
        kind: ActionKind,
        common: &CommonInput,
        started: Instant,
    ) -> ActionResult {
        let request_id = request.request_id.as_str();
        let timeout = kind.effective_timeout(common.timeout_ms);
        let mut admitted =
            self.policy
                .read()
                .admit(session, request_id, kind, common.tab.as_ref(), timeout);
        if admitted.as_ref().err() == Some(&Reason::NotGranted)
            && self.policy.read().settings.access == AccessMode::Ask
        {
            // Admitted again whatever the answer: OFF or a revocation during
            // the wait is reported as such, not as `not_granted`.
            self.ask_user(session);
            admitted =
                self.policy
                    .read()
                    .admit(session, request_id, kind, common.tab.as_ref(), timeout);
        }
        let mut ctx = match admitted {
            Ok(ctx) => ctx,
            Err(reason) => {
                let result = ActionResult::from_reason(request_id, reason);
                return self.finish(session, Some(kind), None, request, started, 0, result);
            }
        };
        let mut connection = self.connection();
        if self.mode().launches() && connection.state != ConnectionState::Connected {
            // The MonoCode browser starts on demand; the action waits for it.
            self.connect_automatically();
            connection = self.wait_for_connection(MANAGED_START_WAIT);
            if connection.state == ConnectionState::Connected {
                // A fresh deadline and epochs for the action itself. Bound
                // first, so the read guard is gone before `finish` locks.
                let readmitted = self.policy.read().admit(
                    session,
                    request_id,
                    kind,
                    common.tab.as_ref(),
                    timeout,
                );
                match readmitted {
                    Ok(fresh) => ctx = fresh,
                    Err(reason) => {
                        let result = ActionResult::from_reason(request_id, reason);
                        return self.finish(session, Some(kind), None, request, started, 0, result);
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
                // A new discovery starts on the next agent request.
                self.connect_automatically();
            }
            let mut result = ActionResult::from_reason(request_id, reason);
            if let Some(detail) = connection.detail {
                result.message = format!("{} {detail}", result.message);
            }
            return self.finish(session, Some(kind), Some(&ctx), request, started, 0, result);
        }
        // One action per tab, N per connection: wait briefly, then `busy`.
        let busy_deadline = Instant::now() + BUSY_WAIT.min(timeout);
        let slot = loop {
            if let Some(slot) = self.policy.write().try_begin_action(ctx.tab.as_ref()) {
                break slot;
            }
            if Instant::now() >= busy_deadline {
                let result = ActionResult::from_reason(request_id, Reason::Busy);
                return self.finish(session, Some(kind), Some(&ctx), request, started, 0, result);
            }
            thread::sleep(Duration::from_millis(20));
        };
        let commands_before = self.link.commands_sent();
        let mut result = self.run_action(&ctx, kind, &request.input, commands_before);
        self.policy.write().end_action(slot);
        self.report_signals(&ctx, kind, &mut result);
        let commands_sent = self.link.commands_sent().saturating_sub(commands_before);
        self.finish(
            session,
            Some(kind),
            Some(&ctx),
            request,
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
                let mut message = match count {
                    0 => "No open tabs. Use tabs.open to open one.".to_string(),
                    1 => "1 open tab.".to_string(),
                    n => format!("{n} open tabs."),
                };
                // In the MonoCode browser, tabs the user opened are free for
                // any thread to adopt. The user's own browser stays private.
                let free = if self.mode().launches() {
                    targets::free_tabs(&self.link).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if !free.is_empty() {
                    message.push_str(&format!(
                        " {} free tab(s) the user opened; tabs.adopt with `free` takes one.",
                        free.len()
                    ));
                }
                // Where this thread's branch serves its app, so the agent
                // checks its own worktree, not another thread's port.
                let apps = crate::devservers::for_session(&ctx.session);
                if let Some(app) = apps.iter().find(|app| app.page).or(apps.first()) {
                    message.push_str(&format!(
                        " This worktree's app runs at {}; open it with tabs.open.",
                        app.url
                    ));
                }
                let apps: Vec<Value> = apps
                    .iter()
                    .map(|app| json!({ "url": app.url, "process": app.process, "page": app.page }))
                    .collect();
                ActionResult::done(request_id, message)
                    .with_data(json!({ "tabs": tabs, "free": free, "apps": apps }))
            }
            ActionKind::TabsAdopt => {
                let params = match parse_params::<TabsAdoptParams>(request_id, input) {
                    Ok(params) => params,
                    Err(result) => return *result,
                };
                if !self.mode().launches() {
                    return ActionResult::from_reason(request_id, Reason::NoLease).with_message(
                        "Only tabs of the MonoCode browser can be adopted. In the user's own browser, the user hands a tab to this thread in MonoCode.",
                    );
                }
                let free = targets::free_tabs(&self.link)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|tab| tab.id == params.free);
                let Some(free) = free else {
                    return ActionResult::from_reason(request_id, Reason::NoLease).with_message(
                        format!(
                            "{} is not a free tab; call tabs.list for the current ones.",
                            params.free
                        ),
                    );
                };
                match targets::adopt_tab(
                    &self.link,
                    targets::Sender::Agent(ctx),
                    &ctx.session,
                    &free,
                ) {
                    Ok(handle) => {
                        self.adopted(&handle, &ctx.session, &free, Actor::Agent);
                        ActionResult::done(
                            request_id,
                            format!(
                                "Adopted the tab at {} as {}. Take a snapshot before acting on it.",
                                free.url, handle.tab.0
                            ),
                        )
                        .with_data(
                            json!({ "tab": handle.tab, "url": free.url, "title": free.title }),
                        )
                    }
                    Err(error) => error.into_result(
                        request_id,
                        true,
                        self.link.commands_sent() > commands_before,
                    ),
                }
            }
            ActionKind::TabsOpen => {
                let params = match parse_params::<TabsOpenParams>(request_id, input) {
                    Ok(params) => params,
                    Err(result) => return *result,
                };
                match targets::open_tab(
                    &self.link,
                    ctx,
                    params.url.as_deref(),
                    WaitUntil::Usable,
                    self.browser_hidden(),
                ) {
                    Ok(opened) => {
                        self.close_startup_pages();
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
                        let mut result = match opened.navigation {
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
                        .with_observed(opened.observed);
                        if params.url.is_some() {
                            let settings = self.policy.read().settings.clone();
                            semantic::attach_snapshot_for(
                                &self.link,
                                ctx,
                                &opened.handle,
                                &settings,
                                input,
                                &mut result,
                            );
                        }
                        result
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
                });
                self.closed_mid_action(ctx, kind, commands_before, result)
            }
        }
    }

    /// Console errors and failed requests of the action's tab that no result
    /// reported yet go into `observed.signals` and the message, so the agent
    /// hears about a broken page without asking: those the action caused and
    /// those the page produced on its own since the last result.
    fn report_signals(&self, ctx: &RequestCtx, kind: ActionKind, result: &mut ActionResult) {
        if result.outcome == Outcome::Blocked
            || matches!(
                kind,
                ActionKind::Console
                    | ActionKind::Network
                    | ActionKind::TabsList
                    | ActionKind::TabsClose
            )
        {
            return;
        }
        let tab = ctx.tab.clone().or_else(|| {
            result
                .data
                .as_ref()
                .and_then(|d| d.get("tab"))
                .and_then(Value::as_str)
                .map(|t| TabId(t.to_string()))
        });
        let Some(tab) = tab else {
            return;
        };
        let session = {
            let state = self.policy.read();
            state
                .lease(&tab)
                .filter(|l| l.owner == ctx.session)
                .and_then(|l| l.cdp_session.clone())
        };
        let Some(session) = session else {
            return;
        };
        let Some(signals) = self.link.diagnostics().take_signals(&session) else {
            return;
        };
        result.observed.signals = Some(signals);
        result.message = format!(
            "{} {}",
            result.message.trim_end(),
            diagnostics::signals_message(signals)
        );
    }

    /// The tab closed or crashed while the action ran:
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
        // Dedup protects against executing a mutation twice. A
        // `blocked` result executed nothing, so a retry with the same id after
        // the user acted (toggle, consent, grant) must run again.
        if result.outcome != Outcome::Blocked {
            self.policy
                .write()
                .dedup_store(session, &request.request_id, &request.input, &result);
        }
        // Closed tabs vanish from the list after the response that reported them.
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

/// A retry of a request that is still running: its result once it ends,
/// or, after `budget`, an answer that says so instead of running it twice.
fn wait_in_flight(entry: &InFlight, request_id: &str, budget: Duration) -> ActionResult {
    let deadline = Instant::now() + budget;
    let mut state = entry.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match &*state {
            InFlightState::Finished(result) => return (**result).clone(),
            InFlightState::Lost => {
                return ActionResult::from_reason(request_id, Reason::LinkDown).with_message(format!(
                    "The first attempt of request {request_id} ended without a result; the action may have happened. Take a snapshot before deciding whether to repeat it with a new request_id."
                ))
            }
            InFlightState::Running => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return ActionResult::from_reason(request_id, Reason::Busy)
                .with_outcome(Outcome::Unknown)
                .with_message(format!(
                    "Request {request_id} is still running in MonoCode (it may be waiting for the user to allow browser access). Retry later with the same request_id to get its result; a new id would run the action again."
                ));
        }
        state = entry
            .done
            .wait_timeout(state, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

/// What the user sees when a hand-over of a tab failed.
fn tab_error_message(error: targets::TabError) -> String {
    match error {
        targets::TabError::Reason(reason, message) => {
            message.unwrap_or_else(|| reason.message().to_string())
        }
        targets::TabError::Cdp(error) => format!("{error:?}"),
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

/// Pages of a `Target.getTargets` reply that still show a blank or new-tab
/// page.
fn blank_pages(reply: &Value) -> Vec<TargetId> {
    reply["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|info| info["type"] == "page")
        .filter(|info| {
            let url = info["url"].as_str().unwrap_or("");
            url == "about:blank"
                || url.starts_with("chrome://newtab")
                || url.starts_with("edge://newtab")
        })
        .filter_map(|info| info["targetId"].as_str())
        .map(|id| TargetId(id.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

fn require_host() -> Result<Arc<BrowserHost>, String> {
    host().ok_or_else(|| "Browser module is not initialised".to_string())
}

/// Run a command's work on a blocking thread, one command at a time: most
/// of them talk to the browser (detach, close, adopt) and wait for its
/// answers, which must not freeze the UI.
async fn on_host<T: Send + 'static>(
    work: impl FnOnce(&Arc<BrowserHost>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _serial = host.commands.lock().unwrap_or_else(|e| e.into_inner());
        work(&host)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_toggle(on: bool) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.set_toggle(on);
        Ok(host.state())
    })
    .await
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
pub async fn browser_revoke(session_id: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.revoke(&session_id);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_transfer(
    tab: String,
    from_session: String,
    to_session: String,
) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.transfer(&TabId(tab), &from_session, &to_session)
            .map_err(|reason| reason.message().to_string())?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_release_tab(tab: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.release_tab(&TabId(tab))?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_connect() -> Result<BrowserState, String> {
    on_host(|host| {
        host.connect();
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_disconnect() -> Result<BrowserState, String> {
    on_host(|host| {
        host.disconnect();
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_set_session_grant(
    window: WebviewWindow,
    session_id: String,
    enabled: bool,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.set_grant(&session_id, &label, enabled);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_answer_request(
    window: WebviewWindow,
    session_id: String,
    allow: bool,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.answer_request(&session_id, &label, allow);
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_user_tabs() -> Result<Vec<targets::FreeTab>, String> {
    on_host(|host| host.user_tabs()).await
}

#[tauri::command]
pub async fn browser_give_tab(
    window: WebviewWindow,
    free: String,
    session_id: String,
) -> Result<BrowserState, String> {
    let label = window.label().to_string();
    on_host(move |host| {
        host.give_tab(&free, &session_id, &label)?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_open_app(
    window: WebviewWindow,
    session_id: String,
    url: String,
) -> Result<BrowserState, String> {
    let host = require_host()?;
    let label = window.label().to_string();
    let state = host.clone();
    tauri::async_runtime::spawn_blocking(move || host.open_app(&session_id, &label, &url))
        .await
        .map_err(|e| e.to_string())??;
    Ok(state.state())
}

/// "New tab" in a thread's browser pane. Returns the tab's id.
#[tauri::command]
pub async fn browser_new_tab(
    window: WebviewWindow,
    session_id: String,
    url: Option<String>,
) -> Result<String, String> {
    let host = require_host()?;
    let label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        host.new_tab(&session_id, &label, url.as_deref())
            .map(|tab| tab.0)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_pick_element(
    session_id: String,
    tab: String,
    in_pane: Option<bool>,
) -> Result<Option<pick::PickedElement>, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || {
        host.pick_element(&session_id, &TabId(tab), in_pane.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn browser_cancel_pick() -> Result<(), String> {
    require_host()?.cancel_pick();
    Ok(())
}

/// Start the view of `view`. Returns when it ends. Pictures are not sent:
/// the pane asks for each with [`browser_live_frame`].
#[tauri::command]
pub async fn browser_live_start(
    session_id: String,
    tab: String,
    view: String,
    width: u32,
    height: u32,
    scale: Option<f64>,
) -> Result<live::LiveEnd, String> {
    let host = require_host()?;
    let size = live::PanelSize::new(width, height, scale.unwrap_or(1.0));
    tauri::async_runtime::spawn_blocking(move || {
        host.live_view(&session_id, &TabId(tab), &view, size, |_| {})
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The next picture of `view` after `after` (its `seq`), as
/// `live::LiveFrame::packed` bytes; empty when none came within a second
/// or the view is over. One request at a time per pane: the pane asks when
/// it is ready to draw, so it always gets what the tab shows now, and the
/// picture crosses to the webview once, as bytes, without a script run or
/// a callback in between.
#[tauri::command]
pub async fn browser_live_frame(view: String, after: u64) -> Result<tauri::ipc::Response, String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_take(&view, after))
        .await
        .map(tauri::ipc::Response::new)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_live_stop(view: Option<String>) -> Result<(), String> {
    require_host()?.stop_live(view.as_deref());
    Ok(())
}

#[tauri::command]
pub fn browser_live_resize(
    view: String,
    width: u32,
    height: u32,
    scale: Option<f64>,
) -> Result<(), String> {
    require_host()?.resize_live(
        &view,
        live::PanelSize::new(width, height, scale.unwrap_or(1.0)),
    );
    Ok(())
}

#[tauri::command]
pub async fn browser_live_navigate(
    session_id: String,
    tab: String,
    nav: live::LiveNav,
) -> Result<(), String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_navigate(&session_id, &TabId(tab), &nav))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_live_input(
    session_id: String,
    tab: String,
    input: live::LiveInput,
) -> Result<(), String> {
    let host = require_host()?;
    tauri::async_runtime::spawn_blocking(move || host.live_input(&session_id, &TabId(tab), &input))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn browser_open_window() -> Result<BrowserState, String> {
    on_host(|host| {
        host.open_window()?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_close_tab(tab: String) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.close_tab_by_user(&TabId(tab))?;
        Ok(host.state())
    })
    .await
}

#[tauri::command]
pub async fn browser_close_all_tabs() -> Result<BrowserState, String> {
    on_host(|host| {
        host.close_all_tabs();
        Ok(host.state())
    })
    .await
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
pub async fn browser_set_settings(settings: BrowserSettings) -> Result<BrowserState, String> {
    on_host(move |host| {
        host.set_settings(settings);
        Ok(host.state())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::protocol::{BrowserChoice, ConsoleLevel, TabInfo, ISOLATED_WORLD};
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
    fn opening_a_branchs_app_gives_the_thread_a_tab_at_the_app() {
        let server = FakeCdp::start();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        assert!(host
            .open_app("s1", "main", "http://localhost:5174/")
            .unwrap_err()
            .contains("Turn on"));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        assert!(
            host.open_app("s1", "main", "https://example.com/").is_err(),
            "only an app on this computer"
        );
        assert!(!host.policy().read().granted("s1"));

        let tab = host
            .open_app("s1", "main", "http://localhost:5174/")
            .unwrap();
        // The click is the thread's access, and the tab is its own.
        assert!(host.policy().read().granted("s1"));
        let tabs = host.policy().read().tabs_for("s1");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].tab, tab);
        let received = server.received();
        let created = received
            .iter()
            .position(|r| r.method == "Target.createTarget")
            .unwrap();
        assert_eq!(received[created].params["url"], "about:blank");
        // Marker and console are set up before the app loads.
        let marker = received
            .iter()
            .position(|r| r.method == "Page.addScriptToEvaluateOnNewDocument")
            .unwrap();
        let navigate = received
            .iter()
            .position(|r| r.method == "Page.navigate")
            .unwrap();
        assert!(created < marker && marker < navigate);
        assert_eq!(received[navigate].params["url"], "http://localhost:5174/");
        assert!(received[navigate].session.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_tab_from_the_pane_is_the_threads_and_blank_or_at_its_address() {
        let server = FakeCdp::start();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));

        let blank = host.new_tab("s1", "main", None).unwrap();
        // The click is the thread's access, and the tab is its own.
        assert!(host.policy().read().granted("s1"));
        assert_eq!(host.policy().read().tabs_for("s1")[0].tab, blank);
        // Blank: nothing to load, the user types the address.
        assert!(!server
            .received()
            .iter()
            .any(|r| r.method == "Page.navigate"));

        let page = host
            .new_tab("s1", "main", Some(" https://example.com/ "))
            .unwrap();
        assert_ne!(page, blank);
        assert_eq!(host.policy().read().tabs_for("s1").len(), 2);
        let received = server.received();
        let navigate = received
            .iter()
            .find(|r| r.method == "Page.navigate")
            .unwrap();
        assert_eq!(navigate.params["url"], "https://example.com/");
        assert!(host
            .new_tab("s1", "main", Some("file:///etc/passwd"))
            .is_err());
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

    #[test]
    fn blank_pages_are_the_start_page_and_new_tab_pages_only() {
        let reply = json!({ "targetInfos": [
            { "targetId": "A", "type": "page", "url": "about:blank" },
            { "targetId": "B", "type": "page", "url": "chrome://newtab/" },
            { "targetId": "C", "type": "page", "url": "https://github.com/" },
            { "targetId": "D", "type": "service_worker", "url": "about:blank" },
        ]});
        let ids: Vec<String> = blank_pages(&reply).into_iter().map(|t| t.0).collect();
        assert_eq!(ids, vec!["A", "B"]);
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

    /// A fake Chrome whose pages log, throw and fail requests, for the
    /// console and network actions and the signals every result carries.
    fn noisy_fake() -> FakeCdp {
        use super::testing::Event;
        let counter = Arc::new(AtomicU64::new(1));
        FakeCdp::start_with(move |received| {
            let session = received.session.as_deref();
            match received.method.as_str() {
                "Page.navigate" => {
                    let url = received.params["url"].as_str().unwrap_or("").to_string();
                    let mut events = vec![Event::new(
                        "Page.frameNavigated",
                        json!({ "frame": { "id": "T1", "loaderId": "L2", "url": url } }),
                        session,
                    )];
                    if url.starts_with("http://localhost") {
                        events.extend([
                            Event::new(
                                "Runtime.exceptionThrown",
                                json!({ "exceptionDetails": {
                                    "text": "Uncaught",
                                    "exception": { "type": "object", "subtype": "error", "description": "TypeError: user.map is not a function" },
                                    "stackTrace": { "callFrames": [{ "functionName": "App", "url": "http://localhost:5173/src/App.tsx", "lineNumber": 11, "columnNumber": 4 }] }
                                } }),
                                session,
                            ),
                            Event::new(
                                "Network.requestWillBeSent",
                                json!({ "requestId": "R1", "type": "Fetch", "request": { "method": "GET", "url": "http://localhost:5173/api/user" } }),
                                session,
                            ),
                            Event::new(
                                "Network.responseReceived",
                                json!({ "requestId": "R1", "type": "Fetch", "response": { "url": "http://localhost:5173/api/user", "status": 500, "statusText": "Internal Server Error" } }),
                                session,
                            ),
                            Event::new("Network.loadingFinished", json!({ "requestId": "R1" }), session),
                        ]);
                    }
                    events.push(Event::new(
                        "Page.lifecycleEvent",
                        json!({ "frameId": "T1", "loaderId": "L2", "name": "load" }),
                        session,
                    ));
                    Response::ResultThen(json!({ "frameId": "T1", "loaderId": "L2" }), events)
                }
                // Chrome replays the messages it kept before it answers.
                "Runtime.enable" => Response::EventsThen(
                    vec![Event::new(
                        "Runtime.consoleAPICalled",
                        json!({ "type": "warning", "executionContextId": 3, "args": [{ "type": "string", "value": "kept by the browser" }] }),
                        session,
                    )],
                    json!({}),
                ),
                "Network.getResponseBody" => Response::Result(
                    json!({ "body": "{\"error\": \"db down\"}", "base64Encoded": false }),
                ),
                _ => super::testing::default_response(received, &counter),
            }
        })
    }

    /// A page with one button; every inspect mode the user is in gets a
    /// click on it while `click` is set.
    fn pickable_fake(click: Arc<AtomicBool>) -> FakeCdp {
        use super::testing::Event;
        let counter = Arc::new(AtomicU64::new(1));
        FakeCdp::start_with(move |received| {
            let session = received.session.as_deref();
            match received.method.as_str() {
                "Page.navigate" => {
                    let url = received.params["url"].as_str().unwrap_or("").to_string();
                    Response::ResultThen(
                        json!({ "frameId": "T1", "loaderId": "L2" }),
                        vec![
                            Event::new(
                                "Page.frameNavigated",
                                json!({ "frame": { "id": "T1", "loaderId": "L2", "url": url } }),
                                session,
                            ),
                            Event::new(
                                "Page.lifecycleEvent",
                                json!({ "frameId": "T1", "loaderId": "L2", "name": "load" }),
                                session,
                            ),
                        ],
                    )
                }
                "Overlay.setInspectMode"
                    if received.params["mode"] == "searchForNode"
                        && click.load(Ordering::Acquire) =>
                {
                    Response::ResultThen(
                        json!({}),
                        vec![Event::new(
                            "Overlay.inspectNodeRequested",
                            json!({ "backendNodeId": 77 }),
                            session,
                        )],
                    )
                }
                "DOM.describeNode" => Response::Result(json!({
                    "node": { "localName": "button", "attributes": ["id", "save", "style", "x"] }
                })),
                "Accessibility.getPartialAXTree" => Response::Result(json!({ "nodes": [
                    { "backendDOMNodeId": 77, "role": { "value": "button" }, "name": { "value": "Save" } }
                ]})),
                "Accessibility.getFullAXTree" => Response::Result(json!({ "nodes": [
                    { "nodeId": "1", "role": { "value": "RootWebArea" }, "name": { "value": "Form" }, "backendDOMNodeId": 1, "childIds": ["2", "3"] },
                    { "nodeId": "2", "role": { "value": "link" }, "name": { "value": "Home" }, "backendDOMNodeId": 76, "childIds": [] },
                    { "nodeId": "3", "role": { "value": "button" }, "name": { "value": "Save" }, "backendDOMNodeId": 77, "childIds": [] },
                ]})),
                "DOM.getBoxModel" => Response::Result(json!({ "model": {
                    "border": [10.0, 10.0, 90.0, 10.0, 90.0, 40.0, 10.0, 40.0]
                }})),
                "Page.getLayoutMetrics" => Response::Result(json!({
                    "cssLayoutViewport": { "pageX": 0, "pageY": 0, "clientWidth": 800, "clientHeight": 600 }
                })),
                "Page.captureScreenshot" => Response::Result(json!({ "data": "iVBORw0KGgo=" })),
                _ => super::testing::default_response(received, &counter),
            }
        })
    }

    #[test]
    fn a_picked_element_reaches_the_composer_and_the_agents_snapshot() {
        let click = Arc::new(AtomicBool::new(true));
        let server = pickable_fake(click.clone());
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let opened = host.handle_request(
            "s1",
            request(
                "p1",
                "tabs.open",
                json!({ "url": "https://example.com/form", "snapshot": false }),
            ),
        );
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        let tab = TabId(
            opened.data.as_ref().unwrap()["tab"]
                .as_str()
                .unwrap()
                .to_string(),
        );

        let picked = host.pick_element("s1", &tab, false).unwrap().unwrap();
        assert_eq!(picked.role, "button");
        assert_eq!(picked.name, "Save");
        assert_eq!(picked.tag, "<button id=\"save\">");
        assert_eq!(picked.reference, None);
        assert_eq!(picked.image.as_deref(), Some("iVBORw0KGgo="));
        assert!(
            picked
                .text
                .contains(&format!("in tab {} (https://example.com/form)", tab.0))
                && picked.text.ends_with("marks it [picked by the user]."),
            "{}",
            picked.text
        );
        let sent: Vec<(String, Value)> = server
            .received()
            .into_iter()
            .map(|r| (r.method, r.params))
            .collect();
        let at = |method: &str, mode: Option<&str>| {
            sent.iter()
                .position(|(m, p)| m == method && mode.is_none_or(|mode| p["mode"] == mode))
                .unwrap_or_else(|| panic!("{method} not sent: {sent:?}"))
        };
        assert!(
            at("Page.bringToFront", None) < at("Overlay.setInspectMode", Some("searchForNode"))
        );
        assert!(at("DOM.enable", None) < at("Overlay.enable", None));
        // Inspect mode ends before the element is photographed.
        assert!(at("Overlay.setInspectMode", Some("none")) < at("Page.captureScreenshot", None));
        assert!(at("Overlay.disable", None) < at("DOM.disable", None));

        let snapshot =
            host.handle_request("s1", request("p2", "snapshot", json!({ "tab": tab.0 })));
        assert_eq!(snapshot.outcome, Outcome::Done, "{snapshot:?}");
        let data = snapshot.data.unwrap();
        let text = data["snapshot"].as_str().unwrap();
        assert!(
            text.contains("- button \"Save\" [ref=e3] [picked by the user]"),
            "{text}"
        );
        assert!(text.contains("picked: e3 is the element"), "{text}");
        assert_eq!(data["picked"], "e3");
        assert!(
            snapshot.message.contains("pointed at e3"),
            "{}",
            snapshot.message
        );

        // Picked again after the snapshot: the ref comes along.
        let again = host.pick_element("s1", &tab, false).unwrap().unwrap();
        assert_eq!(again.reference.as_deref(), Some("e3"));
        assert!(again.text.ends_with(", ref e3."), "{}", again.text);

        // Another thread's tab is not the user's to pick in for this one.
        let other = host.pick_element("s2", &tab, false).unwrap_err();
        assert!(other.contains("another thread"), "{other}");

        // No click: the user cancels.
        click.store(false, Ordering::Release);
        let waiting = {
            let host = host.clone();
            let tab = tab.clone();
            thread::spawn(move || host.pick_element("s1", &tab, false))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.picking.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the pick never started");
            thread::sleep(Duration::from_millis(5));
        }
        host.cancel_pick();
        assert_eq!(waiting.join().unwrap(), Ok(None));
    }

    /// A tab whose screencast sends `frames` pictures, the next one after
    /// each acknowledgement.
    fn screencast_fake(frames: i64) -> FakeCdp {
        use super::testing::Event;
        let counter = Arc::new(AtomicU64::new(1));
        // The viewport MonoCode asked for: pictures show the page at it.
        let viewport = Arc::new(Mutex::new((800, 600)));
        let picture = move |data: String, n: i64, size: (i64, i64), session: Option<&str>| {
            Event::new(
                "Page.screencastFrame",
                json!({
                    "data": data,
                    "sessionId": n,
                    "metadata": { "deviceWidth": size.0, "deviceHeight": size.1, "offsetTop": 0, "pageScaleFactor": 1 }
                }),
                session,
            )
        };
        let size = {
            let viewport = viewport.clone();
            move || *viewport.lock().unwrap()
        };
        // Valid base64 (of `frame<n>`), so a picture packs for the panel.
        let frame = move |n: i64, session: Option<&str>| {
            use base64::Engine as _;
            let data = base64::engine::general_purpose::STANDARD.encode(format!("frame{n}"));
            picture(data, n, size(), session)
        };
        FakeCdp::start_with(move |received| {
            let session = received.session.as_deref();
            match received.method.as_str() {
                "Emulation.setDeviceMetricsOverride" => {
                    let params = &received.params;
                    *viewport.lock().unwrap() = (
                        params["width"].as_i64().unwrap_or(0),
                        params["height"].as_i64().unwrap_or(0),
                    );
                    Response::Result(json!({}))
                }
                "Page.getLayoutMetrics" => Response::Result(json!({
                    "cssContentSize": { "x": 0, "y": 0, "width": 800, "height": 5000 },
                    "cssLayoutViewport": { "pageX": 0, "pageY": 0, "clientWidth": 800, "clientHeight": 600 },
                    "cssVisualViewport": { "pageX": 0, "pageY": 0, "clientWidth": 800, "clientHeight": 600, "scale": 1 },
                })),
                // While the page loads, one picture of the tab's own window
                // comes before the panel's again.
                "Page.reload" => Response::ResultThen(
                    json!({}),
                    vec![
                        picture("loading".into(), 2000, (1200, 800), session),
                        picture("loaded".into(), 2001, *viewport.lock().unwrap(), session),
                    ],
                ),
                "Page.navigate" => {
                    let url = received.params["url"].as_str().unwrap_or("").to_string();
                    Response::ResultThen(
                        json!({ "frameId": "T1", "loaderId": "L2" }),
                        vec![
                            Event::new(
                                "Page.frameNavigated",
                                json!({ "frame": { "id": "T1", "loaderId": "L2", "url": url } }),
                                session,
                            ),
                            Event::new(
                                "Page.lifecycleEvent",
                                json!({ "frameId": "T1", "loaderId": "L2", "name": "load" }),
                                session,
                            ),
                        ],
                    )
                }
                "Page.startScreencast" => Response::ResultThen(json!({}), vec![frame(1, session)]),
                // Chrome on Windows sends the last picture again right after
                // a screenshot.
                "Page.captureScreenshot" => Response::ResultThen(
                    json!({ "data": "c3RpbGw=" }),
                    vec![{
                        use base64::Engine as _;
                        picture(
                            base64::engine::general_purpose::STANDARD
                                .encode(format!("frame{}", frames.max(1))),
                            1000,
                            *viewport.lock().unwrap(),
                            session,
                        )
                    }],
                ),
                "Page.screencastFrameAck" => {
                    let n = received.params["sessionId"].as_i64().unwrap_or(0);
                    if n < frames {
                        Response::ResultThen(json!({}), vec![frame(n + 1, session)])
                    } else {
                        Response::Result(json!({}))
                    }
                }
                _ => super::testing::default_response(received, &counter),
            }
        })
    }

    fn opened_tab(host: &Arc<BrowserHost>, session: &str, url: &str) -> TabId {
        let opened = host.handle_request(
            session,
            request(
                "open",
                "tabs.open",
                json!({ "url": url, "snapshot": false }),
            ),
        );
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        TabId(
            opened.data.as_ref().unwrap()["tab"]
                .as_str()
                .unwrap()
                .to_string(),
        )
    }

    #[test]
    fn the_live_view_streams_acknowledged_pictures_until_stopped() {
        let server = screencast_fake(3);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");

        let frames = Arc::new(Mutex::new(Vec::new()));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(640, 480, 1.0),
                    |frame| frames.lock().unwrap().push(frame),
                )
            })
        };
        // The panel's own path: it asks for the next picture and gets the
        // newest one, packed, with the page's size in front.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut taken = Vec::new();
        while taken.len() < 3 {
            assert!(Instant::now() < deadline, "pictures never came");
            let after = taken.last().copied().unwrap_or(0);
            let packed = host.live_take("v1", after);
            if packed.is_empty() {
                continue;
            }
            assert!(packed.len() > live::PACKED_HEADER);
            let seq = u64::from(u32::from_le_bytes(packed[4..8].try_into().unwrap()));
            assert!(seq > after);
            assert_eq!(f64::from_le_bytes(packed[8..16].try_into().unwrap()), 640.0);
            taken.push(seq);
        }
        assert_eq!(taken, vec![1, 2, 3]);
        // The pane of another view gets nothing.
        assert!(host.live_take("v2", 0).is_empty());
        host.stop_live(None);
        assert_eq!(viewing.join().unwrap(), Ok(live::LiveEnd::Stopped));
        // Once the view is over, the pane's request comes back empty at once.
        assert!(host.live_take("v1", 3).is_empty());

        // A slow run may end on the still picture too; the moving ones come first.
        let frames: Vec<_> = frames
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.format == "jpeg")
            .cloned()
            .collect();
        assert_eq!(
            frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(frames[0].data, "ZnJhbWUx");
        assert_eq!((frames[0].width, frames[0].height), (640.0, 480.0));
        assert_eq!(frames[0].tab, tab.0);

        let sent = server.received();
        let start = sent
            .iter()
            .find(|r| r.method == "Page.startScreencast")
            .expect("screencast started");
        assert_eq!(start.params["maxWidth"], 642);
        assert_eq!(start.params["format"], "jpeg");
        let acks: Vec<i64> = sent
            .iter()
            .filter(|r| r.method == "Page.screencastFrameAck")
            .filter_map(|r| r.params["sessionId"].as_i64())
            .collect();
        assert_eq!(acks, vec![1, 2, 3]);
        let at = |method: &str| {
            sent.iter()
                .position(|r| r.method == method)
                .unwrap_or_else(|| panic!("{method} not sent"))
        };
        assert!(at("Emulation.setDeviceMetricsOverride") < at("Page.startScreencast"));
        assert!(at("Page.stopScreencast") < at("Emulation.clearDeviceMetricsOverride"));
        let metrics = &sent[at("Emulation.setDeviceMetricsOverride")].params;
        assert_eq!(
            (metrics["width"].as_u64(), metrics["height"].as_u64()),
            (Some(640), Some(480))
        );
    }

    #[test]
    fn a_still_page_gets_one_lossless_picture() {
        let server = screencast_fake(2);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");
        let frames = Arc::new(Mutex::new(Vec::new()));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(640, 480, 1.0),
                    |frame| frames.lock().unwrap().push(frame),
                )
            })
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while frames.lock().unwrap().len() < 3 {
            assert!(Instant::now() < deadline, "no still picture came");
            thread::sleep(Duration::from_millis(5));
        }
        // Still: only one lossless picture, not one per poll.
        thread::sleep(Duration::from_millis(700));
        host.stop_live(None);
        assert_eq!(viewing.join().unwrap(), Ok(live::LiveEnd::Stopped));
        let frames = frames.lock().unwrap().clone();
        let formats: Vec<_> = frames.iter().map(|f| (f.format, f.data.as_str())).collect();
        assert_eq!(
            formats,
            vec![
                ("jpeg", "ZnJhbWUx"),
                ("jpeg", "ZnJhbWUy"),
                ("png", "c3RpbGw=")
            ]
        );
        assert_eq!((frames[2].width, frames[2].height), (640.0, 480.0));
    }

    #[test]
    fn a_picture_of_the_tabs_own_window_never_shows_in_the_pane() {
        let server = screencast_fake(0);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");
        let frames = Arc::new(Mutex::new(Vec::new()));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(640, 480, 1.0),
                    |frame| frames.lock().unwrap().push(frame),
                )
            })
        };
        let shown = |data: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !frames.lock().unwrap().iter().any(|f| f.data == data) {
                assert!(Instant::now() < deadline, "{data} was not shown");
                thread::sleep(Duration::from_millis(5));
            }
        };
        shown("ZnJhbWUx");
        host.live_navigate("s1", &tab, &live::LiveNav::Reload)
            .unwrap();
        shown("loaded");
        host.stop_live(None);
        assert_eq!(viewing.join().unwrap(), Ok(live::LiveEnd::Stopped));
        let frames = frames.lock().unwrap();
        assert!(frames.iter().all(|f| f.data != "loading"), "{frames:?}");
        assert!(frames.iter().all(|f| (f.width, f.height) == (640.0, 480.0)));
        // And the panel's viewport went back to the tab.
        let sent: Vec<_> = server.received().into_iter().map(|r| r.method).collect();
        let reload = sent.iter().position(|m| m == "Page.reload").unwrap();
        assert!(
            sent[reload..]
                .iter()
                .any(|m| m == "Emulation.setDeviceMetricsOverride"),
            "{sent:?}"
        );
    }

    #[test]
    fn a_full_page_screenshot_pauses_the_live_view() {
        let server = screencast_fake(0);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");
        let frames = Arc::new(AtomicU64::new(0));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(640, 480, 1.0),
                    |_| {
                        frames.fetch_add(1, Ordering::Relaxed);
                    },
                )
            })
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while frames.load(Ordering::Relaxed) == 0 {
            assert!(Instant::now() < deadline, "no picture");
            thread::sleep(Duration::from_millis(5));
        }
        let shot = host.handle_request(
            "s1",
            request(
                "full",
                "screenshot",
                json!({ "tab": tab.0, "full_page": true }),
            ),
        );
        assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
        // The view starts again at the pane's size once the shot is taken.
        let deadline = Instant::now() + Duration::from_secs(5);
        let sent = loop {
            let sent = server.received();
            let full = sent
                .iter()
                .position(|r| r.params["captureBeyondViewport"] == true)
                .expect("a full-page capture");
            if sent[full..]
                .iter()
                .any(|r| r.method == "Page.startScreencast")
            {
                break sent;
            }
            assert!(Instant::now() < deadline, "the view did not resume");
            thread::sleep(Duration::from_millis(10));
        };
        host.stop_live(None);
        assert_eq!(viewing.join().unwrap(), Ok(live::LiveEnd::Stopped));
        let full = sent
            .iter()
            .position(|r| r.params["captureBeyondViewport"] == true)
            .unwrap();
        let last_start = sent[..full]
            .iter()
            .rposition(|r| r.method == "Page.startScreencast")
            .unwrap();
        let methods: Vec<_> = sent.iter().map(|r| r.method.as_str()).collect();
        for paused in [
            "Page.stopScreencast",
            "Emulation.clearDeviceMetricsOverride",
        ] {
            assert!(
                sent[last_start..full].iter().any(|r| r.method == paused),
                "{methods:?}"
            );
        }
        let resumed = &sent[full..];
        let metrics = resumed
            .iter()
            .position(|r| r.method == "Emulation.setDeviceMetricsOverride")
            .expect("the pane's viewport again");
        assert_eq!(resumed[metrics].params["width"], 640);
    }

    #[test]
    fn a_moved_pane_takes_over_the_tab_after_the_old_view_hands_it_back() {
        let server = screencast_fake(0);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");
        let view = |name: &'static str| {
            let (host, tab) = (host.clone(), tab.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    name,
                    live::PanelSize::new(640, 480, 1.0),
                    |_| {},
                )
            })
        };
        let started = |count: usize| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while server
                .received()
                .iter()
                .filter(|r| r.method == "Page.startScreencast")
                .count()
                < count
            {
                assert!(Instant::now() < deadline, "the view never started");
                thread::sleep(Duration::from_millis(5));
            }
        };
        let old = view("old");
        started(1);
        let new = view("new");
        assert_eq!(old.join().unwrap(), Ok(live::LiveEnd::Stopped));
        started(2);
        // The old pane goes away late: the new view keeps running.
        host.stop_live(Some("old"));
        thread::sleep(Duration::from_millis(250));
        assert!(!new.is_finished());
        host.stop_live(Some("new"));
        assert_eq!(new.join().unwrap(), Ok(live::LiveEnd::Stopped));

        // The old view's clean-up came before the new view's viewport.
        let sent = server.received();
        let methods: Vec<&str> = sent.iter().map(|r| r.method.as_str()).collect();
        let first_clear = methods
            .iter()
            .position(|m| *m == "Emulation.clearDeviceMetricsOverride")
            .unwrap();
        let second_metrics = methods
            .iter()
            .enumerate()
            .filter(|(_, m)| **m == "Emulation.setDeviceMetricsOverride")
            .nth(1)
            .map(|(i, _)| i)
            .unwrap();
        assert!(first_clear < second_metrics, "{methods:?}");
    }

    #[test]
    fn live_input_reaches_only_the_owning_threads_tab() {
        let server = screencast_fake(0);
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", "https://example.com/");
        let click: live::LiveInput = serde_json::from_value(json!({
            "kind": "mouse", "event": "mousePressed", "x": 12.0, "y": 34.0,
            "button": "left", "buttons": 1, "clickCount": 1
        }))
        .unwrap();

        host.live_input("s1", &tab, &click).unwrap();
        let pressed = server
            .received()
            .into_iter()
            .find(|r| r.method == "Input.dispatchMouseEvent")
            .expect("click sent");
        assert_eq!(pressed.params["type"], "mousePressed");
        assert_eq!(
            (pressed.params["x"].as_f64(), pressed.params["y"].as_f64()),
            (Some(12.0), Some(34.0))
        );

        let refused = host.live_input("s2", &tab, &click).unwrap_err();
        assert!(refused.contains("another thread"), "{refused}");
        let other = host
            .live_view(
                "s2",
                &tab,
                "v2",
                live::PanelSize::new(640, 480, 1.0),
                |_| {},
            )
            .unwrap_err();
        assert!(other.contains("another thread"), "{other}");
    }

    /// A real Chrome at `exe` on a fresh profile, ready for MonoCode to
    /// connect to.
    fn start_real_chrome(exe: &std::ffi::OsStr) -> (std::process::Child, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("monocode-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut args = vec![
            format!("--user-data-dir={}", dir.display()),
            "--remote-debugging-port=0".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--window-size=1000,700".to_string(),
        ];
        #[cfg(unix)]
        if unsafe { libc::geteuid() } == 0 {
            args.push("--no-sandbox".to_string());
        }
        if std::env::var_os("DISPLAY").is_none() && cfg!(target_os = "linux") {
            args.push("--headless=new".to_string());
        }
        args.push("about:blank".to_string());
        let chrome = std::process::Command::new(exe).args(&args).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !dir.join(discovery::ACTIVE_PORT_FILE).is_file() {
            assert!(Instant::now() < deadline, "Chrome did not start");
            thread::sleep(Duration::from_millis(50));
        }
        // The port is up a moment before the first window.
        thread::sleep(Duration::from_secs(1));

        (chrome, dir)
    }

    /// Against a real Chrome: a full-page screenshot of a long page could
    /// leave the tab's viewport the page's height, and the pane showed the
    /// whole page squeezed into a strip.
    #[test]
    fn a_full_page_screenshot_leaves_the_pane_at_its_size_in_a_real_chrome() {
        let Some(exe) = std::env::var_os("MONOCODE_TEST_CHROME") else {
            return;
        };
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let page = format!(
                "<!doctype html><title>long</title><body style='margin:0;font:30px sans-serif'>\
                <div id=n>0</div><p>{}</p>\
                <script>let k=0;setInterval(()=>document.getElementById('n').textContent=++k,100)</script>",
                "lorem ipsum dolor ".repeat(2000)
            );
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let page = page.clone();
                thread::spawn(move || {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                        page.len()
                    );
                });
            }
        });
        let (mut chrome, dir) = start_real_chrome(&exe);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", &format!("http://127.0.0.1:{port}/"));
        let frames = Arc::new(Mutex::new(Vec::new()));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(900, 600, 1.0),
                    |frame| frames.lock().unwrap().push(frame),
                )
            })
        };
        thread::sleep(Duration::from_secs(1));
        let taking = Instant::now();
        let shot = host.handle_request(
            "s1",
            request(
                "full",
                "screenshot",
                json!({ "tab": tab.0, "full_page": true }),
            ),
        );
        let took = taking.elapsed();
        assert_eq!(shot.outcome, Outcome::Done, "{shot:?}");
        let seen = frames.lock().unwrap().len();
        thread::sleep(Duration::from_millis(1500));
        host.stop_live(None);
        let _ = viewing.join();
        host.set_toggle(false);
        let _ = chrome.kill();
        let _ = chrome.wait();
        let _ = std::fs::remove_dir_all(&dir);
        let after: Vec<_> = frames.lock().unwrap()[seen..]
            .iter()
            .map(|f| (f.width, f.height))
            .collect();
        assert!(after.len() > 5, "{after:?}");
        assert!(
            after.iter().all(|size| *size == (900.0, 600.0)),
            "{after:?}"
        );
        // As fast as without a pane: with the pictures running it took
        // several times longer.
        eprintln!("full-page screenshot with the pane open: {took:?}");
        assert!(took < Duration::from_secs(6), "{took:?}");
    }

    /// Against a real Chrome: in a hidden MonoCode browser the tab's own
    /// window takes the panel's size, so even a navigation to another site
    /// (a new renderer) never shows the page at another size, and the size
    /// stays the window's once the panel closes.
    #[test]
    fn the_live_view_gives_the_tabs_own_window_the_panels_size_in_a_real_chrome() {
        let Some(exe) = std::env::var_os("MONOCODE_TEST_CHROME") else {
            return;
        };
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            const PAGE: &str = "<!doctype html><title>page</title><body style='margin:0;font:20px sans-serif'>\
                <div id=n>0</div><script>let k=0;(function f(){document.getElementById('n').textContent=++k;requestAnimationFrame(f)})()</script>";
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                thread::spawn(move || {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                        PAGE.len()
                    );
                });
            }
        });
        let (mut chrome, dir) = start_real_chrome(&exe);
        let host = BrowserHost::for_tests(BrowserSettings {
            separate_window: true,
            ..settings_for(&dir)
        });
        host.hidden_for_tests.store(true, Ordering::Release);
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", &format!("http://127.0.0.1:{port}/a"));
        let frames = Arc::new(Mutex::new(Vec::new()));
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(900, 600, 1.0),
                    |frame| frames.lock().unwrap().push((frame.width, frame.height)),
                )
            })
        };
        thread::sleep(Duration::from_millis(800));
        // Another site: Chrome gives the page a new renderer.
        host.live_navigate(
            "s1",
            &tab,
            &live::LiveNav::Go {
                url: format!("http://localhost:{port}/b"),
            },
        )
        .unwrap();
        thread::sleep(Duration::from_millis(1500));
        host.stop_live(None);
        let _ = viewing.join();
        let target = host.held_tab("s1", &tab).unwrap();
        let metrics = host
            .link
            .send_system(Command::in_session(
                &target.main,
                Method::PageGetLayoutMetrics,
                json!({}),
            ))
            .unwrap();
        host.set_toggle(false);
        let _ = chrome.kill();
        let _ = chrome.wait();
        let _ = std::fs::remove_dir_all(&dir);
        let frames = frames.lock().unwrap().clone();
        assert!(frames.len() > 10, "{frames:?}");
        assert!(
            frames
                .iter()
                .all(|(w, h)| (w - 900.0).abs() <= 1.5 && (h - 600.0).abs() <= 1.5),
            "{frames:?}"
        );
        let viewport = &metrics["cssVisualViewport"];
        assert!(
            (viewport["clientWidth"].as_f64().unwrap() - 900.0).abs() <= 1.5,
            "{metrics}"
        );
        assert!(
            (viewport["clientHeight"].as_f64().unwrap() - 600.0).abs() <= 1.5,
            "{metrics}"
        );
    }

    /// Against a real Chrome, when `MONOCODE_TEST_CHROME` names one: the
    /// agent's tab stays in the background, and the live view still gets
    /// moving pictures, and a click and typed text reach the page.
    #[test]
    fn live_view_of_a_background_tab_in_a_real_chrome() {
        let Some(exe) = std::env::var_os("MONOCODE_TEST_CHROME") else {
            return;
        };
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            const PAGE: &str = "<!doctype html><meta charset=utf-8><title>start</title>\
                <body style='margin:0;font:20px sans-serif'><div id=n>0</div>\
                <button id=b style='position:absolute;left:20px;top:80px;width:200px;height:60px' \
                onclick=\"this.textContent='clicked'\">click me</button>\
                <form onsubmit=\"document.getElementById('b').textContent='sent: '+document.getElementById('i').value;return false\">\
                <input id=i oninput=\"document.getElementById('b').textContent='typed: '+this.value\" style='position:absolute;left:20px;top:160px;width:300px;height:40px'></form>\
                <script>let k=0;(function f(){document.getElementById('n').textContent=++k;requestAnimationFrame(f)})()</script>";
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // One thread each: a connection Chrome opens ahead and
                // leaves idle must not hold up the next request.
                thread::spawn(move || {
                    let mut buffer = [0u8; 2048];
                    let read = stream.read(&mut buffer).unwrap_or(0);
                    // A page that keeps still, for the lossless picture.
                    let page = if buffer[..read].starts_with(b"GET /calm") {
                        "<!doctype html><title>calm</title><h1>calm</h1>"
                    } else {
                        PAGE
                    };
                    let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
                });
            }
        });

        let (mut chrome, dir) = start_real_chrome(&exe);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(
            wait_state(&host, ConnectionState::Connected),
            "{:?}",
            host.connection()
        );
        host.turn_started("s1", "main", "/repo", Some(true));
        let tab = opened_tab(&host, "s1", &format!("http://127.0.0.1:{port}/"));

        let frames = Arc::new(Mutex::new(Vec::new()));
        let started = Instant::now();
        let viewing = {
            let (host, tab, frames) = (host.clone(), tab.clone(), frames.clone());
            thread::spawn(move || {
                host.live_view(
                    "s1",
                    &tab,
                    "v1",
                    live::PanelSize::new(900, 600, 1.0),
                    |frame| frames.lock().unwrap().push((started.elapsed(), frame)),
                )
            })
        };
        // The panel's own path meanwhile: pull pictures the way the pane does.
        let pulled = {
            let host = host.clone();
            thread::spawn(move || {
                let mut got = Vec::new();
                let mut after = 0;
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline {
                    let packed = host.live_take("v1", after);
                    if packed.is_empty() {
                        continue;
                    }
                    let seq = u64::from(u32::from_le_bytes(packed[4..8].try_into().unwrap()));
                    assert!(seq > after);
                    assert_eq!(packed[live::PACKED_HEADER..][..2], [0xFF, 0xD8], "a JPEG");
                    after = seq;
                    got.push(seq);
                }
                got
            })
        };
        thread::sleep(Duration::from_secs(3));
        let count = frames.lock().unwrap().len();
        let pulled = pulled.join().unwrap();
        assert!(
            pulled.len() > 10,
            "the panel took {} pictures",
            pulled.len()
        );

        let send = |input: Value| {
            let input: live::LiveInput = serde_json::from_value(input).unwrap();
            host.live_input("s1", &tab, &input).unwrap();
        };
        let click = |x: f64, y: f64| {
            for (event, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
                send(json!({ "kind": "mouse", "event": event, "x": x, "y": y,
                    "button": "left", "buttons": buttons, "clickCount": 1 }));
            }
        };
        let looks = AtomicU64::new(0);
        let snapshot = || {
            let id = format!("look{}", looks.fetch_add(1, Ordering::Relaxed));
            let taken =
                host.handle_request("s1", request(&id, "snapshot", json!({ "tab": tab.0 })));
            taken
                .data
                .as_ref()
                .and_then(|d| d["snapshot"].as_str())
                .unwrap_or("")
                .to_string()
        };
        let wait_for = |want: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let now = snapshot();
                if now.contains(want) || Instant::now() >= deadline {
                    return now;
                }
                thread::sleep(Duration::from_millis(100));
            }
        };
        click(120.0, 110.0);
        let after_click = wait_for("button \"clicked\"");
        assert!(after_click.contains("button \"clicked\""), "{after_click}");
        click(170.0, 180.0);
        send(json!({ "kind": "text", "text": "Ćao šđž" }));
        let after_text = wait_for("button \"typed: Ćao šđž\"");
        assert!(
            after_text.contains("button \"typed: Ćao šđž\""),
            "{after_text}"
        );
        send(
            json!({ "kind": "key", "event": "keyDown", "key": "Enter", "code": "Enter",
            "text": "\r", "keyCode": 13 }),
        );
        send(
            json!({ "kind": "key", "event": "keyUp", "key": "Enter", "code": "Enter",
            "keyCode": 13 }),
        );
        let after_typing = wait_for("sent: Ćao šđž");
        assert!(after_typing.contains("sent: Ćao šđž"), "{after_typing}");

        // "Point at an element" in the pane: the click on the picture picks.
        let picking = {
            let (host, tab) = (host.clone(), tab.clone());
            thread::spawn(move || host.pick_element("s1", &tab, true))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let picked = loop {
            send(
                json!({ "kind": "mouse", "event": "mouseMoved", "x": 120.0, "y": 110.0,
                "button": "none", "buttons": 0, "clickCount": 0 }),
            );
            click(120.0, 110.0);
            thread::sleep(Duration::from_millis(200));
            if picking.is_finished() || Instant::now() >= deadline {
                host.cancel_pick();
                break picking.join().unwrap();
            }
        };
        let picked = picked.unwrap().expect("the click in the pane picked");
        assert_eq!(picked.role, "button", "{picked:?}");

        // The pane got smaller: pictures follow at the new size.
        host.resize_live("v1", live::PanelSize::new(700, 500, 1.0));
        let deadline = Instant::now() + Duration::from_secs(5);
        while frames.lock().unwrap().last().map(|(_, f)| f.width) != Some(700.0) {
            assert!(Instant::now() < deadline, "the view kept its old size");
            thread::sleep(Duration::from_millis(50));
        }
        // The address field takes a bare host; Back returns.
        let current = || {
            host.state()
                .tabs
                .into_iter()
                .find(|t| t.tab.tab == tab)
                .unwrap()
                .tab
                .url
        };
        let wait_url = |want: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !current().ends_with(want) {
                assert!(Instant::now() < deadline, "{} is not {want}", current());
                thread::sleep(Duration::from_millis(50));
            }
        };
        host.live_navigate(
            "s1",
            &tab,
            &live::LiveNav::Go {
                url: format!("127.0.0.1:{port}/next"),
            },
        )
        .unwrap();
        wait_url("/next");
        host.live_navigate("s1", &tab, &live::LiveNav::Back)
            .unwrap();
        wait_url(&format!("{port}/"));
        // A page that stops moving gets one lossless picture at the pane's size.
        host.live_navigate(
            "s1",
            &tab,
            &live::LiveNav::Go {
                url: format!("127.0.0.1:{port}/calm"),
            },
        )
        .unwrap();
        wait_url("/calm");
        let deadline = Instant::now() + Duration::from_secs(5);
        let png = loop {
            let last = frames.lock().unwrap().last().map(|(_, f)| f.clone());
            if let Some(frame) = last.filter(|f| f.format == "png") {
                break frame;
            }
            assert!(Instant::now() < deadline, "no lossless picture");
            thread::sleep(Duration::from_millis(50));
        };
        {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&png.data)
                .unwrap();
            assert_eq!(&bytes[1..4], b"PNG");
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            assert_eq!((width, png.width), (700, 700.0));
        }
        assert!(host
            .live_navigate(
                "s1",
                &tab,
                &live::LiveNav::Go {
                    url: "file:///etc/passwd".into()
                }
            )
            .is_err());
        // An older pane of the same tab going away leaves this view running.
        host.stop_live(Some("v0"));
        assert!(!viewing.is_finished());

        host.stop_live(None);
        assert_eq!(viewing.join().unwrap(), Ok(live::LiveEnd::Stopped));
        let frames = frames.lock().unwrap();
        let first = frames.first().map(|(at, _)| *at);
        eprintln!(
            "live view: {count} pictures in 3 s, first after {first:?}, last {}x{} css px, {} bytes",
            frames.last().map_or(0.0, |(_, f)| f.width),
            frames.last().map_or(0.0, |(_, f)| f.height),
            frames.last().map_or(0, |(_, f)| f.data.len()),
        );
        if let (Some(path), Some((_, frame))) =
            (std::env::var_os("MONOCODE_TEST_LIVE_FRAME"), frames.last())
        {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&frame.data)
                .unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        host.set_toggle(false);
        let _ = chrome.kill();
        let _ = chrome.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(count >= 30, "only {count} pictures in 3 s");
    }

    #[test]
    fn console_network_and_signals_reach_the_agent() {
        let server = noisy_fake();
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        host.turn_started("s1", "main", "/repo", Some(true));
        let methods = |server: &FakeCdp| -> Vec<String> {
            server.received().into_iter().map(|r| r.method).collect()
        };

        // A local server: capture is on before its scripts run, and the
        // result of the very action that loaded it reports the trouble.
        let opened = host.handle_request(
            "s1",
            request(
                "c1",
                "tabs.open",
                json!({ "url": "http://localhost:5173/" }),
            ),
        );
        assert_eq!(opened.outcome, Outcome::Done, "{opened:?}");
        let tab = opened.data.as_ref().unwrap()["tab"]
            .as_str()
            .unwrap()
            .to_string();
        let sent = methods(&server);
        let enable = sent.iter().position(|m| m == "Runtime.enable").unwrap();
        let navigate = sent.iter().position(|m| m == "Page.navigate").unwrap();
        assert!(enable < navigate, "{sent:?}");
        assert_eq!(
            opened.observed.signals,
            Some(protocol::PageSignals {
                console_errors: 1,
                failed_requests: 1
            })
        );
        assert!(
            opened
                .message
                .contains("1 new console error and 1 failed request"),
            "{}",
            opened.message
        );

        let console = host.handle_request("s1", request("c2", "console", json!({ "tab": tab })));
        assert_eq!(console.outcome, Outcome::Done, "{console:?}");
        let data = console.data.unwrap();
        let text = data["text"].as_str().unwrap();
        assert!(
            text.contains(
                "error (uncaught exception): Uncaught TypeError: user.map is not a function"
            ),
            "{text}"
        );
        assert!(
            text.contains("at App (http://localhost:5173/src/App.tsx:12:5)"),
            "{text}"
        );
        assert!(
            !console.message.contains("capture started"),
            "{}",
            console.message
        );
        let next = data["next_since"].as_u64().unwrap();

        let network = host.handle_request("s1", request("c3", "network", json!({ "tab": tab })));
        let text = network.data.as_ref().unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            text.contains("GET http://localhost:5173/api/user: 500 Internal Server Error (fetch)"),
            "{text}"
        );
        assert!(text.contains(r#"body: {"error": "db down"}"#), "{text}");
        assert!(methods(&server)
            .iter()
            .any(|m| m == "Network.getResponseBody"));

        // Read once, reported once: the next result is quiet...
        let quiet = host.handle_request(
            "s1",
            request(
                "c4",
                "wait",
                json!({ "tab": tab, "for": "time", "value": 1 }),
            ),
        );
        assert_eq!(quiet.outcome, Outcome::Done, "{quiet:?}");
        assert_eq!(quiet.observed.signals, None);
        // ...until the page breaks again on its own.
        let session = host
            .policy()
            .read()
            .lease(&TabId(tab.clone()))
            .unwrap()
            .cdp_session
            .clone()
            .unwrap();
        server.emit(
            "Runtime.consoleAPICalled",
            json!({ "type": "error", "executionContextId": 3, "args": [{ "type": "string", "value": "save failed" }] }),
            Some(&session.0),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while host
            .link()
            .diagnostics()
            .console(&session, ConsoleLevel::All, next, 10)
            .entries
            .is_empty()
        {
            assert!(Instant::now() < deadline, "the error never arrived");
            thread::sleep(Duration::from_millis(5));
        }
        let fresh = host.handle_request(
            "s1",
            request(
                "c5",
                "console",
                json!({ "tab": tab, "since": next, "level": "error" }),
            ),
        );
        let text = fresh.data.unwrap()["text"].as_str().unwrap().to_string();
        assert!(text.contains("error (console): save failed"), "{text}");
        assert!(!text.contains("TypeError"), "{text}");

        // A remote site: capture goes off before it loads, and comes back
        // only when the agent asks, with what the browser kept.
        host.handle_request("s1", request("c6", "network", json!({ "tab": tab })));
        let before = methods(&server).len();
        let away = host.handle_request(
            "s1",
            request(
                "c7",
                "navigate",
                json!({ "tab": tab, "url": "https://example.com/" }),
            ),
        );
        assert_eq!(away.outcome, Outcome::Done, "{away:?}");
        let sent = methods(&server)[before..].to_vec();
        assert_eq!(
            sent.first().map(String::as_str),
            Some("Runtime.disable"),
            "{sent:?}"
        );
        assert!(!host.link().diagnostics().capturing(&session));
        let asked = host.handle_request(
            "s1",
            request("c8", "console", json!({ "tab": tab, "level": "warning" })),
        );
        assert!(
            asked.message.contains("Console capture started now"),
            "{}",
            asked.message
        );
        let text = asked.data.unwrap()["text"].as_str().unwrap().to_string();
        assert!(
            text.contains("warning (console): kept by the browser"),
            "{text}"
        );
        assert!(host.link().diagnostics().capturing(&session));
        // Asked once, it stays on for this tab even on remote pages.
        let before = methods(&server).len();
        host.handle_request(
            "s1",
            request(
                "c9",
                "navigate",
                json!({ "tab": tab, "url": "https://example.org/" }),
            ),
        );
        assert!(!methods(&server)[before..]
            .iter()
            .any(|m| m == "Runtime.disable"));

        // Another session's agent cannot read this tab.
        host.turn_started("s2", "main", "/repo", Some(true));
        let foreign = host.handle_request("s2", request("x1", "console", json!({ "tab": tab })));
        assert_eq!(foreign.reason, Some(Reason::NoLease));
        host.set_toggle(false);
        let _ = std::fs::remove_dir_all(&dir);
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

    /// The agent's client lost the answer while the user was still deciding
    /// and retried with the same id: the retry waits for the first attempt
    /// and gets its result; the action runs once.
    #[test]
    fn a_retry_of_a_request_still_running_gets_its_result() {
        let server = FakeCdp::start();
        let (host, dir) = managed_host(&server, AccessMode::Ask);
        host.set_toggle(true);
        host.turn_started("s1", "main", "/repo", None);
        let send = |id: &str, input: Value| {
            let host = host.clone();
            let request = request(id, "tabs.open", input);
            thread::spawn(move || host.handle_request("s1", request))
        };
        let first = send("o1", json!({}));
        assert!(wait_requested(&host, "s1"));
        let retry = send("o1", json!({}));
        // Same id, other input: refused while the first one runs.
        let conflict = host.handle_request(
            "s1",
            request("o1", "tabs.open", json!({ "url": "https://x/" })),
        );
        assert_eq!(conflict.reason, Some(Reason::InvalidRequest));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host
            .inflight
            .lock()
            .unwrap()
            .values()
            .any(|entry| Arc::strong_count(entry) > 2)
        {
            assert!(Instant::now() < deadline, "the retry never waited");
            thread::sleep(Duration::from_millis(5));
        }
        host.answer_request("s1", "main", true);
        let first = first.join().unwrap();
        let retry = retry.join().unwrap();
        assert_eq!(first.outcome, Outcome::Done, "{first:?}");
        assert_eq!(retry.data, first.data);
        assert_eq!(host.policy().read().tabs_for("s1").len(), 1);
        let creates = server
            .received()
            .iter()
            .filter(|r| r.method == "Target.createTarget")
            .count();
        assert_eq!(creates, 1);
        assert!(host.inflight.lock().unwrap().is_empty());
        host.set_toggle(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stand-in browser that never opens its debugging port.
    #[cfg(unix)]
    fn silent_browser() -> (launch::Executable, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("monocode-silent-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser");
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = launch::Executable {
            kind: protocol::BrowserKind::Chrome,
            path,
        };
        (exe, dir)
    }

    /// OFF or a mode change while the MonoCode browser is still starting:
    /// the browser goes, and a private profile with it.
    #[cfg(unix)]
    #[test]
    fn a_browser_still_starting_stops_with_off_and_mode_changes() {
        let (exe, bin) = silent_browser();
        for mode_change in [false, true] {
            let root = bin.join(format!("app-{mode_change}"));
            let host = BrowserHost::for_tests(BrowserSettings {
                mode: BrowserMode::Private,
                access: AccessMode::Always,
                ..BrowserSettings::default()
            });
            *host.test_executable.lock().unwrap() = Some(exe.clone());
            host.set_profile_dir(root.join("browser-profile"));
            host.set_toggle(true);
            host.connect();
            let deadline = Instant::now() + Duration::from_secs(5);
            let (pid, profile) = loop {
                if let Some(managed) = host.managed.lock().unwrap().as_ref() {
                    break (managed.child.id(), managed.profile.clone());
                }
                assert!(Instant::now() < deadline, "the browser never started");
                thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(host.connection().state, ConnectionState::Discovering);
            if mode_change {
                host.set_settings(BrowserSettings {
                    mode: BrowserMode::Managed,
                    ..host.state().settings
                });
            } else {
                host.set_toggle(false);
            }
            let alive = || unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
            assert!(eventually(|| !alive()), "the starting browser kept running");
            assert!(
                eventually(|| !profile.exists()),
                "the private profile stays"
            );
            assert!(eventually(|| host.connecting.load(Ordering::Acquire) == 0));
            assert!(host.managed.lock().unwrap().is_none());
            // The cancelled start reports nothing over the new state.
            let expected = if mode_change {
                ConnectionState::Idle
            } else {
                ConnectionState::Off
            };
            assert_eq!(host.connection().state, expected);
        }
        let _ = std::fs::remove_dir_all(&bin);
    }

    /// A hand-over that fails leaves the thread without browser access it
    /// did not have before, and keeps a grant it had.
    #[test]
    fn a_failed_hand_over_grants_nothing() {
        let counter = Arc::new(AtomicU64::new(1));
        let server = FakeCdp::start_with(move |received| match received.method.as_str() {
            "Target.getTargets" => Response::Result(json!({ "targetInfos": [
                { "targetId": "P1", "type": "page", "url": "https://example.com/", "title": "Example", "attached": false },
            ]})),
            "Target.attachToTarget" => Response::Error {
                code: -32000,
                message: "No target with given id found".into(),
            },
            _ => super::testing::default_response(received, &counter),
        });
        let dir = profile_for(&server);
        let host = BrowserHost::for_tests(settings_for(&dir));
        host.set_toggle(true);
        assert!(wait_state(&host, ConnectionState::Connected));
        let free = host.user_tabs().unwrap();
        assert_eq!(free.len(), 1, "{free:?}");
        assert!(host.give_tab(&free[0].id, "s1", "main").is_err());
        assert!(!host.policy().read().granted("s1"));
        assert!(host
            .open_app("s1", "main", "http://localhost:5174/")
            .is_err());
        assert!(!host.policy().read().granted("s1"));
        // A grant the thread already had stays.
        host.set_grant("s2", "main", true);
        assert!(host.give_tab(&free[0].id, "s2", "main").is_err());
        assert!(host.policy().read().granted("s2"));
        assert!(host.policy().read().tabs_for("s1").is_empty());
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
        // The start page made way for the agent's tab.
        let targets = host
            .link()
            .send_system(Command::browser(Method::TargetGetTargets, json!({})))
            .unwrap();
        let pages = targets["targetInfos"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|info| info["type"] == "page")
            .count();
        assert_eq!(pages, 1, "{targets}");
        let tab = opened.data.unwrap()["tab"].as_str().unwrap().to_string();
        let read = host.handle_request("s1", request("m2", "read", json!({ "tab": tab })));
        assert_eq!(read.outcome, Outcome::Done, "{read:?}");
        host.set_toggle(false);
        // OFF closes the browser in the background.
        assert!(eventually(|| launch::running(&dir).is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Up to 15 s for something OFF finishes in the background.
    fn eventually(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(100));
        }
        done()
    }

    /// Tabs the user opened in the MonoCode browser: an agent adopts one, the
    /// user hands another to a second thread. Needs network for example.com.
    #[test]
    #[ignore = "starts the Chrome or Edge installed on this machine"]
    fn user_tabs_can_be_adopted_and_handed_over() {
        let dir = std::env::temp_dir().join(format!("monocode-adopt-{}", uuid::Uuid::new_v4()));
        let host = BrowserHost::for_tests(BrowserSettings {
            access: AccessMode::Always,
            show_window: false,
            ..BrowserSettings::default()
        });
        host.set_profile_dir(dir.clone());
        host.set_toggle(true);
        host.turn_started("s1", "main", "/repo", Some(true));
        let listed = host.handle_request("s1", request("a0", "tabs.list", json!({})));
        assert_eq!(listed.outcome, Outcome::Done, "{listed:?}");
        let user_page = |url: &str| {
            host.link()
                .send_system(Command::browser(
                    Method::TargetCreateTarget,
                    json!({ "url": url }),
                ))
                .unwrap();
            thread::sleep(Duration::from_millis(1500));
        };
        user_page("https://example.com/?user=1");
        let listed = host.handle_request("s1", request("a1", "tabs.list", json!({})));
        let data = listed.data.unwrap();
        let free = data["free"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tab| tab["url"].as_str().unwrap().contains("user=1"))
            .unwrap_or_else(|| panic!("{data}"))["id"]
            .as_str()
            .unwrap()
            .to_string();
        let adopted =
            host.handle_request("s1", request("a2", "tabs.adopt", json!({ "free": free })));
        assert_eq!(adopted.outcome, Outcome::Done, "{adopted:?}");
        let tab = adopted.data.unwrap()["tab"].as_str().unwrap().to_string();
        let read = host.handle_request("s1", request("a3", "read", json!({ "tab": tab })));
        assert_eq!(read.outcome, Outcome::Done, "{read:?}");
        // Adopted once: no longer free.
        let again = host.handle_request("s1", request("a4", "tabs.adopt", json!({ "free": free })));
        assert_eq!(again.reason, Some(Reason::NoLease));

        user_page("https://example.com/?user=2");
        let mine = host
            .user_tabs()
            .unwrap()
            .into_iter()
            .find(|tab| tab.url.contains("user=2"))
            .unwrap();
        assert!(!host.policy().read().granted("s2"));
        host.give_tab(&mine.id, "s2", "main").unwrap();
        assert!(host.policy().read().granted("s2"));
        assert_eq!(host.policy().read().tabs_for("s2").len(), 1);
        host.set_toggle(false);
        assert!(eventually(|| launch::running(&dir).is_none()));
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
        let left = || std::fs::read_dir(&private).map(|d| d.count()).unwrap_or(0);
        assert!(eventually(|| left() == 0), "the private profile is deleted");
        let _ = std::fs::remove_dir_all(&root);
    }
}
