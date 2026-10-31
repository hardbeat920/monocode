//! What a settings page says about the browser: one sentence and a button
//! for the connection, and the browsers it can choose from.
//!
//! The Tauri app's frontend has the same text in
//! `src/features/settings/model/browser.ts`; Monocode Native uses this.

use crate::protocol::{
    BrowserChoice, BrowserInfo, BrowserKind, BrowserMode, BrowserState, ConnectionState,
    TabOverview, TabState,
};

/// The connection's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Pending,
    Good,
    Warning,
    Error,
}

/// The button beside the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Connect,
    Retry,
    Disconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionView {
    /// One sentence for the state.
    pub message: String,
    pub tone: Tone,
    pub action: Option<Action>,
    /// Show the steps to allow the connection (your own browser only).
    pub guide: bool,
}

/// Every channel "Your own browser" can connect to, with its name.
pub const CHANNELS: [(&str, &str); 12] = [
    ("chrome", "Google Chrome"),
    ("msedge", "Microsoft Edge"),
    ("chrome-beta", "Chrome Beta"),
    ("chrome-dev", "Chrome Dev"),
    ("chrome-canary", "Chrome Canary"),
    ("msedge-beta", "Edge Beta"),
    ("msedge-dev", "Edge Dev"),
    ("msedge-canary", "Edge Canary"),
    ("brave", "Brave"),
    ("vivaldi", "Vivaldi"),
    ("opera", "Opera"),
    ("arc", "Arc (macOS)"),
];

/// What the MonoCode browser can start; Automatic tries Chrome, then Edge,
/// Brave, Vivaldi, Opera and Arc.
pub fn managed_channels() -> impl Iterator<Item = (&'static str, &'static str)> {
    CHANNELS.into_iter().filter(|(channel, _)| {
        !["-beta", "-dev", "-canary"]
            .iter()
            .any(|suffix| channel.ends_with(suffix))
    })
}

/// The managed picker's value for `choice`: `auto`, or the stable channel
/// of the chosen browser (a Beta channel kept from "Your own browser"
/// starts that browser's stable build).
pub fn managed_choice(choice: &BrowserChoice) -> &'static str {
    let BrowserChoice::Channel { channel } = choice else {
        return "auto";
    };
    let kind = channel_kind(channel);
    managed_channels()
        .find(|(candidate, _)| channel_kind(candidate) == kind)
        .map(|(candidate, _)| candidate)
        .unwrap_or("auto")
}

/// The browser behind a channel id.
pub fn channel_kind(channel: &str) -> BrowserKind {
    if channel.starts_with("msedge") {
        BrowserKind::Edge
    } else if channel.starts_with("chrome") {
        BrowserKind::Chrome
    } else if channel.starts_with("brave") {
        BrowserKind::Brave
    } else if channel.starts_with("vivaldi") {
        BrowserKind::Vivaldi
    } else if channel.starts_with("opera") {
        BrowserKind::Opera
    } else if channel.starts_with("arc") {
        BrowserKind::Arc
    } else {
        BrowserKind::Chromium
    }
}

fn kind_label(kind: BrowserKind) -> &'static str {
    match kind {
        BrowserKind::Chrome => "Google Chrome",
        BrowserKind::Edge => "Microsoft Edge",
        BrowserKind::Chromium => "Chromium",
        BrowserKind::Brave => "Brave",
        BrowserKind::Vivaldi => "Vivaldi",
        BrowserKind::Opera => "Opera",
        BrowserKind::Arc => "Arc",
    }
}

/// `Google Chrome 144.0.7559.60`; Brave, Vivaldi, Opera and Arc report the
/// Chromium version they are built on, so it is named as such.
pub fn product_label(browser: &BrowserInfo) -> String {
    let label = kind_label(browser.kind);
    match browser.product.split('/').nth(1).filter(|v| !v.is_empty()) {
        None => label.to_string(),
        Some(version)
            if matches!(
                browser.kind,
                BrowserKind::Chrome | BrowserKind::Edge | BrowserKind::Chromium
            ) =>
        {
            format!("{label} {version}")
        }
        Some(version) => format!("{label} (Chromium {version})"),
    }
}

/// `example.com` for a tab's address; the address itself when it has no
/// host.
pub fn tab_host(url: &str) -> String {
    if url.is_empty() || url == "about:blank" {
        return "about:blank".into();
    }
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| url.to_owned())
}

/// The thread's browser indicator, shown while the thread has access and
/// the browser runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indicator {
    Granted,
    /// An action runs in one of its tabs.
    Busy,
    /// One of its tabs waits on a dialog.
    Dialog,
}

impl Indicator {
    pub fn label(self) -> &'static str {
        match self {
            Indicator::Granted => "Browser access on",
            Indicator::Busy => "Browser action running",
            Indicator::Dialog => "A tab is waiting on a dialog",
        }
    }
}

/// The session's tabs.
pub fn session_tabs<'a>(
    state: &'a BrowserState,
    session: &'a str,
) -> impl Iterator<Item = &'a TabOverview> + 'a {
    state
        .tabs
        .iter()
        .filter(move |tab| tab.session_id == session)
}

/// The indicator for `session`, or `None` while it has no access or no
/// browser runs: once the MonoCode browser closed (its last tab went),
/// nothing of it shows until an agent or the user starts it again.
pub fn indicator(state: &BrowserState, session: &str) -> Option<Indicator> {
    if !crate::turn::granted(state, session) || state.connection.state != ConnectionState::Connected
    {
        return None;
    }
    let tabs: Vec<_> = session_tabs(state, session).collect();
    Some(if tabs.iter().any(|tab| tab.tab.dialog_pending) {
        Indicator::Dialog
    } else if tabs.iter().any(|tab| tab.tab.busy) {
        Indicator::Busy
    } else {
        Indicator::Granted
    })
}

/// A tab's status, as Settings and the thread's menu list it.
pub fn tab_status(tab: &TabOverview) -> &'static str {
    match tab.tab.state {
        TabState::Attaching => "Opening",
        TabState::Crashed => "Crashed",
        TabState::Revoked => "Released",
        TabState::Closed => "Closed",
        TabState::Open if tab.tab.dialog_pending => "Waiting on a dialog",
        TabState::Open if tab.tab.busy => "Action running",
        TabState::Open => "Idle",
    }
}

fn view(
    message: impl Into<String>,
    tone: Tone,
    action: Option<Action>,
    guide: bool,
) -> ConnectionView {
    ConnectionView {
        message: message.into(),
        tone,
        action,
        guide,
    }
}

/// The connection, as Settings shows it.
pub fn connection_view(state: &BrowserState) -> ConnectionView {
    let attach = attach_view(state);
    if !state.settings.mode.launches() {
        return attach;
    }
    // The MonoCode browser has no consent step, so it never shows the guide.
    let private = state.settings.mode == BrowserMode::Private;
    let name = if private {
        "private MonoCode browser"
    } else {
        "MonoCode browser"
    };
    let connection = &state.connection;
    let mut view = match connection.state {
        ConnectionState::Discovering => ConnectionView {
            message: format!("Starting the {name}…"),
            ..attach
        },
        ConnectionState::Idle => ConnectionView {
            message: if private {
                format!("Ready. The {name} starts when an agent needs it and forgets everything when it closes.")
            } else {
                format!("Ready. The {name} starts when an agent needs it.")
            },
            action: None,
            ..attach
        },
        ConnectionState::Connected => ConnectionView {
            message: match &connection.browser {
                Some(browser) => format!("The {name} is running on {}.", product_label(browser)),
                None => format!("The {name} is running."),
            },
            ..attach
        },
        ConnectionState::Disconnected => view(
            format!("The {name} closed. It starts again when an agent needs it."),
            Tone::Neutral,
            None,
            false,
        ),
        _ => attach,
    };
    view.guide = false;
    view
}

fn attach_view(state: &BrowserState) -> ConnectionView {
    use Action::{Connect, Disconnect, Retry};
    use Tone::{Error, Good, Neutral, Pending, Warning};
    match state.connection.state {
        ConnectionState::Off => view("Browser access is off.", Neutral, None, false),
        ConnectionState::Discovering => view("Looking for your browser…", Pending, None, false),
        ConnectionState::NeedsToggle => view(
            "Remote debugging is not turned on in the browser yet.",
            Warning,
            Some(Retry),
            true,
        ),
        ConnectionState::StalePort => view(
            "The browser is not running, or it left an old debugging record behind.",
            Warning,
            Some(Retry),
            false,
        ),
        ConnectionState::AwaitingConsent => {
            view("Confirm the connection in your browser.", Pending, None, true)
        }
        ConnectionState::ConsentDenied => view(
            "The browser denied the connection. Connect again when you are ready.",
            Warning,
            Some(Connect),
            true,
        ),
        ConnectionState::ConsentTimeout => view(
            "Nobody answered the browser's prompt in time. Connect again when you are ready.",
            Warning,
            Some(Connect),
            true,
        ),
        ConnectionState::UnsupportedVersion => view(
            "A browser built on Chromium 144 or newer is required.",
            Error,
            Some(Retry),
            false,
        ),
        ConnectionState::PolicyBlocked => view(
            "An administrator policy blocks remote debugging in this browser. Contact your administrator.",
            Error,
            None,
            false,
        ),
        ConnectionState::Connected => view(
            match &state.connection.browser {
                Some(browser) => format!("Connected to {}.", product_label(browser)),
                None => "Connected.".to_string(),
            },
            Good,
            Some(Disconnect),
            false,
        ),
        ConnectionState::Disconnected => view(
            "The connection ended. The next connection asks for permission in the browser again.",
            Warning,
            Some(Connect),
            false,
        ),
        ConnectionState::Idle => view(
            "Ready. MonoCode connects when an agent needs the browser.",
            Neutral,
            Some(Connect),
            false,
        ),
    }
}

/// The detail under the connection: the profile, and the last error or
/// the state's own detail.
pub fn connection_detail(state: &BrowserState) -> Option<String> {
    let connection = &state.connection;
    let mut parts = Vec::new();
    if let Some(browser) = &connection.browser {
        parts.push(format!("Profile {}", browser.user_data_dir));
    }
    if let Some(error) = &connection.last_error {
        parts.push(error.clone());
    } else if let Some(detail) = connection
        .detail
        .as_ref()
        .filter(|_| connection.state != ConnectionState::Connected)
    {
        parts.push(detail.clone());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BrowserSettings, SessionAccess};

    fn state(mode: BrowserMode, connection: ConnectionState) -> BrowserState {
        let mut state = crate::BrowserHost::for_tests(BrowserSettings::default()).state();
        state.settings.mode = mode;
        state.connection.state = connection;
        state
    }

    #[test]
    fn the_globe_shows_only_while_a_browser_runs() {
        let mut idle = state(BrowserMode::Managed, ConnectionState::Idle);
        idle.toggle_on = true;
        idle.sessions.push(SessionAccess {
            session_id: "s1".into(),
            window: "main".into(),
            granted: true,
            turn_active: false,
            granted_since_ms: None,
            tabs: 0,
            requested: false,
            turned_off: false,
        });
        assert_eq!(indicator(&idle, "s1"), None, "the browser closed");
        let mut running = idle.clone();
        running.connection.state = ConnectionState::Connected;
        assert_eq!(indicator(&running, "s1"), Some(Indicator::Granted));
        assert_eq!(indicator(&running, "s2"), None, "no access");
    }

    #[test]
    fn the_monocode_browser_never_shows_the_consent_guide() {
        let managed = connection_view(&state(BrowserMode::Managed, ConnectionState::Idle));
        assert!(managed.message.contains("MonoCode browser starts"));
        assert_eq!(managed.action, None);
        assert!(
            !connection_view(&state(BrowserMode::Private, ConnectionState::ConsentDenied)).guide
        );
        let attach = connection_view(&state(BrowserMode::Attach, ConnectionState::ConsentDenied));
        assert!(attach.guide);
        assert_eq!(attach.action, Some(Action::Connect));
    }

    #[test]
    fn managed_picker_maps_channels_to_their_stable_browser() {
        assert_eq!(managed_choice(&BrowserChoice::Auto), "auto");
        let beta = BrowserChoice::Channel {
            channel: "msedge-beta".into(),
        };
        assert_eq!(managed_choice(&beta), "msedge");
        assert_eq!(managed_channels().count(), 6);
    }

    #[test]
    fn tabs_are_named_by_their_host() {
        assert_eq!(tab_host("https://example.com:8080/a?b"), "example.com");
        assert_eq!(tab_host(""), "about:blank");
        assert_eq!(tab_host("data:text/plain,hi"), "data:text/plain,hi");
    }
}
