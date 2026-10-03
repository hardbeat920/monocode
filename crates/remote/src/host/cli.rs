//! Port of host/cli.ts: the `monocode-host` commands, and `serve`, which
//! runs a host until it is stopped.
//!
//! The app's `monocode-app host ...` subcommand calls [`main`] or [`run`]
//! with its arguments after `host`, and a function that builds the engine
//! backend once this process owns the data directory.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

use serde_json::{Value, json};

use super::backend::HostBackend;
use super::connect::{
    ConnectOptions, Output, RuntimeSource, connect, connect_disable, connect_pair, connect_status,
    start_detached,
};
use super::control::{HostStatus, LifecycleAction, NetworkStatus, lifecycle, read_running};
use super::http::{BodyError, HttpServer, Request, Response};
use super::listener::{HostListener, HostListenerOptions, listen_host};
use super::network::{
    NetworkSettings, network_endpoints, network_interfaces, read_network_settings, tailscale_name,
};
use super::owner::{HostOwner, acquire_host_owner};
use super::protocol::{REMOTE_PROVIDERS, RemoteProvider, provider_name};
use super::server::{HostServerOptions, Lifecycle, create_host_server, home_dir};
use super::service::{
    ServiceOptions, UninstallSystem, connection_info, install_service, uninstall_service,
};
use super::store::{HostStore, random_token};
use super::tls::{HostIdentity, load_host_identity};

pub fn help(version: &str) -> String {
    // TODO(port): the Node host's help ended with "Requires Node.js 22.13 or
    // newer." This host needs no Node.
    format!(
        "MonoCode Host {version}

Set up this machine for MonoCode:
  npx monocode-host connect           Install the host as a background service, turn on
                                      network access, and print a pairing link
    --local-only                      Listen on loopback only; pair over SSH
    --bind <address>                  Listen on one address instead of all (0.0.0.0)
    --name <label>                    Name shown in MonoCode (default: hostname)
    --no-service                      Run detached instead of as a login service
    --restart                         Reinstall and restart this version
    --yes                             Restart without asking, interrupting running turns
    --json                            Print one JSON line; progress goes to stderr
  connect pair [--json]               Print a new one-time pairing link
  connect status [--json]             Show the host, network access, and paired desktops
  connect disable                     Turn off network access; SSH pairing keeps working

Manage the host:
  status | stop | start | serve       Check, stop, start detached, or run in the foreground
  service install | uninstall         Add or remove the login service; data is kept
  devices                             List paired desktops
  revoke <device-id>                  Revoke a desktop's access
  connection-info                     Print the running host's port (JSON)
  pair --name <device>                Issue a raw device credential (advanced)

Options: --data-dir <directory> (default ~/.monocode-host) --port <port> (default 3774)"
    )
}

/// What [`serve`] needs besides the backend.
pub struct HostOptions {
    pub directory: PathBuf,
    pub port: u16,
    /// Reported as `hostVersion` and in status.
    pub version: String,
    /// Ownership of `directory`, taken before the backend was built.
    pub owner: HostOwner,
}

struct Serving {
    directory: PathBuf,
    port: u16,
    version: String,
    backend: Arc<dyn HostBackend>,
    available: Vec<RemoteProvider>,
    identity: HostIdentity,
    secret: String,
    network: Mutex<NetworkStatus>,
    magic_dns: Mutex<Option<String>>,
    front: Mutex<Option<HostListener>>,
    http: OnceLock<Arc<HttpServer>>,
    owner: Mutex<Option<HostOwner>>,
    stopping: AtomicBool,
    stopped: Mutex<bool>,
    done: Condvar,
}

fn bind_address(settings: &NetworkSettings) -> String {
    if settings.enabled {
        settings.bind.clone()
    } else {
        "127.0.0.1".into()
    }
}

impl Serving {
    fn status(&self) -> HostStatus {
        HostStatus {
            version: Some(self.version.clone()),
            pid: Some(i64::from(std::process::id())),
            port: Some(self.port),
            running_turns: self.backend.store().running_turns().ok(),
            providers: Some(
                self.available
                    .iter()
                    .map(|provider| provider_name(*provider).to_string())
                    .collect(),
            ),
            network: Some(
                self.network
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
            ),
        }
    }

    fn open(&self, settings: &NetworkSettings) -> Result<HostListener, String> {
        let http = self
            .http
            .get()
            .ok_or("The host server is not ready")?
            .clone();
        listen_host(
            http,
            HostListenerOptions {
                port: self.port,
                bind: bind_address(settings),
                identity: Some(self.identity.clone()),
                loopback: None,
            },
        )
    }

    /// Listening on a network address failed, such as when the address no
    /// longer exists. Keep loopback working and report why.
    fn open_or_loopback(&self, settings: &NetworkSettings) -> Result<(), String> {
        let (front, network) = match self.open(settings) {
            Ok(front) => (
                front,
                NetworkStatus {
                    enabled: settings.enabled,
                    bind: settings.bind.clone(),
                    error: None,
                },
            ),
            Err(error) => {
                if !settings.enabled {
                    return Err(error);
                }
                let front = self.open(&NetworkSettings {
                    enabled: false,
                    bind: settings.bind.clone(),
                })?;
                (
                    front,
                    NetworkStatus {
                        enabled: settings.enabled,
                        bind: settings.bind.clone(),
                        error: Some(error),
                    },
                )
            }
        };
        *self.front.lock().unwrap_or_else(PoisonError::into_inner) = Some(front);
        *self.network.lock().unwrap_or_else(PoisonError::into_inner) = network;
        Ok(())
    }

    /// Rebinds without a restart, so turning network access on or off does
    /// not interrupt agents. Open connections are unaffected.
    fn apply_network(&self) -> Result<(), String> {
        let next = read_network_settings(&self.directory);
        {
            let mut network = self.network.lock().unwrap_or_else(PoisonError::into_inner);
            let current = NetworkSettings {
                enabled: network.enabled,
                bind: network.bind.clone(),
            };
            if network.error.is_none() && bind_address(&next) == bind_address(&current) {
                *network = NetworkStatus {
                    enabled: next.enabled,
                    bind: next.bind,
                    error: None,
                };
                return Ok(());
            }
        }
        let previous = self
            .front
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(previous) = previous {
            previous.close();
        }
        let mut attempt = 0;
        loop {
            match self.open_or_loopback(&next) {
                Ok(()) => return Ok(()),
                Err(error) if attempt >= 20 => return Err(error),
                Err(_) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn endpoints(&self) -> Vec<String> {
        let network = self
            .network
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if !network.enabled || network.error.is_some() {
            return Vec::new();
        }
        let names: Vec<String> = self
            .magic_dns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        network_endpoints(self.port, &network.bind, &network_interfaces(), &names)
    }

    /// The local administrative endpoint. Its secret is separate from paired
    /// client credentials and never sent to a desktop.
    fn lifecycle(self: &Arc<Self>, request: &mut Request<'_>) -> Response {
        let authorized =
            request.header("authorization") == Some(&format!("Bearer {}", self.secret));
        if request.method != "POST"
            || request
                .header("origin")
                .is_some_and(|origin| !origin.is_empty())
            || !authorized
        {
            return Response::new(403);
        }
        let body = match request.read_body(128) {
            Ok(body) => body,
            Err(BodyError::TooLarge) | Err(_) => return Response::new(400),
        };
        let action = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("action")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        match action.as_deref() {
            Some("network") => {
                if let Err(error) = self.apply_network() {
                    eprintln!("Could not apply network settings: {error}");
                }
            }
            Some("status") | Some("stop") => {}
            _ => return Response::new(400),
        }
        let status = serde_json::to_vec(&self.status()).unwrap_or_default();
        if action.as_deref() == Some("stop") {
            let serving = self.clone();
            std::thread::spawn(move || {
                // Let this response reach the caller first.
                std::thread::sleep(Duration::from_millis(100));
                serving.stop();
            });
        }
        Response::new(200)
            .header("Content-Type", "application/json")
            .body(status)
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_file(self.directory.join("running.json"));
        self.backend.store().close();
        if let Some(owner) = self
            .owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            owner.release();
        }
    }

    fn stop(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            self.wait();
            return;
        }
        let front = self
            .front
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(front) = front {
            front.close();
        }
        if let Some(http) = self.http.get() {
            http.close();
            http.close_all_connections();
        }
        self.backend.store().changes.close();
        self.backend.close();
        self.cleanup();
        *self.stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.done.notify_all();
    }

    fn wait(&self) {
        let mut stopped = self.stopped.lock().unwrap_or_else(PoisonError::into_inner);
        while !*stopped {
            stopped = self
                .done
                .wait(stopped)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// A running host.
#[derive(Clone)]
pub struct HostHandle {
    serving: Arc<Serving>,
}

impl HostHandle {
    /// The address the host listens on now.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.serving
            .front
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(HostListener::local_addr)
    }

    pub fn status(&self) -> HostStatus {
        self.serving.status()
    }

    pub fn environment_id(&self) -> String {
        self.serving.backend.store().environment_id.clone()
    }

    pub fn providers(&self) -> Vec<RemoteProvider> {
        self.serving.available.clone()
    }

    /// Stops listening, ends open connections, closes the backend and the
    /// store, removes `running.json`, and releases the data directory.
    pub fn stop(&self) {
        self.serving.stop();
    }

    /// Blocks until the host stops, such as after a lifecycle `stop`.
    pub fn wait(&self) {
        self.serving.wait();
    }
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(
        &mut options.open(path).map_err(|error| error.to_string())?,
        contents.as_bytes(),
    )
    .map_err(|error| error.to_string())
}

/// Runs the host server for `backend` on `options.port`: TLS for other
/// computers, plain HTTP from loopback, and the local lifecycle endpoint.
/// Writes `running.json` once it listens. Returns at once; call
/// [`HostHandle::wait`] to block until the host is stopped.
pub fn serve(options: HostOptions, backend: impl HostBackend) -> Result<HostHandle, String> {
    let backend: Arc<dyn HostBackend> = Arc::new(backend);
    let HostOptions {
        directory,
        port,
        version,
        owner,
    } = options;
    let fail =
        |backend: &Arc<dyn HostBackend>, owner: HostOwner, directory: &Path, error: String| {
            backend.close();
            let _ = std::fs::remove_file(directory.join("running.json"));
            backend.store().close();
            owner.release();
            Err(error)
        };
    let available: Vec<RemoteProvider> = REMOTE_PROVIDERS
        .into_iter()
        .filter(|provider| backend.resolve_binary(*provider).is_ok())
        .collect();
    let identity = match load_host_identity(&directory) {
        Ok(identity) => identity,
        Err(error) => return fail(&backend, owner, &directory, error),
    };
    let settings = read_network_settings(&directory);
    let serving = Arc::new(Serving {
        directory: directory.clone(),
        port,
        version: version.clone(),
        backend: backend.clone(),
        available: available.clone(),
        identity,
        secret: random_token(),
        network: Mutex::new(NetworkStatus {
            enabled: settings.enabled,
            bind: settings.bind.clone(),
            error: None,
        }),
        magic_dns: Mutex::new(None),
        front: Mutex::new(None),
        http: OnceLock::new(),
        owner: Mutex::new(Some(owner)),
        stopping: AtomicBool::new(false),
        stopped: Mutex::new(false),
        done: Condvar::new(),
    });
    let weak: Weak<Serving> = Arc::downgrade(&serving);
    std::thread::spawn(move || {
        let name = tailscale_name();
        if let Some(serving) = weak.upgrade() {
            *serving
                .magic_dns
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = name;
        }
    });
    let weak = Arc::downgrade(&serving);
    let lifecycle: Lifecycle = Arc::new(move |request| match weak.upgrade() {
        Some(serving) => serving.lifecycle(request),
        None => Response::new(403),
    });
    let weak = Arc::downgrade(&serving);
    let http = create_host_server(
        backend,
        available,
        HostServerOptions {
            endpoints: Some(Arc::new(move || {
                weak.upgrade()
                    .map(|serving| serving.endpoints())
                    .unwrap_or_default()
            })),
            lifecycle: Some(lifecycle),
            version,
            ..Default::default()
        },
    );
    let _ = serving.http.set(http);
    let started = serving.open_or_loopback(&settings).and_then(|()| {
        write_private(
            &directory.join("running.json"),
            &json!({ "pid": std::process::id(), "port": port, "secret": serving.secret })
                .to_string(),
        )
    });
    if let Err(error) = started {
        serving.stopping.store(true, Ordering::SeqCst);
        if let Some(front) = serving
            .front
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            front.close();
        }
        serving.backend.close();
        serving.cleanup();
        return Err(error);
    }
    Ok(HostHandle { serving })
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| *arg == format!("--{name}"))
}

fn option(args: &[String], name: &str) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| *arg == format!("--{name}")) else {
        return Ok(None);
    };
    match args.get(index + 1) {
        Some(value) if !value.is_empty() && !value.starts_with("--") => Ok(Some(value.clone())),
        _ => Err(format!("Missing --{name} value")),
    }
}

fn prepare_directory(directory: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|error| error.to_string())?;
    #[cfg(windows)]
    super::windows::protect_windows_directory(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Stops the host on SIGTERM or SIGINT, as the Node host did.
fn stop_on_signals(handle: &HostHandle) {
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGINT, SIGTERM};
        if let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT]) {
            let handle = handle.clone();
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    handle.stop();
                }
            });
        }
    }
    #[cfg(not(unix))]
    let _ = handle;
}

/// Runs one host command. `args` follow the program name, such as
/// `["connect", "--json"]`. `runtime` names this program and its files;
/// `start`, `service install`, and `connect` run `serve` through it.
/// `backend` builds the engine for `serve` once this process owns the data
/// directory.
pub fn run<B: HostBackend>(
    args: &[String],
    runtime: &RuntimeSource,
    version: &str,
    backend: impl FnOnce(Arc<HostStore>) -> Result<B, String>,
    out: &mut Output,
) -> Result<(), String> {
    let command = args.first().map(String::as_str).unwrap_or("help");
    if command == "--version" || command == "-v" {
        out.log(version);
        return Ok(());
    }
    if matches!(command, "help" | "--help" | "-h") {
        out.log(&help(version));
        return Ok(());
    }
    let directory = std::path::absolute(
        option(args, "data-dir")?
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".monocode-host")),
    )
    .map_err(|error| error.to_string())?;
    let requested = option(args, "port")?;
    let port = match &requested {
        None => 3774,
        Some(value) => {
            let number = super::js::number_from_str(value);
            if number.fract() != 0.0 || !(1.0..=65535.0).contains(&number) {
                return Err("Invalid port".into());
            }
            number as u16
        }
    };
    let requested = requested.map(|_| port);
    prepare_directory(&directory)?;
    let program = runtime.program();
    let state_path = directory.join("running.json");

    if command == "connect" {
        let sub = args
            .get(1)
            .filter(|arg| !arg.starts_with("--"))
            .map(String::as_str);
        return match sub {
            Some("pair") => connect_pair(
                &directory,
                flag(args, "json"),
                option(args, "name")?.as_deref(),
                out,
            ),
            Some("status") => connect_status(&directory, flag(args, "json"), out),
            Some("disable") => connect_disable(&directory, out),
            Some(sub) => Err(format!("Unknown connect command: {sub}. Run with --help.")),
            None => connect(
                &ConnectOptions {
                    directory,
                    port: requested,
                    version: version.into(),
                    runtime: runtime.clone(),
                    bind: option(args, "bind")?,
                    local_only: flag(args, "local-only"),
                    json: flag(args, "json"),
                    yes: flag(args, "yes"),
                    service: !flag(args, "no-service"),
                    restart: flag(args, "restart"),
                    name: option(args, "name")?,
                },
                out,
            ),
        };
    }
    if command == "connection-info" {
        out.log(&serde_json::to_string(&connection_info(&directory)?).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if command == "service" && args.get(1).map(String::as_str) == Some("uninstall") {
        let notes = uninstall_service(UninstallSystem::default())?;
        // Also stops a manually started host, or one the service manager left.
        if let Some(state) = read_running(&directory) {
            let _ = lifecycle(&state, LifecycleAction::Stop);
            for _ in 0..200 {
                if !state_path.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let mut lines = vec![
            if state_path.exists() {
                "The host service was removed, but the host is still running. Run stop, or end its process.".to_string()
            } else {
                "The host is stopped and will not start automatically.".to_string()
            },
            format!(
                "Sessions, logs and device credentials are kept in {}. Delete that directory only if you want to erase them.",
                directory.display()
            ),
        ];
        lines.extend(notes);
        out.log(&lines.join("\n"));
        return Ok(());
    }
    if command == "service" {
        if args.get(1).map(String::as_str) != Some("install") {
            return Err("Use: service install, or service uninstall".into());
        }
        let info = install_service(&ServiceOptions {
            directory,
            port,
            program,
        })?;
        out.log(&serde_json::to_string(&info).map_err(|error| error.to_string())?);
        return Ok(());
    }
    if command == "status" || command == "stop" {
        let Some(state) = read_running(&directory) else {
            out.log("Host is stopped");
            return Ok(());
        };
        let action = if command == "stop" {
            LifecycleAction::Stop
        } else {
            LifecycleAction::Status
        };
        let status = lifecycle(&state, action)?;
        out.log(&if command == "stop" {
            "Host is stopping".to_string()
        } else {
            format!(
                "Host{} is running (PID {}, port {})",
                status
                    .version
                    .map(|version| format!(" {version}"))
                    .unwrap_or_default(),
                state.pid,
                state.port
            )
        });
        return Ok(());
    }
    if command == "start" {
        let state = start_detached(&directory, port, &program)?;
        out.log(&format!(
            "Host started on port {}. It will continue after this terminal closes.",
            state.port
        ));
        return Ok(());
    }
    let store = Arc::new(HostStore::open(&directory.join("host.db"))?);
    match command {
        "pair" => {
            let name = option(args, "name")?.unwrap_or_else(|| "Desktop".into());
            let device = store.issue_device(&name)?;
            let value = json!({
                "id": device.id,
                "token": device.token,
                "environmentId": store.environment_id,
            });
            out.log(&if flag(args, "json") {
                value.to_string()
            } else {
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
            });
            store.close();
            Ok(())
        }
        "devices" => {
            let devices = store.devices()?;
            out.log(&serde_json::to_string_pretty(&devices).map_err(|error| error.to_string())?);
            store.close();
            Ok(())
        }
        "revoke" => {
            let Some(id) = args.get(1).filter(|arg| !arg.starts_with("--")) else {
                return Err("Provide a device ID to revoke".into());
            };
            let revoked = store.revoke_device(id)?;
            store.close();
            if !revoked {
                return Err("Device not found".into());
            }
            out.log("Device revoked");
            Ok(())
        }
        "serve" => {
            let owner = acquire_host_owner(&directory)?;
            let backend = backend(store)?;
            let handle = serve(
                HostOptions {
                    directory,
                    port,
                    version: version.into(),
                    owner,
                },
                backend,
            )?;
            stop_on_signals(&handle);
            let status = handle.status();
            let network = status.network.clone().unwrap_or_default();
            out.log(&format!(
                "MonoCode Host {version} ({}) listening on {}",
                handle.environment_id(),
                if network.enabled && network.error.is_none() {
                    format!("{}:{port} (TLS; plain HTTP from loopback)", network.bind)
                } else {
                    format!("127.0.0.1:{port}")
                }
            ));
            if let Some(error) = &network.error {
                out.log(&format!(
                    "Network access failed, serving loopback only: {error}"
                ));
            }
            let providers: Vec<&str> = handle
                .providers()
                .iter()
                .map(|provider| provider_name(*provider))
                .collect();
            out.log(&format!(
                "Providers: {}",
                if providers.is_empty() {
                    "none found; install and authenticate a supported provider on this host"
                        .to_string()
                } else {
                    providers.join(", ")
                }
            ));
            handle.wait();
            Ok(())
        }
        _ => {
            store.close();
            Err("Unknown command; run with --help".into())
        }
    }
}

/// `monocode-host <args>` as a process: sets a private umask, runs the
/// command, and prints a failure to stderr. Returns the exit code.
pub fn main<B: HostBackend>(
    args: &[String],
    runtime: &RuntimeSource,
    version: &str,
    backend: impl FnOnce(Arc<HostStore>) -> Result<B, String>,
) -> i32 {
    #[cfg(unix)]
    // SAFETY: umask only changes this process's file creation mask.
    unsafe {
        libc::umask(0o077);
    }
    // TODO(port): the Node host put its own Node first on PATH so npm-based
    // provider CLIs could start without another Node install. This host has
    // no bundled Node; providers rely on the PATH the service was given.
    let mut out = Output::process();
    match run(args, runtime, version, backend, &mut out) {
        Ok(()) => 0,
        Err(error) => {
            let _ = std::io::Write::write_all(&mut out.stderr, format!("{error}\n").as_bytes());
            1
        }
    }
}

#[cfg(test)]
mod tests;
