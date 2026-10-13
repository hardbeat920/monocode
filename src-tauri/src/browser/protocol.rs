//! Shared wire contract of the browser module.
//!
//! FROZEN. Everything in this file is used by all five browser work packages
//! (core, semantic layer, CLI/MCP adapters, UI, resilience tests). Add to it
//! only through a coordinated change; never rename or remove existing items.
//!
//! The file holds no behaviour, only the types that cross module boundaries:
//! action names and their parameters, the four outcome classes, the closed set
//! of reasons (spec 11.3), the result envelope (spec 11.1), tab and connection
//! state as the UI and the agent see them, and the settings record.
//!
//! Naming follows the spec: `tab` is the agent-facing opaque tab id (`t_` +
//! 8 hex), `ref` is an element ref from a snapshot (`e12`, optionally
//! `e12@3` with the snapshot version). CDP `targetId` / `sessionId` never
//! appear here; the agent must not see them.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tauri event carrying the whole [`BrowserState`] (never a delta) on every change.
pub const STATE_EVENT: &str = "monocode-browser-state";
/// Environment variables the provider process (and its MCP/CLI children) inherit.
pub const ENV_ENDPOINT: &str = "MONOCODE_BROWSER_ENDPOINT";
pub const ENV_TOKEN: &str = "MONOCODE_BROWSER_TOKEN";
/// Control transport namespace used by the CLI and the MCP server (spec 6).
pub const NAMESPACE: &str = "browser";
/// Title prefix the marker script puts in front of the page title (spec 7).
pub const MARKER_PREFIX: &str = "\u{25C6} MonoCode \u{B7} ";
/// Title used when the page has no title of its own.
pub const MARKER_BARE: &str = "\u{25C6} MonoCode";
/// Minimum supported browser major version (Chrome and Edge alike, spec 4).
pub const MIN_BROWSER_MAJOR: u32 = 144;
/// Name of the isolated world all injected scripts run in (spec 5).
pub const ISOLATED_WORLD: &str = "__monocode";

/// Strip the user-facing marker from a title so results never carry it (spec 7).
pub fn strip_marker(title: &str) -> &str {
    if let Some(rest) = title.strip_prefix(MARKER_PREFIX) {
        rest
    } else if title == MARKER_BARE {
        ""
    } else {
        title
    }
}

/// Opaque, agent-facing tab identifier: `t_` followed by 8 hex characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TabId(pub String);

impl TabId {
    pub fn is_well_formed(&self) -> bool {
        self.0.len() == 10
            && self.0.starts_with("t_")
            && self.0[2..].bytes().all(|b| b.is_ascii_hexdigit())
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Permission generation. `run` is random per app run, so a value from a
/// previous run never compares equal to a current one (spec 14.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Epoch {
    pub run: u64,
    pub seq: u64,
}

impl Epoch {
    pub fn next(self) -> Epoch {
        Epoch {
            run: self.run,
            seq: self.seq + 1,
        }
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x}:{}", self.run, self.seq)
    }
}

// ---------------------------------------------------------------------------
// Outcomes and reasons (spec 11.2, 11.3)
// ---------------------------------------------------------------------------

/// The four outcome classes. `done` never means "the page did what the agent
/// wanted", only that the input reached the browser (spec 9.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Input accepted by the browser; `observed` says what followed.
    Done,
    /// Policy or state prevented sending. No CDP command left the process.
    Blocked,
    /// Attempted and failed for a known reason; possibly partially applied.
    Failed,
    /// A command was written but no answer came back. The effect may exist.
    Unknown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Done => "done",
            Outcome::Blocked => "blocked",
            Outcome::Failed => "failed",
            Outcome::Unknown => "unknown",
        }
    }
}

/// Closed set of reasons (spec 11.3 plus the connection states of spec 4 and
/// the snapshot/screenshot reasons of spec 8). A new reason needs a row in the
/// spec table and a test in `protocol::tests`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    // blocked: permission and state
    Off,
    NotGranted,
    Revoked,
    TurnInactive,
    NoLease,
    UrlDenied,
    PathDenied,
    DialogPending,
    TabLimit,
    Busy,
    InvalidRequest,
    // blocked: connection not usable (spec 4 "Agent dobija")
    BrowserConnecting,
    BrowserNeedsSetup,
    BrowserUnavailable,
    BrowserConsentPending,
    BrowserConsentDenied,
    BrowserUnsupported,
    BrowserPolicyBlocked,
    BrowserDisconnected,
    // failed
    RefStale,
    NotVisible,
    NotEnabled,
    NotEditable,
    NotStable,
    Detached,
    Obscured,
    OptionNotFound,
    NavigationFailed,
    Timeout,
    ChooserTimeout,
    NotFileInput,
    TargetClosed,
    NoDialog,
    SnapshotTimeout,
    ScreenshotUnavailableBackground,
    UnsupportedFrame,
    // unknown
    CdpTimeout,
    LinkDown,
}

impl Reason {
    pub const ALL: [Reason; 38] = [
        Reason::Off,
        Reason::NotGranted,
        Reason::Revoked,
        Reason::TurnInactive,
        Reason::NoLease,
        Reason::UrlDenied,
        Reason::PathDenied,
        Reason::DialogPending,
        Reason::TabLimit,
        Reason::Busy,
        Reason::InvalidRequest,
        Reason::BrowserConnecting,
        Reason::BrowserNeedsSetup,
        Reason::BrowserUnavailable,
        Reason::BrowserConsentPending,
        Reason::BrowserConsentDenied,
        Reason::BrowserUnsupported,
        Reason::BrowserPolicyBlocked,
        Reason::BrowserDisconnected,
        Reason::RefStale,
        Reason::NotVisible,
        Reason::NotEnabled,
        Reason::NotEditable,
        Reason::NotStable,
        Reason::Detached,
        Reason::Obscured,
        Reason::OptionNotFound,
        Reason::NavigationFailed,
        Reason::Timeout,
        Reason::ChooserTimeout,
        Reason::NotFileInput,
        Reason::TargetClosed,
        Reason::NoDialog,
        Reason::SnapshotTimeout,
        Reason::ScreenshotUnavailableBackground,
        Reason::UnsupportedFrame,
        Reason::CdpTimeout,
        Reason::LinkDown,
    ];

    /// The outcome class this reason belongs to.
    pub fn outcome(self) -> Outcome {
        use Reason::*;
        match self {
            Off
            | NotGranted
            | Revoked
            | TurnInactive
            | NoLease
            | UrlDenied
            | PathDenied
            | DialogPending
            | TabLimit
            | Busy
            | InvalidRequest
            | BrowserConnecting
            | BrowserNeedsSetup
            | BrowserUnavailable
            | BrowserConsentPending
            | BrowserConsentDenied
            | BrowserUnsupported
            | BrowserPolicyBlocked
            | BrowserDisconnected => Outcome::Blocked,
            RefStale
            | NotVisible
            | NotEnabled
            | NotEditable
            | NotStable
            | Detached
            | Obscured
            | OptionNotFound
            | NavigationFailed
            | Timeout
            | ChooserTimeout
            | NotFileInput
            | TargetClosed
            | NoDialog
            | SnapshotTimeout
            | ScreenshotUnavailableBackground
            | UnsupportedFrame => Outcome::Failed,
            CdpTimeout | LinkDown => Outcome::Unknown,
        }
    }

    /// Whether the same request may reasonably be retried by the agent
    /// (for `ref_stale` only after a new snapshot; for `unknown` only after a
    /// snapshot confirmed the effect did not happen).
    pub fn retryable(self) -> bool {
        use Reason::*;
        matches!(
            self,
            Busy | BrowserConnecting
                | BrowserConsentPending
                | RefStale
                | NotVisible
                | NotEnabled
                | NotEditable
                | NotStable
                | Detached
                | Obscured
                | Timeout
                | ChooserTimeout
                | NotFileInput
                | SnapshotTimeout
                | ScreenshotUnavailableBackground
        )
    }

    /// One English sentence for the model, with the next step when there is one.
    pub fn message(self) -> &'static str {
        use Reason::*;
        match self {
            Off => "Browser access is switched off in MonoCode settings; ask the user to enable it.",
            NotGranted => "This session has no browser access; ask the user to run /browser in this thread.",
            Revoked => "Browser access for this session was revoked; nothing further will be sent.",
            TurnInactive => "No active turn; browser actions are only allowed while responding.",
            NoLease => "Tab is not owned by this session; use browser_tabs to list or open your own tabs.",
            UrlDenied => "URL scheme not allowed; only http(s) pages and about:blank can be opened.",
            PathDenied => "File path is outside the allowed directories for upload.",
            DialogPending => "A dialog is waiting on this tab; answer it with browser_dialog first.",
            TabLimit => "Tab limit reached; close a tab you no longer need first.",
            Busy => "Another action is running on this tab; retry shortly.",
            InvalidRequest => "The request is invalid.",
            BrowserConnecting => "MonoCode is connecting to the browser; retry shortly.",
            BrowserNeedsSetup => "Remote debugging is not enabled in the browser; the user must enable it in chrome://inspect/#remote-debugging (or edge://inspect). Retrying will not help until then.",
            BrowserUnavailable => "The browser is not running or left a stale debugging record; the user must start it and retry from MonoCode.",
            BrowserConsentPending => "The browser is asking the user to allow this connection; wait for the user instead of retrying in a loop.",
            BrowserConsentDenied => "The user denied the browser connection; ask them to connect again from MonoCode settings.",
            BrowserUnsupported => "Chrome or Edge 144 or newer is required.",
            BrowserPolicyBlocked => "An administrator policy blocks remote debugging in this browser.",
            BrowserDisconnected => "The browser connection dropped; the next connection needs the user's approval again.",
            RefStale => "Element refs are stale; call browser_snapshot and use fresh refs.",
            NotVisible => "Element is not visible.",
            NotEnabled => "Element is disabled.",
            NotEditable => "Element does not accept text input.",
            NotStable => "Element kept moving; retry after the page settles.",
            Detached => "Element is no longer in the document; take a new snapshot.",
            Obscured => "Element is covered by another element; dismiss it or scroll.",
            OptionNotFound => "No option matches the requested value.",
            NavigationFailed => "Navigation failed.",
            Timeout => "Condition not met within the time limit.",
            ChooserTimeout => "No file chooser opened after the click.",
            NotFileInput => "Element is not a file input.",
            TargetClosed => "Tab was closed.",
            NoDialog => "No dialog is waiting on this tab.",
            SnapshotTimeout => "The snapshot took too long; use scope to narrow it.",
            ScreenshotUnavailableBackground => "The background tab did not render a screenshot; use browser_snapshot instead.",
            UnsupportedFrame => "This element lives in a frame the module cannot act in.",
            CdpTimeout => "Browser did not answer in time; take a snapshot before retrying, the action may have happened.",
            LinkDown => "Connection to the browser dropped during the action; take a snapshot before retrying.",
        }
    }

    pub fn as_str(self) -> &'static str {
        use Reason::*;
        match self {
            Off => "off",
            NotGranted => "not_granted",
            Revoked => "revoked",
            TurnInactive => "turn_inactive",
            NoLease => "no_lease",
            UrlDenied => "url_denied",
            PathDenied => "path_denied",
            DialogPending => "dialog_pending",
            TabLimit => "tab_limit",
            Busy => "busy",
            InvalidRequest => "invalid_request",
            BrowserConnecting => "browser_connecting",
            BrowserNeedsSetup => "browser_needs_setup",
            BrowserUnavailable => "browser_unavailable",
            BrowserConsentPending => "browser_consent_pending",
            BrowserConsentDenied => "browser_consent_denied",
            BrowserUnsupported => "browser_unsupported",
            BrowserPolicyBlocked => "browser_policy_blocked",
            BrowserDisconnected => "browser_disconnected",
            RefStale => "ref_stale",
            NotVisible => "not_visible",
            NotEnabled => "not_enabled",
            NotEditable => "not_editable",
            NotStable => "not_stable",
            Detached => "detached",
            Obscured => "obscured",
            OptionNotFound => "option_not_found",
            NavigationFailed => "navigation_failed",
            Timeout => "timeout",
            ChooserTimeout => "chooser_timeout",
            NotFileInput => "not_file_input",
            TargetClosed => "target_closed",
            NoDialog => "no_dialog",
            SnapshotTimeout => "snapshot_timeout",
            ScreenshotUnavailableBackground => "screenshot_unavailable_background",
            UnsupportedFrame => "unsupported_frame",
            CdpTimeout => "cdp_timeout",
            LinkDown => "link_down",
        }
    }
}

// ---------------------------------------------------------------------------
// Actions (spec 9.1, 9.5)
// ---------------------------------------------------------------------------

/// Every action the agent can request, by wire name. Closed set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    #[serde(rename = "snapshot")]
    Snapshot,
    #[serde(rename = "navigate")]
    Navigate,
    #[serde(rename = "click")]
    Click,
    #[serde(rename = "hover")]
    Hover,
    #[serde(rename = "type")]
    Type,
    #[serde(rename = "press")]
    Press,
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "scroll")]
    Scroll,
    #[serde(rename = "upload")]
    Upload,
    #[serde(rename = "dialog")]
    Dialog,
    #[serde(rename = "wait")]
    Wait,
    #[serde(rename = "back")]
    Back,
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "tabs.list")]
    TabsList,
    #[serde(rename = "tabs.open")]
    TabsOpen,
    #[serde(rename = "tabs.close")]
    TabsClose,
    #[serde(rename = "tabs.focus")]
    TabsFocus,
    #[serde(rename = "screenshot")]
    Screenshot,
    #[serde(rename = "read")]
    Read,
}

impl ActionKind {
    pub const ALL: [ActionKind; 19] = [
        ActionKind::Snapshot,
        ActionKind::Navigate,
        ActionKind::Click,
        ActionKind::Hover,
        ActionKind::Type,
        ActionKind::Press,
        ActionKind::Select,
        ActionKind::Scroll,
        ActionKind::Upload,
        ActionKind::Dialog,
        ActionKind::Wait,
        ActionKind::Back,
        ActionKind::Forward,
        ActionKind::TabsList,
        ActionKind::TabsOpen,
        ActionKind::TabsClose,
        ActionKind::TabsFocus,
        ActionKind::Screenshot,
        ActionKind::Read,
    ];

    pub fn name(self) -> &'static str {
        use ActionKind::*;
        match self {
            Snapshot => "snapshot",
            Navigate => "navigate",
            Click => "click",
            Hover => "hover",
            Type => "type",
            Press => "press",
            Select => "select",
            Scroll => "scroll",
            Upload => "upload",
            Dialog => "dialog",
            Wait => "wait",
            Back => "back",
            Forward => "forward",
            TabsList => "tabs.list",
            TabsOpen => "tabs.open",
            TabsClose => "tabs.close",
            TabsFocus => "tabs.focus",
            Screenshot => "screenshot",
            Read => "read",
        }
    }

    pub fn parse(name: &str) -> Option<ActionKind> {
        ActionKind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether the action targets a tab the session must hold a lease on.
    /// `tabs.list` and `tabs.open` are the only session-level actions.
    pub fn requires_lease(self) -> bool {
        !matches!(self, ActionKind::TabsList | ActionKind::TabsOpen)
    }

    /// Whether the action can change page or browser state. Mutating actions
    /// interrupted after a write are reported as `unknown`, observations as
    /// `failed` (spec 6, 11.2).
    pub fn mutates(self) -> bool {
        use ActionKind::*;
        !matches!(self, Snapshot | Read | Screenshot | TabsList | Wait | Hover)
    }

    /// Default and maximum action timeouts (spec 9.5).
    pub fn default_timeout(self) -> Duration {
        use ActionKind::*;
        match self {
            Click | Hover | Type | Press | Select | Scroll | Upload | Dialog => {
                Duration::from_secs(10)
            }
            Navigate | Back | Forward => Duration::from_secs(30),
            Wait => Duration::from_secs(10),
            Snapshot | Read | Screenshot => Duration::from_secs(15),
            TabsList | TabsOpen | TabsClose | TabsFocus => Duration::from_secs(5),
        }
    }

    pub fn max_timeout(self) -> Duration {
        use ActionKind::*;
        match self {
            Click | Hover | Type | Press | Select | Scroll | Upload | Dialog => {
                Duration::from_secs(30)
            }
            Navigate | Back | Forward | Wait => Duration::from_secs(120),
            Snapshot | Read | Screenshot => Duration::from_secs(60),
            TabsList | TabsOpen | TabsClose | TabsFocus => Duration::from_secs(10),
        }
    }

    /// Effective timeout for a request: the agent's `timeout_ms` clamped to
    /// `[100 ms, max_timeout]`, else the default.
    pub fn effective_timeout(self, timeout_ms: Option<u64>) -> Duration {
        match timeout_ms {
            Some(ms) => Duration::from_millis(ms.max(100)).min(self.max_timeout()),
            None => self.default_timeout(),
        }
    }
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A request as it arrives from the CLI or MCP server over the control
/// transport: `{"action": "click", "input": {...}}` plus the request id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRequest {
    pub request_id: String,
    pub action: String,
    pub input: Value,
}

// Parameter records (spec 9.1, 12.3). `tab` is the agent-facing tab id.
// All of them reject unknown fields so typos surface as `invalid_request`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    #[default]
    Interactive,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotParams {
    pub tab: TabId,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub mode: Option<SnapshotMode>,
    #[serde(default)]
    pub max_nodes: Option<u32>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    #[default]
    Load,
    Domcontentloaded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigateParams {
    pub tab: TabId,
    pub url: String,
    #[serde(default)]
    pub wait_until: Option<WaitUntil>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modifier {
    Alt,
    Control,
    Meta,
    Shift,
}

impl Modifier {
    /// CDP modifier bit (spec 9.3).
    pub fn bit(self) -> u32 {
        match self {
            Modifier::Alt => 1,
            Modifier::Control => 2,
            Modifier::Meta => 4,
            Modifier::Shift => 8,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickParams {
    pub tab: TabId,
    #[serde(rename = "ref")]
    pub element: String,
    #[serde(default)]
    pub button: Option<MouseButton>,
    #[serde(default)]
    pub count: Option<u8>,
    #[serde(default)]
    pub modifiers: Option<Vec<Modifier>>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoverParams {
    pub tab: TabId,
    #[serde(rename = "ref")]
    pub element: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeParams {
    pub tab: TabId,
    #[serde(rename = "ref")]
    pub element: String,
    pub text: String,
    #[serde(default)]
    pub clear: Option<bool>,
    #[serde(default)]
    pub submit: Option<bool>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressParams {
    pub tab: TabId,
    /// `Modifier+...+Key`, e.g. `Enter`, `Control+A` (spec 9.3).
    pub key: String,
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectParams {
    pub tab: TabId,
    #[serde(rename = "ref")]
    pub element: String,
    pub values: Vec<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollParams {
    pub tab: TabId,
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    #[serde(default)]
    pub direction: Option<ScrollDirection>,
    /// Pixels; default 600.
    #[serde(default)]
    pub amount: Option<u32>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadParams {
    pub tab: TabId,
    #[serde(rename = "ref")]
    pub element: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogAction {
    Accept,
    Dismiss,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogParams {
    pub tab: TabId,
    pub action: DialogAction,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitFor {
    Time,
    Text,
    Ref,
    Url,
    Load,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitParams {
    pub tab: TabId,
    #[serde(rename = "for")]
    pub condition: WaitFor,
    /// Milliseconds for `time`, text for `text`, ref for `ref`, prefix or glob for `url`.
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// `back` and `forward` share this record.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryParams {
    pub tab: TabId,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabsListParams {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabsOpenParams {
    /// http(s) only; defaults to `about:blank`.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabsCloseParams {
    pub tab: TabId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabsFocusParams {
    pub tab: TabId,
    /// Must be `true`: focusing changes what the user sees (spec 9.1).
    #[serde(default)]
    pub user_visible: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    pub tab: TabId,
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    #[serde(default)]
    pub full_page: Option<bool>,
    #[serde(default)]
    pub format: Option<ImageFormat>,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat {
    #[default]
    Text,
    Markdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadParams {
    pub tab: TabId,
    #[serde(default, rename = "ref")]
    pub element: Option<String>,
    #[serde(default)]
    pub format: Option<ReadFormat>,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Pull `tab` and `timeout_ms` out of an action's raw input without knowing
/// its full parameter record. Used by the host before dispatch.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CommonInput {
    #[serde(default)]
    pub tab: Option<TabId>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Result envelope (spec 11.1)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogType {
    Alert,
    Confirm,
    Prompt,
    Beforeunload,
}

/// A JavaScript dialog waiting on a tab (spec 10.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogInfo {
    #[serde(rename = "type")]
    pub kind: DialogType,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_prompt: Option<String>,
    pub url: String,
    pub opened_at_ms: i64,
}

/// A tab the action opened as a side effect (popup), spec 10.4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTabInfo {
    pub tab: TabId,
    pub url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadState {
    Load,
    Domcontentloaded,
    Timeout,
}

/// What the module saw right after the action (spec 9.2 step 9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Title without the MonoCode marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub navigated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_state: Option<LoadState>,
    #[serde(default)]
    pub snapshot_stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<DialogInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_tab: Option<NewTabInfo>,
    #[serde(default)]
    pub popup_blocked: bool,
}

/// The one result shape every action returns, over MCP and CLI alike.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    pub request_id: String,
    pub outcome: Outcome,
    pub reason: Option<Reason>,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub observed: Observed,
    /// Action-specific payload: snapshot text, read content, tab list, base64 image.
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub timing_ms: u64,
}

impl ActionResult {
    /// A result whose class, retryable flag and message come from the reason.
    pub fn from_reason(request_id: &str, reason: Reason) -> ActionResult {
        ActionResult {
            request_id: request_id.to_string(),
            outcome: reason.outcome(),
            reason: Some(reason),
            message: reason.message().to_string(),
            retryable: reason.retryable(),
            observed: Observed::default(),
            data: None,
            timing_ms: 0,
        }
    }

    pub fn done(request_id: &str, message: impl Into<String>) -> ActionResult {
        ActionResult {
            request_id: request_id.to_string(),
            outcome: Outcome::Done,
            reason: None,
            message: message.into(),
            retryable: false,
            observed: Observed::default(),
            data: None,
            timing_ms: 0,
        }
    }

    /// Override the class (e.g. `revoked` after a write is `unknown`, not `blocked`).
    pub fn with_outcome(mut self, outcome: Outcome) -> ActionResult {
        self.outcome = outcome;
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> ActionResult {
        self.message = message.into();
        self
    }

    pub fn with_data(mut self, data: Value) -> ActionResult {
        self.data = Some(data);
        self
    }

    pub fn with_observed(mut self, observed: Observed) -> ActionResult {
        self.observed = observed;
        self
    }

    pub fn is_done(&self) -> bool {
        self.outcome == Outcome::Done
    }
}

// ---------------------------------------------------------------------------
// Tabs as the agent and the UI see them (spec 7)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabState {
    /// `Target.createTarget` done, session being initialised.
    Attaching,
    Open,
    Closed,
    Crashed,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    pub tab: TabId,
    pub url: String,
    /// Without the marker prefix.
    pub title: String,
    pub state: TabState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<TabId>,
    pub dialog_pending: bool,
    pub busy: bool,
    pub created_at_ms: i64,
    pub last_action_at_ms: i64,
}

/// UI-only view of a tab: adds the owner, which the agent never sees.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabOverview {
    #[serde(flatten)]
    pub tab: TabInfo,
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// Connection and module state for the UI (spec 4, 13)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Off,
    Discovering,
    NeedsToggle,
    StalePort,
    AwaitingConsent,
    ConsentDenied,
    ConsentTimeout,
    UnsupportedVersion,
    PolicyBlocked,
    Connected,
    Disconnected,
}

impl ConnectionState {
    /// The reason an agent request gets while the connection is in this state;
    /// `None` only for `Connected`.
    pub fn agent_reason(self) -> Option<Reason> {
        use ConnectionState::*;
        Some(match self {
            Off => Reason::Off,
            Discovering => Reason::BrowserConnecting,
            NeedsToggle => Reason::BrowserNeedsSetup,
            StalePort => Reason::BrowserUnavailable,
            AwaitingConsent => Reason::BrowserConsentPending,
            ConsentDenied | ConsentTimeout => Reason::BrowserConsentDenied,
            UnsupportedVersion => Reason::BrowserUnsupported,
            PolicyBlocked => Reason::BrowserPolicyBlocked,
            Disconnected => Reason::BrowserDisconnected,
            Connected => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    Chrome,
    Edge,
    Chromium,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserInfo {
    pub kind: BrowserKind,
    /// Channel id (`chrome`, `msedge`, `chrome-beta`, ...) or `custom` for a manual user-data-dir.
    pub channel: String,
    /// `product` from `Browser.getVersion`, e.g. `Chrome/144.0.7559.60`.
    pub product: String,
    pub major: u32,
    pub user_data_dir: String,
    /// Number of our own tabs at connect time; diagnostics only.
    pub tabs_total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub state: ConnectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserInfo>,
    /// Human-readable detail for the state (instruction text, error, profile path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub since_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Which browser discovery looks at (spec 4, 13.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserChoice {
    /// First found: chrome, msedge, then beta/dev/canary variants.
    #[default]
    Auto,
    /// One channel id.
    Channel { channel: String },
    /// An explicit user-data-dir (Snap, Flatpak, portable installs).
    UserDataDir { path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub tabs_per_session: u32,
    pub tabs_total: u32,
    pub concurrent_actions: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            tabs_per_session: 8,
            tabs_total: 24,
            concurrent_actions: 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserSettings {
    pub browser: BrowserChoice,
    /// `Target.createTarget { newWindow: true }`; off by default (decision D3).
    pub separate_window: bool,
    pub limits: Limits,
    /// Extra roots allowed for `upload` besides the session cwd (spec 10.2).
    pub upload_roots: Vec<String>,
    pub allow_dotfiles: bool,
    pub protocol_trace: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        BrowserSettings {
            browser: BrowserChoice::Auto,
            separate_window: false,
            limits: Limits::default(),
            upload_roots: Vec::new(),
            allow_dotfiles: false,
            protocol_trace: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAccess {
    pub session_id: String,
    pub window: String,
    pub granted: bool,
    pub turn_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_since_ms: Option<i64>,
    pub tabs: u32,
}

/// Whole module state, emitted as [`STATE_EVENT`] on every change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserState {
    pub toggle_on: bool,
    pub connection: ConnectionInfo,
    pub sessions: Vec<SessionAccess>,
    pub tabs: Vec<TabOverview>,
    pub settings: BrowserSettings,
    pub global_epoch: Epoch,
}

/// One `browser_audit` row (spec 11.5), as the Settings panel lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts_ms: i64,
    pub session_id: String,
    pub request_id: String,
    pub global_epoch: String,
    pub session_epoch: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Redacted parameters (spec 11.5).
    pub params: Value,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub duration_ms: i64,
    pub commands_sent: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_has_a_class_a_retry_flag_and_a_message() {
        for reason in Reason::ALL {
            let _ = reason.outcome();
            let _ = reason.retryable();
            assert!(!reason.message().is_empty(), "{reason:?}");
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                Value::String(reason.as_str().into())
            );
        }
        assert_eq!(Reason::Off.outcome(), Outcome::Blocked);
        assert_eq!(Reason::RefStale.outcome(), Outcome::Failed);
        assert_eq!(Reason::CdpTimeout.outcome(), Outcome::Unknown);
        assert!(!Reason::CdpTimeout.retryable());
        assert!(Reason::Busy.retryable());
        // Blocked reasons never claim a retry will help without user action,
        // except the transient ones.
        for reason in Reason::ALL {
            if reason.outcome() == Outcome::Blocked && reason.retryable() {
                assert!(matches!(
                    reason,
                    Reason::Busy | Reason::BrowserConnecting | Reason::BrowserConsentPending
                ));
            }
        }
    }

    #[test]
    fn action_names_round_trip_and_timeouts_are_clamped() {
        for kind in ActionKind::ALL {
            assert_eq!(ActionKind::parse(kind.name()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                Value::String(kind.name().into())
            );
            assert!(kind.default_timeout() <= kind.max_timeout());
        }
        assert_eq!(ActionKind::parse("tabs.list"), Some(ActionKind::TabsList));
        assert_eq!(ActionKind::parse("evaluate"), None);
        assert!(!ActionKind::TabsOpen.requires_lease());
        assert!(ActionKind::Click.requires_lease());
        assert_eq!(
            ActionKind::Click.effective_timeout(Some(999_999)),
            Duration::from_secs(30)
        );
        assert_eq!(
            ActionKind::Click.effective_timeout(Some(1)),
            Duration::from_millis(100)
        );
        assert_eq!(
            ActionKind::Navigate.effective_timeout(None),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn result_envelope_matches_the_spec_shape() {
        let result = ActionResult::from_reason("req_1", Reason::DialogPending);
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["outcome"], "blocked");
        assert_eq!(json["reason"], "dialog_pending");
        assert_eq!(json["retryable"], false);
        assert_eq!(json["observed"]["navigated"], false);
        assert!(json["data"].is_null());
        let back: ActionResult = serde_json::from_value(json).unwrap();
        assert_eq!(back, result);

        let done = ActionResult::done("req_2", "Opened tab.").with_data(serde_json::json!({
            "tab": "t_0a1b2c3d"
        }));
        assert!(done.is_done());
        assert!(done.reason.is_none());
    }

    #[test]
    fn params_reject_unknown_fields_and_accept_ref_alias() {
        let click: ClickParams = serde_json::from_value(serde_json::json!({
            "tab": "t_0a1b2c3d", "ref": "e12", "count": 2
        }))
        .unwrap();
        assert_eq!(click.element, "e12");
        assert!(click.tab.is_well_formed());
        assert!(serde_json::from_value::<ClickParams>(serde_json::json!({
            "tab": "t_0a1b2c3d", "ref": "e12", "selector": "#x"
        }))
        .is_err());
        let wait: WaitParams =
            serde_json::from_value(serde_json::json!({"tab": "t_0a1b2c3d", "for": "load"}))
                .unwrap();
        assert_eq!(wait.condition, WaitFor::Load);
        let common: CommonInput =
            serde_json::from_value(serde_json::json!({"tab": "t_0a1b2c3d", "ref": "e1"})).unwrap();
        assert_eq!(common.tab.unwrap().0, "t_0a1b2c3d");
    }

    #[test]
    fn marker_is_stripped_from_titles() {
        assert_eq!(strip_marker("\u{25C6} MonoCode \u{B7} GitHub"), "GitHub");
        assert_eq!(strip_marker("\u{25C6} MonoCode"), "");
        assert_eq!(strip_marker("GitHub"), "GitHub");
        assert!(!TabId("t_zz".into()).is_well_formed());
    }

    #[test]
    fn connection_states_map_to_agent_reasons() {
        assert_eq!(ConnectionState::Connected.agent_reason(), None);
        assert_eq!(ConnectionState::Off.agent_reason(), Some(Reason::Off));
        assert_eq!(
            ConnectionState::NeedsToggle.agent_reason(),
            Some(Reason::BrowserNeedsSetup)
        );
        let settings: BrowserSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.limits.tabs_per_session, 8);
        assert_eq!(Epoch { run: 7, seq: 1 }.next().seq, 2);
    }
}
