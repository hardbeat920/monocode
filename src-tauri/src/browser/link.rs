//! BrowserLink: the one WebSocket to the browser (spec 5).
//!
//! A single `browser-link` thread owns the socket. Callers hand it commands
//! over a channel; the thread re-checks the policy under a read lock and
//! writes the frame while still holding that lock, so a revocation (which
//! takes the write lock) can never interleave between the check and the
//! write. That is the whole OFF guarantee: after OFF or any epoch bump no new
//! command reaches the socket, while a frame already written may still be
//! executed by the browser (spec 6, "Garancije").
//!
//! Responses are routed back by `id`; events fan out to subscriptions by
//! `sessionId` and method. The set of methods is closed (`Method`), so the
//! agent can never reach `Runtime.evaluate`, `Network.*`, `Storage.*` or any
//! browser-level `Target.setAutoAttach` (decision D7).

use std::collections::{HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::policy::{GateError, Policy, RequestCtx};
use super::protocol::{ActionResult, Outcome, Reason};

/// Largest CDP message we accept (screenshots and AX trees), spec 5.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
/// Default per-command timeout (spec 5).
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// Ping timeout once a command timed out (spec 14.1 row 6).
pub const PING_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the link thread blocks in one socket read before checking the
/// command channel again.
const READ_SLICE: Duration = Duration::from_millis(15);
/// Per-subscription event queue (spec 5: 256, oldest dropped and counted).
pub const EVENT_QUEUE: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CdpSessionId(pub String);

/// What a command addresses, for the lease check in the gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetRef {
    /// Browser-level command without a target (`Browser.getVersion`, `Target.createTarget`, ...).
    Browser,
    /// A command inside a page or frame session, or `Target.detachFromTarget {sessionId}`.
    Session(CdpSessionId),
    /// `Target.attachToTarget` / `Target.closeTarget {targetId}`.
    Target(TargetId),
}

/// Closed set of CDP methods (spec appendix 19.1). Nothing else can be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    BrowserGetVersion,
    TargetGetTargets,
    TargetCreateTarget,
    TargetCloseTarget,
    TargetAttachToTarget,
    TargetDetachFromTarget,
    /// Only inside a page session (OOPIFs and popups of our tab).
    TargetSetAutoAttach,
    PageEnable,
    PageSetLifecycleEventsEnabled,
    PageNavigate,
    PageGetNavigationHistory,
    PageNavigateToHistoryEntry,
    PageCreateIsolatedWorld,
    PageAddScriptToEvaluateOnNewDocument,
    PageCaptureScreenshot,
    PageGetLayoutMetrics,
    PageHandleJavaScriptDialog,
    PageSetInterceptFileChooserDialog,
    PageBringToFront,
    DomEnable,
    DomGetDocument,
    DomDescribeNode,
    DomResolveNode,
    DomScrollIntoViewIfNeeded,
    DomGetContentQuads,
    DomGetBoxModel,
    DomGetNodeForLocation,
    DomFocus,
    DomSetFileInputFiles,
    AccessibilityEnable,
    AccessibilityGetFullAXTree,
    AccessibilityGetPartialAXTree,
    RuntimeCallFunctionOn,
    RuntimeRunIfWaitingForDebugger,
    InputDispatchMouseEvent,
    InputDispatchKeyEvent,
    InputInsertText,
    EmulationSetFocusEmulationEnabled,
}

impl Method {
    pub fn name(self) -> &'static str {
        use Method::*;
        match self {
            BrowserGetVersion => "Browser.getVersion",
            TargetGetTargets => "Target.getTargets",
            TargetCreateTarget => "Target.createTarget",
            TargetCloseTarget => "Target.closeTarget",
            TargetAttachToTarget => "Target.attachToTarget",
            TargetDetachFromTarget => "Target.detachFromTarget",
            TargetSetAutoAttach => "Target.setAutoAttach",
            PageEnable => "Page.enable",
            PageSetLifecycleEventsEnabled => "Page.setLifecycleEventsEnabled",
            PageNavigate => "Page.navigate",
            PageGetNavigationHistory => "Page.getNavigationHistory",
            PageNavigateToHistoryEntry => "Page.navigateToHistoryEntry",
            PageCreateIsolatedWorld => "Page.createIsolatedWorld",
            PageAddScriptToEvaluateOnNewDocument => "Page.addScriptToEvaluateOnNewDocument",
            PageCaptureScreenshot => "Page.captureScreenshot",
            PageGetLayoutMetrics => "Page.getLayoutMetrics",
            PageHandleJavaScriptDialog => "Page.handleJavaScriptDialog",
            PageSetInterceptFileChooserDialog => "Page.setInterceptFileChooserDialog",
            PageBringToFront => "Page.bringToFront",
            DomEnable => "DOM.enable",
            DomGetDocument => "DOM.getDocument",
            DomDescribeNode => "DOM.describeNode",
            DomResolveNode => "DOM.resolveNode",
            DomScrollIntoViewIfNeeded => "DOM.scrollIntoViewIfNeeded",
            DomGetContentQuads => "DOM.getContentQuads",
            DomGetBoxModel => "DOM.getBoxModel",
            DomGetNodeForLocation => "DOM.getNodeForLocation",
            DomFocus => "DOM.focus",
            DomSetFileInputFiles => "DOM.setFileInputFiles",
            AccessibilityEnable => "Accessibility.enable",
            AccessibilityGetFullAXTree => "Accessibility.getFullAXTree",
            AccessibilityGetPartialAXTree => "Accessibility.getPartialAXTree",
            RuntimeCallFunctionOn => "Runtime.callFunctionOn",
            RuntimeRunIfWaitingForDebugger => "Runtime.runIfWaitingForDebugger",
            InputDispatchMouseEvent => "Input.dispatchMouseEvent",
            InputDispatchKeyEvent => "Input.dispatchKeyEvent",
            InputInsertText => "Input.insertText",
            EmulationSetFocusEmulationEnabled => "Emulation.setFocusEmulationEnabled",
        }
    }

    /// The only methods that may be sent without a `sessionId` (spec 5).
    pub fn browser_level_allowed(self) -> bool {
        use Method::*;
        matches!(
            self,
            BrowserGetVersion
                | TargetGetTargets
                | TargetCreateTarget
                | TargetAttachToTarget
                | TargetDetachFromTarget
                | TargetCloseTarget
        )
    }

    /// Per-command timeout (spec 5).
    pub fn timeout(self) -> Duration {
        use Method::*;
        match self {
            PageNavigate => Duration::from_secs(30),
            PageCaptureScreenshot => Duration::from_secs(20),
            AccessibilityGetFullAXTree | AccessibilityGetPartialAXTree => Duration::from_secs(15),
            TargetGetTargets
            | TargetCreateTarget
            | TargetCloseTarget
            | TargetAttachToTarget
            | TargetDetachFromTarget
            | TargetSetAutoAttach => Duration::from_secs(5),
            _ => DEFAULT_COMMAND_TIMEOUT,
        }
    }
}

/// One CDP command. Build with [`Command::browser`] or [`Command::in_session`].
#[derive(Clone, Debug)]
pub struct Command {
    pub method: Method,
    pub params: Value,
    pub session: Option<CdpSessionId>,
}

impl Command {
    pub fn browser(method: Method, params: Value) -> Command {
        Command {
            method,
            params,
            session: None,
        }
    }

    pub fn in_session(session: &CdpSessionId, method: Method, params: Value) -> Command {
        Command {
            method,
            params,
            session: Some(session.clone()),
        }
    }

    /// What the gate must find a lease for.
    pub fn target_ref(&self) -> TargetRef {
        if let Some(session) = &self.session {
            return TargetRef::Session(session.clone());
        }
        match self.method {
            Method::TargetAttachToTarget | Method::TargetCloseTarget => self
                .params
                .get("targetId")
                .and_then(Value::as_str)
                .map(|id| TargetRef::Target(TargetId(id.to_string())))
                .unwrap_or(TargetRef::Browser),
            Method::TargetDetachFromTarget => self
                .params
                .get("sessionId")
                .and_then(Value::as_str)
                .map(|id| TargetRef::Session(CdpSessionId(id.to_string())))
                .unwrap_or(TargetRef::Browser),
            _ => TargetRef::Browser,
        }
    }
}

/// Why a command did not get a result. `sent` tells whether the frame was
/// written before the failure, which decides `blocked` vs `unknown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CdpError {
    /// Toggle is OFF.
    Off,
    /// Epoch mismatch (OFF→ON, turn end, session revoke, connection loss).
    Revoked {
        sent: bool,
    },
    TurnInactive,
    NoLease,
    /// Method not allowed at this level (a bug in the caller, never the agent's input).
    NotAllowed,
    /// No open link.
    NotConnected,
    /// No answer within the command timeout; the command may have run.
    Timeout,
    /// Socket closed while the command was pending (`sent`) or before it was written.
    LinkDown {
        sent: bool,
    },
    /// The browser answered with an error.
    Protocol {
        code: i64,
        message: String,
    },
}

impl CdpError {
    pub fn reason(&self) -> Reason {
        match self {
            CdpError::Off => Reason::Off,
            CdpError::Revoked { .. } => Reason::Revoked,
            CdpError::TurnInactive => Reason::TurnInactive,
            CdpError::NoLease => Reason::NoLease,
            CdpError::NotAllowed => Reason::InvalidRequest,
            CdpError::NotConnected => Reason::BrowserDisconnected,
            CdpError::Timeout => Reason::CdpTimeout,
            CdpError::LinkDown { .. } => Reason::LinkDown,
            CdpError::Protocol { .. } => Reason::InvalidRequest,
        }
    }

    /// Whether a frame reached the socket, so the action may have had effect.
    pub fn was_sent(&self) -> bool {
        matches!(
            self,
            CdpError::Revoked { sent: true }
                | CdpError::LinkDown { sent: true }
                | CdpError::Timeout
        )
    }

    /// The action result for this error. `wrote_before` says whether the
    /// action had already written an earlier command: an interrupted
    /// multi-step mutation is `unknown`, never `not sent` (spec 6).
    pub fn into_result(self, request_id: &str, mutating: bool, wrote_before: bool) -> ActionResult {
        let reason = self.reason();
        let mut result = ActionResult::from_reason(request_id, reason);
        if let CdpError::Protocol { code, message } = &self {
            result =
                result.with_message(format!("Browser rejected the command ({code}): {message}"));
        }
        if mutating && (self.was_sent() || wrote_before) {
            result = result.with_outcome(Outcome::Unknown);
            if matches!(self, CdpError::Revoked { .. }) {
                result = result.with_message(
                    "Browser access was revoked while the action was in progress; part of it may have happened. Take a snapshot before retrying.",
                );
            }
        }
        result
    }
}

/// An inbound CDP event.
#[derive(Clone, Debug)]
pub struct CdpEvent {
    pub method: String,
    pub params: Value,
    pub session: Option<CdpSessionId>,
}

/// Which events a subscription receives. Empty `methods` means all methods;
/// `session: None` means events of every session, browser-level included.
#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    pub session: Option<CdpSessionId>,
    pub methods: Vec<&'static str>,
}

impl EventFilter {
    pub fn all() -> EventFilter {
        EventFilter::default()
    }

    pub fn session(session: &CdpSessionId) -> EventFilter {
        EventFilter {
            session: Some(session.clone()),
            methods: Vec::new(),
        }
    }

    fn matches(&self, event: &CdpEvent) -> bool {
        if let Some(session) = &self.session {
            if event.session.as_ref() != Some(session) {
                return false;
            }
        }
        self.methods.is_empty() || self.methods.iter().any(|m| *m == event.method)
    }
}

struct SubInner {
    filter: EventFilter,
    queue: Mutex<VecDeque<CdpEvent>>,
    ready: Condvar,
    closed: AtomicBool,
    dropped: AtomicU64,
}

/// A bounded event queue. Dropping it unsubscribes.
pub struct Subscription {
    inner: Arc<SubInner>,
}

impl Subscription {
    pub fn recv_timeout(&self, timeout: Duration) -> Option<CdpEvent> {
        let deadline = Instant::now() + timeout;
        let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(event) = queue.pop_front() {
                return Some(event);
            }
            if self.inner.closed.load(Ordering::Acquire) {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = self
                .inner
                .ready
                .wait_timeout(queue, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            queue = guard;
        }
    }

    pub fn try_recv(&self) -> Option<CdpEvent> {
        self.inner
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }

    /// True once the link closed; no more events will arrive.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    Closed,
    Connecting,
    Open,
    /// OFF in progress: system detaches only, agent commands refused.
    Closing,
}

/// Who ended the connection; the host maps this to a connection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosedBy {
    /// `Link::close` or `Link::abort` (toggle OFF, disconnect from the UI).
    Us,
    /// The browser closed the socket (quit, user revoked in chrome://inspect).
    Peer,
    /// Protocol or I/O failure, or the post-timeout ping failed.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    AlreadyOpen,
    /// TCP connect failed: nothing listens on the port.
    Unreachable(String),
    /// The upgrade did not complete within the consent timeout.
    ConsentTimeout,
    /// The browser refused the upgrade (user denied, or non-101 status).
    Refused(String),
    /// Aborted by `Link::abort` while waiting.
    Aborted,
}

enum SendCtx {
    Agent(RequestCtx),
    System,
}

struct SendRequest {
    ctx: SendCtx,
    command: Command,
    timeout: Duration,
    reply: mpsc::Sender<Result<Value, CdpError>>,
}

enum LinkMsg {
    Send(Box<SendRequest>),
    Close {
        detach: Vec<CdpSessionId>,
        budget: Duration,
        done: mpsc::Sender<()>,
    },
}

struct Pending {
    reply: mpsc::Sender<Result<Value, CdpError>>,
    deadline: Instant,
    method: Method,
    agent: bool,
}

type ClosedHook = Box<dyn Fn(ClosedBy) + Send + Sync>;

struct LinkInner {
    policy: Arc<Policy>,
    status: Mutex<LinkStatus>,
    tx: Mutex<Option<mpsc::Sender<LinkMsg>>>,
    stream: Mutex<Option<TcpStream>>,
    subs: Mutex<Vec<Arc<SubInner>>>,
    on_closed: Mutex<Option<ClosedHook>>,
    commands_sent: AtomicU64,
    /// Commands the link refused at the gate; diagnostics.
    commands_refused: AtomicU64,
    aborted: AtomicBool,
    #[cfg(test)]
    write_log: Mutex<Vec<(u64, Instant)>>,
}

/// Handle to the link; cheap to clone.
#[derive(Clone)]
pub struct Link {
    inner: Arc<LinkInner>,
}

impl Link {
    pub fn new(policy: Arc<Policy>) -> Link {
        Link {
            inner: Arc::new(LinkInner {
                policy,
                status: Mutex::new(LinkStatus::Closed),
                tx: Mutex::new(None),
                stream: Mutex::new(None),
                subs: Mutex::new(Vec::new()),
                on_closed: Mutex::new(None),
                commands_sent: AtomicU64::new(0),
                commands_refused: AtomicU64::new(0),
                aborted: AtomicBool::new(false),
                #[cfg(test)]
                write_log: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn policy(&self) -> &Arc<Policy> {
        &self.inner.policy
    }

    pub fn status(&self) -> LinkStatus {
        *self.inner.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status(), LinkStatus::Open)
    }

    pub fn commands_sent(&self) -> u64 {
        self.inner.commands_sent.load(Ordering::Relaxed)
    }

    /// Called from the link thread once the socket is closed, whoever closed
    /// it (`ClosedBy` says). For [`Link::close`] it runs before `close`
    /// returns.
    pub fn set_on_closed(&self, hook: impl Fn(ClosedBy) + Send + Sync + 'static) {
        *self
            .inner
            .on_closed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Box::new(hook));
    }

    /// Blocking: TCP connect, then the WebSocket upgrade, which Chrome 144+
    /// holds until the user answers the consent dialog (spec 4). On success
    /// the link thread is running and the status is `Open`.
    pub fn open(&self, ws_url: &str, consent_timeout: Duration) -> Result<(), OpenError> {
        {
            let mut status = self.inner.status.lock().unwrap_or_else(|e| e.into_inner());
            if *status != LinkStatus::Closed {
                return Err(OpenError::AlreadyOpen);
            }
            *status = LinkStatus::Connecting;
        }
        self.inner.aborted.store(false, Ordering::Release);
        let result = self.handshake(ws_url, consent_timeout);
        match result {
            Ok((socket, rx)) => {
                let inner = self.inner.clone();
                *self.inner.status.lock().unwrap_or_else(|e| e.into_inner()) = LinkStatus::Open;
                thread::Builder::new()
                    .name("browser-link".into())
                    .spawn(move || run(socket, rx, inner))
                    .map_err(|e| OpenError::Unreachable(e.to_string()))?;
                Ok(())
            }
            Err(error) => {
                *self.inner.status.lock().unwrap_or_else(|e| e.into_inner()) = LinkStatus::Closed;
                *self.inner.stream.lock().unwrap_or_else(|e| e.into_inner()) = None;
                *self.inner.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                if self.inner.aborted.load(Ordering::Acquire) {
                    return Err(OpenError::Aborted);
                }
                Err(error)
            }
        }
    }

    fn handshake(
        &self,
        ws_url: &str,
        consent_timeout: Duration,
    ) -> Result<(WebSocket<TcpStream>, mpsc::Receiver<LinkMsg>), OpenError> {
        let request = ws_url
            .into_client_request()
            .map_err(|e| OpenError::Refused(format!("invalid endpoint: {e}")))?;
        let authority = request
            .uri()
            .authority()
            .ok_or_else(|| OpenError::Refused("endpoint has no host".into()))?
            .to_string();
        let addr: SocketAddr = authority
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .ok_or_else(|| OpenError::Unreachable(format!("cannot resolve {authority}")))?;
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .map_err(|e| OpenError::Unreachable(e.to_string()))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(consent_timeout));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        *self.inner.stream.lock().unwrap_or_else(|e| e.into_inner()) = stream.try_clone().ok();
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let (socket, _response) =
            tungstenite::client::client_with_config(request, stream, Some(config)).map_err(
                |error| match error {
                    tungstenite::HandshakeError::Failure(tungstenite::Error::Io(e))
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                    {
                        OpenError::ConsentTimeout
                    }
                    tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                        OpenError::Refused(format!("HTTP {}", response.status()))
                    }
                    // The read timeout fired mid-handshake: Chrome is still
                    // showing its consent dialog (or the user walked away).
                    tungstenite::HandshakeError::Interrupted(_) => OpenError::ConsentTimeout,
                    other => OpenError::Refused(other.to_string()),
                },
            )?;
        let _ = socket.get_ref().set_read_timeout(Some(READ_SLICE));
        let (tx, rx) = mpsc::channel();
        *self.inner.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        Ok((socket, rx))
    }

    /// Send on behalf of an agent request. Blocks until the browser answers,
    /// the command times out, or the gate refuses it.
    pub fn send(&self, ctx: &RequestCtx, command: Command) -> Result<Value, CdpError> {
        let timeout = command
            .method
            .timeout()
            .min(ctx.remaining().max(Duration::from_millis(50)));
        self.dispatch(SendCtx::Agent(ctx.clone()), command, timeout)
    }

    /// Send a module-internal lifecycle command (ping, detach on OFF, close on
    /// session delete). Still limited to the closed method set and to the
    /// browser-level allowlist; never used for agent input.
    pub fn send_system(&self, command: Command) -> Result<Value, CdpError> {
        let timeout = command.method.timeout();
        self.dispatch(SendCtx::System, command, timeout)
    }

    /// [`Self::send_system`] that gives up after `budget` when that is
    /// shorter than the method's own timeout (cleanup on OFF).
    pub fn send_system_within(
        &self,
        command: Command,
        budget: Duration,
    ) -> Result<Value, CdpError> {
        let timeout = command.method.timeout().min(budget);
        self.dispatch(SendCtx::System, command, timeout)
    }

    fn dispatch(
        &self,
        ctx: SendCtx,
        command: Command,
        timeout: Duration,
    ) -> Result<Value, CdpError> {
        let tx = self
            .inner
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(CdpError::NotConnected)?;
        let (reply, rx) = mpsc::channel();
        tx.send(LinkMsg::Send(Box::new(SendRequest {
            ctx,
            command,
            timeout,
            reply,
        })))
        .map_err(|_| CdpError::NotConnected)?;
        match rx.recv_timeout(timeout + Duration::from_secs(2)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(CdpError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(CdpError::LinkDown { sent: false }),
        }
    }

    pub fn subscribe(&self, filter: EventFilter) -> Subscription {
        let inner = Arc::new(SubInner {
            filter,
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            closed: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
        });
        self.inner
            .subs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(inner.clone());
        Subscription { inner }
    }

    /// Graceful close used by OFF and by the UI "Prekini": detach the given
    /// sessions (system commands, total `budget`), send a Close frame, shut
    /// the socket. Blocks until the link thread has finished or `budget`
    /// plus half a second has passed.
    pub fn close(&self, detach: Vec<CdpSessionId>, budget: Duration) {
        let tx = self
            .inner
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(tx) = tx else {
            self.abort();
            return;
        };
        *self.inner.status.lock().unwrap_or_else(|e| e.into_inner()) = LinkStatus::Closing;
        let (done, wait) = mpsc::channel();
        if tx
            .send(LinkMsg::Close {
                detach,
                budget,
                done,
            })
            .is_err()
        {
            self.abort();
            return;
        }
        if wait
            .recv_timeout(budget + Duration::from_millis(500))
            .is_err()
        {
            self.abort();
        }
    }

    /// Immediate shutdown of the TCP stream (also wakes a handshake waiting
    /// for consent). The link thread notices and exits.
    pub fn abort(&self) {
        self.inner.aborted.store(true, Ordering::Release);
        if let Some(stream) = self
            .inner
            .stream
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    #[cfg(test)]
    pub fn write_log(&self) -> Vec<(u64, Instant)> {
        self.inner
            .write_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub fn commands_refused(&self) -> u64 {
        self.inner.commands_refused.load(Ordering::Relaxed)
    }
}

fn map_gate(error: GateError) -> CdpError {
    match error {
        GateError::Off => CdpError::Off,
        GateError::Revoked => CdpError::Revoked { sent: false },
        GateError::TurnInactive => CdpError::TurnInactive,
        GateError::NoLease => CdpError::NoLease,
        GateError::NotAllowed => CdpError::NotAllowed,
    }
}

struct Closing {
    deadline: Instant,
    done: mpsc::Sender<()>,
}

struct Loop {
    socket: WebSocket<TcpStream>,
    inner: Arc<LinkInner>,
    pending: HashMap<u64, Pending>,
    next_id: u64,
    closing: Option<Closing>,
    /// Pending id of the post-timeout ping, if one is in flight.
    ping: Option<u64>,
    closed_by: Option<ClosedBy>,
}

fn run(socket: WebSocket<TcpStream>, rx: mpsc::Receiver<LinkMsg>, inner: Arc<LinkInner>) {
    let mut state = Loop {
        socket,
        inner,
        pending: HashMap::new(),
        next_id: 1,
        closing: None,
        ping: None,
        closed_by: None,
    };
    let mut channel_open = true;
    while state.closed_by.is_none() {
        if channel_open {
            loop {
                match rx.try_recv() {
                    Ok(LinkMsg::Send(request)) => state.handle_send(*request),
                    Ok(LinkMsg::Close {
                        detach,
                        budget,
                        done,
                    }) => state.begin_close(detach, budget, done),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        channel_open = false;
                        if state.closing.is_none() {
                            let (done, _) = mpsc::channel();
                            state.begin_close(Vec::new(), Duration::from_millis(200), done);
                        }
                        break;
                    }
                }
            }
        }
        state.read_once();
        state.expire();
        state.finish_close_if_due();
    }
    state.shutdown();
}

impl Loop {
    fn handle_send(&mut self, request: SendRequest) {
        let SendRequest {
            ctx,
            command,
            timeout,
            reply,
        } = request;
        let target = command.target_ref();
        let browser_ok = command.method.browser_level_allowed();
        let agent = matches!(ctx, SendCtx::Agent(_));
        if agent && self.closing.is_some() {
            self.inner.commands_refused.fetch_add(1, Ordering::Relaxed);
            let _ = reply.send(Err(CdpError::Revoked { sent: false }));
            return;
        }
        // The read lock is held until the frame is written: OFF takes the
        // write lock, bumps the epoch and only then closes the socket, so no
        // send after that point passes (spec 5, `send()` gate).
        let policy = self.inner.policy.read();
        let verdict = match &ctx {
            SendCtx::Agent(ctx) => policy.gate(ctx, &target, browser_ok).map_err(map_gate),
            SendCtx::System => {
                if matches!(target, TargetRef::Browser) && !browser_ok {
                    Err(CdpError::NotAllowed)
                } else {
                    Ok(())
                }
            }
        };
        if let Err(error) = verdict {
            drop(policy);
            self.inner.commands_refused.fetch_add(1, Ordering::Relaxed);
            let _ = reply.send(Err(error));
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut frame = json!({
            "id": id,
            "method": command.method.name(),
            "params": command.params,
        });
        if let Some(session) = &command.session {
            frame["sessionId"] = Value::String(session.0.clone());
        }
        let written = self.socket.send(Message::text(frame.to_string()));
        #[cfg(test)]
        self.inner
            .write_log
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, Instant::now()));
        drop(policy);
        match written {
            Ok(()) => {
                self.inner.commands_sent.fetch_add(1, Ordering::Relaxed);
                self.pending.insert(
                    id,
                    Pending {
                        reply,
                        deadline: Instant::now() + timeout,
                        method: command.method,
                        agent,
                    },
                );
            }
            Err(error) => {
                let _ = reply.send(Err(CdpError::LinkDown { sent: false }));
                log::debug!("browser-link write failed: {error}");
                self.closed_by = Some(ClosedBy::Error);
            }
        }
    }

    fn begin_close(&mut self, detach: Vec<CdpSessionId>, budget: Duration, done: mpsc::Sender<()>) {
        if self.closing.is_some() {
            return;
        }
        *self.inner.status.lock().unwrap_or_else(|e| e.into_inner()) = LinkStatus::Closing;
        // Agent requests still waiting for an answer were written: `unknown`.
        for (_, pending) in self.pending.drain() {
            let error = if pending.agent {
                CdpError::Revoked { sent: true }
            } else {
                CdpError::LinkDown { sent: true }
            };
            let _ = pending.reply.send(Err(error));
        }
        self.ping = None;
        for session in detach {
            let (reply, _rx) = mpsc::channel();
            self.handle_send(SendRequest {
                ctx: SendCtx::System,
                command: Command::browser(
                    Method::TargetDetachFromTarget,
                    json!({ "sessionId": session.0 }),
                ),
                timeout: budget,
                reply,
            });
        }
        self.closing = Some(Closing {
            deadline: Instant::now() + budget,
            done,
        });
    }

    fn finish_close_if_due(&mut self) {
        let Some(closing) = &self.closing else {
            return;
        };
        let detaches_done = self.pending.is_empty();
        if !detaches_done && Instant::now() < closing.deadline {
            return;
        }
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
        self.closed_by = Some(ClosedBy::Us);
    }

    fn read_once(&mut self) {
        match self.socket.read() {
            Ok(Message::Text(text)) => self.dispatch(text.as_str()),
            Ok(Message::Close(_)) => {
                if self.closed_by.is_none() {
                    self.closed_by = Some(if self.closing.is_some() {
                        ClosedBy::Us
                    } else {
                        ClosedBy::Peer
                    });
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::AlreadyClosed) => {
                if self.closed_by.is_none() {
                    self.closed_by = Some(if self.closing.is_some() {
                        ClosedBy::Us
                    } else {
                        ClosedBy::Peer
                    });
                }
            }
            Err(error) => {
                if self.closed_by.is_none() {
                    log::debug!("browser-link read failed: {error}");
                    self.closed_by = Some(if self.inner.aborted.load(Ordering::Acquire) {
                        ClosedBy::Us
                    } else {
                        ClosedBy::Error
                    });
                }
            }
        }
    }

    fn dispatch(&mut self, text: &str) {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            log::debug!("browser-link: unparseable frame ({} bytes)", text.len());
            return;
        };
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if self.ping == Some(id) {
                self.ping = None;
            }
            if let Some(pending) = self.pending.remove(&id) {
                let result = match value.get("error") {
                    Some(error) => Err(CdpError::Protocol {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    }),
                    None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = pending.reply.send(result);
            }
            return;
        }
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return;
        };
        let event = CdpEvent {
            method: method.to_string(),
            params: value.get("params").cloned().unwrap_or(Value::Null),
            session: value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(|s| CdpSessionId(s.to_string())),
        };
        let mut subs = self.inner.subs.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|sub| !sub.closed.load(Ordering::Acquire));
        for sub in subs.iter() {
            if !sub.filter.matches(&event) {
                continue;
            }
            let mut queue = sub.queue.lock().unwrap_or_else(|e| e.into_inner());
            if queue.len() >= EVENT_QUEUE {
                queue.pop_front();
                sub.dropped.fetch_add(1, Ordering::Relaxed);
            }
            queue.push_back(event.clone());
            drop(queue);
            sub.ready.notify_all();
        }
    }

    /// Command timeouts (spec 5) and the post-timeout ping (spec 14.1 row 6).
    fn expire(&mut self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, p)| now >= p.deadline)
            .map(|(id, _)| *id)
            .collect();
        if expired.is_empty() {
            return;
        }
        let mut ping_failed = false;
        for id in expired {
            if let Some(pending) = self.pending.remove(&id) {
                if self.ping == Some(id) {
                    ping_failed = true;
                    self.ping = None;
                }
                log::debug!("browser-link: {} timed out", pending.method.name());
                let _ = pending.reply.send(Err(CdpError::Timeout));
            }
        }
        if ping_failed {
            self.closed_by = Some(ClosedBy::Error);
            return;
        }
        if self.ping.is_none() && self.closing.is_none() {
            let (reply, _rx) = mpsc::channel();
            let id = self.next_id;
            self.handle_send(SendRequest {
                ctx: SendCtx::System,
                command: Command::browser(Method::BrowserGetVersion, json!({})),
                timeout: PING_TIMEOUT,
                reply,
            });
            if self.pending.contains_key(&id) {
                self.ping = Some(id);
            }
        }
    }

    fn shutdown(mut self) {
        let closed_by = self.closed_by.unwrap_or(ClosedBy::Error);
        let _ = self.socket.get_ref().shutdown(std::net::Shutdown::Both);
        for (_, pending) in self.pending.drain() {
            let error = match closed_by {
                ClosedBy::Us if pending.agent => CdpError::Revoked { sent: true },
                _ => CdpError::LinkDown { sent: true },
            };
            let _ = pending.reply.send(Err(error));
        }
        *self.inner.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.inner.stream.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.inner.status.lock().unwrap_or_else(|e| e.into_inner()) = LinkStatus::Closed;
        {
            let mut subs = self.inner.subs.lock().unwrap_or_else(|e| e.into_inner());
            for sub in subs.drain(..) {
                sub.closed.store(true, Ordering::Release);
                sub.ready.notify_all();
            }
        }
        // The hook (the host's bookkeeping: epoch, connection state) runs
        // before `Link::close` returns, so whatever the closer records next
        // is not overwritten by it a moment later.
        {
            let hook = self
                .inner
                .on_closed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(hook) = hook.as_ref() {
                hook(closed_by);
            }
        }
        if let Some(closing) = self.closing.take() {
            let _ = closing.done.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::PolicyState;
    use super::super::protocol::{ActionKind, BrowserSettings, TabState};
    use super::super::testing::{FakeCdp, Response};
    use super::*;

    fn policy_with_lease() -> (Arc<Policy>, RequestCtx) {
        let mut state = PolicyState::new(1, BrowserSettings::default());
        state.set_toggle(true);
        state.turn_started("s1", "main", "/repo", Some(true));
        let tab = state.new_tab_id();
        state.insert_lease(super::super::policy::Lease {
            tab: tab.clone(),
            target_id: TargetId("T1".into()),
            cdp_session: Some(CdpSessionId("S1".into())),
            owner: "s1".into(),
            state: TabState::Open,
            main_frame_id: Some("T1".into()),
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
        });
        let ctx = state
            .admit(
                "s1",
                "r1",
                ActionKind::Click,
                Some(&tab),
                Duration::from_secs(10),
            )
            .unwrap();
        (Arc::new(Policy::new(state)), ctx)
    }

    fn click(session: &str) -> Command {
        Command::in_session(
            &CdpSessionId(session.into()),
            Method::InputDispatchMouseEvent,
            json!({"type": "mousePressed", "x": 1, "y": 1}),
        )
    }

    #[test]
    fn routes_responses_by_id_and_events_by_session() {
        let server = FakeCdp::start();
        let (policy, ctx) = policy_with_lease();
        let link = Link::new(policy);
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        assert_eq!(link.status(), LinkStatus::Open);
        let all = link.subscribe(EventFilter::all());
        let only_s1 = link.subscribe(EventFilter::session(&CdpSessionId("S1".into())));

        let version = link
            .send_system(Command::browser(Method::BrowserGetVersion, json!({})))
            .unwrap();
        assert!(version["product"].as_str().unwrap().starts_with("Chrome/"));
        let clicked = link.send(&ctx, click("S1")).unwrap();
        assert_eq!(clicked, json!({}));
        let received = server.received();
        assert_eq!(received.len(), 2);
        assert_eq!(received[1].method, "Input.dispatchMouseEvent");
        assert_eq!(received[1].session.as_deref(), Some("S1"));
        assert!(received[1].id > received[0].id);

        server.emit("Page.loadEventFired", json!({"timestamp": 1.0}), Some("S9"));
        server.emit("Page.loadEventFired", json!({"timestamp": 2.0}), Some("S1"));
        let first = all.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(first.session, Some(CdpSessionId("S9".into())));
        let second = all.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(second.session, Some(CdpSessionId("S1".into())));
        let filtered = only_s1.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(filtered.params["timestamp"], 2.0);
        assert!(only_s1.try_recv().is_none());

        // Protocol errors come back as CdpError::Protocol.
        let err = link
            .send(
                &ctx,
                Command::in_session(
                    &CdpSessionId("S1".into()),
                    Method::DomResolveNode,
                    json!({"backendNodeId": -1}),
                ),
            )
            .unwrap_err();
        assert!(matches!(err, CdpError::Protocol { code: -32000, .. }));
        link.close(Vec::new(), Duration::from_millis(200));
        assert_eq!(link.status(), LinkStatus::Closed);
    }

    #[test]
    fn gate_refusals_never_reach_the_socket() {
        let server = FakeCdp::start();
        let (policy, ctx) = policy_with_lease();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        // Session we do not hold.
        assert_eq!(link.send(&ctx, click("S2")), Err(CdpError::NoLease));
        // Browser-level command outside the allowlist.
        assert_eq!(
            link.send(
                &ctx,
                Command::browser(Method::TargetSetAutoAttach, json!({}))
            ),
            Err(CdpError::NotAllowed)
        );
        assert_eq!(
            link.send_system(Command::browser(Method::TargetSetAutoAttach, json!({}))),
            Err(CdpError::NotAllowed)
        );
        // Target we did not create.
        assert_eq!(
            link.send(
                &ctx,
                Command::browser(Method::TargetCloseTarget, json!({"targetId": "T9"}))
            ),
            Err(CdpError::NoLease)
        );
        // Turn ended: session epoch moved on.
        policy.write().turn_finished("s1");
        assert_eq!(
            link.send(&ctx, click("S1")),
            Err(CdpError::Revoked { sent: false })
        );
        policy.write().turn_started("s1", "main", "/repo", None);
        assert_eq!(
            link.send(&ctx, click("S1")),
            Err(CdpError::Revoked { sent: false })
        );
        // OFF.
        policy.write().set_toggle(false);
        assert_eq!(link.send(&ctx, click("S1")), Err(CdpError::Off));
        assert!(server.received().is_empty());
        assert_eq!(link.commands_refused(), 7);
        link.close(Vec::new(), Duration::from_millis(200));
    }

    #[test]
    fn command_timeout_pings_and_a_dead_browser_closes_the_link() {
        let server = FakeCdp::start_with(|received| {
            if received.method == "Input.dispatchMouseEvent"
                || received.method == "Browser.getVersion"
            {
                Response::Hold
            } else {
                Response::Result(json!({}))
            }
        });
        let (policy, mut ctx) = policy_with_lease();
        ctx.deadline = Instant::now() + Duration::from_millis(300);
        let link = Link::new(policy);
        let closed = Arc::new(Mutex::new(None));
        let seen = closed.clone();
        link.set_on_closed(move |by| *seen.lock().unwrap() = Some(by));
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let started = Instant::now();
        assert_eq!(link.send(&ctx, click("S1")), Err(CdpError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
        // A ping went out and was never answered: the link closes as Error.
        assert!(server.wait_for(
            |log| log.iter().any(|r| r.method == "Browser.getVersion"),
            Duration::from_secs(2)
        ));
        let deadline = Instant::now() + PING_TIMEOUT + Duration::from_secs(3);
        while Instant::now() < deadline && link.status() != LinkStatus::Closed {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(link.status(), LinkStatus::Closed);
        assert_eq!(*closed.lock().unwrap(), Some(ClosedBy::Error));
        assert_eq!(link.send(&ctx, click("S1")), Err(CdpError::NotConnected));
    }

    #[test]
    fn peer_close_fails_pending_commands_as_link_down() {
        let server = FakeCdp::start_with(|_| Response::Hold);
        let (policy, ctx) = policy_with_lease();
        let link = Link::new(policy);
        let closed = Arc::new(Mutex::new(None));
        let seen = closed.clone();
        link.set_on_closed(move |by| *seen.lock().unwrap() = Some(by));
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let worker = {
            let link = link.clone();
            let ctx = ctx.clone();
            thread::spawn(move || link.send(&ctx, click("S1")))
        };
        assert!(server.wait_for(|log| !log.is_empty(), Duration::from_secs(2)));
        server.close_current();
        assert_eq!(
            worker.join().unwrap(),
            Err(CdpError::LinkDown { sent: true })
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && closed.lock().unwrap().is_none() {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*closed.lock().unwrap(), Some(ClosedBy::Peer));
    }

    #[test]
    fn off_detaches_our_sessions_then_closes_without_closing_tabs() {
        let server = FakeCdp::start();
        let (policy, ctx) = policy_with_lease();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        link.send(&ctx, click("S1")).unwrap();
        let detach = policy.write().set_toggle(false);
        assert_eq!(detach, vec![CdpSessionId("S1".into())]);
        let closed = Arc::new(Mutex::new(None));
        let seen = closed.clone();
        link.set_on_closed(move |by| *seen.lock().unwrap() = Some(by));
        link.close(detach, Duration::from_millis(500));
        assert_eq!(link.status(), LinkStatus::Closed);
        // The hook ran before `close` returned: the host's state written
        // after a close is final.
        assert_eq!(*closed.lock().unwrap(), Some(ClosedBy::Us));
        let methods: Vec<String> = server.received().into_iter().map(|r| r.method).collect();
        assert_eq!(
            methods,
            vec!["Input.dispatchMouseEvent", "Target.detachFromTarget"]
        );
        assert!(server.wait_for_close(Duration::from_secs(2)));
        assert_eq!(link.send(&ctx, click("S1")), Err(CdpError::NotConnected));
    }

    #[test]
    fn open_reports_unreachable_and_refused_endpoints() {
        let (policy, _) = policy_with_lease();
        let link = Link::new(policy);
        let err = link
            .open(
                "ws://127.0.0.1:1/devtools/browser/x",
                Duration::from_secs(1),
            )
            .unwrap_err();
        assert!(matches!(err, OpenError::Unreachable(_)));
        assert_eq!(link.status(), LinkStatus::Closed);
        let server = FakeCdp::start_refusing();
        let err = link.open(&server.url, Duration::from_secs(2)).unwrap_err();
        assert!(matches!(err, OpenError::Refused(_)), "{err:?}");
        let server = FakeCdp::start_silent();
        let started = Instant::now();
        let err = link
            .open(&server.url, Duration::from_millis(300))
            .unwrap_err();
        assert_eq!(err, OpenError::ConsentTimeout);
        assert!(started.elapsed() < Duration::from_secs(3));
        // abort() wakes a pending handshake.
        let server = FakeCdp::start_silent();
        let aborter = link.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            aborter.abort();
        });
        let err = link.open(&server.url, Duration::from_secs(10)).unwrap_err();
        assert_eq!(err, OpenError::Aborted);
    }

    #[test]
    fn errors_classify_interrupted_mutations_as_unknown() {
        let r = CdpError::Revoked { sent: false }.into_result("r", true, false);
        assert_eq!(r.outcome, Outcome::Blocked);
        let r = CdpError::Revoked { sent: true }.into_result("r", true, false);
        assert_eq!(r.outcome, Outcome::Unknown);
        assert_eq!(r.reason, Some(Reason::Revoked));
        let r = CdpError::Revoked { sent: false }.into_result("r", true, true);
        assert_eq!(r.outcome, Outcome::Unknown);
        let r = CdpError::Revoked { sent: true }.into_result("r", false, true);
        assert_eq!(r.outcome, Outcome::Blocked);
        let r = CdpError::Timeout.into_result("r", true, false);
        assert_eq!(r.outcome, Outcome::Unknown);
        assert_eq!(r.reason, Some(Reason::CdpTimeout));
        let r = CdpError::Protocol {
            code: -32000,
            message: "No node".into(),
        }
        .into_result("r", true, false);
        assert_eq!(r.outcome, Outcome::Blocked);
        assert!(r.message.contains("No node"));
        assert_eq!(
            Command::browser(Method::TargetCloseTarget, json!({"targetId": "T1"})).target_ref(),
            TargetRef::Target(TargetId("T1".into()))
        );
        assert_eq!(
            Command::browser(Method::TargetDetachFromTarget, json!({"sessionId": "S1"}))
                .target_ref(),
            TargetRef::Session(CdpSessionId("S1".into()))
        );
        assert!(!Method::TargetSetAutoAttach.browser_level_allowed());
        assert_eq!(Method::PageNavigate.timeout(), Duration::from_secs(30));
    }

    /// Spec 16.3: thread A sends in a loop, thread B revokes after a random
    /// delay. No frame may be written after the revocation took effect, and
    /// every request gets exactly one outcome.
    #[test]
    fn revocation_race_never_writes_after_the_epoch_moves() {
        let server = FakeCdp::start();
        let (policy, _) = policy_with_lease();
        let link = Link::new(policy.clone());
        link.open(&server.url, Duration::from_secs(5)).unwrap();
        let tab = policy.read().tabs_for("s1")[0].tab.clone();
        let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ Instant::now().elapsed().as_nanos() as u64;
        let mut total_sent = 0usize;
        for iteration in 0..200 {
            // Re-admit under the current epochs.
            {
                let mut state = policy.write();
                if !state.turn_active("s1") {
                    state.turn_started("s1", "main", "/repo", Some(true));
                }
                if !state.granted("s1") {
                    state.set_grant("s1", "main", true);
                }
                if let Some(lease) = state.lease_mut(&tab) {
                    lease.state = TabState::Open;
                }
            }
            let ctx = policy
                .read()
                .admit(
                    "s1",
                    &format!("r{iteration}"),
                    ActionKind::Click,
                    Some(&tab),
                    Duration::from_secs(5),
                )
                .unwrap();
            let sender = {
                let link = link.clone();
                let ctx = ctx.clone();
                thread::spawn(move || {
                    let mut ok = 0usize;
                    let mut refused = 0usize;
                    let mut outcomes = 0usize;
                    loop {
                        let result = link.send(&ctx, click("S1"));
                        outcomes += 1;
                        match result {
                            Ok(_) => ok += 1,
                            Err(CdpError::Revoked { sent: false })
                            | Err(CdpError::TurnInactive) => {
                                refused += 1;
                                break;
                            }
                            Err(other) => panic!("unexpected error {other:?}"),
                        }
                        if ok > 20_000 {
                            break;
                        }
                    }
                    (ok, refused, outcomes)
                })
            };
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            thread::sleep(Duration::from_micros(seed % 20_000));
            let before = Instant::now();
            let revoked_at = {
                let mut state = policy.write();
                if iteration % 2 == 0 {
                    state.turn_finished("s1");
                } else {
                    state.revoke_session("s1");
                }
                state.last_revocation.unwrap()
            };
            let (ok, refused, outcomes) = sender.join().unwrap();
            assert_eq!(
                refused, 1,
                "iteration {iteration}: sender must stop on the first refusal"
            );
            assert_eq!(
                outcomes,
                ok + refused,
                "every request gets exactly one outcome"
            );
            // No frame was written after the revocation took the write lock.
            let late: Vec<_> = link
                .write_log()
                .into_iter()
                .filter(|(_, at)| *at > revoked_at)
                .collect();
            assert!(
                late.is_empty(),
                "iteration {iteration}: {} frames written after revocation at {:?}",
                late.len(),
                before
            );
            total_sent += ok;
            assert_eq!(link.commands_sent() as usize, total_sent);
            // The fake server saw exactly the frames the link wrote.
            assert!(server.wait_for(|log| log.len() == total_sent, Duration::from_secs(2)));
            if iteration % 2 == 1 {
                // A revoked lease must be re-registered for the next round.
                let mut state = policy.write();
                if let Some(lease) = state.lease_mut(&tab) {
                    lease.state = TabState::Open;
                }
            }
        }
        link.close(Vec::new(), Duration::from_millis(200));
    }
}
