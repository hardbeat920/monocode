//! SSH remote targets: `ssh://<connectionId>/<remote-path>` URIs, the ssh
//! command builder every remote operation funnels through, and process-wide
//! caches keyed by connection.
//!
//! Transport is the system `ssh` binary. Background channels run with
//! `BatchMode` (key/agent auth only); the interactive terminal spawns
//! `ssh -tt` so password prompts surface in xterm. The remote command is
//! assembled as ONE pre-quoted string: ssh joins argv after the host with
//! spaces and re-parses the result through the remote shell, so quoting
//! happens exactly once — here, for the remote shell.

use std::io::{BufRead, Read};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::connections::ConnectionProfile;

pub(crate) const REMOTE_URI_PREFIX: &str = "ssh://";

/// A parsed `ssh://<connectionId>/<remote-path>` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteRef {
    pub connection_id: String,
    /// Remote absolute path; always starts with `/`.
    pub path: String,
}

/// Detect a remote URI on any path/cwd string. Returns None for local paths.
pub(crate) fn parse_remote(path: &str) -> Option<RemoteRef> {
    let rest = path.strip_prefix(REMOTE_URI_PREFIX)?;
    let (connection_id, path) = rest.split_once('/')?;
    if !valid_connection_id(connection_id) {
        return None;
    }
    Some(RemoteRef {
        connection_id: connection_id.to_string(),
        path: format!("/{path}"),
    })
}

/// Connection ids travel inside URIs, so they are restricted to a flat,
/// shell-safe charset (see `connections::new_connection_id`).
pub(crate) fn valid_connection_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Compose the URI form of a remote path.
pub(crate) fn remote_uri(connection_id: &str, remote_path: &str) -> String {
    let path = if remote_path.starts_with('/') {
        remote_path
    } else {
        &format!("/{remote_path}")
    };
    format!("{REMOTE_URI_PREFIX}{connection_id}{path}")
}

/// Join a child name (which may nest with `/` or `\`) onto a remote path,
/// refusing escapes above the base. Mirrors `resolve_under` for URIs.
pub(crate) fn join_remote_path(base: &str, name: &str) -> Result<String, String> {
    let mut segments: Vec<&str> = base.trim_end_matches('/').split('/').collect();
    for segment in name.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.len() <= 1 {
                    return Err("Invalid path".into());
                }
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    Ok(segments.join("/"))
}

/// POSIX single-quote escaping: `'` becomes `'\''`.
pub(crate) fn sh_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for c in value.chars() {
        if c == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

/// The remote command string ssh hands to the remote shell.
///
/// Server target: `cd '<dir>' && exec '<argv0>' '<argv1>' …`
/// Container target: the same, wrapped in `docker exec -i[-t] <container>`.
/// `exec` makes the argv the process sshd tracks, so exit codes propagate and
/// a dropped connection signals it directly.
pub(crate) fn remote_command_string(
    container: Option<&str>,
    workdir: Option<&str>,
    argv: &[String],
    tty: bool,
) -> String {
    let mut inner = String::new();
    if let Some(dir) = workdir {
        if dir != "/" {
            inner.push_str("cd ");
            inner.push_str(&sh_quote(dir));
            inner.push_str(" && ");
        }
    }
    inner.push_str("exec");
    for arg in argv {
        inner.push(' ');
        inner.push_str(&sh_quote(arg));
    }
    wrap_for_container(container, inner, tty)
}

/// The same command string without the `exec` prefix, for the persistent
/// channel where the loop shell must survive each op.
fn inner_command_string(workdir: Option<&str>, argv: &[String]) -> String {
    remote_command_string(None, workdir, argv, false)
        .strip_prefix("exec ")
        .map(str::to_string)
        .unwrap_or_else(|| remote_command_string(None, workdir, argv, false))
}

fn wrap_for_container(container: Option<&str>, inner: String, tty: bool) -> String {
    match container {
        Some(container) => {
            let tty_flag = if tty { "-it" } else { "-i" };
            format!(
                "exec docker exec {tty_flag} {} sh -c {}",
                sh_quote(container),
                sh_quote(&inner)
            )
        }
        None => inner,
    }
}

/// Start building an `ssh` command for a profile. `tty` selects the
/// interactive mode (`-tt`, password prompts allowed) vs the background mode
/// (`-T` + BatchMode + keepalives).
pub(crate) fn ssh_command(
    app: &AppHandle,
    profile: &ConnectionProfile,
    tty: bool,
) -> Result<Command, String> {
    ssh_command_with_cache_dir(app.path().app_cache_dir().ok(), profile, tty)
}

fn ssh_command_with_cache_dir(
    cache_dir: Option<std::path::PathBuf>,
    profile: &ConnectionProfile,
    tty: bool,
) -> Result<Command, String> {
    let mut cmd = Command::new("ssh");
    crate::hide_window_console(&mut cmd);
    if tty {
        cmd.arg("-tt");
    } else {
        cmd.arg("-T")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=10")
            .arg("-o")
            .arg("ServerAliveInterval=30")
            .arg("-o")
            .arg("ServerAliveCountMax=4");
    }
    // Multiplexing turns every file op's ssh round-trip into a cheap hop on
    // one persistent connection. The control socket lives in /tmp: Unix
    // domain socket paths are capped at 104 bytes (macOS) / 108 (Linux), and
    // the app cache dir plus ssh's 40-char %C hash blows past that. ssh only
    // reuses sockets it owns, so /tmp's stickiness is safe. Unsupported on
    // Windows OpenSSH.
    #[cfg(not(windows))]
    if !tty {
        let _ = cache_dir;
        let uid = unsafe { libc::getuid() };
        let control_path = format!("/tmp/monocode-ssh-{uid}.%C");
        cmd.arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg(format!("ControlPath={control_path}"))
            .arg("-o")
            .arg("ControlPersist=10m");
    }
    if let Some(port) = profile.port {
        cmd.arg("-p").arg(port.to_string());
    }
    if let Some(user) = profile.user.as_deref() {
        if !user.trim().is_empty() {
            cmd.arg(format!("{user}@{}", profile.host));
            return Ok(cmd);
        }
    }
    cmd.arg(&profile.host);
    Ok(cmd)
}

/// Build the full ssh command running `argv` (remotely) in `workdir`.
pub(crate) fn remote_exec(
    app: &AppHandle,
    profile: &ConnectionProfile,
    workdir: Option<&str>,
    tty: bool,
    argv: &[String],
) -> Result<Command, String> {
    finish_remote_exec(ssh_command(app, profile, tty)?, profile, workdir, tty, argv)
}

fn finish_remote_exec(
    mut cmd: Command,
    profile: &ConnectionProfile,
    workdir: Option<&str>,
    tty: bool,
    argv: &[String],
) -> Result<Command, String> {
    cmd.arg("--").arg(remote_command_string(
        profile.container.as_deref(),
        workdir,
        argv,
        tty,
    ));
    Ok(cmd)
}

/// AppHandle-free variant for deep git/fs helpers: looks the profile up in
/// the global connection cache.
pub(crate) fn exec_for_remote(
    remote: &RemoteRef,
    workdir: Option<&str>,
    tty: bool,
    argv: &[String],
) -> Result<Command, String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    let cache_dir = crate::connections::global_cache_dir();
    finish_remote_exec(
        ssh_command_with_cache_dir(cache_dir, &profile, tty)?,
        &profile,
        workdir,
        tty,
        argv,
    )
}

/// A git invocation for a remote project root. The git env vars cannot be
/// set on the local ssh client, so they travel as an `env` prefix; every
/// argument is quoted for the remote shell (commit messages have spaces).
pub(crate) fn git_command(remote: &RemoteRef, args: &[&str]) -> Result<Command, String> {
    let mut argv: Vec<String> = vec![
        "env".into(),
        "GIT_OPTIONAL_LOCKS=0".into(),
        "GIT_TERMINAL_PROMPT=0".into(),
        "git".into(),
        "--no-pager".into(),
        "-C".into(),
        remote.path.clone(),
    ];
    argv.extend(args.iter().map(|arg| arg.to_string()));
    exec_for_remote(remote, None, false, &argv)
}

/// Captured remote git over the persistent channel — the hot path the UI's
/// git panel and branch polling hit on every focus change.
pub(crate) fn git_capture(remote: &RemoteRef, args: &[&str]) -> Result<CapturedOutput, String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    let mut argv: Vec<String> = vec![
        "env".into(),
        "GIT_OPTIONAL_LOCKS=0".into(),
        "GIT_TERMINAL_PROMPT=0".into(),
        "git".into(),
        "--no-pager".into(),
        "-C".into(),
        remote.path.clone(),
    ];
    argv.extend(args.iter().map(|arg| arg.to_string()));
    channel_exec(&profile, None, &argv)
}

/// A captured child run: stdout/stderr as lossy UTF-8 plus exit status.
pub(crate) struct CapturedOutput {
    pub code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CapturedOutput {
    pub(crate) fn from_output(output: std::process::Output) -> Self {
        CapturedOutput {
            code: output.status.code(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

enum CaptureMsg {
    Out(Vec<u8>),
    Err(Vec<u8>),
    Exit(Option<i32>),
}

const MAX_CAPTURE_BYTES: u64 = 8 * 1024 * 1024;

/// Run a command to completion with a hard timeout, killing it on expiry.
/// Reader threads keep draining so a killed child cannot wedge the pipes.
pub(crate) fn capture_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> Result<CapturedOutput, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to run {program}: {error}"))?;
    let pid = child.id();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = std::sync::mpsc::channel::<CaptureMsg>();
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(MAX_CAPTURE_BYTES).read_to_end(&mut buf);
        let _ = out_tx.send(CaptureMsg::Out(buf));
    });
    let err_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stderr).take(MAX_CAPTURE_BYTES).read_to_end(&mut buf);
        let _ = err_tx.send(CaptureMsg::Err(buf));
    });
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        let _ = tx.send(CaptureMsg::Exit(code));
    });

    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut code = None;
    let deadline = Instant::now() + timeout;
    // `child.wait()` can observe the exit before the pipe readers have
    // finished draining, so `Exit` alone never ends the wait: the channel
    // disconnects only when every sender (stdout, stderr, waiter) is done.
    let timed_out = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break true;
        }
        match rx.recv_timeout(remaining) {
            Ok(CaptureMsg::Out(buf)) => out = buf,
            Ok(CaptureMsg::Err(buf)) => err = buf,
            Ok(CaptureMsg::Exit(status)) => code = status,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break true,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break false,
        }
    };
    if timed_out {
        // Kill and keep draining so the reader threads finish.
        crate::harness::terminate(pid);
        for msg in rx.iter() {
            match msg {
                CaptureMsg::Out(buf) => out = buf,
                CaptureMsg::Err(buf) => err = buf,
                CaptureMsg::Exit(status) => code = status,
            }
        }
    }
    Ok(CapturedOutput {
        success: code == Some(0),
        code,
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    })
}

/// Human-readable failure for a finished ssh run. Exit 255 is ssh's own error
/// code (auth, DNS, unreachable) — callers must not confuse it with the
/// remote command's status.
pub(crate) fn ssh_failure(profile: &ConnectionProfile, output: &CapturedOutput) -> String {
    let detail = output.stderr.trim();
    let detail = if detail.is_empty() {
        output.stdout.trim().to_string()
    } else {
        detail.to_string()
    };
    let target = match profile.container.as_deref() {
        Some(container) => format!("{} (container {container})", profile.host),
        None => profile.host.clone(),
    };
    if output.code == Some(255) && detail.is_empty() {
        return format!("Could not connect to {target}.");
    }
    if detail.is_empty() {
        return format!(
            "Command failed on {target} (exit {}).",
            output.code.unwrap_or(-1)
        );
    }
    detail.to_string()
}

// ---------------------------------------------------------------------------
// Persistent exec channel.
//
// Every one-shot op pays the docker exec toll (~200-300 ms): ssh → host →
// docker CLI → dockerd → container. The channel keeps ONE long-running
// `docker exec … sh` loop over a dedicated ssh connection; each op is then a
// base64 line down the pipe and a framed response (~tens of ms). Ops are
// serialized per target, which is plenty for UI-sized reads.
// ---------------------------------------------------------------------------

const CHANNEL_LOOP: &str = r#"printf '__MONO_READY__ %s\n' "$$"
__p=$$
while IFS= read -r __cmd; do
  [ -n "$__cmd" ] || continue
  printf '\n__MONO_O_%s__\n' "$__p"
  printf '%s' "$__cmd" | base64 -d | sh 2>&1
  printf '\n__MONO_R_%s_%s__\n' "$__p" "$?"
done"#;

struct ChannelState {
    stdin: std::process::ChildStdin,
    reader: std::io::BufReader<std::process::ChildStdout>,
    child: std::process::Child,
    /// The remote loop shell's pid — markers embed it so command output can
    /// never counterfeit a frame boundary.
    marker_pid: String,
}

impl Drop for ChannelState {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn channels() -> &'static Mutex<std::collections::HashMap<String, Arc<Mutex<ChannelState>>>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<String, Arc<Mutex<ChannelState>>>>> =
        OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn spawn_channel(profile: &ConnectionProfile) -> Result<ChannelState, String> {
    let argv = vec!["sh".to_string(), "-c".to_string(), CHANNEL_LOOP.to_string()];
    let mut command = finish_remote_exec(
        ssh_command_with_cache_dir(crate::connections::global_cache_dir(), profile, false)?,
        profile,
        None,
        false,
        &argv,
    )?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not open the remote channel: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Remote channel has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Remote channel has no stdout".to_string())?;
    let mut reader = std::io::BufReader::new(stdout);
    let mut banner = String::new();
    reader
        .read_line(&mut banner)
        .map_err(|error| format!("Remote channel did not start: {error}"))?;
    let marker_pid = banner
        .trim()
        .strip_prefix("__MONO_READY__ ")
        .ok_or_else(|| "Remote channel sent an unexpected banner".to_string())?
        .to_string();
    Ok(ChannelState {
        stdin,
        reader,
        child,
        marker_pid,
    })
}

fn channel_state(profile: &ConnectionProfile) -> Result<Arc<Mutex<ChannelState>>, String> {
    let key = helper_cache_key(profile);
    if let Ok(map) = channels().lock() {
        if let Some(state) = map.get(&key) {
            return Ok(state.clone());
        }
    }
    let state = Arc::new(Mutex::new(spawn_channel(profile)?));
    if let Ok(mut map) = channels().lock() {
        map.insert(key, state.clone());
    }
    Ok(state)
}

/// Remove the channel from the registry so the next op respawns it. The
/// underlying child dies with its last Arc (see Drop).
fn drop_channel(profile: &ConnectionProfile) {
    let key = helper_cache_key(profile);
    if let Ok(mut map) = channels().lock() {
        map.remove(&key);
    }
}

/// Run one op down the channel. `argv` is the command as it should run inside
/// the target (container); quoting happens exactly as for one-shot exec.
fn channel_exec(
    profile: &ConnectionProfile,
    workdir: Option<&str>,
    argv: &[String],
) -> Result<CapturedOutput, String> {
    channel_exec_raw(profile, &inner_command_string(workdir, argv))
}

/// Run a pre-composed remote command string down the channel, respawning the
/// channel once if it died (container restart, dropped ssh).
fn channel_exec_raw(profile: &ConnectionProfile, inner: &str) -> Result<CapturedOutput, String> {
    match channel_exec_inner(profile, inner) {
        Ok(output) => Ok(output),
        Err(error) => {
            drop_channel(profile);
            channel_exec_inner(profile, inner).map_err(|retry| format!("{error} ({retry})"))
        }
    }
}

fn channel_exec_inner(profile: &ConnectionProfile, inner: &str) -> Result<CapturedOutput, String> {
    use std::io::Write;
    let state = channel_state(profile)?;
    let mut guard = state.lock().map_err(|_| "Remote channel lock poisoned")?;
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, inner);
    if let Err(error) = guard
        .stdin
        .write_all(encoded.as_bytes())
        .and_then(|_| guard.stdin.write_all(b"\n"))
        .and_then(|_| guard.stdin.flush())
    {
        drop_channel(profile);
        return Err(format!("Remote channel broke: {error}"));
    }

    let out_marker = format!("__MONO_O_{}__", guard.marker_pid);
    let rc_prefix = format!("__MONO_R_{}_", guard.marker_pid);

    let mut frame = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = guard
            .reader
            .read_until(b'\n', &mut line)
            .map_err(|error| format!("Remote channel read failed: {error}"))?;
        if read == 0 {
            drop_channel(profile);
            return Err("Remote channel closed".into());
        }
        let is_end = line.last() == Some(&b'\n')
            && line.starts_with(rc_prefix.as_bytes())
            && line.ends_with(b"__\n");
        frame.extend_from_slice(&line);
        if is_end {
            break;
        }
    }

    // Line-oriented parse: content sits between the O and R marker lines.
    // stderr is merged into stdout by the loop (2>&1) — failure detail still
    // surfaces via ssh_failure's stdout fallback.
    let lines: Vec<&[u8]> = frame.split(|byte| *byte == b'\n').collect();
    let out_at = lines
        .iter()
        .position(|line| *line == out_marker.as_bytes())
        .unwrap_or(0);
    let rc_at = lines
        .iter()
        .rposition(|line| line.starts_with(rc_prefix.as_bytes()) && line.ends_with(b"__"))
        .unwrap_or(lines.len().saturating_sub(1));
    let stdout = lines[out_at + 1..rc_at.max(out_at + 1)]
        .iter()
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<String>>()
        .join("\n");
    let rc = String::from_utf8_lossy(lines.get(rc_at).copied().unwrap_or(&[]))
        .trim()
        .trim_start_matches(&rc_prefix)
        .trim_end_matches("__")
        .parse::<i32>()
        .ok();
    Ok(CapturedOutput {
        code: rc,
        success: rc == Some(0),
        stdout,
        stderr: String::new(),
    })
}

// ---------------------------------------------------------------------------
// Process-wide caches, keyed by connection.
// ---------------------------------------------------------------------------

/// Targets we have uploaded the file-op helper script to.
fn installed_helpers() -> &'static Mutex<std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Cache key for the helper: the helper is installed per connection (and for
/// container connections it lives inside the container filesystem).
pub(crate) fn helper_cache_key(profile: &ConnectionProfile) -> String {
    profile.id.clone()
}

/// Resolved agent binaries per (connection, agent), with a TTL so a CLI
/// installed mid-session is picked up without an app restart (M5).
type AgentResolutionMap = std::collections::HashMap<(String, String), (String, Instant)>;

fn agent_resolutions() -> &'static Mutex<AgentResolutionMap> {
    static MAP: OnceLock<Mutex<AgentResolutionMap>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

const AGENT_RESOLUTION_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) fn cached_agent_resolution(connection_id: &str, agent: &str) -> Option<String> {
    let map = agent_resolutions().lock().ok()?;
    let (path, resolved_at) = map.get(&(connection_id.to_string(), agent.to_string()))?;
    if resolved_at.elapsed() > AGENT_RESOLUTION_TTL {
        return None;
    }
    Some(path.clone())
}

pub(crate) fn store_agent_resolution(connection_id: &str, agent: &str, path: &str) {
    if let Ok(mut map) = agent_resolutions().lock() {
        map.insert(
            (connection_id.to_string(), agent.to_string()),
            (path.to_string(), Instant::now()),
        );
    }
}

pub(crate) fn mark_helper_installed(profile: &ConnectionProfile) {
    if let Ok(mut set) = installed_helpers().lock() {
        set.insert(helper_cache_key(profile));
    }
}

pub(crate) fn helper_is_installed(profile: &ConnectionProfile) -> bool {
    installed_helpers()
        .lock()
        .map(|set| set.contains(&helper_cache_key(profile)))
        .unwrap_or(false)
}

/// Drop every cached artifact for a connection (called on profile removal or
/// edit, so a changed host/container cannot reuse stale state).
pub(crate) fn invalidate_connection(connection_id: &str) {
    if let Ok(mut set) = installed_helpers().lock() {
        set.remove(connection_id);
    }
    if let Ok(mut map) = agent_resolutions().lock() {
        map.retain(|(conn, _), _| conn != connection_id);
    }
    // The persistent channel was spawned with the previous profile; drop it
    // so the next op reconnects with the current settings.
    if let Ok(mut map) = channels().lock() {
        map.remove(connection_id);
    }
}

// ---------------------------------------------------------------------------
// Remote file operations.
//
// A small POSIX sh helper is uploaded to the target once per connection and
// implements the primitives (ls / stat / read / write / …) with portable
// tooling. Names travel hex-encoded so any byte sequence survives; paths
// arrive shell-quoted by `sh_quote`. gitignore flags come from a second
// `git check-ignore` hop (skipped silently outside a repo).
// ---------------------------------------------------------------------------

const HELPER_VERSION: u8 = 3;

const HELPER_SCRIPT: &str = r#"#!/bin/sh
# MonoCode remote file helper. Output lines are hex-encoded so names with
# spaces, quotes, or newlines survive the ssh transport unchanged.
#
# Containers can charge ~30 ms per process spawn, so the hot ops (ls, stat)
# must not fork per entry: ls hex-encodes the whole listing in one awk pass,
# stat tries one bulk invocation before falling back per file.
set -u

fail() { echo "$2" >&2; exit 1; }
# Byte-exact hex of a whole NUL-separated stream (single od process).
hex() { od -An -v -tx1 | tr -d ' \n'; }
# Byte-exact hex of stdin, one output line per input line. LC_ALL=C keeps
# awk byte-oriented so multibyte names encode correctly.
hexlines() {
  LC_ALL=C awk 'BEGIN { t = ""; for (i = 1; i < 256; i++) t = t sprintf("%c", i) }
    { s = $0; o = ""; for (i = 1; i <= length(s); i++) o = o sprintf("%02x", index(t, substr(s, i, 1))); print o }'
}

op=${1:-}
[ $# -gt 0 ] && shift

case "$op" in
  home)
    printf '%s\n' "${HOME:-/}"
    ;;
  ls)
    [ $# -ge 1 ] || fail ls "missing directory"
    dir=$1
    [ -d "$dir" ] || fail ls "not a directory: $dir"
    for entry in "$dir"/* "$dir"/.[!.]* "$dir"/..?*; do
      [ -e "$entry" ] || continue
      name=${entry##*/}
      [ "$name" = ".DS_Store" ] && continue
      if [ -d "$entry" ]; then printf 'D%s\n' "$name"; else printf 'F%s\n' "$name"; fi
    done | hexlines
    ;;
  ignored)
    # Names arrive newline-separated on stdin; prints hex(newline-separated).
    # (check-ignore -z would need NUL-separated input, which POSIX sh cannot
    # produce; names containing newlines simply go unflagged.)
    [ $# -ge 1 ] || fail ignored "missing directory"
    git -C "$1" check-ignore --stdin 2>/dev/null | hexlines
    ;;
  stat)
    # One mtime (seconds) per input path, "-" when absent or not a file.
    # A non-zero bulk exit means some path failed — fall back per file.
    if bulk=$(stat -c %Y "$@" 2>/dev/null) || bulk=$(stat -f %m "$@" 2>/dev/null); then
      printf '%s\n' "$bulk"
    else
      for p in "$@"; do
        m=""
        if [ -f "$p" ]; then
          m=$(stat -c %Y "$p" 2>/dev/null || stat -f %m "$p" 2>/dev/null) || m=""
        fi
        printf '%s\n' "${m:--}"
      done
    fi
    ;;
  read)
    [ $# -ge 2 ] || fail read "missing path or cap"
    [ -f "$1" ] || fail read "no such file: $1"
    cat "$1" | head -c "$2"
    ;;
  write)
    # Payload arrives on stdin; temp file in the same directory keeps the
    # final move atomic.
    [ $# -ge 1 ] || fail write "missing path"
    dest=$1
    dir=$(dirname "$dest")
    [ -d "$dir" ] || fail write "no such directory: $dir"
    tmp="$dir/.monocode-write.$$"
    if ! cat > "$tmp"; then
      rm -f "$tmp"
      fail write "cannot write to $dir"
    fi
    if ! mv -f "$tmp" "$dest"; then
      rm -f "$tmp"
      fail write "cannot replace $dest"
    fi
    ;;
  create)
    # Atomic no-clobber empty-file create: `set -C` makes the redirection
    # fail when the target exists, so a new file never replaces anything.
    [ $# -ge 1 ] || fail create "missing path"
    dir=$(dirname "$1")
    [ -d "$dir" ] || fail create "no such directory: $dir"
    if ( set -C; : > "$1" ) 2>/dev/null; then
      :
    elif [ -e "$1" ]; then
      fail create "already exists: $1"
    else
      fail create "cannot create $1"
    fi
    ;;
  mkdir)
    # Creating an existing path is "already exists", not success — the local
    # create contract reports the collision instead of adopting the dir.
    [ $# -ge 1 ] || fail mkdir "missing path"
    [ -e "$1" ] && fail mkdir "already exists: $1"
    mkdir -p "$1" || fail mkdir "cannot create $1"
    ;;
  rename)
    [ $# -ge 2 ] || fail rename "missing paths"
    [ -e "$2" ] && fail rename "already exists: $2"
    mv -f "$1" "$2" || fail rename "cannot rename $1"
    ;;
  delete)
    [ $# -ge 1 ] || fail delete "missing path"
    [ -e "$1" ] || fail delete "no such path: $1"
    rm -rf "$1" || fail delete "cannot delete $1"
    ;;
  copy)
    [ $# -ge 2 ] || fail copy "missing paths"
    [ -e "$1" ] || fail copy "no such path: $1"
    cp -R -p "$1" "$2" || fail copy "cannot copy $1"
    ;;
  move)
    [ $# -ge 2 ] || fail move "missing paths"
    [ -e "$1" ] || fail move "no such path: $1"
    [ -e "$2" ] && fail move "already exists: $2"
    mv -f "$1" "$2" || fail move "cannot move $1"
    ;;
  find)
    # Quick-open index: git-tracked + untracked non-ignored when possible,
    # else a bounded walk that prunes vendor trees. Hex(NUL-separated).
    [ $# -ge 1 ] || fail find "missing root"
    root=$1
    if git -C "$root" rev-parse --git-dir >/dev/null 2>&1; then
      git -C "$root" ls-files -co --exclude-standard -z | hex
    else
      find "$root" \( -name node_modules -o -name .git -o -name vendor -o -name target -o -name dist \) -prune -o -type f -print0 | hex
    fi
    echo
    ;;
  *)
    fail usage "unknown op: $op"
    ;;
esac
"#;

fn helper_remote_path() -> String {
    format!("$HOME/.cache/monocode/helper-v{HELPER_VERSION}.sh")
}

/// The `sh -c` dispatch that expands `$HOME` on the target and hands the op
/// and paths over as `"$@"` — keeping Rust's per-argument quoting intact.
fn helper_dispatch() -> String {
    format!("exec \"{}\" \"$@\"", helper_remote_path())
}

fn helper_op(remote: &RemoteRef, op: &str, args: &[&str]) -> Result<std::process::Command, String> {
    let mut argv: Vec<String> = vec![
        "sh".to_string(),
        "-c".to_string(),
        helper_dispatch(),
        "sh".to_string(),
        op.to_string(),
    ];
    argv.extend(args.iter().map(|arg| arg.to_string()));
    exec_for_remote(remote, None, false, &argv)
}

fn helper_run(remote: &RemoteRef, op: &str, args: &[&str]) -> Result<CapturedOutput, String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    ensure_helper(remote, &profile)?;
    // Direct helper invocation down the channel: $HOME expands on the target
    // and quoting applies per argument, same as the one-shot dispatch.
    let mut inner = format!(
        "$HOME/.cache/monocode/helper-v{HELPER_VERSION}.sh {}",
        sh_quote(op)
    );
    for arg in args {
        inner.push(' ');
        inner.push_str(&sh_quote(arg));
    }
    channel_exec_raw(&profile, &inner).map_err(|error| format!("{} ({op}): {error}", profile.host))
}

fn helper_run_checked(remote: &RemoteRef, op: &str, args: &[&str]) -> Result<String, String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    let output = helper_run(remote, op, args)?;
    if !output.success {
        return Err(ssh_failure(&profile, &output));
    }
    Ok(output.stdout)
}

/// Upload the helper once per target. For container connections the script
/// lands inside the container filesystem ($HOME there).
fn ensure_helper(remote: &RemoteRef, profile: &ConnectionProfile) -> Result<(), String> {
    if helper_is_installed(profile) {
        return Ok(());
    }
    let check = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("test -x \"{}\"", helper_remote_path()),
    ];
    let mut command = exec_for_remote(remote, None, false, &check)?;
    let output = capture_timeout(&mut command, Duration::from_secs(20))?;
    if !output.success {
        let install = vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "mkdir -p \"$HOME/.cache/monocode\" && cat > \"{}\" && chmod 700 \"{}\"",
                helper_remote_path(),
                helper_remote_path()
            ),
        ];
        let mut command = exec_for_remote(remote, None, false, &install)?;
        use std::io::Write;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not upload helper to {}: {error}", profile.host))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(HELPER_SCRIPT.as_bytes())
                .and_then(|_| stdin.flush())
                .map_err(|error| format!("Could not upload helper: {error}"))?;
        }
        drop(child.stdin.take());
        let status = child
            .wait()
            .map_err(|error| format!("Helper upload failed: {error}"))?;
        if !status.success() {
            return Err("Could not install the remote helper script.".into());
        }
    }
    mark_helper_installed(profile);
    Ok(())
}

// --- transport-safe decoding -------------------------------------------------

fn decode_hex(line: &str) -> Option<Vec<u8>> {
    let line = line.trim();
    if !line.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(line.len() / 2);
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let hi = chars[index].to_digit(16)?;
        let lo = chars[index + 1].to_digit(16)?;
        bytes.push((hi * 16 + lo) as u8);
        index += 2;
    }
    Some(bytes)
}

fn decode_hex_str(line: &str) -> Option<String> {
    decode_hex(line).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Decode a hex blob back into its NUL-separated records.
fn decode_hex_records(line: &str) -> Vec<String> {
    decode_hex(line)
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|record| !record.is_empty())
                .map(|record| String::from_utf8_lossy(record).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Decode a hex blob back into its newline-separated records.
fn decode_hex_lines(line: &str) -> Vec<String> {
    decode_hex(line)
        .map(|bytes| {
            bytes
                .split(|byte| *byte == b'\n')
                .filter(|record| !record.is_empty())
                .map(|record| String::from_utf8_lossy(record).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

// --- remote twins of the fs.rs operations ------------------------------------

/// Remote $HOME (seeds the connect dialog's path browser).
pub(crate) fn remote_home_dir(remote: &RemoteRef) -> Result<String, String> {
    helper_run_checked(remote, "home", &[])
        .map(|out| out.lines().next().unwrap_or("/").trim().to_string())
}

/// Remote $HOME for a connection id — used by the connect dialog.
#[tauri::command]
pub async fn remote_home(connection_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = crate::connections::profile_by_id(&connection_id)?;
        let remote = RemoteRef {
            connection_id: profile.id,
            path: "/".to_string(),
        };
        remote_home_dir(&remote)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Where each agent CLI installs, for the not-found error hint.
fn agent_install_hint(agent: &str) -> &'static str {
    match agent {
        "claude" => "npm install -g @anthropic-ai/claude-code",
        "codex" => "npm install -g @openai/codex",
        _ => "npm install -g <agent>",
    }
}

/// Resolve an agent CLI on a remote target. Uses a login shell so
/// nvm/mise-managed paths load; caches for a few minutes so an install via
/// the remote terminal is picked up without an app restart.
#[tauri::command]
pub async fn remote_resolve_agent(
    connection_id: String,
    agent: String,
) -> Result<RemoteBinary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !["claude", "codex"].contains(&agent.as_str()) {
            return Err(format!("{agent} is not available for remote sessions yet."));
        }
        if let Some(cached) = cached_agent_resolution(&connection_id, &agent) {
            return Ok(RemoteBinary { path: cached });
        }
        let profile = crate::connections::profile_by_id(&connection_id)?;
        let remote = RemoteRef {
            connection_id: profile.id,
            path: "/".to_string(),
        };
        let target = match profile.container.as_deref() {
            Some(container) => format!("{} (container {container})", profile.host),
            None => profile.host.clone(),
        };
        // Login shell so ~/.profile/nvm paths apply; `command -v` is POSIX.
        let probe = vec![
            "sh".to_string(),
            "-lc".to_string(),
            format!("command -v {agent} 2>/dev/null || true"),
        ];
        let mut command = exec_for_remote(&remote, None, false, &probe)?;
        let output = capture_timeout(&mut command, Duration::from_secs(20))?;
        let found = output.stdout.lines().next().map(str::trim).unwrap_or("");
        if !output.success || found.is_empty() || !found.starts_with('/') {
            return Err(format!(
                "{agent} is not installed on {target}. Open a terminal on the remote project and run: {}",
                agent_install_hint(&agent)
            ));
        }
        store_agent_resolution(&connection_id, &agent, found);
        Ok(RemoteBinary {
            path: found.to_string(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBinary {
    pub path: String,
}

/// One-shot captured exec of a resolved remote agent binary — the remote
/// twin of `harness_exec`, used for `--version` and model catalogs.
#[tauri::command]
pub async fn remote_harness_exec(
    connection_id: String,
    command: String,
    args: Vec<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !crate::harness::exec_args_allowed(&args) {
            return Err("Remote exec arguments are not allowed".into());
        }
        let resolved = cached_agent_resolution(&connection_id, "claude")
            .filter(|path| path == &command)
            .or_else(|| {
                cached_agent_resolution(&connection_id, "codex").filter(|path| path == &command)
            })
            .ok_or_else(|| "Resolve the agent on this server first".to_string())?;
        let remote = RemoteRef {
            connection_id,
            path: "/".to_string(),
        };
        let mut argv = vec![resolved];
        argv.extend(args);
        let mut command = exec_for_remote(&remote, None, false, &argv)?;
        let output = capture_timeout(&mut command, Duration::from_secs(15))?;
        if !output.success {
            return Err(ssh_failure(
                &crate::connections::profile_by_id(&remote.connection_id)?,
                &output,
            ));
        }
        Ok(output.stdout)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn remote_list_dir(remote: &RemoteRef) -> Result<Vec<crate::fs::DirEntry>, String> {
    let out = helper_run_checked(remote, "ls", &[&remote.path])?;
    let mut entries: Vec<(bool, String)> = Vec::new();
    for line in out.lines() {
        let Some(decoded) = decode_hex_str(line) else {
            continue;
        };
        let mut chars = decoded.chars();
        match chars.next() {
            Some('D') => entries.push((true, chars.as_str().to_string())),
            Some('F') => entries.push((false, chars.as_str().to_string())),
            _ => continue,
        }
    }
    // gitignore flags in a second hop; absent outside a repo.
    let mut ignored: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !entries.is_empty() {
        if let Ok(output) = helper_run(remote, "ignored", &[&remote.path]) {
            ignored.extend(decode_hex_lines(output.stdout.trim()));
        }
    }
    let base = remote_uri(&remote.connection_id, &remote.path);
    let mut out = Vec::with_capacity(entries.len());
    for (is_dir, name) in entries {
        let mut child = base.clone();
        if !child.ends_with('/') {
            child.push('/');
        }
        child.push_str(&name);
        out.push(crate::fs::DirEntry {
            ignored: ignored.contains(&name),
            name,
            path: child,
            is_dir,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    Ok(out)
}

pub(crate) fn remote_list_project_files(
    remote: &RemoteRef,
) -> Result<Vec<crate::fs::ProjectFile>, String> {
    let out = helper_run_checked(remote, "find", &[&remote.path])?;
    let base = remote_uri(&remote.connection_id, &remote.path);
    let mut files = Vec::new();
    for relative in decode_hex_records(out.trim()) {
        if relative.is_empty() {
            continue;
        }
        // The fallback walk emits absolute paths; the git path emits relative.
        let relative = relative
            .strip_prefix(&format!("{}/", remote.path.trim_end_matches('/')))
            .map(str::to_string)
            .unwrap_or(relative);
        if relative.starts_with('/') || relative.contains('\0') {
            continue;
        }
        let name = relative.rsplit('/').next().unwrap_or(&relative).to_string();
        let mut path = base.clone();
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(&relative);
        files.push(crate::fs::ProjectFile {
            name,
            path,
            relative,
        });
        if files.len() >= 20_000 {
            break;
        }
    }
    Ok(files)
}

pub(crate) fn remote_stat_files(remote: &RemoteRef, paths: &[String]) -> Vec<crate::fs::FileMtime> {
    let mut out = Vec::with_capacity(paths.len());
    let refs: Vec<String> = paths
        .iter()
        .map(|path| parse_remote(path).map(|parsed| parsed.path))
        .map(|parsed| parsed.unwrap_or_default())
        .collect();
    let args: Vec<&str> = refs.iter().map(|path| path.as_str()).collect();
    let output = helper_run(remote, "stat", &args).ok();
    let mut lines = output
        .as_ref()
        .map(|output| output.stdout.lines().collect::<Vec<&str>>())
        .unwrap_or_default()
        .into_iter();
    for path in paths {
        let mtime_ms = lines
            .next()
            .and_then(|line| line.trim().parse::<u64>().ok())
            .map(|seconds| seconds * 1000);
        out.push(crate::fs::FileMtime {
            path: path.clone(),
            mtime_ms,
        });
    }
    out
}

pub(crate) fn remote_read_bytes(remote: &RemoteRef, max_bytes: u64) -> Result<Vec<u8>, String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    ensure_helper(remote, &profile)?;
    let cap = max_bytes.to_string();
    let mut command = helper_op(remote, "read", &[&remote.path, &cap])?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", profile.host))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut buf = Vec::new();
    let _ = (&mut stdout).take(max_bytes + 1).read_to_end(&mut buf);
    let mut err = String::new();
    let _ = stderr.read_to_string(&mut err);
    let status = child.wait();
    match status {
        Ok(status) if !status.success() => {
            let detail = err.trim();
            if detail.is_empty() {
                Err("Could not read the remote file.".into())
            } else {
                Err(detail.to_string())
            }
        }
        _ if buf.len() as u64 > max_bytes => Err(format!(
            "File is larger than the {} MB limit.",
            max_bytes / 1024 / 1024
        )),
        _ => Ok(buf),
    }
}

pub(crate) fn remote_write(remote: &RemoteRef, contents: &[u8]) -> Result<(), String> {
    let profile = crate::connections::profile_by_id(&remote.connection_id)?;
    ensure_helper(remote, &profile)?;
    let mut command = helper_op(remote, "write", &[&remote.path])?;
    use std::io::Write;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", profile.host))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(contents)
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("Could not write the remote file: {error}"))?;
    }
    drop(child.stdin.take());
    let status = child
        .wait()
        .map_err(|error| format!("Remote write failed: {error}"))?;
    if !status.success() {
        return Err("Could not save the remote file.".into());
    }
    Ok(())
}

fn remote_mutation(remote: &RemoteRef, op: &str, args: &[&str]) -> Result<(), String> {
    helper_run_checked(remote, op, args).map(|_| ())
}

pub(crate) fn remote_create_dir(remote: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "mkdir", &[&remote.path])
}

/// Atomic no-clobber empty-file create, for "new file" — unlike
/// [`remote_write`], an existing file is never replaced.
pub(crate) fn remote_create_file(remote: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "create", &[&remote.path])
}

pub(crate) fn remote_rename(remote: &RemoteRef, dest: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "rename", &[&remote.path, &dest.path])
}

pub(crate) fn remote_delete(remote: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "delete", &[&remote.path])
}

pub(crate) fn remote_copy(remote: &RemoteRef, dest: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "copy", &[&remote.path, &dest.path])
}

pub(crate) fn remote_move(remote: &RemoteRef, dest: &RemoteRef) -> Result<(), String> {
    remote_mutation(remote, "move", &[&remote.path, &dest.path])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parses_remote_uris() {
        let parsed = parse_remote("ssh://conn_abc123/home/user/project").unwrap();
        assert_eq!(parsed.connection_id, "conn_abc123");
        assert_eq!(parsed.path, "/home/user/project");

        assert_eq!(parse_remote("/home/user/project"), None);
        assert_eq!(parse_remote("~"), None);
        assert_eq!(parse_remote("ssh://conn_abc123"), None); // no path
        assert_eq!(parse_remote("ssh://Conn_Abc/x"), None); // uppercase id
        assert_eq!(parse_remote("ssh://bad-id/x"), None); // dash not allowed
    }

    #[test]
    fn uri_roundtrips() {
        let uri = remote_uri("conn_abc123", "/home/user/my project");
        assert_eq!(uri, "ssh://conn_abc123/home/user/my project");
        let parsed = parse_remote(&uri).unwrap();
        assert_eq!(parsed.path, "/home/user/my project");
        assert_eq!(
            parse_remote(&remote_uri("conn_abc123", "/")).unwrap().path,
            "/"
        );
    }

    #[test]
    fn quotes_shell_metacharacters() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's"), r#"'it'\''s'"#);
        assert_eq!(sh_quote("a b$c `d`; e"), r#"'a b$c `d`; e'"#);
        assert_eq!(sh_quote("路径/空白"), "'路径/空白'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn builds_server_command_string() {
        let out = remote_command_string(
            None,
            Some("/srv/app"),
            &argv(&["claude", "--version"]),
            false,
        );
        assert_eq!(out, "cd '/srv/app' && exec 'claude' '--version'");
    }

    #[test]
    fn builds_server_command_without_workdir() {
        let out = remote_command_string(None, None, &argv(&["git", "status"]), false);
        assert_eq!(out, "exec 'git' 'status'");
    }

    #[test]
    fn skips_cd_for_root_workdir() {
        let out = remote_command_string(None, Some("/"), &argv(&["uname"]), false);
        assert_eq!(out, "exec 'uname'");
    }

    #[test]
    fn builds_container_command_string() {
        let out = remote_command_string(
            Some("web-1"),
            Some("/work"),
            &argv(&["claude", "--model", "opus"]),
            false,
        );
        // Host shell words: the whole inner command must be one argument.
        assert_eq!(
            shell_split(&out),
            vec![
                "exec",
                "docker",
                "exec",
                "-i",
                "web-1",
                "sh",
                "-c",
                "cd '/work' && exec 'claude' '--model' 'opus'"
            ]
        );
        // And the inner command parses to the intended words.
        assert_eq!(
            shell_split("cd '/work' && exec 'claude' '--model' 'opus'"),
            vec!["cd", "/work", "&&", "exec", "claude", "--model", "opus"]
        );
        let tty = remote_command_string(Some("web"), Some("/work"), &argv(&["bash"]), true);
        assert!(tty.starts_with("exec docker exec -it 'web' sh -c '"));
    }

    #[test]
    fn quotes_spaces_and_apostrophes_in_every_layer() {
        let out = remote_command_string(
            Some("my container"),
            Some("/srv/it's app"),
            &argv(&["/usr/local/bin/my agent", "-msg", "don't"]),
            false,
        );
        // Host shell: container name and the whole inner command each parse
        // back as single words despite spaces and quotes.
        let words = shell_split(&out);
        assert_eq!(
            words[..7],
            vec!["exec", "docker", "exec", "-i", "my container", "sh", "-c"]
        );
        // Inner shell: workdir and each argv element survive the second parse.
        assert_eq!(
            shell_split(&words[7]),
            vec![
                "cd",
                "/srv/it's app",
                "&&",
                "exec",
                "/usr/local/bin/my agent",
                "-msg",
                "don't"
            ]
        );
    }

    #[test]
    fn quotes_metacharacters_on_the_server_path() {
        let out = remote_command_string(
            None,
            Some("/srv/we;ird $path"),
            &argv(&["/bin/echo", "a`b", "c|d", "e&f", "g(h)", "*"]),
            false,
        );
        assert_eq!(
            shell_split(&out),
            vec![
                "cd",
                "/srv/we;ird $path",
                "&&",
                "exec",
                "/bin/echo",
                "a`b",
                "c|d",
                "e&f",
                "g(h)",
                "*"
            ]
        );
    }

    /// Split a command string into words the way /bin/sh would: single
    /// quotes delimit verbatim, backslash escapes outside quotes. Test-only —
    /// it proves the builder's output re-parses to the intended words.
    fn shell_split(input: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut has_word = false;
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            if in_quotes {
                if c == '\'' {
                    in_quotes = false;
                } else {
                    current.push(c);
                }
                continue;
            }
            match c {
                '\'' => {
                    in_quotes = true;
                    has_word = true;
                }
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        current.push(next);
                        chars.next();
                        has_word = true;
                    }
                }
                ' ' | '\t' | '\n' => {
                    if has_word {
                        words.push(std::mem::take(&mut current));
                        has_word = false;
                    }
                }
                _ => {
                    current.push(c);
                    has_word = true;
                }
            }
        }
        if has_word {
            words.push(current);
        }
        words
    }
}
