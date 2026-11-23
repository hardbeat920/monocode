//! Discord Rich Presence for the focused MonoCode session.
//!
//! Speaks Discord's local IPC protocol directly, so no extra runtime or
//! dependency is needed. A worker thread owns the socket: it handshakes on
//! connect, sends SET_ACTIVITY whenever the focused project, harness or turn
//! changes, reconnects while Discord starts and stops, and clears the presence
//! when MonoCode quits.

#[cfg(unix)]
use std::io::Read;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

/// Discord application registered for MonoCode.
const CLIENT_ID: &str = "1574386892118560891";
/// MonoCode's mark, served from the official site. Discord proxies HTTPS asset
/// URLs sent over RPC, so this needs no upload to the Developer Portal.
const LOGO_URL: &str = "https://usemono.dev/icon.png";
const LOGO_TEXT: &str = "MonoCode";
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const CONNECT_RETRY: Duration = Duration::from_secs(5);
const IDLE_WAIT: Duration = Duration::from_secs(1);

/// Set `MONOCODE_DISCORD_DEBUG=1` to trace IPC attempts in the Tauri terminal.
fn debug_enabled() -> bool {
    std::env::var("MONOCODE_DISCORD_DEBUG").as_deref() == Ok("1")
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if debug_enabled() {
            eprintln!("monocode: discord: {}", format_args!($($arg)*));
        }
    };
}

/// `MONOCODE_DISCORD_IPC_PORT` pins one socket so a mock client can run beside
/// a real Discord, which normally owns port 0.
fn ipc_ports() -> Vec<u16> {
    let raw = std::env::var("MONOCODE_DISCORD_IPC_PORT").ok();
    ports_from_override(raw.as_deref())
}

fn ports_from_override(raw: Option<&str>) -> Vec<u16> {
    match raw {
        Some(value) => match value.trim().parse::<u16>() {
            Ok(port) if port < 10 => vec![port],
            _ => (0..10).collect(),
        },
        None => (0..10).collect(),
    }
}

/// `MONOCODE_DISCORD_LOGO_URL` can point at a Discord-hosted asset key (or a
/// different HTTPS URL) instead of the mark on the official site.
fn logo_image() -> String {
    let raw = std::env::var("MONOCODE_DISCORD_LOGO_URL").ok();
    logo_from_override(raw.as_deref()).to_string()
}

fn logo_from_override(raw: Option<&str>) -> &str {
    match raw {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => LOGO_URL,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub state: String,
    pub details: String,
    /// Unix epoch milliseconds; Discord expects the start in ms.
    pub started_at: i64,
}

/// Mirrored by `DiscordPresenceStatus` in src/features/settings/model/discord.ts.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PresenceStatus {
    Disabled,
    Connecting,
    Connected,
    Unavailable,
    Error,
}

struct Inner {
    enabled: bool,
    activity: Option<Activity>,
    status: PresenceStatus,
    /// Bumped on every `set` so a connection attempt can tell whether it was
    /// superseded while it was blocked in connect/handshake.
    generation: u64,
}

struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
}

pub struct DiscordPresence {
    shared: Arc<Shared>,
}

impl DiscordPresence {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    enabled: false,
                    activity: None,
                    status: PresenceStatus::Disabled,
                    generation: 0,
                }),
                wake: Condvar::new(),
            }),
        }
    }

    pub fn start(&self) {
        let shared = Arc::clone(&self.shared);
        let _ = std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || worker(shared));
    }

    pub fn set(&self, enabled: bool, activity: Option<Activity>) -> PresenceStatus {
        let mut inner = lock(&self.shared.inner);
        let turning_on = enabled && !inner.enabled;
        trace!("set enabled={enabled} (was {})", inner.enabled);
        inner.generation = inner.generation.wrapping_add(1);
        inner.enabled = enabled;
        if let Some(activity) = activity {
            inner.activity = Some(activity);
        }
        inner.status = if !enabled {
            PresenceStatus::Disabled
        } else if turning_on {
            PresenceStatus::Connecting
        } else {
            inner.status
        };
        let status = inner.status;
        trace!("status -> {status:?}");
        drop(inner);
        self.shared.wake.notify_all();
        status
    }

    fn status(&self) -> PresenceStatus {
        lock(&self.shared.inner).status
    }
}

fn lock(mutex: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn set_status(shared: &Shared, status: PresenceStatus) {
    lock(&shared.inner).status = status;
}

/// A connect/handshake may block while the user disables presence or changes
/// the activity. Only commit its result if that request is still the current
/// one and presence is still enabled.
fn still_current(shared: &Shared, generation: u64) -> bool {
    let inner = lock(&shared.inner);
    inner.enabled && inner.generation == generation
}

fn wait(shared: &Shared, timeout: Option<Duration>) {
    let inner = lock(&shared.inner);
    match timeout {
        Some(duration) => {
            let _ = shared.wake.wait_timeout(inner, duration);
        }
        None => {
            drop(shared.wake.wait_while(inner, |state| !state.enabled));
        }
    }
}

fn worker(shared: Arc<Shared>) {
    let mut connection: Option<IpcConnection> = None;
    let mut last_sent: Option<String> = None;
    let mut was_enabled = false;
    let mut next_retry = Instant::now();

    loop {
        let (enabled, activity, generation) = {
            let inner = lock(&shared.inner);
            (inner.enabled, inner.activity.clone(), inner.generation)
        };

        if was_enabled && !enabled {
            trace!("clearing presence");
            if let Some(connection) = connection.as_mut() {
                let _ = connection.send(OP_FRAME, &clear_activity_json());
            }
            connection = None;
            last_sent = None;
            next_retry = Instant::now();
        }
        was_enabled = enabled;

        if !enabled {
            wait(&shared, None);
            continue;
        }

        if connection.is_none() && Instant::now() >= next_retry {
            match IpcConnection::connect() {
                Ok(mut connected) => {
                    let handshake = connected.handshake();
                    // Both calls can block. Do not let a stale attempt overwrite
                    // Disabled or publish an activity from an older request.
                    if !still_current(&shared, generation) {
                        trace!("discarding superseded connection");
                        next_retry = Instant::now();
                    } else {
                        match handshake {
                            Ok(()) => {
                                connection = Some(connected);
                                last_sent = None;
                                set_status(&shared, PresenceStatus::Connected);
                            }
                            Err(error) => {
                                trace!("handshake failed: {error}");
                                next_retry = Instant::now() + CONNECT_RETRY;
                                set_status(&shared, PresenceStatus::Unavailable);
                            }
                        }
                    }
                }
                Err(error) => {
                    trace!("connect failed: {error}");
                    if still_current(&shared, generation) {
                        next_retry = Instant::now() + CONNECT_RETRY;
                        set_status(&shared, PresenceStatus::Unavailable);
                    }
                }
            }
        }

        let mut dropped = false;
        if let Some(connection) = connection.as_mut() {
            if !still_current(&shared, generation) {
                // A newer set() arrived after the handshake. Let the next loop
                // publish its activity instead of this captured one.
                trace!("skipping activity for superseded request");
                last_sent = None;
            } else if let Some(activity) = activity.as_ref() {
                // Compare the activity itself: every frame gets a fresh nonce.
                let content = activity_payload(activity).to_string();
                if last_sent.as_deref() != Some(content.as_str()) {
                    trace!(
                        "SET_ACTIVITY state={:?} details={:?}",
                        activity.state,
                        activity.details
                    );
                    match connection.send(OP_FRAME, &activity_json(activity)) {
                        Ok(()) => last_sent = Some(content),
                        Err(error) => {
                            trace!("send failed, will reconnect: {error}");
                            dropped = true;
                            last_sent = None;
                            next_retry = Instant::now() + CONNECT_RETRY;
                            set_status(&shared, PresenceStatus::Error);
                        }
                    }
                }
            }
            if !dropped {
                connection.drain();
            }
        }
        if dropped {
            connection = None;
        }

        wait(&shared, Some(IDLE_WAIT));
    }
}

/// IPC frames are a little-endian opcode, a little-endian length, then JSON.
fn frame(opcode: u32, payload: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload.as_bytes());
    bytes
}

fn handshake_json() -> String {
    serde_json::json!({ "v": 1, "client_id": CLIENT_ID }).to_string()
}

/// The activity body, without the per-frame nonce. Used to decide whether the
/// presence actually changed and should be sent again.
fn activity_payload(activity: &Activity) -> serde_json::Value {
    serde_json::json!({
        "state": activity.state,
        "details": activity.details,
        "timestamps": { "start": activity.started_at },
        "assets": {
            "large_image": logo_image(),
            "large_text": LOGO_TEXT,
        },
        "instance": false,
    })
}

fn activity_json(activity: &Activity) -> String {
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity_payload(activity),
        },
        "nonce": uuid::Uuid::new_v4().to_string(),
    })
    .to_string()
}

fn clear_activity_json() -> String {
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": serde_json::Value::Null,
        },
        "nonce": uuid::Uuid::new_v4().to_string(),
    })
    .to_string()
}

/// XDG_RUNTIME_DIR wins when set; otherwise try the conventional temp folders
/// before falling back to /tmp. Discord uses one of these for its IPC socket.
#[cfg(unix)]
fn ipc_directories(
    xdg: Option<&str>,
    tmpdir: Option<&str>,
    tmp: Option<&str>,
    temp: Option<&str>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut add = |value: Option<&str>| {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            directories.push(PathBuf::from(value));
        }
    };

    if let Some(runtime) = xdg.filter(|value| !value.is_empty()) {
        directories.push(PathBuf::from(runtime));
    } else {
        add(tmpdir);
        add(tmp);
        add(temp);
    }
    directories.push(PathBuf::from("/tmp"));
    directories
}

#[cfg(unix)]
struct IpcConnection {
    stream: UnixStream,
}

#[cfg(unix)]
impl IpcConnection {
    fn connect() -> io::Result<Self> {
        let xdg = std::env::var("XDG_RUNTIME_DIR").ok();
        let tmpdir = std::env::var("TMPDIR").ok();
        let tmp = std::env::var("TMP").ok();
        let temp = std::env::var("TEMP").ok();
        let directories = ipc_directories(
            xdg.as_deref(),
            tmpdir.as_deref(),
            tmp.as_deref(),
            temp.as_deref(),
        );

        let mut error = io::Error::new(io::ErrorKind::NotFound, "Discord IPC not available");
        for directory in directories {
            for port in ipc_ports() {
                let path = directory.join(format!("discord-ipc-{port}"));
                match UnixStream::connect(&path) {
                    Ok(stream) => {
                        trace!("connected to {}", path.display());
                        return Ok(Self { stream });
                    }
                    Err(err) => {
                        trace!("no socket at {}: {err}", path.display());
                        error = err;
                    }
                }
            }
        }
        Err(error)
    }

    fn handshake(&mut self) -> io::Result<()> {
        self.send(OP_HANDSHAKE, &handshake_json())?;
        // Read the READY frame best-effort: a client that never answers still
        // accepts SET_ACTIVITY, so only the write is a hard requirement.
        self.stream
            .set_read_timeout(Some(Duration::from_millis(200)))?;
        let mut header = [0u8; 8];
        if self.stream.read_exact(&mut header).is_ok() {
            let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let mut body = vec![0u8; length.min(64 * 1024)];
            let _ = self.stream.read_exact(&mut body);
        }
        self.stream
            .set_read_timeout(Some(Duration::from_millis(20)))?;
        Ok(())
    }

    fn send(&mut self, opcode: u32, payload: &str) -> io::Result<()> {
        self.stream.write_all(&frame(opcode, payload))
    }

    fn drain(&mut self) {
        let mut scratch = [0u8; 4096];
        while let Ok(count) = self.stream.read(&mut scratch) {
            if count == 0 {
                break;
            }
        }
    }
}

#[cfg(windows)]
struct IpcConnection {
    file: File,
}

#[cfg(windows)]
impl IpcConnection {
    fn connect() -> io::Result<Self> {
        let mut error = io::Error::new(io::ErrorKind::NotFound, "Discord IPC not available");
        for port in ipc_ports() {
            let path = format!(r"\\.\pipe\discord-ipc-{port}");
            match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(file) => {
                    trace!("connected to {path}");
                    return Ok(Self { file });
                }
                Err(err) => {
                    trace!("no pipe at {path}: {err}");
                    error = err;
                }
            }
        }
        Err(error)
    }

    fn handshake(&mut self) -> io::Result<()> {
        // Discord's Windows pipe is byte mode: a blocking read for the
        // handshake reply would stall until Discord writes. SET_ACTIVITY is
        // accepted immediately after the handshake, so only the write counts.
        self.send(OP_HANDSHAKE, &handshake_json())
    }

    fn send(&mut self, opcode: u32, payload: &str) -> io::Result<()> {
        self.file.write_all(&frame(opcode, payload))?;
        self.file.flush()
    }

    fn drain(&mut self) {
        // No nonblocking read on a byte-mode pipe; a failed write reconnects.
    }
}

#[tauri::command]
pub fn discord_presence_set(
    state: State<'_, DiscordPresence>,
    enabled: bool,
    activity: Option<Activity>,
) -> PresenceStatus {
    state.set(enabled, activity)
}

#[tauri::command]
pub fn discord_presence_status(state: State<'_, DiscordPresence>) -> PresenceStatus {
    state.status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_override_pins_one_port_and_falls_back_on_bad_values() {
        assert_eq!(ports_from_override(None), (0..10).collect::<Vec<_>>());
        assert_eq!(ports_from_override(Some(" 9 ")), vec![9]);
        for bad in ["", "10", "-1", "abc"] {
            assert_eq!(ports_from_override(Some(bad)), (0..10).collect::<Vec<_>>());
        }
    }

    #[test]
    fn frame_header_is_little_endian_opcode_and_length() {
        let bytes = frame(OP_HANDSHAKE, "{}");
        assert_eq!(&bytes[0..4], &OP_HANDSHAKE.to_le_bytes());
        assert_eq!(&bytes[4..8], &2u32.to_le_bytes());
        assert_eq!(&bytes[8..], b"{}");
    }

    #[test]
    fn handshake_carries_the_client_id() {
        let value: serde_json::Value =
            serde_json::from_str(&handshake_json()).expect("valid handshake");
        assert_eq!(value["v"], 1);
        assert_eq!(value["client_id"], CLIENT_ID);
    }

    #[test]
    fn activity_payload_uses_discord_command_shape() {
        let payload = activity_json(&Activity {
            state: "state".into(),
            details: "details".into(),
            started_at: 1_700_000_000_000,
        });
        let value: serde_json::Value = serde_json::from_str(&payload).expect("valid activity");
        assert_eq!(value["cmd"], "SET_ACTIVITY");
        assert_eq!(value["args"]["activity"]["state"], "state");
        assert_eq!(value["args"]["activity"]["details"], "details");
        assert_eq!(
            value["args"]["activity"]["timestamps"]["start"],
            1_700_000_000_000u64
        );
        assert_eq!(value["args"]["activity"]["assets"]["large_image"], LOGO_URL);
        assert_eq!(value["args"]["activity"]["assets"]["large_text"], LOGO_TEXT);
        assert!(value["nonce"]
            .as_str()
            .is_some_and(|nonce| !nonce.is_empty()));
    }

    #[test]
    fn logo_override_falls_back_on_empty_values() {
        assert_eq!(logo_from_override(None), LOGO_URL);
        assert_eq!(logo_from_override(Some("  ")), LOGO_URL);
        assert_eq!(logo_from_override(Some(" asset-key ")), "asset-key");
    }

    #[test]
    fn activity_dedup_ignores_the_fresh_nonce() {
        let activity = Activity {
            state: "state".into(),
            details: "details".into(),
            started_at: 1_700_000_000_000,
        };
        let first = activity_json(&activity);
        let second = activity_json(&activity);
        assert_ne!(first, second, "each frame gets its own nonce");
        assert_eq!(
            activity_payload(&activity).to_string(),
            activity_payload(&activity).to_string()
        );
        let value: serde_json::Value = serde_json::from_str(&first).expect("valid activity");
        assert_eq!(value["args"]["activity"], activity_payload(&activity));
    }

    #[test]
    fn superseded_requests_are_not_current() {
        let presence = DiscordPresence::new();
        let initial = lock(&presence.shared.inner).generation;
        assert!(!still_current(&presence.shared, initial));

        presence.set(true, None);
        let current = lock(&presence.shared.inner).generation;
        assert!(still_current(&presence.shared, current));

        presence.set(
            true,
            Some(Activity {
                state: "state".into(),
                details: "details".into(),
                started_at: 1,
            }),
        );
        assert!(!still_current(&presence.shared, current));

        presence.set(false, None);
        let after_disable = lock(&presence.shared.inner).generation;
        assert!(!still_current(&presence.shared, after_disable));
    }

    #[test]
    #[cfg(unix)]
    fn unix_ipc_directories_prefer_xdg_then_temp_then_tmp() {
        assert_eq!(
            ipc_directories(Some("/run/user/1000"), None, None, None),
            vec![PathBuf::from("/run/user/1000"), PathBuf::from("/tmp")]
        );
        assert_eq!(
            ipc_directories(None, Some("/t1"), Some("/t2"), Some("/t3")),
            vec![
                PathBuf::from("/t1"),
                PathBuf::from("/t2"),
                PathBuf::from("/t3"),
                PathBuf::from("/tmp"),
            ]
        );
        assert_eq!(
            ipc_directories(Some(""), Some("/t1"), None, None),
            vec![PathBuf::from("/t1"), PathBuf::from("/tmp")]
        );
        assert_eq!(
            ipc_directories(None, Some(""), Some("/t2"), None),
            vec![PathBuf::from("/t2"), PathBuf::from("/tmp")]
        );
    }

    #[test]
    fn clearing_sends_a_null_activity() {
        let value: serde_json::Value =
            serde_json::from_str(&clear_activity_json()).expect("valid clear");
        assert_eq!(value["cmd"], "SET_ACTIVITY");
        assert!(value["args"]["activity"].is_null());
    }
}
