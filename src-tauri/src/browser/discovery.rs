//! Discovery (spec 4): where `DevToolsActivePort` lives for each browser
//! channel, reading it, probing the port, version and policy checks.
//!
//! Discovery never launches a browser and never passes
//! `--remote-debugging-port`; the user enables remote debugging in
//! `chrome://inspect/#remote-debugging` (or `edge://inspect`, and the same
//! page in Brave, Vivaldi, Opera and Arc) and the browser writes the file for
//! us.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::protocol::{BrowserChoice, BrowserKind, MIN_BROWSER_MAJOR};

/// TCP probe before the WebSocket upgrade (spec 4, step 3).
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
/// How long we wait for the user to answer Chrome's consent dialog.
pub const CONSENT_TIMEOUT: Duration = Duration::from_secs(90);
/// The file the browser writes next to its profile when remote debugging is on.
pub const ACTIVE_PORT_FILE: &str = "DevToolsActivePort";

/// Channel ids in discovery order (spec 4, step 1): Chrome and Edge with
/// their test builds, then the other Chromium browsers.
pub const CHANNELS: [&str; 12] = [
    "chrome",
    "msedge",
    "chrome-beta",
    "chrome-dev",
    "chrome-canary",
    "msedge-beta",
    "msedge-dev",
    "msedge-canary",
    "brave",
    "vivaldi",
    "opera",
    "arc",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

/// A browser profile directory discovery may look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// `chrome`, `msedge`, ... or `custom` for a user-supplied directory.
    pub channel: String,
    pub kind: BrowserKind,
    pub user_data_dir: PathBuf,
}

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

/// The browser behind a profile folder the user picked, from its path.
fn path_kind(path: &str) -> BrowserKind {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    if lower.contains("edge") {
        BrowserKind::Edge
    } else if lower.contains("brave") {
        BrowserKind::Brave
    } else if lower.contains("vivaldi") {
        BrowserKind::Vivaldi
    } else if lower.contains("opera") {
        BrowserKind::Opera
    } else if lower.contains("/arc/") {
        BrowserKind::Arc
    } else if lower.contains("chrome") {
        BrowserKind::Chrome
    } else {
        BrowserKind::Chromium
    }
}

/// The user-data-dir of a channel under `base` (`%LOCALAPPDATA%` on Windows,
/// the home directory elsewhere), spec 4 table.
pub fn user_data_dir_in(channel: &str, base: &Path, os: Platform) -> Option<PathBuf> {
    let path = match (os, channel) {
        (Platform::Windows, "chrome") => base.join("Google").join("Chrome").join("User Data"),
        (Platform::Windows, "chrome-beta") => {
            base.join("Google").join("Chrome Beta").join("User Data")
        }
        (Platform::Windows, "chrome-dev") => {
            base.join("Google").join("Chrome Dev").join("User Data")
        }
        (Platform::Windows, "chrome-canary") => {
            base.join("Google").join("Chrome SxS").join("User Data")
        }
        (Platform::Windows, "msedge") => base.join("Microsoft").join("Edge").join("User Data"),
        (Platform::Windows, "msedge-beta") => {
            base.join("Microsoft").join("Edge Beta").join("User Data")
        }
        (Platform::Windows, "msedge-dev") => {
            base.join("Microsoft").join("Edge Dev").join("User Data")
        }
        (Platform::Windows, "msedge-canary") => {
            base.join("Microsoft").join("Edge SxS").join("User Data")
        }
        (Platform::Windows, "brave") => base
            .join("BraveSoftware")
            .join("Brave-Browser")
            .join("User Data"),
        (Platform::Windows, "vivaldi") => base.join("Vivaldi").join("User Data"),
        // Opera keeps its profile under Roaming, next to `base` (Local).
        (Platform::Windows, "opera") => base
            .parent()?
            .join("Roaming")
            .join("Opera Software")
            .join("Opera Stable"),
        (Platform::MacOs, channel) => {
            let support = base.join("Library").join("Application Support");
            match channel {
                "chrome" => support.join("Google").join("Chrome"),
                "chrome-beta" => support.join("Google").join("Chrome Beta"),
                "chrome-dev" => support.join("Google").join("Chrome Dev"),
                "chrome-canary" => support.join("Google").join("Chrome Canary"),
                "msedge" => support.join("Microsoft Edge"),
                "msedge-beta" => support.join("Microsoft Edge Beta"),
                "msedge-dev" => support.join("Microsoft Edge Dev"),
                "msedge-canary" => support.join("Microsoft Edge Canary"),
                "brave" => support.join("BraveSoftware").join("Brave-Browser"),
                "vivaldi" => support.join("Vivaldi"),
                "opera" => support.join("com.operasoftware.Opera"),
                "arc" => support.join("Arc").join("User Data"),
                _ => return None,
            }
        }
        (Platform::Linux, channel) => {
            let config = base.join(".config");
            match channel {
                "chrome" => config.join("google-chrome"),
                "chrome-beta" => config.join("google-chrome-beta"),
                "chrome-dev" => config.join("google-chrome-unstable"),
                "chrome-canary" => config.join("google-chrome-canary"),
                "msedge" => config.join("microsoft-edge"),
                "msedge-beta" => config.join("microsoft-edge-beta"),
                "msedge-dev" => config.join("microsoft-edge-dev"),
                "msedge-canary" => config.join("microsoft-edge-canary"),
                "brave" => config.join("BraveSoftware").join("Brave-Browser"),
                "vivaldi" => config.join("vivaldi"),
                "opera" => config.join("opera"),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(path)
}

fn base_dir(os: Platform) -> Option<PathBuf> {
    let var = match os {
        Platform::Windows => "LOCALAPPDATA",
        Platform::MacOs | Platform::Linux => "HOME",
    };
    std::env::var_os(var).map(PathBuf::from)
}

pub fn user_data_dir(channel: &str) -> Option<PathBuf> {
    let os = Platform::current();
    user_data_dir_in(channel, &base_dir(os)?, os)
}

/// Candidates for `choice`, existing directories only (spec 4, step 1).
pub fn candidates(choice: &BrowserChoice) -> Vec<Candidate> {
    let os = Platform::current();
    let base = base_dir(os);
    candidates_in(choice, base.as_deref(), os, |path| path.is_dir())
}

pub fn candidates_in(
    choice: &BrowserChoice,
    base: Option<&Path>,
    os: Platform,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Candidate> {
    match choice {
        BrowserChoice::UserDataDir { path } => {
            let dir = PathBuf::from(path);
            if !exists(&dir) {
                return Vec::new();
            }
            vec![Candidate {
                channel: "custom".into(),
                kind: path_kind(path),
                user_data_dir: dir,
            }]
        }
        BrowserChoice::Channel { channel } => {
            let Some(base) = base else { return Vec::new() };
            user_data_dir_in(channel, base, os)
                .filter(|dir| exists(dir))
                .map(|dir| Candidate {
                    channel: channel.clone(),
                    kind: channel_kind(channel),
                    user_data_dir: dir,
                })
                .into_iter()
                .collect()
        }
        BrowserChoice::Auto => {
            let Some(base) = base else { return Vec::new() };
            CHANNELS
                .iter()
                .filter_map(|channel| {
                    let dir = user_data_dir_in(channel, base, os)?;
                    exists(&dir).then(|| Candidate {
                        channel: channel.to_string(),
                        kind: channel_kind(channel),
                        user_data_dir: dir,
                    })
                })
                .collect()
        }
    }
}

/// Contents of `DevToolsActivePort`: the port and the browser endpoint path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivePort {
    pub port: u16,
    /// `/devtools/browser/<id>`; `/devtools/browser` when the second line is missing.
    pub path: String,
}

pub fn parse_active_port(text: &str) -> Option<ActivePort> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let port: u16 = lines.next()?.parse().ok()?;
    if port == 0 {
        return None;
    }
    let path = match lines.next() {
        Some(path) if path.starts_with("/devtools/browser") => path.to_string(),
        Some(_) | None => "/devtools/browser".to_string(),
    };
    Some(ActivePort { port, path })
}

pub fn read_active_port(user_data_dir: &Path) -> Option<ActivePort> {
    let text = std::fs::read_to_string(user_data_dir.join(ACTIVE_PORT_FILE)).ok()?;
    parse_active_port(&text)
}

/// `ws://127.0.0.1:<port><path>`: loopback literal so the `Host` header is
/// one the browser accepts (spec 4, step 4).
pub fn ws_url(active: &ActivePort) -> String {
    format!("ws://127.0.0.1:{}{}", active.port, active.path)
}

/// Does anything accept TCP on the port (spec 4, step 3)?
pub fn port_accepts(port: u16, timeout: Duration) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(stream) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            true
        }
        Err(_) => false,
    }
}

/// Outcome of [`locate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Located {
    /// A live endpoint; `ws_url` is what the link should open.
    Found {
        candidate: Candidate,
        active: ActivePort,
        ws_url: String,
    },
    /// The file exists but the port does not answer: browser closed or a
    /// stale file after a crash.
    StalePort { candidate: Candidate, port: u16 },
    /// Profiles exist but none has the file: remote debugging is off.
    NeedsToggle { candidates: Vec<Candidate> },
    /// No supported browser profile on this machine (or the chosen one is missing).
    NoBrowser,
}

/// Spec 4, steps 1-4. Pure file and TCP work; the WebSocket is the link's job.
pub fn locate(choice: &BrowserChoice) -> Located {
    locate_with(candidates(choice), read_active_port, |port| {
        port_accepts(port, PROBE_TIMEOUT)
    })
}

pub fn locate_with(
    candidates: Vec<Candidate>,
    read: impl Fn(&Path) -> Option<ActivePort>,
    probe: impl Fn(u16) -> bool,
) -> Located {
    if candidates.is_empty() {
        return Located::NoBrowser;
    }
    let mut stale: Option<(Candidate, u16)> = None;
    for candidate in &candidates {
        let Some(active) = read(&candidate.user_data_dir) else {
            continue;
        };
        if probe(active.port) {
            let ws_url = ws_url(&active);
            return Located::Found {
                candidate: candidate.clone(),
                active,
                ws_url,
            };
        }
        if stale.is_none() {
            stale = Some((candidate.clone(), active.port));
        }
    }
    match stale {
        Some((candidate, port)) => Located::StalePort { candidate, port },
        None => Located::NeedsToggle { candidates },
    }
}

/// The instruction shown in `NeedsToggle` (UI and agent alike).
pub fn setup_instruction(kind: Option<BrowserKind>) -> String {
    const AFTER: &str =
        "turn on remote debugging, then retry. Retrying without that step will not help.";
    match kind {
        Some(kind) => format!("Open {} in {} and {AFTER}", inspect_page(kind), kind.label()),
        None => format!("Open chrome://inspect/#remote-debugging (Chrome, Brave, Vivaldi, Opera, Arc) or edge://inspect (Edge), {AFTER}"),
    }
}

/// Where a browser turns remote debugging on.
pub fn inspect_page(kind: BrowserKind) -> &'static str {
    match kind {
        BrowserKind::Edge => "edge://inspect",
        BrowserKind::Brave => "brave://inspect/#remote-debugging",
        _ => "chrome://inspect/#remote-debugging",
    }
}

/// Browser name and major version from `Browser.getVersion.product`
/// (`Chrome/144.0.7559.60`, `HeadlessChrome/144.0.0.0`, `Edg/144.0.0.0`).
pub fn parse_version(product: &str) -> Option<(String, u32)> {
    let (name, version) = product.trim().split_once('/')?;
    let major: u32 = version.split('.').next()?.trim().parse().ok()?;
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), major))
}

pub fn version_supported(major: u32) -> bool {
    major >= MIN_BROWSER_MAJOR
}

/// Best-effort check whether an administrator policy turns remote debugging
/// off (`RemoteDebuggingAllowed = false`). Only used for the message in
/// `NeedsToggle`; nothing is ever bypassed (spec 4, "Politike i okruženje").
pub fn policy_blocks_debugging(kind: BrowserKind) -> Option<String> {
    match Platform::current() {
        Platform::Linux => {
            let dir = match kind {
                BrowserKind::Edge => "/etc/opt/edge/policies/managed",
                BrowserKind::Chrome => "/etc/opt/chrome/policies/managed",
                BrowserKind::Chromium => "/etc/chromium/policies/managed",
                BrowserKind::Brave => "/etc/brave/policies/managed",
                BrowserKind::Vivaldi | BrowserKind::Opera | BrowserKind::Arc => return None,
            };
            linux_policy_blocks(Path::new(dir))
        }
        Platform::MacOs => {
            let domain = match kind {
                BrowserKind::Edge => "com.microsoft.Edge",
                BrowserKind::Chrome | BrowserKind::Chromium => "com.google.Chrome",
                BrowserKind::Brave => "com.brave.Browser",
                BrowserKind::Vivaldi | BrowserKind::Opera | BrowserKind::Arc => return None,
            };
            let output = std::process::Command::new("defaults")
                .args([
                    "read",
                    &format!("/Library/Managed Preferences/{domain}"),
                    "RemoteDebuggingAllowed",
                ])
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&output.stdout);
            (output.status.success() && text.trim() == "0")
                .then(|| format!("Managed preference {domain} RemoteDebuggingAllowed = 0"))
        }
        Platform::Windows => {
            let key = match kind {
                BrowserKind::Edge => r"Software\Policies\Microsoft\Edge",
                BrowserKind::Chrome | BrowserKind::Chromium => r"Software\Policies\Google\Chrome",
                BrowserKind::Brave => r"Software\Policies\BraveSoftware\Brave",
                BrowserKind::Vivaldi | BrowserKind::Opera | BrowserKind::Arc => return None,
            };
            for hive in ["HKLM", "HKCU"] {
                let output = std::process::Command::new("reg")
                    .args([
                        "query",
                        &format!(r"{hive}\{key}"),
                        "/v",
                        "RemoteDebuggingAllowed",
                    ])
                    .output()
                    .ok()?;
                let text = String::from_utf8_lossy(&output.stdout);
                if output.status.success() && text.contains("0x0") {
                    return Some(format!(r"{hive}\{key}\RemoteDebuggingAllowed = 0"));
                }
            }
            None
        }
    }
}

fn linux_policy_blocks(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if value.get("RemoteDebuggingAllowed") == Some(&serde_json::Value::Bool(false)) {
            return Some(format!(
                "{} sets RemoteDebuggingAllowed = false",
                path.display()
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn user_data_dirs_follow_the_spec_table() {
        let win = Path::new(r"C:\Users\o\AppData\Local");
        assert_eq!(
            user_data_dir_in("chrome", win, Platform::Windows).unwrap(),
            win.join("Google").join("Chrome").join("User Data")
        );
        assert_eq!(
            user_data_dir_in("msedge-canary", win, Platform::Windows).unwrap(),
            win.join("Microsoft").join("Edge SxS").join("User Data")
        );
        let home = Path::new("/Users/o");
        assert_eq!(
            user_data_dir_in("chrome-canary", home, Platform::MacOs).unwrap(),
            PathBuf::from("/Users/o/Library/Application Support/Google/Chrome Canary")
        );
        assert_eq!(
            user_data_dir_in("msedge", home, Platform::MacOs).unwrap(),
            PathBuf::from("/Users/o/Library/Application Support/Microsoft Edge")
        );
        let home = Path::new("/home/o");
        assert_eq!(
            user_data_dir_in("chrome-dev", home, Platform::Linux).unwrap(),
            PathBuf::from("/home/o/.config/google-chrome-unstable")
        );
        assert_eq!(
            user_data_dir_in("msedge-beta", home, Platform::Linux).unwrap(),
            PathBuf::from("/home/o/.config/microsoft-edge-beta")
        );
        assert!(user_data_dir_in("firefox", home, Platform::Linux).is_none());
        assert_eq!(channel_kind("msedge-dev"), BrowserKind::Edge);
        assert_eq!(channel_kind("chrome"), BrowserKind::Chrome);
    }

    #[test]
    fn other_chromium_browsers_have_their_profile_folders() {
        let win = Path::new(r"C:\Users\o\AppData\Local");
        assert_eq!(
            user_data_dir_in("brave", win, Platform::Windows).unwrap(),
            win.join("BraveSoftware")
                .join("Brave-Browser")
                .join("User Data")
        );
        // Forward slashes, so the parent folder is found on every test host.
        let local = Path::new("C:/Users/o/AppData/Local");
        assert_eq!(
            user_data_dir_in("opera", local, Platform::Windows).unwrap(),
            Path::new("C:/Users/o/AppData")
                .join("Roaming")
                .join("Opera Software")
                .join("Opera Stable")
        );
        assert!(user_data_dir_in("arc", win, Platform::Windows).is_none());
        let mac = Path::new("/Users/o");
        assert_eq!(
            user_data_dir_in("arc", mac, Platform::MacOs).unwrap(),
            PathBuf::from("/Users/o/Library/Application Support/Arc/User Data")
        );
        assert_eq!(
            user_data_dir_in("vivaldi", mac, Platform::MacOs).unwrap(),
            PathBuf::from("/Users/o/Library/Application Support/Vivaldi")
        );
        let linux = Path::new("/home/o");
        assert_eq!(
            user_data_dir_in("brave", linux, Platform::Linux).unwrap(),
            PathBuf::from("/home/o/.config/BraveSoftware/Brave-Browser")
        );
        assert_eq!(
            user_data_dir_in("opera", linux, Platform::Linux).unwrap(),
            PathBuf::from("/home/o/.config/opera")
        );
        assert!(user_data_dir_in("arc", linux, Platform::Linux).is_none());
        for (channel, kind) in [
            ("brave", BrowserKind::Brave),
            ("vivaldi", BrowserKind::Vivaldi),
            ("opera", BrowserKind::Opera),
            ("arc", BrowserKind::Arc),
        ] {
            assert_eq!(channel_kind(channel), kind);
            assert!(CHANNELS.contains(&channel));
        }
        assert_eq!(
            path_kind(r"C:\Users\o\AppData\Local\BraveSoftware\Brave-Browser\User Data"),
            BrowserKind::Brave
        );
        assert_eq!(
            path_kind("/Users/o/Library/Application Support/Arc/User Data"),
            BrowserKind::Arc
        );
        assert_eq!(path_kind("/snap/chromium/current"), BrowserKind::Chromium);
        assert!(setup_instruction(Some(BrowserKind::Brave)).contains("brave://inspect"));
        assert!(setup_instruction(Some(BrowserKind::Opera)).contains("in Opera"));
    }

    #[test]
    fn candidates_respect_the_choice_and_skip_missing_dirs() {
        let home = Path::new("/home/o");
        let existing = |p: &Path| {
            p == Path::new("/home/o/.config/microsoft-edge")
                || p == Path::new("/home/o/.config/google-chrome-beta")
                || p == Path::new("/snap/chromium/current")
        };
        let auto = candidates_in(&BrowserChoice::Auto, Some(home), Platform::Linux, existing);
        let channels: Vec<&str> = auto.iter().map(|c| c.channel.as_str()).collect();
        assert_eq!(channels, vec!["msedge", "chrome-beta"]);
        let one = candidates_in(
            &BrowserChoice::Channel {
                channel: "chrome".into(),
            },
            Some(home),
            Platform::Linux,
            existing,
        );
        assert!(one.is_empty());
        let custom = candidates_in(
            &BrowserChoice::UserDataDir {
                path: "/snap/chromium/current".into(),
            },
            Some(home),
            Platform::Linux,
            existing,
        );
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].channel, "custom");
        assert_eq!(custom[0].kind, BrowserKind::Chromium);
        assert!(candidates_in(&BrowserChoice::Auto, None, Platform::Linux, existing).is_empty());
    }

    #[test]
    fn active_port_file_parses_both_shapes() {
        let active = parse_active_port("9222\n/devtools/browser/abc-123\n").unwrap();
        assert_eq!(active.port, 9222);
        assert_eq!(active.path, "/devtools/browser/abc-123");
        assert_eq!(
            ws_url(&active),
            "ws://127.0.0.1:9222/devtools/browser/abc-123"
        );
        let bare = parse_active_port("1234").unwrap();
        assert_eq!(bare.path, "/devtools/browser");
        assert!(parse_active_port("").is_none());
        assert!(parse_active_port("0\n/devtools/browser/x").is_none());
        assert!(parse_active_port("abc\n/devtools/browser/x").is_none());
        let odd = parse_active_port("4000\n/not/devtools").unwrap();
        assert_eq!(odd.path, "/devtools/browser");
    }

    #[test]
    fn locate_classifies_missing_stale_and_live_endpoints() {
        let candidate = |channel: &str| Candidate {
            channel: channel.into(),
            kind: channel_kind(channel),
            user_data_dir: PathBuf::from(format!("/profiles/{channel}")),
        };
        assert_eq!(
            locate_with(Vec::new(), |_| None, |_| true),
            Located::NoBrowser
        );
        let needs = locate_with(
            vec![candidate("chrome"), candidate("msedge")],
            |_| None,
            |_| true,
        );
        assert!(matches!(needs, Located::NeedsToggle { candidates } if candidates.len() == 2));
        let stale = locate_with(
            vec![candidate("chrome")],
            |_| {
                Some(ActivePort {
                    port: 1,
                    path: "/devtools/browser/x".into(),
                })
            },
            |_| false,
        );
        assert_eq!(
            stale,
            Located::StalePort {
                candidate: candidate("chrome"),
                port: 1
            }
        );
        // A stale first candidate does not hide a live second one.
        let found = locate_with(
            vec![candidate("chrome"), candidate("msedge")],
            |dir| {
                let port = if dir.ends_with("chrome") { 1 } else { 2 };
                Some(ActivePort {
                    port,
                    path: format!("/devtools/browser/{port}"),
                })
            },
            |port| port == 2,
        );
        assert!(
            matches!(found, Located::Found { candidate, ws_url, .. } if candidate.channel == "msedge" && ws_url == "ws://127.0.0.1:2/devtools/browser/2")
        );
    }

    #[test]
    fn locate_reads_real_files_and_probes_real_ports() {
        let dir = std::env::temp_dir().join(format!("monocode-discovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let candidate = Candidate {
            channel: "chrome".into(),
            kind: BrowserKind::Chrome,
            user_data_dir: dir.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::write(
            dir.join(ACTIVE_PORT_FILE),
            format!("{port}\n/devtools/browser/live\n"),
        )
        .unwrap();
        let located = locate_with(vec![candidate.clone()], read_active_port, |p| {
            port_accepts(p, PROBE_TIMEOUT)
        });
        assert!(
            matches!(located, Located::Found { ws_url, .. } if ws_url == format!("ws://127.0.0.1:{port}/devtools/browser/live"))
        );
        drop(listener);
        // Port 1 is never listening on loopback.
        std::fs::write(dir.join(ACTIVE_PORT_FILE), "1\n/devtools/browser/dead\n").unwrap();
        let located = locate_with(vec![candidate.clone()], read_active_port, |p| {
            port_accepts(p, PROBE_TIMEOUT)
        });
        assert_eq!(
            located,
            Located::StalePort {
                candidate: candidate.clone(),
                port: 1
            }
        );
        std::fs::remove_file(dir.join(ACTIVE_PORT_FILE)).unwrap();
        let located = locate_with(vec![candidate], read_active_port, |_| true);
        assert!(matches!(located, Located::NeedsToggle { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_parse_and_the_floor_is_144() {
        assert_eq!(
            parse_version("Chrome/144.0.7559.60").unwrap(),
            ("Chrome".to_string(), 144)
        );
        assert_eq!(parse_version("HeadlessChrome/150.0.0.0").unwrap().1, 150);
        assert_eq!(
            parse_version(" Edg/143.0.1 ").unwrap(),
            ("Edg".to_string(), 143)
        );
        assert!(parse_version("Chrome").is_none());
        assert!(parse_version("/144.0").is_none());
        assert!(version_supported(144));
        assert!(!version_supported(143));
        assert!(setup_instruction(Some(BrowserKind::Edge)).contains("edge://inspect"));
        assert!(setup_instruction(None).contains("chrome://inspect"));
    }

    #[test]
    fn linux_policy_scan_reads_managed_json() {
        let dir = std::env::temp_dir().join(format!("monocode-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(linux_policy_blocks(&dir).is_none());
        std::fs::write(
            dir.join("other.json"),
            r#"{"RemoteDebuggingAllowed": true}"#,
        )
        .unwrap();
        assert!(linux_policy_blocks(&dir).is_none());
        std::fs::write(
            dir.join("lock.json"),
            r#"{"RemoteDebuggingAllowed": false}"#,
        )
        .unwrap();
        assert!(linux_policy_blocks(&dir).unwrap().contains("lock.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
