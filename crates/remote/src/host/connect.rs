//! Port of host/connect.ts: `monocode-host connect` and its subcommands.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;

use super::control::{
    HostStatus, LifecycleAction, NetworkStatus, RunningHost, RunningStatus, compare_versions,
    lifecycle, read_running, running_status,
};
use super::network::{
    DEFAULT_BIND, NetworkSettings, PairingOffer, network_endpoints, network_interfaces,
    pairing_link, read_network_settings, tailscale_name, write_network_settings,
};
use super::runtime::{HostProgram, InstallRuntime, RuntimeBundle, install_runtime, prune_runtimes};
use super::server::{hostname, node_platform};
use super::service::{ServiceOptions, UninstallSystem, install_service, uninstall_service};
use super::store::{HostStore, now_ms};
use super::tls::load_host_identity;

/// Where CLI commands write. The process's own streams, or buffers in tests.
pub struct Output {
    pub stdout: Box<dyn Write + Send>,
    pub stderr: Box<dyn Write + Send>,
    /// Whether stdin is a terminal that can answer questions.
    pub interactive: bool,
}

impl Output {
    pub fn process() -> Self {
        use std::io::IsTerminal;
        Self {
            stdout: Box::new(std::io::stdout()),
            stderr: Box::new(std::io::stderr()),
            interactive: std::io::stdin().is_terminal(),
        }
    }

    /// Output collected in memory: `(output, stdout, stderr)`.
    pub fn captured() -> (Self, SharedBuffer, SharedBuffer) {
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();
        (
            Self {
                stdout: Box::new(stdout.clone()),
                stderr: Box::new(stderr.clone()),
                interactive: false,
            },
            stdout,
            stderr,
        )
    }

    /// `console.log`.
    pub fn log(&mut self, line: &str) {
        let _ = writeln!(self.stdout, "{line}");
        let _ = self.stdout.flush();
    }

    /// Progress goes to stderr in JSON mode, so stdout carries one JSON line.
    fn say(&mut self, json: bool, line: &str) {
        let stream = if json {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        let _ = writeln!(stream, "{line}");
        let _ = stream.flush();
    }
}

#[derive(Clone, Default)]
pub struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The running host program and its files, which `connect` copies into the
/// data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSource {
    /// The folder holding the running bundle.
    pub source: PathBuf,
    pub bundle: RuntimeBundle,
    /// The interpreter that runs the entry point. `None` runs it directly.
    pub interpreter: Option<PathBuf>,
    /// Arguments after the entry point, such as `host`.
    pub args: Vec<String>,
}

impl RuntimeSource {
    /// This executable, run as `<exe> <args...>`, such as `monocode-app host`.
    pub fn current_exe(args: &[&str]) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let file = executable
            .file_name()
            .ok_or("The host executable has no file name")?
            .to_string_lossy()
            .into_owned();
        Ok(Self {
            source: executable
                .parent()
                .ok_or("The host executable has no folder")?
                .to_path_buf(),
            bundle: RuntimeBundle::native(&file),
            interpreter: None,
            args: args.iter().map(|arg| arg.to_string()).collect(),
        })
    }

    /// Runs the bundle where it is, without copying it.
    pub fn program(&self) -> HostProgram {
        let entry = self.source.join(&self.bundle.files[0]);
        match &self.interpreter {
            Some(interpreter) => HostProgram {
                executable: interpreter.clone(),
                args: std::iter::once(entry.to_string_lossy().into_owned())
                    .chain(self.args.iter().cloned())
                    .collect(),
            },
            None => HostProgram {
                executable: entry,
                args: self.args.clone(),
            },
        }
    }
}

pub struct ConnectOptions {
    pub directory: PathBuf,
    /// Defaults to the running host's port, then 3774.
    pub port: Option<u16>,
    pub version: String,
    /// The running bundle, such as the copy in npx's cache or a download.
    pub runtime: RuntimeSource,
    pub bind: Option<String>,
    pub local_only: bool,
    pub json: bool,
    /// Restart a host that has running turns without asking.
    pub yes: bool,
    /// Install a login service. Without it, the host runs detached.
    pub service: bool,
    /// Reinstall and restart even when this version is already running.
    pub restart: bool,
    pub name: Option<String>,
}

/// Starts `serve` detached from this terminal and waits for it to answer.
pub fn start_detached(
    directory: &Path,
    port: u16,
    program: &HostProgram,
) -> Result<RunningHost, String> {
    let log_path = directory.join("host.log");
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log = options.open(&log_path).map_err(|error| error.to_string())?;
    let mut command = std::process::Command::new(&program.executable);
    command
        .args(program.serve_args(directory, port))
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|error| error.to_string())?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid only detaches the child from this terminal's session.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let pid = i64::from(child.id());
    // Reap the child if it exits while this process still runs.
    std::thread::spawn(move || child.wait());
    let attempts = if cfg!(windows) { 150 } else { 50 };
    for _ in 0..attempts {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(state) = read_running(directory).filter(|state| state.pid == pid) {
            return Ok(state);
        }
    }
    Err(format!("Host did not start. See {}", log_path.display()))
}

/// Stops the running host. With `service`, first removes the login service
/// so its manager does not start the old version again.
pub fn stop_host(
    directory: &Path,
    state: Option<&RunningHost>,
    service: bool,
) -> Result<(), String> {
    if service {
        let _ = uninstall_service(UninstallSystem::default());
    }
    if let Some(state) = state {
        let _ = lifecycle(state, LifecycleAction::Stop);
    }
    let running = directory.join("running.json");
    for _ in 0..200 {
        if !running.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if running.exists() && running_status(directory).is_some() {
        return Err(
            "The running host did not stop. Stop it with `monocode-host stop`, then run connect again."
                .into(),
        );
    }
    Ok(())
}

fn confirm(out: &mut Output, question: &str) -> bool {
    if !out.interactive {
        return false;
    }
    let _ = write!(out.stdout, "{question} [y/N] ");
    let _ = out.stdout.flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    let answer = answer.trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

fn service_description(kind: &str) -> &'static str {
    match kind {
        "linux" => "Background service running (systemd user service; keeps running after logout)",
        "darwin" => {
            "Background service running (launch agent; runs while you are logged in to this Mac)"
        }
        "win32" => {
            "Background service running (Task Scheduler; runs while you are signed in to Windows)"
        }
        "detached" => "Host running in the background until this machine restarts or you log out",
        _ => "Host running",
    }
}

fn provider_label(id: &str) -> String {
    match id {
        "codex" => "Codex".into(),
        "claude" => "Claude Code".into(),
        other => other.into(),
    }
}

/// Endpoints for the network settings a host reports.
fn advertised(port: u16, network: Option<&NetworkStatus>) -> Vec<String> {
    let Some(network) = network.filter(|network| network.enabled) else {
        return Vec::new();
    };
    let names: Vec<String> = tailscale_name().into_iter().collect();
    network_endpoints(port, &network.bind, &network_interfaces(), &names)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedLink {
    pub environment_id: String,
    pub fingerprint: String,
    pub expires_at: i64,
    pub link: String,
}

fn issue_link(
    directory: &Path,
    endpoints: &[String],
    name: Option<&str>,
) -> Result<IssuedLink, String> {
    let identity = load_host_identity(directory)?;
    let store = HostStore::open(&directory.join("host.db"))?;
    let pairing = store.issue_pairing(now_ms());
    store.close();
    let pairing = pairing?;
    let name = name
        .map(monocode_core::js::trim)
        .map(|name| monocode_core::js::slice_prefix(name, 100).to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(hostname);
    Ok(IssuedLink {
        link: pairing_link(&PairingOffer {
            name,
            environment_id: store.environment_id.clone(),
            fingerprint: identity.fingerprint.clone(),
            code: pairing.code,
            endpoints: endpoints.to_vec(),
        }),
        environment_id: store.environment_id.clone(),
        fingerprint: identity.fingerprint,
        expires_at: pairing.expires_at,
    })
}

fn print_link(out: &mut Output, json: bool, link: &str, endpoints: &[String]) {
    out.say(json, "");
    out.say(json, "Pair a desktop");
    out.say(
        json,
        "  In MonoCode, open Settings → Connections → Pair machine and paste this link.",
    );
    out.say(json, "  It works once and expires in 15 minutes.");
    out.say(json, "");
    out.say(json, &format!("  {link}"));
    if endpoints.is_empty() {
        out.say(json, "");
        out.say(
            json,
            "  This host listens only on loopback. Pair it from MonoCode with Set up over SSH,",
        );
        out.say(
            json,
            "  or run connect without --local-only to enable network access.",
        );
    }
}

fn merge(status: HostStatus, applied: HostStatus) -> HostStatus {
    HostStatus {
        version: applied.version.or(status.version),
        pid: applied.pid.or(status.pid),
        port: applied.port.or(status.port),
        running_turns: applied.running_turns.or(status.running_turns),
        providers: applied.providers.or(status.providers),
        network: applied.network.or(status.network),
    }
}

/// `monocode-host connect`: installs this version as a background service,
/// enables network access over TLS, and prints a one-time pairing link.
/// Running it again reuses a running host of the same or a newer version,
/// so it is also how a new desktop gets a link.
pub fn connect(options: &ConnectOptions, out: &mut Output) -> Result<(), String> {
    let json = options.json;
    let directory = options.directory.as_path();
    out.say(json, "MonoCode Connect");
    out.say(json, "");

    let previous = read_network_settings(directory);
    let network = NetworkSettings {
        enabled: !options.local_only,
        bind: options.bind.clone().unwrap_or(if previous.bind.is_empty() {
            DEFAULT_BIND.into()
        } else {
            previous.bind
        }),
    };
    write_network_settings(directory, &network)?;
    load_host_identity(directory)?;

    let mut status = running_status(directory);
    let mut service = "existing".to_string();
    let reuse = status.as_ref().is_some_and(|running| {
        running.status.version.as_deref().is_some_and(|version| {
            !options.restart && compare_versions(version, &options.version) >= 0
        })
    });
    let status = match status.take() {
        Some(running) if reuse => {
            let applied = lifecycle(&running.state, LifecycleAction::Network)?;
            let running = RunningStatus {
                status: merge(running.status, applied),
                state: running.state,
            };
            let version = running.status.version.clone().unwrap_or_default();
            out.say(
                json,
                &format!("✓ Host {version} is running (PID {})", running.state.pid),
            );
            if compare_versions(&version, &options.version) > 0 {
                out.say(
                    json,
                    &format!(
                        "  It is newer than this command ({}), so it was left unchanged.",
                        options.version
                    ),
                );
            }
            running
        }
        previous => {
            if let Some(running) = &previous {
                // Hosts before 0.5 do not report their turns, so assume some may run.
                let turns = running.status.running_turns;
                if turns.is_none_or(|turns| turns > 0) && !options.yes {
                    let question = match turns {
                        None => "An older host is running. Updating restarts it and interrupts any running turns. Continue?".to_string(),
                        Some(turns) => format!(
                            "The host has {turns} running turn{}. Updating restarts it and interrupts them. Continue?",
                            if turns == 1 { "" } else { "s" }
                        ),
                    };
                    if json || !confirm(out, &question) {
                        return Err(if json || !out.interactive {
                            format!(
                                "{} Run connect again with --yes to update anyway.",
                                question.trim_end_matches(" Continue?")
                            )
                        } else {
                            "Update cancelled. The running host was left unchanged.".into()
                        });
                    }
                }
                out.say(
                    json,
                    &format!(
                        "Updating the host from {} to {}…",
                        running
                            .status
                            .version
                            .clone()
                            .unwrap_or_else(|| "an older version".into()),
                        options.version
                    ),
                );
                stop_host(directory, Some(&running.state), options.service)?;
            } else if options.service {
                // A stale service definition may point at a removed runtime.
                let _ = uninstall_service(UninstallSystem::default());
            }
            let runtime = install_runtime(InstallRuntime {
                directory,
                version: &options.version,
                source: &options.runtime.source,
                bundle: &options.runtime.bundle,
                interpreter: options.runtime.interpreter.as_deref(),
                args: &options.runtime.args,
                platform: node_platform(),
            })?;
            let target = ServiceOptions {
                directory: directory.to_path_buf(),
                port: options
                    .port
                    .or(previous.as_ref().map(|running| running.state.port))
                    .unwrap_or(3774),
                program: runtime.program.clone(),
            };
            if options.service {
                match install_service(&target) {
                    Ok(_) => service = node_platform().into(),
                    Err(error) => {
                        out.say(
                            json,
                            &format!("! The background service could not be installed: {error}"),
                        );
                        // Remove what was installed and wait for any host it
                        // started to exit, so two hosts never share the data
                        // directory. This fails instead of starting a second
                        // host if one is still running.
                        stop_host(directory, read_running(directory).as_ref(), true)?;
                        start_detached(directory, target.port, &target.program)?;
                        service = "detached".into();
                    }
                }
            } else {
                start_detached(directory, target.port, &target.program)?;
                service = "detached".into();
            }
            let running = running_status(directory).ok_or_else(|| {
                format!(
                    "The host did not answer after starting. See {}",
                    directory.join("host.log").display()
                )
            })?;
            if let Some(folder) = runtime.entry.parent() {
                prune_runtimes(directory, folder);
            }
            out.say(
                json,
                &format!(
                    "✓ Host {} installed in {}",
                    options.version,
                    directory.display()
                ),
            );
            out.say(json, &format!("✓ {}", service_description(&service)));
            out.say(
                json,
                &format!("  Manage it with {}", runtime.launcher.display()),
            );
            running
        }
    };

    let providers: Vec<String> = status
        .status
        .providers
        .clone()
        .unwrap_or_default()
        .iter()
        .map(|id| provider_label(id))
        .collect();
    out.say(
        json,
        &if providers.is_empty() {
            "! No Codex or Claude Code CLI was found. Install one and sign in as this user, then run connect --restart.".to_string()
        } else {
            format!("✓ Providers: {}", providers.join(", "))
        },
    );

    let port = status.state.port;
    let endpoints = advertised(port, status.status.network.as_ref());
    match &status.status.network {
        Some(NetworkStatus {
            error: Some(error), ..
        }) => out.say(json, &format!("! Network access is off: {error}")),
        Some(network) if network.enabled => {
            out.say(json, &format!("✓ Network access on port {port} (TLS)"));
            for endpoint in &endpoints {
                out.say(json, &format!("    {endpoint}"));
            }
            if endpoints.is_empty() {
                out.say(
                    json,
                    "  No network address was found. Pair over SSH instead.",
                );
            }
        }
        _ => out.say(json, &format!("✓ Loopback only, on 127.0.0.1:{port}")),
    }

    let issued = issue_link(directory, &endpoints, options.name.as_deref())?;
    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Report<'a> {
            link: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<&'a str>,
            port: u16,
            environment_id: &'a str,
            fingerprint: &'a str,
            endpoints: &'a [String],
            expires_at: i64,
            service: &'a str,
        }
        let report = Report {
            link: &issued.link,
            version: status.status.version.as_deref(),
            port,
            environment_id: &issued.environment_id,
            fingerprint: &issued.fingerprint,
            endpoints: &endpoints,
            expires_at: issued.expires_at,
            service: &service,
        };
        out.log(&serde_json::to_string(&report).map_err(|error| error.to_string())?);
        return Ok(());
    }
    print_link(out, json, &issued.link, &endpoints);
    out.say(json, "");
    out.say(json, "Run `monocode-host connect pair` for another link.");
    Ok(())
}

/// `monocode-host connect pair`: a new link for the running host.
pub fn connect_pair(
    directory: &Path,
    json: bool,
    name: Option<&str>,
    out: &mut Output,
) -> Result<(), String> {
    let status = running_status(directory).ok_or(
        "MonoCode Host is not running. Run `monocode-host connect` to install and start it.",
    )?;
    let endpoints = advertised(status.state.port, status.status.network.as_ref());
    let issued = issue_link(directory, &endpoints, name)?;
    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Report<'a> {
            #[serde(flatten)]
            issued: &'a IssuedLink,
            endpoints: &'a [String],
            port: u16,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<&'a str>,
        }
        out.log(
            &serde_json::to_string(&Report {
                issued: &issued,
                endpoints: &endpoints,
                port: status.state.port,
                version: status.status.version.as_deref(),
            })
            .map_err(|error| error.to_string())?,
        );
        return Ok(());
    }
    print_link(out, json, &issued.link, &endpoints);
    Ok(())
}

/// `monocode-host connect status`.
pub fn connect_status(directory: &Path, json: bool, out: &mut Output) -> Result<(), String> {
    let status = running_status(directory);
    let settings = read_network_settings(directory);
    let identity = load_host_identity(directory)?;
    let store = HostStore::open(&directory.join("host.db"))?;
    let read = (|| Ok::<_, String>((store.devices()?, store.pending_pairings(now_ms())?)))();
    store.close();
    let (devices, pending) = read?;
    let environment_id = store.environment_id.clone();
    let network = status
        .as_ref()
        .and_then(|running| running.status.network.clone())
        .unwrap_or(NetworkStatus {
            enabled: settings.enabled,
            bind: settings.bind,
            error: None,
        });
    let endpoints = status
        .as_ref()
        .map(|running| advertised(running.state.port, Some(&network)))
        .unwrap_or_default();
    let providers = status
        .as_ref()
        .and_then(|running| running.status.providers.clone())
        .unwrap_or_default();
    if json {
        let mut report = serde_json::json!({ "running": status.is_some() });
        if let Some(running) = &status {
            if let Some(version) = &running.status.version {
                report["version"] = version.clone().into();
            }
            report["pid"] = running.state.pid.into();
            report["port"] = running.state.port.into();
        }
        report["providers"] = serde_json::json!(providers);
        report["runningTurns"] = status
            .as_ref()
            .and_then(|running| running.status.running_turns)
            .unwrap_or(0)
            .into();
        report["network"] = serde_json::to_value(&network).map_err(|error| error.to_string())?;
        report["endpoints"] = serde_json::json!(endpoints);
        report["fingerprint"] = identity.fingerprint.clone().into();
        report["environmentId"] = environment_id.into();
        report["devices"] = serde_json::to_value(&devices).map_err(|error| error.to_string())?;
        report["pendingPairings"] = pending.into();
        out.log(&serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?);
        return Ok(());
    }
    out.log("MonoCode Connect");
    out.log("");
    out.log(&match &status {
        Some(running) => format!(
            "  Host: {}, PID {}, port {}",
            running
                .status
                .version
                .clone()
                .unwrap_or_else(|| "unknown version".into()),
            running.state.pid,
            running.state.port
        ),
        None => "  Host: stopped. Run `monocode-host connect` to start it.".into(),
    });
    if status.is_some() {
        let names: Vec<String> = providers.iter().map(|id| provider_label(id)).collect();
        out.log(&format!(
            "  Providers: {}",
            if names.is_empty() {
                "none found".to_string()
            } else {
                names.join(", ")
            }
        ));
    }
    let failed = status
        .as_ref()
        .and_then(|running| running.status.network.as_ref())
        .and_then(|network| network.error.clone())
        .map(|error| format!(", failed: {error}"))
        .unwrap_or_default();
    out.log(&format!(
        "  Network: {}{failed}",
        if network.enabled {
            format!("on ({}, TLS)", network.bind)
        } else {
            "loopback only".into()
        }
    ));
    for endpoint in &endpoints {
        out.log(&format!("    {endpoint}"));
    }
    out.log(&format!("  Certificate: {}", identity.fingerprint));
    out.log(&format!("  Paired desktops: {}", devices.len()));
    for device in &devices {
        out.log(&format!("    {} ({})", device.name, device.id));
    }
    if pending > 0 {
        out.log(&format!("  Unused pairing links: {pending}"));
    }
    Ok(())
}

/// `monocode-host connect disable`: loopback only; keeps paired desktops.
pub fn connect_disable(directory: &Path, out: &mut Output) -> Result<(), String> {
    let settings = read_network_settings(directory);
    write_network_settings(
        directory,
        &NetworkSettings {
            enabled: false,
            ..settings
        },
    )?;
    if let Some(state) = read_running(directory) {
        let _ = lifecycle(&state, LifecycleAction::Network);
    }
    out.log("Network access is off. The host listens only on loopback; paired desktops can still reach it through SSH. Run connect again to turn network access back on.");
    Ok(())
}
