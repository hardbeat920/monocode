//! Supervises expo-device-hub: a Node child on loopback that lists, boots and
//! streams simulators and emulators. It is a separate process because
//! serve-sim loads private CoreSimulator frameworks through a native addon,
//! and a crash there must not take the app down.
//!
//! The hub starts on first use (the Device tab or an agent's device tool), not
//! with the app, and stops after it has gone unused for a while. It requires a
//! token: serve-sim can run shell commands, so nothing else on this machine
//! gets to drive it.
use super::{toolchain, Device, DeviceList, Platform};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

/// The most frames a second the hub encodes. Simulators draw at 60, but an
/// encoder paced at exactly 60 beats against them and drops a frame in four;
/// at 120 it takes each one as it comes (measured: median gap 21 → 16.5 ms).
/// It encodes only changed frames, so a still screen costs nothing more.
pub const VIDEO_FPS: u32 = 120;
/// Taller than a phone in the panel is drawn on a Retina display. Larger
/// costs Android's software encoder its frame rate (32 fps at 2048, 40 at
/// 1600) for detail the panel cannot show.
const MAX_DIMENSION: u32 = 1600;
/// An unwatched, unused hub stops after this long.
const IDLE: Duration = Duration::from_secs(10 * 60);
const READY_TIMEOUT: Duration = Duration::from_secs(45);

/// How to reach a running hub.
#[derive(Debug)]
pub struct Endpoint {
    pub port: u16,
    token: String,
}

impl Endpoint {
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin())
    }
    pub fn ws_url(&self, path: &str) -> String {
        format!("ws://127.0.0.1:{}{path}", self.port)
    }
    /// The header every request carries; never shown to agents.
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }
    pub fn get(&self, path: &str, timeout: Duration) -> Result<ureq::Response, String> {
        touch();
        response(
            ureq::get(&self.url(path))
                .set("Authorization", &self.authorization())
                .timeout(timeout)
                .call(),
        )
    }
    pub fn post(
        &self,
        path: &str,
        body: Value,
        timeout: Duration,
    ) -> Result<ureq::Response, String> {
        touch();
        response(
            ureq::post(&self.url(path))
                .set("Authorization", &self.authorization())
                .timeout(timeout)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
        )
    }
    pub fn post_empty(&self, path: &str, timeout: Duration) -> Result<ureq::Response, String> {
        touch();
        response(
            ureq::post(&self.url(path))
                .set("Authorization", &self.authorization())
                .timeout(timeout)
                .call(),
        )
    }
}

fn response(result: Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, String> {
    match result {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| body.chars().take(300).collect());
            Err(format!("Device Hub answered {code}: {message}"))
        }
        Err(error) => Err(format!("Device Hub unreachable: {error}")),
    }
}

/// Where the hub is in its life, for the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Stopped,
    Starting,
    Running,
    Failed(String),
}

struct Running {
    child: Child,
    endpoint: Arc<Endpoint>,
    /// The hub's recent output, for an error that needs it.
    log: Arc<Mutex<Vec<String>>>,
}

struct Supervisor {
    running: Option<Running>,
    state: State,
}

fn supervisor() -> &'static (Mutex<Supervisor>, Condvar) {
    static HUB: OnceLock<(Mutex<Supervisor>, Condvar)> = OnceLock::new();
    HUB.get_or_init(|| {
        std::thread::Builder::new()
            .name("device-hub-idle".into())
            .spawn(idle_watch)
            .ok();
        (
            Mutex::new(Supervisor {
                running: None,
                state: State::Stopped,
            }),
            Condvar::new(),
        )
    })
}

static LAST_USE: AtomicU64 = AtomicU64::new(0);
static VIEWERS: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn epoch() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

/// Counts as use, holding off the idle stop.
pub fn touch() {
    LAST_USE.store(epoch().elapsed().as_secs(), Ordering::Relaxed);
}

/// Held while a viewer streams; the hub never idles out from under one.
pub struct Viewer(());
impl Viewer {
    pub fn new() -> Self {
        VIEWERS.fetch_add(1, Ordering::Relaxed);
        touch();
        Self(())
    }
}
impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        VIEWERS.fetch_sub(1, Ordering::Relaxed);
        touch();
    }
}

fn idle_watch() {
    loop {
        std::thread::sleep(Duration::from_secs(30));
        let idle = epoch()
            .elapsed()
            .as_secs()
            .saturating_sub(LAST_USE.load(Ordering::Relaxed));
        if VIEWERS.load(Ordering::Relaxed) == 0 && idle >= IDLE.as_secs() {
            let (lock, _) = supervisor();
            let running = lock
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .running
                .is_some();
            if running {
                stop();
            }
        }
    }
}

pub fn state() -> State {
    let (lock, _) = supervisor();
    let mut hub = lock.lock().unwrap_or_else(|e| e.into_inner());
    // A hub that exited by itself is reported, not silently restarted.
    let exited = hub.running.as_mut().and_then(|running| {
        let status = running.child.try_wait().ok().flatten()?;
        let log = running
            .log
            .lock()
            .map(|log| log.join("\n"))
            .unwrap_or_default();
        Some(exit_message(status, &log))
    });
    if let Some(message) = exited {
        hub.running = None;
        hub.state = State::Failed(message);
    }
    hub.state.clone()
}

/// Bumped whenever the hub starts or stops, so viewers reconnect.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn exit_message(status: std::process::ExitStatus, log: &str) -> String {
    let tail: Vec<&str> = log.lines().rev().take(8).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    format!("The Device Hub stopped ({status}).\n{}", tail.join("\n"))
}

/// The running hub, without starting one.
pub fn running() -> Option<Arc<Endpoint>> {
    if !matches!(state(), State::Running) {
        return None;
    }
    let (lock, _) = supervisor();
    let hub = lock.lock().unwrap_or_else(|e| e.into_inner());
    hub.running.as_ref().map(|running| running.endpoint.clone())
}

/// Blocking: the running hub, started (and installed) if need be. Callers
/// on the UI thread must run this in the background.
pub fn ensure() -> Result<Arc<Endpoint>, String> {
    if !super::enabled() {
        return Err("Devices are turned off. Turn them on in Settings › Devices.".into());
    }
    touch();
    let (lock, ready) = supervisor();
    let mut hub = lock.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(running) = hub.running.as_mut() {
            match running.child.try_wait() {
                Ok(None) => return Ok(running.endpoint.clone()),
                _ => hub.running = None,
            }
        }
        if hub.state != State::Starting {
            break;
        }
        // Another caller is starting it; wait for that rather than racing.
        hub = ready.wait(hub).unwrap_or_else(|e| e.into_inner());
    }
    hub.state = State::Starting;
    drop(hub);
    let started = start();
    let mut hub = lock.lock().unwrap_or_else(|e| e.into_inner());
    let result = match started {
        Ok(running) => {
            let endpoint = running.endpoint.clone();
            hub.running = Some(running);
            hub.state = State::Running;
            GENERATION.fetch_add(1, Ordering::Relaxed);
            Ok(endpoint)
        }
        Err(error) => {
            hub.state = State::Failed(error.clone());
            Err(error)
        }
    };
    ready.notify_all();
    result
}

/// Stops the hub and the simulator helpers it started. Devices stay booted.
pub fn stop() {
    let (lock, _) = supervisor();
    let running = {
        let mut hub = lock.lock().unwrap_or_else(|e| e.into_inner());
        hub.state = State::Stopped;
        hub.running.take()
    };
    if let Some(mut running) = running {
        terminate(&mut running.child);
        GENERATION.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::remove_file(state_file());
    }
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        // The hub leads its own group, with serve-sim's helpers in it.
        let group = -(child.id() as i32);
        libc::kill(group, libc::SIGTERM);
        for _ in 0..20 {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        libc::kill(group, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn state_file() -> std::path::PathBuf {
    toolchain::root().join("hub.json")
}

/// A hub a crashed MonoCode left running holds simulator captures; stop it.
/// Only a process still running this app's hub entry is touched.
pub fn reap_orphan() {
    let Ok(text) = std::fs::read_to_string(state_file()) else {
        return;
    };
    let _ = std::fs::remove_file(state_file());
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let (Some(pid), Some(owner)) = (value["pid"].as_u64(), value["owner"].as_u64()) else {
        return;
    };
    #[cfg(unix)]
    {
        if owner as u32 == std::process::id() || unsafe { libc::kill(owner as i32, 0) } == 0 {
            return;
        }
        let Ok(output) = Command::new("/bin/ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
        else {
            return;
        };
        let command = String::from_utf8_lossy(&output.stdout);
        if command.contains(&*toolchain::Tool::Hub.entry_path().to_string_lossy()) {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (pid, owner);
}

/// The hub's flags. Android capture prefers scrcpy, then the emulator's
/// shared-memory frames encoded in hardware: the default software x264 of
/// raw frames costs a CPU core.
fn arguments(entry: &std::path::Path, port: u16) -> Vec<String> {
    let mut args = vec![
        entry.to_string_lossy().into_owned(),
        "--port".into(),
        port.to_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--require-token".into(),
        "--hide-sidebar".into(),
        "--hide-boot-device".into(),
        "--transport".into(),
        "h264".into(),
        "--video-fps".into(),
        VIDEO_FPS.to_string(),
        "--max-dimension".into(),
        MAX_DIMENSION.to_string(),
        "--encoder".into(),
        "hardware".into(),
    ];
    // The hub ships scrcpy's server and pushes it over adb, so this needs
    // nothing installed. It shows an emulator's first frame in about 0.5 s
    // where the emulator's gRPC screenshots take 1.3 to 1.7.
    args.extend(["--stream-source".into(), "scrcpy".into()]);
    if !cfg!(target_os = "macos") {
        args.extend(["--platform".into(), "android".into()]);
    }
    if let Ok(extra) = std::env::var("MONOCODE_DEVICE_HUB_ARGS") {
        // For measuring other settings; see the live stream test.
        args.extend(extra.split_whitespace().map(String::from));
    }
    args
}

/// Flags for emulators the hub starts. It starts them without a window, and
/// headless `-gpu auto` picks SwiftShader, which draws Android on the CPU:
/// the launcher managed 25 fps with three frames in four janky. On the Mac's
/// GPU it draws at the display's 60 with 6 to 18% janky.
fn emulator_args() -> String {
    let own = std::env::var("EXPO_DEVICE_HUB_EMULATOR_EXTRA_ARGS").unwrap_or_default();
    // The emulator takes the last `-gpu`, so the person's own still wins.
    format!("-gpu host {own}").trim().to_owned()
}

/// Reads `Local: http://localhost:PORT/?token=TOKEN` from the startup banner.
fn parse_banner(line: &str) -> Option<(u16, String)> {
    let rest = line.trim().strip_prefix("Local:")?.trim();
    let url = url::Url::parse(rest).ok()?;
    let port = url.port()?;
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.into_owned())?;
    Some((port, token))
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    Ok(listener.local_addr().map_err(|e| e.to_string())?.port())
}

fn start() -> Result<Running, String> {
    reap_orphan();
    let entry = toolchain::install(toolchain::Tool::Hub)?;
    // Only the paths: the full doctor check spawns two processes.
    let node = toolchain::node().ok_or("MonoCode needs Node.js to run the Device Hub.")?;
    let android_sdk = toolchain::android_sdk();
    let port = free_port()?;
    let state_dir = toolchain::root().join("hub-state");
    std::fs::create_dir_all(&state_dir).map_err(|e| e.to_string())?;
    let mut command = Command::new(&node);
    command
        .args(arguments(&entry, port))
        .current_dir(toolchain::root())
        .env("PATH", toolchain::search_path())
        .env("EXPO_DEVICE_HUB_STATE_DIR", &state_dir)
        .env("FORCE_COLOR", "0")
        .env("NO_COLOR", "1")
        .env("EXPO_DEVICE_HUB_EMULATOR_EXTRA_ARGS", emulator_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(sdk) = &android_sdk {
        command.env("ANDROID_HOME", sdk);
    }
    crate::process::configure_child(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start the Device Hub: {e}"))?;
    let log = Arc::new(Mutex::new(Vec::new()));
    let (banner_tx, banner_rx) = std::sync::mpsc::channel();
    let forward = |stream: Box<dyn Read + Send>,
                   banner: Option<std::sync::mpsc::Sender<(u16, String)>>| {
        let log = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if let Some(found) = banner.as_ref().and(parse_banner(&line)) {
                    let banner = banner.as_ref().expect("checked above");
                    let _ = banner.send(found);
                    // The banner carries the token; keep it out of the log.
                    continue;
                }
                if std::env::var_os("MONOCODE_DEVICE_HUB_LOG").is_some() {
                    eprintln!("hub| {line}");
                }
                if let Ok(mut log) = log.lock() {
                    if log.len() >= 64 {
                        log.remove(0);
                    }
                    log.push(line);
                }
            }
        });
    };
    if let Some(stdout) = child.stdout.take() {
        forward(Box::new(stdout), Some(banner_tx));
    }
    if let Some(stderr) = child.stderr.take() {
        forward(Box::new(stderr), None);
    }
    let fail = |child: &mut Child, message: String| {
        terminate(child);
        let tail = log
            .lock()
            .map(|log| {
                log.iter()
                    .rev()
                    .take(8)
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if tail.is_empty() {
            message
        } else {
            format!("{message}\n{tail}")
        }
    };
    let (port, token) = match banner_rx.recv_timeout(READY_TIMEOUT) {
        Ok(found) => found,
        Err(_) => {
            let message = match child.try_wait() {
                Ok(Some(status)) => format!("The Device Hub exited at startup ({status})."),
                _ => "The Device Hub did not start in time.".into(),
            };
            return Err(fail(&mut child, message));
        }
    };
    let endpoint = Arc::new(Endpoint { port, token });
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let ready = ureq::get(&endpoint.url("/readyz"))
            .timeout(Duration::from_secs(2))
            .call()
            .is_ok();
        if ready {
            break;
        }
        if Instant::now() > deadline || child.try_wait().ok().flatten().is_some() {
            return Err(fail(
                &mut child,
                "The Device Hub never became ready.".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = std::fs::write(
        state_file(),
        json!({"pid": child.id(), "owner": std::process::id(), "port": port}).to_string(),
    );
    Ok(Running {
        child,
        endpoint,
        log,
    })
}

/// Blocking: the hub's device list.
pub fn list() -> Result<DeviceList, String> {
    let endpoint = ensure()?;
    let response = endpoint.get("/api/devices", Duration::from_secs(20))?;
    let value: Value = serde_json::from_str(&response.into_string().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let list = DeviceList::from_hub(&value)?;
    *LAST_LIST.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.clone());
    Ok(list)
}

/// The list the hub last gave, to show while a fresh one loads: asking the
/// hub runs simctl and adb, which takes half a second or more.
static LAST_LIST: Mutex<Option<DeviceList>> = Mutex::new(None);

pub fn cached_list() -> Option<DeviceList> {
    LAST_LIST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Blocking, once per run: when a simulator or emulator is already running,
/// starts the hub and readies booted simulators' streams, so the Device tab
/// and agents' device tools find them ready instead of spending seconds on
/// it. With nothing running it starts nothing; the hub still stops after its
/// idle time.
pub fn prewarm() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !super::enabled() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    if !toolchain::Tool::Hub.installed() || !anything_booted() {
        return;
    }
    let Ok(endpoint) = ensure() else {
        return;
    };
    let Ok(list) = list() else {
        return;
    };
    for device in &list.devices {
        if device.platform == Platform::Ios && device.booted && device.supported {
            let _ = attach(&endpoint, &device.id);
        }
    }
}

/// Whether a simulator or emulator runs, without starting the hub or adb.
fn anything_booted() -> bool {
    #[cfg(target_os = "macos")]
    {
        let simulator = Command::new("/usr/bin/xcrun")
            .args(["simctl", "list", "devices", "booted", "--json"])
            .output()
            .ok()
            .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
            .is_some_and(|value| {
                value["devices"].as_object().is_some_and(|runtimes| {
                    runtimes
                        .values()
                        .any(|d| d.as_array().is_some_and(|d| !d.is_empty()))
                })
            });
        if simulator {
            return true;
        }
    }
    Command::new("/usr/bin/pgrep")
        .args(["-f", "qemu-system"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn vendor(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios => "/vendor/serve-sim",
        Platform::Android => "/vendor/serve-emu",
    }
}

pub fn vendor_path(platform: Platform, path: &str) -> String {
    format!("{}{path}", vendor(platform))
}

fn checked(response: ureq::Response, action: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(&response.into_string().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["ok"] == false {
        let mut detail = vec![value["error"].as_str().unwrap_or("").to_owned()];
        for error in value["errors"].as_array().into_iter().flatten() {
            for key in ["message", "error"] {
                if let Some(text) = error[key].as_str() {
                    detail.push(text.to_owned());
                }
            }
        }
        let detail = detail
            .into_iter()
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(if detail.is_empty() {
            format!("Could not {action} the device.")
        } else {
            detail.chars().take(2000).collect()
        });
    }
    Ok(value)
}

/// Blocking: boots a device and, for a simulator, attaches its stream helper.
/// Returns the id the device has once booted (an emulator's serial).
pub fn boot(device: &Device) -> Result<String, String> {
    if device.booted && device.physical {
        return Ok(device.id.clone());
    }
    device.ensure_virtual("start")?;
    let endpoint = ensure()?;
    let body = json!({"platform": device.platform, "id": device.id, "name": device.name});
    let value = checked(
        endpoint.post("/api/devices/boot", body, Duration::from_secs(180))?,
        "boot",
    )?;
    let id = value["serial"]
        .as_str()
        .or_else(|| value["id"].as_str())
        .unwrap_or(&device.id)
        .to_owned();
    if device.platform == Platform::Ios {
        attach(&endpoint, &id)?;
    }
    Ok(id)
}

/// Booting alone does not start serve-sim's helper for a simulator; the grid
/// start does both and is idempotent for a booted one. It takes 1.5 s or
/// more even then, so each simulator is attached once per hub run, until
/// `forget_attached` says its helper is gone.
pub fn attach(endpoint: &Endpoint, udid: &str) -> Result<(), String> {
    let generation = generation();
    let known = |attached: &(u64, Vec<String>)| {
        attached.0 == generation && attached.1.iter().any(|id| id == udid)
    };
    if known(&ATTACHED.lock().unwrap_or_else(|e| e.into_inner())) {
        return Ok(());
    }
    checked(
        endpoint.post(
            &vendor_path(Platform::Ios, "/grid/api/start"),
            json!({"udid": udid}),
            Duration::from_secs(180),
        )?,
        "attach",
    )?;
    let mut attached = ATTACHED.lock().unwrap_or_else(|e| e.into_inner());
    if attached.0 != generation {
        *attached = (generation, Vec::new());
    }
    if !known(&attached) {
        attached.1.push(udid.to_owned());
    }
    Ok(())
}

/// The simulators attached in the hub run of the given generation.
static ATTACHED: Mutex<(u64, Vec<String>)> = Mutex::new((0, Vec::new()));

/// The simulator's stream helper is gone (it shut down, or its stream could
/// not be opened); the next `attach` starts it again.
pub fn forget_attached(udid: &str) {
    ATTACHED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .1
        .retain(|id| id != udid);
}

/// Blocking: shuts a device down. serve-sim's own shutdown also closes its
/// capture session, which the hub's generic one can leave cached.
pub fn shutdown(device: &Device) -> Result<(), String> {
    device.ensure_virtual("shut down")?;
    let endpoint = ensure()?;
    forget_attached(&device.id);
    let result = match device.platform {
        Platform::Ios => endpoint.post(
            &vendor_path(Platform::Ios, "/grid/api/shutdown"),
            json!({"udid": device.id}),
            Duration::from_secs(60),
        ),
        Platform::Android => endpoint.post(
            "/api/devices/shutdown",
            json!({"platform": device.platform, "id": device.id, "name": device.name}),
            Duration::from_secs(60),
        ),
    };
    match result.and_then(|response| checked(response, "shut down")) {
        Ok(_) => Ok(()),
        // serve-sim runs simctl bare, which fails for a simulator already off.
        Err(_) if list().is_ok_and(|list| list.find(&device.id).is_some_and(|d| !d.booted)) => {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// One model a new device can be, such as "iPhone 17 Pro" or "Pixel 9".
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOption {
    pub id: String,
    pub name: String,
}

/// An iOS version or Android system image already on this Mac, with the
/// models it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeOption {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelOption>,
}

/// What new devices can be made from what this Mac has. Only models the
/// hub streams are offered: iPhones and Pixel phones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewDeviceOptions {
    pub ios: Vec<RuntimeOption>,
    pub android: Vec<RuntimeOption>,
    pub errors: Vec<String>,
}

impl NewDeviceOptions {
    pub fn from_hub(value: &Value) -> Self {
        let runtimes = |platform: Platform| -> Vec<RuntimeOption> {
            let key = match platform {
                Platform::Ios => "ios",
                Platform::Android => "android",
            };
            value[key]["runtimes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|runtime| {
                    let id = runtime["value"].as_str()?.to_owned();
                    let mut models: Vec<ModelOption> = runtime["models"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|model| model["supported"].as_bool().unwrap_or(false))
                        .filter_map(|model| {
                            Some(ModelOption {
                                id: model["value"].as_str()?.to_owned(),
                                name: model["label"].as_str()?.to_owned(),
                            })
                        })
                        .collect();
                    if models.is_empty() {
                        return None;
                    }
                    // The hub lists Pixels oldest first; newest first, as iOS.
                    if platform == Platform::Android {
                        models.sort_by(|a, b| {
                            model_number(&b.name)
                                .cmp(&model_number(&a.name))
                                .then(a.name.cmp(&b.name))
                        });
                    }
                    let name = match platform {
                        Platform::Ios => runtime["label"].as_str().unwrap_or(&id).to_owned(),
                        Platform::Android => android_image_name(&id),
                    };
                    Some(RuntimeOption { id, name, models })
                })
                .collect()
        };
        let errors = value["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|error| error["message"].as_str())
            .filter(|message| !message.starts_with("[android-utils] "))
            .map(str::to_owned)
            .collect();
        Self {
            ios: runtimes(Platform::Ios),
            android: runtimes(Platform::Android),
            errors,
        }
    }
    pub fn runtimes(&self, platform: Platform) -> &[RuntimeOption] {
        match platform {
            Platform::Ios => &self.ios,
            Platform::Android => &self.android,
        }
    }
}

/// The first number in a model's name: "Pixel 10 Pro" is 10, "Pixel" is 0.
fn model_number(name: &str) -> u32 {
    name.split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
        .unwrap_or(0)
}

/// "system-images;android-36;google_apis;arm64-v8a" as "Android 16 · Google APIs".
pub fn android_image_name(package: &str) -> String {
    let mut parts = package.split(';').skip(1);
    let api = parts
        .next()
        .and_then(|level| level.strip_prefix("android-"))
        .unwrap_or_default();
    let release = match api {
        "36" => "Android 16".to_owned(),
        "35" => "Android 15".to_owned(),
        "34" => "Android 14".to_owned(),
        "33" => "Android 13".to_owned(),
        "32" => "Android 12L".to_owned(),
        "31" => "Android 12".to_owned(),
        "30" => "Android 11".to_owned(),
        "29" => "Android 10".to_owned(),
        "" => "Android".to_owned(),
        other => format!("Android API {other}"),
    };
    let flavor = match parts.next().unwrap_or_default() {
        "google_apis" => "Google APIs",
        "google_apis_playstore" => "Google Play",
        "default" => "AOSP",
        "" => return release,
        other => return format!("{release} · {}", other.replace('_', " ")),
    };
    format!("{release} · {flavor}")
}

/// A name the platform accepts: an Android emulator's may hold only
/// letters, digits, `.`, `_` and `-`.
pub fn device_name(platform: Platform, wanted: &str) -> String {
    let wanted = wanted.trim();
    match platform {
        Platform::Ios => wanted.to_owned(),
        Platform::Android => {
            let mut name = String::new();
            for c in wanted.chars() {
                let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                };
                if !(c == '_' && name.ends_with('_')) {
                    name.push(c);
                }
            }
            name.trim_matches('_').to_owned()
        }
    }
}

/// `base`, or `base 2`, `base 3`… (`base_2` on Android) if a device on that
/// platform has the name already.
pub fn unique_name(platform: Platform, base: &str, list: &DeviceList) -> String {
    let base = device_name(platform, base);
    let taken = |name: &str| {
        list.devices
            .iter()
            .any(|d| d.platform == platform && d.name.eq_ignore_ascii_case(name))
    };
    if !taken(&base) {
        return base;
    }
    let separator = if platform == Platform::Android {
        "_"
    } else {
        " "
    };
    (2..)
        .map(|n| format!("{base}{separator}{n}"))
        .find(|name| !taken(name))
        .expect("an unused name")
}

/// Blocking: what new devices can be made.
pub fn new_device_options() -> Result<NewDeviceOptions, String> {
    let endpoint = ensure()?;
    let response = endpoint.get("/api/new-device-options", Duration::from_secs(60))?;
    let value: Value = serde_json::from_str(&response.into_string().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(NewDeviceOptions::from_hub(&value))
}

/// Blocking: makes a device and starts it, as the hub always does. Returns
/// it booted, with the id it streams under (an emulator's serial).
pub fn create(
    platform: Platform,
    name: &str,
    runtime: &RuntimeOption,
    model: &str,
) -> Result<Device, String> {
    let name = device_name(platform, name);
    if name.is_empty() {
        return Err("Give the device a name.".into());
    }
    let endpoint = ensure()?;
    let body =
        json!({"platform": platform, "name": name, "runtime": runtime.id, "deviceType": model});
    // Creating an emulator includes its first boot, which can take minutes.
    let value = checked(
        endpoint.post("/api/devices/create", body, Duration::from_secs(300))?,
        "create",
    )
    .map_err(|error| explain_create_failure(platform, &error))?;
    let id = value["serial"]
        .as_str()
        .or_else(|| value["id"].as_str())
        .map(str::to_owned)
        .ok_or("The Device Hub did not say which device it made.")?;
    if platform == Platform::Ios {
        attach(&endpoint, &id)?;
    }
    Ok(Device {
        id,
        name,
        version: match platform {
            Platform::Ios => runtime.name.clone(),
            Platform::Android => runtime
                .name
                .split(" · ")
                .next()
                .unwrap_or_default()
                .to_owned(),
        },
        platform,
        booted: true,
        physical: false,
        supported: true,
    })
}

/// About what an emulator needs free for its data partition.
const EMULATOR_SPACE: u64 = 12 * 1024 * 1024 * 1024;

/// The hub's error, without the emulator command line it suggests running.
/// An emulator that was made but could not start usually lacked disk space,
/// which the hub does not pass on; say so when the disk is short.
fn explain_create_failure(platform: Platform, error: &str) -> String {
    let error = error
        .split("\n\nFor details")
        .next()
        .unwrap_or(error)
        .trim();
    let short = (platform == Platform::Android && error.contains("before coming online"))
        .then(|| free_space(&avd_home()))
        .flatten()
        .filter(|free| *free < EMULATOR_SPACE + 1024 * 1024 * 1024);
    if let Some(free) = short {
        return format!(
            "The emulator was made but could not start: Android emulators need about 12 GB of free disk space, and this Mac has {:.1} GB. Free some space, then start it from the shelf.",
            free as f64 / 1e9
        );
    }
    error.to_owned()
}

fn avd_home() -> std::path::PathBuf {
    std::env::var_os("ANDROID_AVD_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| crate::accounts::home().map(|home| home.join(".android/avd")))
        .unwrap_or_default()
}

/// Bytes free on the volume holding `path`.
#[cfg(not(unix))]
fn free_space(_: &std::path::Path) -> Option<u64> {
    None
}
#[cfg(unix)]
fn free_space(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: statvfs fills the buffer on success and reads only the path.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the call succeeded, so the buffer is initialized.
    let stat = unsafe { stat.assume_init() };
    // The field types differ between platforms.
    #[allow(clippy::unnecessary_cast)]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

/// Blocking: shuts a device down and deletes it, with its apps and data.
/// An emulator is deleted by its AVD name, whatever serial it runs under.
pub fn remove(device: &Device) -> Result<(), String> {
    device.ensure_virtual("delete")?;
    let endpoint = ensure()?;
    let body = json!({"platform": device.platform, "id": device.id, "name": device.name});
    checked(
        endpoint.post("/api/devices/remove", body, Duration::from_secs(90))?,
        "delete",
    )
    .map(|_| ())
}

/// Blocking: a PNG of the device's screen.
pub fn screenshot(platform: Platform, id: &str) -> Result<Vec<u8>, String> {
    let endpoint = ensure()?;
    let path = vendor_path(platform, &format!("/api/screenshot?device={}", encode(id)));
    let response = endpoint.post_empty(&path, Duration::from_secs(20))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(32 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("The device returned an empty screenshot.".into());
    }
    Ok(bytes)
}

/// Blocking: runs serve-sim's own command line with the hub's Node, and
/// returns what it printed.
///
/// The hub's exec socket runs most simulator settings (`ui`, `permissions`)
/// through serve-sim's command line, but inside the hub it resolves that to
/// the hub's own `cli.mjs`, which rejects them. Running serve-sim's script
/// directly does the same work. It needs no running hub.
pub fn serve_sim_cli(args: &[&str]) -> Result<String, String> {
    let node = toolchain::node().ok_or("MonoCode needs Node.js to change simulator settings.")?;
    let script = toolchain::Tool::Hub
        .install_dir()
        .join("node_modules/expo-device-hub/vendor/serve-sim/dist/serve-sim.js");
    if !script.is_file() {
        return Err("The Device Hub is not installed.".into());
    }
    touch();
    let mut command = Command::new(&node);
    command
        .arg(&script)
        .args(args)
        .current_dir(toolchain::root())
        .env("PATH", toolchain::search_path())
        .env("FORCE_COLOR", "0")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    crate::process::configure_child(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("Could not run serve-sim: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(if message.is_empty() {
        format!("serve-sim {} failed.", args.first().unwrap_or(&""))
    } else {
        message.lines().take(6).collect::<Vec<_>>().join("\n")
    })
}

/// Percent-encodes a device id for a query string.
pub fn encode(id: &str) -> String {
    url::form_urlencoded::byte_serialize(id.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_device_options_keep_streamable_models_newest_first() {
        let options = NewDeviceOptions::from_hub(&json!({
            "ios": {"runtimes": [{"value": "rt.iOS-27-0", "label": "iOS 27.0", "models": [
                {"value": "t.iPhone-18-Pro", "label": "iPhone 18 Pro", "supported": true},
                {"value": "t.iPad-Pro", "label": "iPad Pro", "supported": false},
            ]}, {"value": "rt.watch", "label": "watchOS 12", "models": [
                {"value": "t.watch", "label": "Watch", "supported": false},
            ]}]},
            "android": {"runtimes": [{"value": "system-images;android-36;google_apis;arm64-v8a", "label": "x", "models": [
                {"value": "pixel", "label": "Pixel", "supported": true},
                {"value": "pixel_9", "label": "Pixel 9", "supported": true},
                {"value": "pixel_10", "label": "Pixel 10", "supported": true},
                {"value": "pixel_10_pro", "label": "Pixel 10 Pro", "supported": true},
                {"value": "pixel_tablet", "label": "Pixel Tablet", "supported": false},
            ]}]},
            "errors": [{"message": "[android-utils] no sdk"}, {"message": "Xcode is busy"}],
        }));
        assert_eq!(
            options.ios.len(),
            1,
            "a runtime with nothing to stream is left out"
        );
        assert_eq!(options.ios[0].models.len(), 1);
        let android = &options.android[0];
        assert_eq!(android.name, "Android 16 · Google APIs");
        let names: Vec<&str> = android.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Pixel 10", "Pixel 10 Pro", "Pixel 9", "Pixel"]);
        assert_eq!(options.errors, ["Xcode is busy"]);
    }

    #[test]
    fn create_failures_drop_the_command_line() {
        let error = "Failed to create iOS simulator X\n\nFor details, try running it manually:\n/opt/emulator -avd X";
        assert_eq!(
            explain_create_failure(Platform::Ios, error),
            "Failed to create iOS simulator X"
        );
        #[cfg(unix)]
        assert!(free_space(std::path::Path::new("/")).is_some_and(|free| free > 0));
    }

    #[test]
    fn android_names_are_sanitized_and_names_kept_unique() {
        assert_eq!(
            device_name(Platform::Android, " Pixel 9 Pro (work) "),
            "Pixel_9_Pro_work"
        );
        assert_eq!(
            device_name(Platform::Ios, " iPhone 17 Pro "),
            "iPhone 17 Pro"
        );
        assert_eq!(
            android_image_name("system-images;android-35;google_apis_playstore;arm64-v8a"),
            "Android 15 · Google Play"
        );
        assert_eq!(
            android_image_name("system-images;android-40;default;x86_64"),
            "Android API 40 · AOSP"
        );
        let device = |platform, name: &str| Device {
            id: name.into(),
            name: name.into(),
            version: String::new(),
            platform,
            booted: false,
            physical: false,
            supported: true,
        };
        let list = DeviceList {
            devices: vec![
                device(Platform::Ios, "iPhone 17 Pro"),
                device(Platform::Ios, "iPhone 17 Pro 2"),
                device(Platform::Android, "Pixel_9"),
            ],
            errors: vec![],
        };
        assert_eq!(
            unique_name(Platform::Ios, "iPhone 17 Pro", &list),
            "iPhone 17 Pro 3"
        );
        assert_eq!(
            unique_name(Platform::Android, "Pixel 9", &list),
            "Pixel_9_2"
        );
        assert_eq!(unique_name(Platform::Ios, "Pixel 9", &list), "Pixel 9");
    }

    /// Makes a throwaway device with the real hub, checks it is listed and
    /// running, then deletes it: `MONOCODE_DEVICE_PLATFORM=android` for an
    /// emulator. `cargo test --lib live_create -- --ignored --nocapture`.
    #[test]
    #[ignore = "creates and deletes a real simulator or emulator"]
    fn live_create_and_remove() {
        crate::device::set_enabled(true, false);
        let platform = match std::env::var("MONOCODE_DEVICE_PLATFORM").as_deref() {
            Ok("android") => Platform::Android,
            _ => Platform::Ios,
        };
        let options = new_device_options().expect("options");
        let runtime = options
            .runtimes(platform)
            .first()
            .expect("a runtime")
            .clone();
        let model = runtime
            .models
            .iter()
            .find(|m| m.name.contains("17e") || m.name == "Pixel 9")
            .unwrap_or(&runtime.models[0])
            .clone();
        let name = unique_name(platform, "MonoCode Check", &list().unwrap());
        let started = Instant::now();
        let device = create(platform, &name, &runtime, &model.id).expect("created");
        eprintln!(
            "created {} ({}) as {} in {:?}",
            device.name,
            model.name,
            device.id,
            started.elapsed()
        );
        let listed = list().unwrap();
        let entry = listed
            .devices
            .iter()
            .find(|d| d.platform == platform && d.name == device.name)
            .expect("listed")
            .clone();
        assert!(entry.booted, "it starts as soon as it is made");
        let png = screenshot(platform, &device.id).expect("a screenshot");
        eprintln!("screenshot {} bytes", png.len());
        let started = Instant::now();
        remove(&entry).expect("deleted");
        eprintln!("deleted in {:?}", started.elapsed());
        assert!(
            !list()
                .unwrap()
                .devices
                .iter()
                .any(|d| d.platform == platform && d.name == device.name),
            "gone from the list"
        );
        stop();
    }

    /// Prints what the hub offers for new devices:
    /// `cargo test --lib live_options -- --ignored --nocapture`.
    #[test]
    #[ignore = "installs and runs the real Device Hub"]
    fn live_options_print() {
        crate::device::set_enabled(true, false);
        let endpoint = ensure().expect("the hub starts");
        let raw = endpoint
            .get("/api/new-device-options", Duration::from_secs(60))
            .unwrap()
            .into_string()
            .unwrap();
        std::fs::write("/tmp/hub-new-device-options.json", &raw).unwrap();
        let devices = endpoint
            .get("/api/devices", Duration::from_secs(60))
            .unwrap()
            .into_string()
            .unwrap();
        std::fs::write("/tmp/hub-devices.json", &devices).unwrap();
        stop();
    }

    /// Installs and runs the real hub against a booted iOS Simulator:
    /// How long each hub call takes on repeat, with a booted simulator:
    /// `cargo test --lib live_timings -- --ignored --nocapture`.
    #[test]
    #[ignore = "runs the real Device Hub"]
    fn live_timings() {
        super::super::set_enabled(true, false);
        let time = |label: &str, f: &mut dyn FnMut()| {
            let started = Instant::now();
            f();
            eprintln!("timing: {label} {:?}", started.elapsed());
        };
        let mut endpoint = None;
        time("hub start", &mut || endpoint = Some(ensure().unwrap()));
        let endpoint = endpoint.unwrap();
        let mut udid = String::new();
        for round in 0..3 {
            time(&format!("list #{round}"), &mut || {
                let list = list().unwrap();
                udid = list
                    .devices
                    .iter()
                    .find(|d| d.platform == Platform::Ios && d.booted)
                    .map(|d| d.id.clone())
                    .unwrap_or_default();
            });
        }
        for round in 0..3 {
            time(&format!("attach #{round}"), &mut || {
                attach(&endpoint, &udid).unwrap()
            });
        }
        for path in ["/api/devices", "/readyz"] {
            time(&format!("GET {path}"), &mut || {
                endpoint.get(path, Duration::from_secs(20)).unwrap();
            });
        }
        stop();
    }

    /// `cargo test --lib live_hub -- --ignored --nocapture`.
    #[test]
    #[ignore = "installs and runs the real Device Hub"]
    fn live_hub_lists_attaches_and_screenshots() {
        crate::device::set_enabled(true, false);
        let started = Instant::now();
        let endpoint = ensure().expect("the hub starts");
        eprintln!(
            "hub ready on port {} in {:?}",
            endpoint.port,
            started.elapsed()
        );
        let list = list().expect("the hub lists devices");
        for device in &list.devices {
            eprintln!(
                "{:?} {} {} booted={}",
                device.platform, device.name, device.id, device.booted
            );
        }
        eprintln!("errors: {:?}", list.errors);
        let device = list
            .devices
            .iter()
            .find(|d| d.booted && d.platform == Platform::Ios)
            .expect("a booted simulator");
        attach(&endpoint, &device.id).expect("the simulator attaches");
        let png = screenshot(device.platform, &device.id).expect("a screenshot");
        assert!(png.starts_with(b"\x89PNG"), "a PNG");
        eprintln!("screenshot: {} bytes", png.len());
        stop();
        assert!(running().is_none());
    }

    #[test]
    fn the_banner_gives_port_and_token() {
        assert_eq!(
            parse_banner("  Local:   http://localhost:3412/?token=abc-DEF_1"),
            Some((3412, "abc-DEF_1".into()))
        );
        assert_eq!(parse_banner("Expo Device Hub ready"), None);
        assert_eq!(parse_banner("  Local:   http://localhost:3412"), None);
    }

    #[test]
    fn the_hub_streams_with_hardware_encoding_and_scrcpy_on_loopback() {
        let args = arguments(std::path::Path::new("/hub/cli.mjs"), 4000);
        let pairs: Vec<_> = args.windows(2).collect();
        let has = |a: &str, b: &str| pairs.contains(&&[a.to_string(), b.to_string()][..]);
        assert!(has("--encoder", "hardware"));
        assert!(has("--stream-source", "scrcpy"));
        assert!(has("--video-fps", "120"));
        assert!(has("--max-dimension", "1600"));
        assert!(has("--host", "127.0.0.1"));
        assert!(args.contains(&"--require-token".to_string()));
    }
}
