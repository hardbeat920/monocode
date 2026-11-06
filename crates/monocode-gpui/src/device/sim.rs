//! What the tool rail changes on an iOS Simulator: its settings, location,
//! app permissions, the app in front, pushes, logs and network requests.
//!
//! serve-sim runs these as actions over its exec socket, the same one its own
//! web tools use. Each call here blocks; the panel runs them in the background.
use monocode_core::device::hub::{self, Endpoint};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    time::{Duration, Instant},
};
use tungstenite::{Message, client::IntoClientRequest};

const EXEC_PATH: &str = "/vendor/serve-sim/exec-ws";
/// `settings.status` reads every setting through simctl and an
/// accessibility helper; it takes a second or two.
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs one serve-sim action and returns what it printed.
pub(crate) fn exec(endpoint: &Endpoint, action: &str, params: Value) -> Result<String, String> {
    hub::touch();
    let mut request = endpoint
        .ws_url(EXEC_PATH)
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        endpoint
            .authorization()
            .parse()
            .map_err(|_| "invalid token".to_string())?,
    );
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], endpoint.port));
    let stream = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|e| format!("Device Hub unreachable: {e}"))?;
    stream
        .set_read_timeout(Some(EXEC_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let (mut socket, _) = tungstenite::client(request, stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(error) => error.to_string(),
        tungstenite::HandshakeError::Interrupted(_) => "handshake interrupted".into(),
    })?;
    // The socket admits a client by its first message, which carries the
    // hub's token (the Authorization header only gets it through the hub).
    let token = endpoint
        .authorization()
        .trim_start_matches("Bearer ")
        .to_owned();
    socket
        .send(Message::Text(json!({"token": token}).to_string().into()))
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + EXEC_TIMEOUT;
    let mut sent = false;
    loop {
        if Instant::now() > deadline {
            return Err(format!("{action} took too long."));
        }
        let message = socket.read().map_err(|e| match e {
            tungstenite::Error::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock => {
                format!("{action} took too long.")
            }
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                "The Device Hub closed the connection.".into()
            }
            other => other.to_string(),
        })?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if value["ready"] == true && !sent {
            sent = true;
            socket
                .send(Message::Text(
                    json!({"id": 1, "action": action, "params": params})
                        .to_string()
                        .into(),
                ))
                .map_err(|e| e.to_string())?;
            continue;
        }
        if value["id"] != 1 {
            continue;
        }
        let _ = socket.close(None);
        return reply(action, &value);
    }
}

/// An action's reply: what it printed, or why it failed.
fn reply(action: &str, value: &Value) -> Result<String, String> {
    if let Some(error) = value["error"].as_str() {
        return Err(error.to_owned());
    }
    let stdout = value["stdout"].as_str().unwrap_or_default().to_owned();
    if value["exitCode"].as_i64().unwrap_or(0) != 0 {
        let stderr = value["stderr"].as_str().unwrap_or_default().trim();
        return Err(if stderr.is_empty() {
            format!("{action} failed.")
        } else {
            stderr.chars().take(600).collect()
        });
    }
    Ok(stdout)
}

/// A simulator-wide setting the Display and Accessibility tools change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Setting {
    Appearance,
    TextSize,
    ColorFilter,
    LiquidGlass,
    ReduceMotion,
    IncreaseContrast,
    ReduceTransparency,
    VoiceOver,
    ButtonShapes,
}

pub(crate) const TEXT_SIZES: [&str; 12] = [
    "extra-small",
    "small",
    "medium",
    "large",
    "extra-large",
    "extra-extra-large",
    "extra-extra-extra-large",
    "accessibility-medium",
    "accessibility-large",
    "accessibility-extra-large",
    "accessibility-extra-extra-large",
    "accessibility-extra-extra-extra-large",
];
/// Short names for the text sizes, as iOS shows them in Xcode's previews.
pub(crate) const TEXT_SIZE_NAMES: [&str; 12] = [
    "XS", "S", "M", "L", "XL", "XXL", "XXXL", "AX1", "AX2", "AX3", "AX4", "AX5",
];
pub(crate) const COLOR_FILTERS: [&str; 5] =
    ["none", "grayscale", "red-green", "green-red", "blue-yellow"];

impl Setting {
    pub(crate) const ALL: [Setting; 9] = [
        Self::Appearance,
        Self::TextSize,
        Self::ColorFilter,
        Self::LiquidGlass,
        Self::ReduceMotion,
        Self::IncreaseContrast,
        Self::ReduceTransparency,
        Self::VoiceOver,
        Self::ButtonShapes,
    ];

    /// Its name in `serve-sim ui status --json`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::TextSize => "text-size",
            Self::ColorFilter => "color-filter",
            Self::LiquidGlass => "liquid-glass",
            Self::ReduceMotion => "reduce-motion",
            Self::IncreaseContrast => "increase-contrast",
            Self::ReduceTransparency => "reduce-transparency",
            Self::VoiceOver => "voiceover",
            Self::ButtonShapes => "show-borders",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }

    /// The values this setting takes, as serve-sim names them.
    pub(crate) fn values(self) -> &'static [&'static str] {
        match self {
            Self::Appearance => &["light", "dark"],
            Self::TextSize => &TEXT_SIZES,
            Self::ColorFilter => &COLOR_FILTERS,
            Self::LiquidGlass => &["clear", "tinted"],
            _ => &["on", "off"],
        }
    }

    /// `value` as serve-sim names it: text sizes also go by their short
    /// names ("AX3"), and toggles by true and false.
    pub(crate) fn accept(self, value: &str) -> Result<&'static str, String> {
        let wanted = value.trim().to_ascii_lowercase();
        let wanted = match (self, wanted.as_str()) {
            (Self::TextSize, short) => TEXT_SIZE_NAMES
                .iter()
                .position(|name| name.eq_ignore_ascii_case(short))
                .map(|ix| TEXT_SIZES[ix].to_owned())
                .unwrap_or(wanted),
            (_, "true" | "yes" | "1") if self.values() == ["on", "off"] => "on".into(),
            (_, "false" | "no" | "0") if self.values() == ["on", "off"] => "off".into(),
            _ => wanted,
        };
        self.values()
            .iter()
            .find(|known| **known == wanted)
            .copied()
            .ok_or_else(|| format!("{} takes {}", self.key(), self.values().join(", ")))
    }

    /// What a new simulator starts with, so the panel can list what differs.
    pub(crate) fn default_value(self) -> &'static str {
        match self {
            Self::Appearance => "light",
            Self::TextSize => "large",
            Self::ColorFilter => "none",
            Self::LiquidGlass => "clear",
            _ => "off",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::TextSize => "Text size",
            Self::ColorFilter => "Color filter",
            Self::LiquidGlass => "Liquid Glass",
            Self::ReduceMotion => "Reduce Motion",
            Self::IncreaseContrast => "Increase Contrast",
            Self::ReduceTransparency => "Reduce Transparency",
            Self::VoiceOver => "VoiceOver",
            Self::ButtonShapes => "Button Shapes",
        }
    }

    /// How the overrides strip names this setting at `value`.
    pub(crate) fn describe(self, value: &str) -> String {
        match self {
            Self::Appearance => title_case(value),
            Self::TextSize => format!("Text {}", text_size_name(value)),
            Self::ColorFilter => color_filter_name(value).to_owned(),
            Self::LiquidGlass => format!("Liquid Glass {}", title_case(value)),
            _ if value == "on" => self.label().to_owned(),
            _ => format!("{} off", self.label()),
        }
    }
}

pub(crate) fn text_size_name(value: &str) -> &'static str {
    TEXT_SIZES
        .iter()
        .position(|size| *size == value)
        .map(|ix| TEXT_SIZE_NAMES[ix])
        .unwrap_or("?")
}

pub(crate) fn color_filter_name(value: &str) -> &'static str {
    match value {
        "grayscale" => "Grayscale",
        "red-green" => "Red/Green",
        "green-red" => "Green/Red",
        "blue-yellow" => "Blue/Yellow",
        _ => "No filter",
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The settings a simulator reported; ones it does not support are absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Settings(pub(crate) BTreeMap<Setting, String>);

impl Settings {
    pub(crate) fn parse(stdout: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(stdout.trim())
            .map_err(|_| "The simulator's settings could not be read.".to_string())?;
        let mut settings = BTreeMap::new();
        for setting in Setting::ALL {
            if let Some(found) = value[setting.key()].as_str()
                && found != "unsupported"
            {
                settings.insert(setting, found.to_owned());
            }
        }
        Ok(Self(settings))
    }

    pub(crate) fn get(&self, setting: Setting) -> Option<&str> {
        self.0.get(&setting).map(String::as_str)
    }

    pub(crate) fn changed(&self, setting: Setting) -> bool {
        self.get(setting)
            .is_some_and(|value| value != setting.default_value())
    }
}

/// Every setting at once; it takes a second or two.
pub(crate) fn settings(udid: &str) -> Result<Settings, String> {
    Settings::parse(&hub::serve_sim_cli(&[
        "ui", "status", "--json", "-d", udid,
    ])?)
}

pub(crate) fn set(udid: &str, setting: Setting, value: &str) -> Result<(), String> {
    hub::serve_sim_cli(&["ui", setting.key(), value, "-d", udid]).map(|_| ())
}

pub(crate) fn set_location(
    endpoint: &Endpoint,
    udid: &str,
    at: Option<(f64, f64)>,
) -> Result<(), String> {
    match at {
        Some((lat, lng)) => exec(
            endpoint,
            "location.set",
            json!({"udid": udid, "lat": lat, "lng": lng}),
        ),
        None => exec(endpoint, "location.clear", json!({"udid": udid})),
    }
    .map(|_| ())
}

/// A permission the Permissions tool shows, by serve-sim's name for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Permission {
    Camera,
    Photos,
    Location,
    Notifications,
    Microphone,
    Contacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    Allowed,
    Denied,
    /// Never asked, or reset: the app asks next time.
    Ask,
}

impl Grant {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allow",
            Self::Denied => "Deny",
            Self::Ask => "Ask",
        }
    }
    fn verb(self) -> &'static str {
        match self {
            Self::Allowed => "grant",
            Self::Denied => "revoke",
            Self::Ask => "reset",
        }
    }
}

impl Permission {
    pub(crate) const ALL: [Permission; 6] = [
        Self::Camera,
        Self::Photos,
        Self::Location,
        Self::Notifications,
        Self::Microphone,
        Self::Contacts,
    ];
    pub(crate) fn service(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Photos => "photos",
            Self::Location => "location",
            Self::Notifications => "notifications",
            Self::Microphone => "microphone",
            Self::Contacts => "contacts",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Photos => "Photos",
            Self::Location => "Location",
            Self::Notifications => "Notifications",
            Self::Microphone => "Microphone",
            Self::Contacts => "Contacts",
        }
    }
}

/// What an app has been granted, from `permissions list <bundle>`.
pub(crate) fn parse_permissions(stdout: &str) -> Result<BTreeMap<&'static str, Grant>, String> {
    let value: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| "The app's permissions could not be read.".to_string())?;
    let mut grants = BTreeMap::new();
    for permission in Permission::ALL {
        let grant = match permission {
            // TCC's auth_value: 0 denied, 2 allowed, 3 limited.
            Permission::Location => match value["location"]["Authorization"].as_i64() {
                Some(2) | Some(4) | Some(3) => Grant::Allowed,
                Some(1) => Grant::Denied,
                _ => Grant::Ask,
            },
            Permission::Notifications => {
                match value["notifications"]["allowsNotifications"].as_bool() {
                    Some(true) => Grant::Allowed,
                    Some(false) => Grant::Denied,
                    None => Grant::Ask,
                }
            }
            other => match value["tcc"][other.service()].as_i64() {
                Some(0) => Grant::Denied,
                Some(_) => Grant::Allowed,
                None => Grant::Ask,
            },
        };
        grants.insert(permission.service(), grant);
    }
    Ok(grants)
}

pub(crate) fn permissions(
    udid: &str,
    bundle: &str,
) -> Result<BTreeMap<&'static str, Grant>, String> {
    parse_permissions(&hub::serve_sim_cli(&[
        "permissions",
        "list",
        bundle,
        "-q",
        "-d",
        udid,
    ])?)
}

pub(crate) fn set_permission(
    udid: &str,
    bundle: &str,
    permission: Permission,
    grant: Grant,
) -> Result<(), String> {
    hub::serve_sim_cli(&[
        "permissions",
        grant.verb(),
        permission.service(),
        bundle,
        "-q",
        "-d",
        udid,
    ])
    .map(|_| ())
}

/// The app in front, from serve-sim's app-state stream: its first event says
/// which app it is now.
pub(crate) fn foreground_app(endpoint: &Endpoint, udid: &str) -> Result<Option<String>, String> {
    // App state and logs come from serve-sim's helper for the simulator,
    // which the stream normally starts; free when it already runs.
    hub::attach(endpoint, udid)?;
    let path = format!("/vendor/serve-sim/appstate?device={}", hub::encode(udid));
    let first = sse_events(endpoint, &path, 1, Duration::from_secs(3))?;
    Ok(first
        .first()
        .and_then(|data| serde_json::from_str::<Value>(data).ok())
        .and_then(|value| value["bundleId"].as_str().map(str::to_owned))
        .filter(|bundle| bundle != "com.apple.springboard"))
}

pub(crate) fn restart_app(endpoint: &Endpoint, udid: &str, bundle: &str) -> Result<(), String> {
    exec(
        endpoint,
        "app.launch",
        json!({"udid": udid, "bundleId": bundle, "launchArgs": []}),
    )
    .map(|_| ())
}

pub(crate) fn stop_app(endpoint: &Endpoint, udid: &str, bundle: &str) -> Result<(), String> {
    exec(
        endpoint,
        "app.stop",
        json!({"udid": udid, "bundleId": bundle}),
    )
    .map(|_| ())
}

pub(crate) fn open_url(
    endpoint: &Endpoint,
    udid: &str,
    bundle: &str,
    url: &str,
) -> Result<(), String> {
    exec(
        endpoint,
        "app.openUrl",
        json!({"udid": udid, "bundleId": bundle, "url": url}),
    )
    .map(|_| ())
}

/// Sends a push notification to an app. serve-sim has no action for it, so
/// this asks simctl directly.
pub(crate) fn push(udid: &str, bundle: &str, payload: &str) -> Result<(), String> {
    let parsed: Value =
        serde_json::from_str(payload).map_err(|e| format!("The payload is not JSON: {e}"))?;
    if !parsed["aps"].is_object() {
        return Err("The payload needs an \"aps\" object.".into());
    }
    let file = std::env::temp_dir().join(format!(
        "monocode-push-{}-{}.apns",
        std::process::id(),
        monocode_core::time::now_ms()
    ));
    std::fs::write(&file, payload).map_err(|e| e.to_string())?;
    let output = std::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "push", udid, bundle])
        .arg(&file)
        .output();
    let _ = std::fs::remove_file(&file);
    let output = output.map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().chars().take(600).collect())
    }
}

/// One log line from the apps on the simulator.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogLine {
    pub(crate) seq: u64,
    /// The process that logged it, such as "Orders".
    pub(crate) process: String,
    pub(crate) message: String,
    /// An `Error` or `Fault` line.
    pub(crate) error: bool,
}

impl LogLine {
    /// serve-sim passes on `log stream --style ndjson` lines as they are.
    fn parse(seq: u64, raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return (!raw.is_empty()).then(|| Self {
                seq,
                process: String::new(),
                message: raw.to_owned(),
                error: false,
            });
        };
        let message = value["eventMessage"]
            .as_str()
            .or_else(|| value["formatString"].as_str())?
            .trim()
            .to_owned();
        if message.is_empty() {
            return None;
        }
        let process = value["processImagePath"]
            .as_str()
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or_default()
            .to_owned();
        let error = matches!(value["messageType"].as_str(), Some("Error" | "Fault"));
        Some(Self {
            seq,
            process,
            message,
            error,
        })
    }
}

/// The log lines after `since`, from serve-sim's log buffer. The first call
/// starts its log stream for the device.
pub(crate) fn logs(endpoint: &Endpoint, udid: &str, since: u64) -> Result<Vec<LogLine>, String> {
    hub::attach(endpoint, udid)?;
    let path = format!(
        "/vendor/serve-sim/logs?device={}&scope=user-apps&snapshot=1&follow=1&since={since}&limit=400",
        hub::encode(udid)
    );
    let response = endpoint.get(&path, Duration::from_secs(10))?;
    let value: Value = serde_json::from_str(&response.into_string().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(parse_logs(&value, since))
}

fn parse_logs(value: &Value, since: u64) -> Vec<LogLine> {
    value["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| {
            let seq = line["seq"].as_u64().filter(|seq| *seq > since)?;
            LogLine::parse(seq, line["raw"].as_str()?)
        })
        .collect()
}

/// "42.69, 23.32" or "42.69 23.32".
pub(crate) fn parse_coordinates(text: &str) -> Option<(f64, f64)> {
    let mut parts = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>);
    let (lat, lng) = (parts.next()?.ok()?, parts.next()?.ok()?);
    (parts.next().is_none() && (-90. ..=90.).contains(&lat) && (-180. ..=180.).contains(&lng))
        .then_some((lat, lng))
}

/// Where the tool rail keeps screenshots and recordings it attaches.
pub(crate) fn media_dir() -> Result<std::path::PathBuf, String> {
    let dir = monocode_core::device::toolchain::root().join("tool-media");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// A screen recording in progress, made by simctl.
pub(crate) struct Recording {
    child: std::process::Child,
    pub(crate) path: std::path::PathBuf,
    pub(crate) started: Instant,
}

pub(crate) fn start_recording(udid: &str) -> Result<Recording, String> {
    let path = media_dir()?.join(format!("recording-{}.mp4", monocode_core::time::now_ms()));
    let child = std::process::Command::new("/usr/bin/xcrun")
        .args([
            "simctl",
            "io",
            udid,
            "recordVideo",
            "--codec=h264",
            "--force",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start recording: {e}"))?;
    Ok(Recording {
        child,
        path,
        started: Instant::now(),
    })
}

impl Recording {
    /// Blocking: simctl writes the movie's index when interrupted, so it is
    /// stopped the way Ctrl-C would, then waited for.
    pub(crate) fn stop(mut self) -> Result<std::path::PathBuf, String> {
        // SAFETY: signals the child this owns, which has not been reaped.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGINT);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err("The recording did not finish.".into());
                }
            }
        }
        match std::fs::metadata(&self.path) {
            Ok(meta) if meta.len() > 0 => Ok(self.path.clone()),
            _ => {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.child.stderr.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stderr);
                }
                Err(stderr
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("The recording is empty.")
                    .trim()
                    .to_owned())
            }
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Blocking: saves a PNG of the simulator's screen for attaching.
pub(crate) fn screenshot(udid: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let png = hub::screenshot(monocode_core::device::Platform::Ios, udid)?;
    let path = media_dir()?.join(format!("{name}-{}.png", monocode_core::time::now_ms()));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Reads up to `count` server-sent `data:` events, giving up after `wait`.
pub(crate) fn sse_events(
    endpoint: &Endpoint,
    path: &str,
    count: usize,
    wait: Duration,
) -> Result<Vec<String>, String> {
    let response = endpoint.get(path, wait)?;
    let reader = BufReader::new(response.into_reader());
    let mut events = Vec::new();
    let deadline = Instant::now() + wait;
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if let Some(data) = line.strip_prefix("data: ") {
            events.push(data.to_owned());
            if events.len() >= count {
                break;
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_parse_skips_unsupported_and_spots_changes() {
        let settings = Settings::parse(
            r#"{"appearance":"dark","liquid-glass":"unsupported","color-filter":"none","text-size":"large","reduce-motion":"on","hardware-keyboard":"on"}"#,
        )
        .unwrap();
        assert_eq!(settings.get(Setting::Appearance), Some("dark"));
        assert_eq!(settings.get(Setting::LiquidGlass), None);
        assert!(settings.changed(Setting::Appearance));
        assert!(settings.changed(Setting::ReduceMotion));
        assert!(!settings.changed(Setting::TextSize));
        assert!(!settings.changed(Setting::ColorFilter));
    }

    #[test]
    fn settings_describe_reads_like_the_strip() {
        assert_eq!(Setting::Appearance.describe("dark"), "Dark");
        assert_eq!(
            Setting::TextSize.describe("accessibility-extra-large"),
            "Text AX3"
        );
        assert_eq!(Setting::ColorFilter.describe("grayscale"), "Grayscale");
        assert_eq!(Setting::ReduceMotion.describe("on"), "Reduce Motion");
        assert_eq!(Setting::VoiceOver.describe("off"), "VoiceOver off");
    }

    #[test]
    fn settings_accept_short_names_and_refuse_strangers() {
        assert_eq!(
            Setting::TextSize.accept("AX3"),
            Ok("accessibility-extra-large")
        );
        assert_eq!(Setting::TextSize.accept("large"), Ok("large"));
        assert_eq!(Setting::Appearance.accept(" Dark "), Ok("dark"));
        assert_eq!(Setting::ReduceMotion.accept("true"), Ok("on"));
        assert_eq!(Setting::ColorFilter.accept("grayscale"), Ok("grayscale"));
        assert!(
            Setting::Appearance
                .accept("dim")
                .unwrap_err()
                .contains("light, dark")
        );
        assert!(Setting::LiquidGlass.accept("on").is_err());
        assert_eq!(
            Setting::from_key("show-borders"),
            Some(Setting::ButtonShapes)
        );
    }

    #[test]
    fn replies_carry_errors_and_stderr() {
        assert_eq!(
            reply(
                "x",
                &json!({"id": 1, "stdout": "ok\n", "stderr": "", "exitCode": 0})
            ),
            Ok("ok\n".into())
        );
        assert_eq!(
            reply("x", &json!({"id": 1, "error": "unknown action y"})),
            Err("unknown action y".into())
        );
        assert_eq!(
            reply(
                "x",
                &json!({"id": 1, "stdout": "", "stderr": " boom \n", "exitCode": 2})
            ),
            Err("boom".into())
        );
        assert_eq!(
            reply(
                "x",
                &json!({"id": 1, "stdout": "", "stderr": "", "exitCode": 2})
            ),
            Err("x failed.".into())
        );
    }

    #[test]
    fn permissions_parse_tcc_location_and_notifications() {
        let grants = parse_permissions(
            r#"{"udid":"U","bundleId":"b","tcc":{"camera":2,"photos":0},"location":{"Authorization":2},"notifications":{"allowsNotifications":false,"critical":false}}"#,
        )
        .unwrap();
        assert_eq!(grants["camera"], Grant::Allowed);
        assert_eq!(grants["photos"], Grant::Denied);
        assert_eq!(grants["location"], Grant::Allowed);
        assert_eq!(grants["notifications"], Grant::Denied);
        assert_eq!(grants["microphone"], Grant::Ask);
        let unknown =
            parse_permissions(r#"{"tcc":{},"location":null,"notifications":null}"#).unwrap();
        assert!(unknown.values().all(|grant| *grant == Grant::Ask));
    }

    #[test]
    fn logs_keep_lines_after_the_cursor() {
        let event = json!({
            "eventMessage": "Menu: 503 Service Unavailable",
            "messageType": "Error",
            "processImagePath": "/Users/x/Library/Developer/CoreSimulator/Devices/U/data/Containers/Bundle/Application/A/Orders.app/Orders",
        });
        let value = json!({"lines": [
            {"seq": 4, "at": 1, "raw": "old"},
            {"seq": 5, "at": 2, "raw": event.to_string()},
            {"seq": 6, "at": 3, "raw": "plain text\n"},
            {"seq": 7, "at": 4, "raw": json!({"eventMessage": " "}).to_string()},
        ]});
        assert_eq!(
            parse_logs(&value, 4),
            vec![
                LogLine {
                    seq: 5,
                    process: "Orders".into(),
                    message: "Menu: 503 Service Unavailable".into(),
                    error: true,
                },
                LogLine {
                    seq: 6,
                    process: String::new(),
                    message: "plain text".into(),
                    error: false,
                },
            ]
        );
    }

    #[test]
    fn pushes_need_an_aps_object() {
        assert!(push("U", "b", "not json").unwrap_err().contains("not JSON"));
        assert!(push("U", "b", "{}").unwrap_err().contains("aps"));
    }

    /// Prints what serve-sim answers on a booted simulator, for checking the
    /// parsers against the real thing. Starts its own hub.
    #[test]
    #[ignore]
    fn live_simulator_tools() {
        monocode_core::device::set_enabled(true, false);
        let endpoint = hub::ensure().expect("the hub starts");
        let list = hub::list().expect("the hub lists devices");
        let device = list
            .devices
            .iter()
            .find(|d| d.platform == monocode_core::device::Platform::Ios && d.booted)
            .expect("a booted simulator")
            .clone();
        hub::attach(&endpoint, &device.id).expect("the simulator attaches");
        let started = Instant::now();
        let settings = self::settings(&device.id);
        eprintln!("settings {:?}: {settings:?}", started.elapsed());
        let started = Instant::now();
        set(&device.id, Setting::Appearance, "dark").unwrap();
        set(&device.id, Setting::TextSize, "accessibility-medium").unwrap();
        eprintln!("two sets {:?}", started.elapsed());
        let now = self::settings(&device.id).unwrap();
        assert_eq!(now.get(Setting::Appearance), Some("dark"));
        assert_eq!(now.get(Setting::TextSize), Some("accessibility-medium"));
        set(&device.id, Setting::Appearance, "light").unwrap();
        set(&device.id, Setting::TextSize, "large").unwrap();
        let started = Instant::now();
        set_location(&endpoint, &device.id, Some((35.6762, 139.6503))).unwrap();
        eprintln!("location {:?}", started.elapsed());
        set_location(&endpoint, &device.id, None).unwrap();
        let started = Instant::now();
        let app = foreground_app(&endpoint, &device.id);
        eprintln!("foreground {:?}: {app:?}", started.elapsed());
        let bundle = app
            .ok()
            .flatten()
            .unwrap_or("com.apple.mobilesafari".into());
        let started = Instant::now();
        let raw = hub::serve_sim_cli(&["permissions", "list", &bundle, "-q", "-d", &device.id]);
        eprintln!("permissions {:?}: {raw:?}", started.elapsed());
        eprintln!("parsed {:?}", raw.ok().map(|raw| parse_permissions(&raw)));
        let started = Instant::now();
        eprintln!("restart {:?}", restart_app(&endpoint, &device.id, &bundle));
        eprintln!("restart took {:?}", started.elapsed());
        let logs = logs(&endpoint, &device.id, 0);
        eprintln!(
            "logs {:?}",
            logs.map(|l| (l.len(), l.into_iter().rev().take(5).collect::<Vec<_>>()))
        );
        let path = format!(
            "/vendor/serve-sim/network-capture?device={}",
            hub::encode(&device.id)
        );
        eprintln!(
            "network {:?}",
            sse_events(&endpoint, &path, 4, Duration::from_secs(3))
        );
    }
}
