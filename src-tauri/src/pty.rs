use std::collections::HashMap;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dirs_home;
use crate::fs::expand_home;

const DATA_EVENT: &str = "pty-data";
const EXIT_EVENT: &str = "pty-exit";
const READ_CHUNK: usize = 32 * 1024;
/// Cap how often a busy PTY hops the webview. Each `emit` is a JS eval; a
/// flood of small reads was thousands per second and froze input.
const PTY_COALESCE: Duration = Duration::from_millis(8);
#[cfg(unix)]
const KILL_ESCALATE: Duration = Duration::from_secs(1);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyData {
    id: String,
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyExit {
    id: String,
    code: Option<i32>,
}

struct LivePty {
    writer: Mutex<Box<dyn Write + Send>>,
    #[cfg(unix)]
    master_fd: i32,
    #[cfg(windows)]
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    pid: u32,
}

pub struct PtyHost {
    sessions: Mutex<HashMap<String, Arc<LivePty>>>,
}

impl PtyHost {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn insert(&self, id: String, live: Arc<LivePty>) -> Option<Arc<LivePty>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, live)
    }

    fn get(&self, id: &str) -> Option<Arc<LivePty>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn remove(&self, id: &str) -> Option<Arc<LivePty>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
    }

    fn remove_if_pid(&self, id: &str, pid: u32) -> Option<Arc<LivePty>> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if sessions.get(id).map(|live| live.pid) != Some(pid) {
            return None;
        }
        sessions.remove(id)
    }

    pub(crate) fn kill_all(&self) {
        let kids: Vec<Arc<LivePty>> = {
            let mut map = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            map.drain().map(|(_, live)| live).collect()
        };
        let pids: Vec<u32> = kids.iter().map(|live| live.pid).collect();
        for live in kids {
            #[cfg(unix)]
            {
                hangup(live.pid);
                close_fd(live.master_fd);
            }
            #[cfg(not(unix))]
            drop(live);
        }
        // Quit and `Drop` both exit the process, so the SIGKILL has to land
        // before this returns. `terminate`'s detached escalate thread never gets
        // to run, and every shell is its own `setsid` session that outlives us.
        crate::harness::terminate_all(&pids);
    }
}

impl Drop for PtyHost {
    fn drop(&mut self) {
        self.kill_all();
    }
}

#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    host: State<PtyHost>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    if let Some(prev) = host.remove(&id) {
        terminate(prev.pid);
        #[cfg(unix)]
        close_fd(prev.master_fd);
    }

    #[cfg(unix)]
    {
        spawn_unix(app, host, id, cwd, cols.max(2), rows.max(2))
    }

    #[cfg(windows)]
    {
        spawn_windows(app, host, id, cwd, cols.max(2), rows.max(2))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (app, cwd, cols, rows);
        Err("Terminals are not supported on this platform.".into())
    }
}

#[tauri::command]
pub fn pty_write(host: State<PtyHost>, id: String, data: String) -> Result<(), String> {
    let live = host
        .get(&id)
        .ok_or_else(|| "Terminal is not running".to_string())?;
    let mut writer = live.writer.lock().unwrap_or_else(|e| e.into_inner());
    writer
        .write_all(data.as_bytes())
        .and_then(|_| writer.flush())
        .map_err(|e| format!("Failed to write to terminal: {e}"))
}

#[tauri::command]
pub fn pty_resize(host: State<PtyHost>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let live = host
        .get(&id)
        .ok_or_else(|| "Terminal is not running".to_string())?;
    #[cfg(unix)]
    {
        resize_fd(live.master_fd, cols.max(2), rows.max(2))
    }
    #[cfg(windows)]
    {
        let master = live.master.lock().unwrap_or_else(|e| e.into_inner());
        master
            .resize(portable_pty::PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| format!("Failed to resize terminal: {err}"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (live, cols, rows);
        Err("Terminals are not supported on this platform.".into())
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PtyStatus {
    foreground: Option<String>,
}

/// Off the main thread: this forks `ps`, and the title poll calls it once a
/// second for every open terminal.
#[tauri::command(async)]
pub fn pty_status(host: State<'_, PtyHost>, id: String) -> Result<PtyStatus, String> {
    let live = host
        .get(&id)
        .ok_or_else(|| "Terminal is not running".to_string())?;
    #[cfg(unix)]
    {
        let foreground = foreground_label(live.master_fd, live.pid);
        Ok(PtyStatus { foreground })
    }
    #[cfg(not(unix))]
    {
        let _ = live;
        Ok(PtyStatus { foreground: None })
    }
}

#[tauri::command]
pub fn pty_kill(host: State<PtyHost>, id: String) -> Result<(), String> {
    if let Some(live) = host.remove(&id) {
        terminate(live.pid);
        #[cfg(unix)]
        close_fd(live.master_fd);
    }
    Ok(())
}

/// Off the main thread: `kill_all` waits for the shells to die before it
/// returns, and a window close calls this while the app keeps running.
#[tauri::command(async)]
pub fn pty_kill_all(host: State<'_, PtyHost>) -> Result<(), String> {
    host.kill_all();
    Ok(())
}

#[cfg(unix)]
fn spawn_unix(
    app: AppHandle,
    host: State<PtyHost>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    use std::fs::File;
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let workdir = working_dir(&cwd);
    let (shell, args) = default_shell();
    let (master, slave) = open_pty(cols, rows)?;

    let mut cmd = Command::new(&shell);
    cmd.args(&args)
        .current_dir(&workdir)
        .stdin(dup_stdio(slave)?)
        .stdout(dup_stdio(slave)?)
        .stderr(dup_stdio(slave)?)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("COLORFGBG", "15;0")
        .env("TERM_PROGRAM", "MonoCode")
        .env("PATH", crate::harness::gui_search_path());
    if let Some(home) = dirs_home() {
        cmd.env("HOME", &home);
    }
    cmd.env("PWD", &workdir);

    // setsid() already creates a new session and process group. Calling
    // process_group(0) first makes the child a group leader, so setsid()
    // fails with EPERM ("Operation not permitted").
    let slave_fd = slave;
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Controlling tty is best-effort; the shell still runs without it.
            let _ = libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            if slave_fd > 2 {
                libc::close(slave_fd);
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {shell}: {e}"))?;
    close_fd(slave);
    let pid = child.id();

    set_cloexec(master);
    let reader = unsafe { File::from_raw_fd(dup_fd(master)?) };
    let writer = unsafe { File::from_raw_fd(dup_fd(master)?) };

    let live = Arc::new(LivePty {
        writer: Mutex::new(Box::new(writer)),
        master_fd: master,
        pid,
    });
    host.insert(id.clone(), live);

    let data_app = app.clone();
    let data_id = id.clone();
    thread::spawn(move || {
        let mut file = reader;
        let fd = file.as_raw_fd();
        let mut buf = vec![0_u8; READ_CHUNK];
        let mut acc = Vec::with_capacity(READ_CHUNK);
        let mut last_emit = Instant::now();
        loop {
            if acc.is_empty() {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        acc.extend_from_slice(&buf[..n]);
                        last_emit = Instant::now();
                    }
                    Err(_) => break,
                }
            } else if pty_should_flush(acc.len(), last_emit.elapsed())
                || !wait_readable(fd, PTY_COALESCE.saturating_sub(last_emit.elapsed()))
            {
                emit_pty_data(&data_app, &data_id, &acc);
                acc.clear();
                last_emit = Instant::now();
            } else {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => acc.extend_from_slice(&buf[..n]),
                    Err(_) => break,
                }
            }
        }
        emit_pty_data(&data_app, &data_id, &acc);
    });

    let wait_app = app;
    let wait_id = id;
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        // Only announce this child. A remount/respawn reuses the id, and the
        // previous wait thread must not paint "[process exited]" on the new PTY
        // or yank the replacement out of the host map.
        let emit = if let Some(host) = wait_app.try_state::<PtyHost>() {
            if let Some(live) = host.remove_if_pid(&wait_id, pid) {
                close_fd(live.master_fd);
                true
            } else {
                false
            }
        } else {
            false
        };
        if emit {
            let _ = wait_app.emit(EXIT_EVENT, PtyExit { id: wait_id, code });
        }
    });

    Ok(())
}

#[cfg(windows)]
fn spawn_windows(
    app: AppHandle,
    host: State<PtyHost>,
    id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let workdir = working_dir(&cwd);
    let (shell, args) = default_shell();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("Failed to open terminal: {err}"))?;

    let mut cmd = CommandBuilder::new(&shell);
    cmd.args(&args);
    cmd.cwd(&workdir);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("COLORFGBG", "15;0");
    cmd.env("TERM_PROGRAM", "MonoCode");
    cmd.env("PATH", crate::harness::gui_search_path());
    if let Some(home) = dirs_home() {
        cmd.env("HOME", &home);
        cmd.env("USERPROFILE", &home);
    }
    cmd.env("PWD", workdir.to_string_lossy().as_ref());

    let mut child = crate::windows::spawn_pty(pair.slave.as_ref(), cmd)
        .map_err(|err| format!("Failed to start {shell}: {err}"))?;
    let pid = child.process_id().unwrap_or(0);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("Failed to read terminal: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("Failed to write to terminal: {err}"))?;

    let live = Arc::new(LivePty {
        writer: Mutex::new(Box::new(writer)),
        master: Mutex::new(pair.master),
        pid,
    });
    host.insert(id.clone(), live);

    let data_app = app.clone();
    let data_id = id.clone();
    thread::spawn(move || {
        let mut buf = vec![0_u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    // ponytail: caps bridge traffic at 125 emits/s; use a timed
                    // drain only if sustained PTY throughput becomes limiting.
                    thread::sleep(PTY_COALESCE);
                    emit_pty_data(&data_app, &data_id, &buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    let wait_app = app;
    let wait_id = id;
    thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code() as i32);
        let emit = if let Some(host) = wait_app.try_state::<PtyHost>() {
            host.remove_if_pid(&wait_id, pid).is_some()
        } else {
            false
        };
        if emit {
            let _ = wait_app.emit(EXIT_EVENT, PtyExit { id: wait_id, code });
        }
    });

    Ok(())
}

fn working_dir(cwd: &str) -> std::path::PathBuf {
    let path = expand_home(cwd);
    if path.is_dir() {
        return path;
    }
    dirs_home().map(std::path::PathBuf::from).unwrap_or(path)
}

fn default_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if !comspec.is_empty() {
                return (comspec, Vec::new());
            }
        }
        ("powershell.exe".into(), vec!["-NoLogo".into()])
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    "/bin/zsh".into()
                } else {
                    "/bin/bash".into()
                }
            });
        let args = login_args(&shell)
            .iter()
            .map(|arg| (*arg).to_string())
            .collect();
        (shell, args)
    }
}

#[cfg(not(windows))]
fn login_args(shell: &str) -> &'static [&'static str] {
    match std::path::Path::new(shell)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
    {
        "zsh" | "bash" | "sh" | "fish" => &["-l"],
        _ => &[],
    }
}

/// The hangup a closing shell expects, without `terminate`'s escalation.
#[cfg(unix)]
fn hangup(pid: u32) {
    if pid == 0 || pid == 1 {
        return;
    }
    let ipid = pid as i32;
    unsafe {
        libc::kill(ipid, libc::SIGHUP);
        libc::kill(-ipid, libc::SIGHUP);
    }
}

fn terminate(pid: u32) {
    if pid == 0 || pid == 1 {
        return;
    }
    #[cfg(unix)]
    {
        let ipid = pid as i32;
        unsafe {
            libc::kill(ipid, libc::SIGHUP);
            libc::kill(-ipid, libc::SIGHUP);
            libc::kill(ipid, libc::SIGTERM);
            libc::kill(-ipid, libc::SIGTERM);
        }
        thread::spawn(move || {
            thread::sleep(KILL_ESCALATE);
            unsafe {
                libc::kill(ipid, libc::SIGKILL);
                libc::kill(-ipid, libc::SIGKILL);
            }
        });
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        crate::hide_window_console(&mut cmd);
        let _ = cmd
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

#[cfg(unix)]
fn open_pty(cols: u16, rows: u16) -> Result<(i32, i32), String> {
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(os_err("Failed to open terminal"));
    }
    if unsafe { libc::grantpt(master) } != 0 || unsafe { libc::unlockpt(master) } != 0 {
        close_fd(master);
        return Err(os_err("Failed to unlock terminal"));
    }
    let name = slave_name(master).inspect_err(|_| {
        close_fd(master);
    })?;
    let slave = unsafe { libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
    if slave < 0 {
        close_fd(master);
        return Err(os_err("Failed to open terminal slave"));
    }
    if let Err(err) = resize_fd(master, cols, rows) {
        close_fd(master);
        close_fd(slave);
        return Err(err);
    }
    Ok((master, slave))
}

#[cfg(unix)]
fn slave_name(master: i32) -> Result<std::ffi::CString, String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let mut buf = vec![0_i8; 64];
        let ret = unsafe { libc::ptsname_r(master, buf.as_mut_ptr(), buf.len()) };
        if ret != 0 {
            return Err(os_err("Failed to resolve terminal name"));
        }
        let last = buf.len() - 1;
        buf[last] = 0;
        Ok(unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_owned())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let ptr = unsafe { libc::ptsname(master) };
        if ptr.is_null() {
            return Err(os_err("Failed to resolve terminal name"));
        }
        Ok(unsafe { std::ffi::CStr::from_ptr(ptr) }.to_owned())
    }
}

#[cfg(unix)]
fn resize_fd(fd: i32, cols: u16, rows: u16) -> Result<(), String> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } != 0 {
        return Err(os_err("Failed to resize terminal"));
    }
    Ok(())
}

#[cfg(unix)]
fn dup_fd(fd: i32) -> Result<i32, String> {
    let next = unsafe { libc::dup(fd) };
    if next < 0 {
        return Err(os_err("Failed to duplicate terminal"));
    }
    Ok(next)
}

#[cfg(unix)]
fn dup_stdio(fd: i32) -> Result<std::process::Stdio, String> {
    use std::os::unix::io::FromRawFd;
    let next = dup_fd(fd)?;
    Ok(unsafe { std::process::Stdio::from_raw_fd(next) })
}

#[cfg(unix)]
fn set_cloexec(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

#[cfg(unix)]
fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}

#[cfg(unix)]
fn os_err(ctx: &str) -> String {
    format!("{ctx}: {}", std::io::Error::last_os_error())
}

fn emit_pty_data(app: &AppHandle, id: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    let _ = app.emit(
        DATA_EVENT,
        PtyData {
            id: id.to_string(),
            data,
        },
    );
}

#[cfg(unix)]
fn pty_should_flush(buffered: usize, since: Duration) -> bool {
    buffered >= READ_CHUNK || since >= PTY_COALESCE
}

#[cfg(unix)]
fn wait_readable(fd: i32, timeout: Duration) -> bool {
    if timeout.is_zero() {
        return false;
    }
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    unsafe { libc::poll(&mut pollfd, 1, ms) > 0 }
}

#[cfg(unix)]
fn foreground_label(master_fd: i32, shell_pid: u32) -> Option<String> {
    let mut pgrp: libc::pid_t = 0;
    if unsafe { libc::ioctl(master_fd, libc::TIOCGPGRP, &mut pgrp) } != 0 {
        return None;
    }
    let pid = pgrp;
    if pid <= 0 {
        return None;
    }
    let label = process_label(pid)?;
    if pid == shell_pid as i32 || is_shell_name(&label) {
        return None;
    }
    Some(label)
}

#[cfg(unix)]
fn process_label(pid: i32) -> Option<String> {
    use std::process::Command;
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let args = raw.trim();
    if args.is_empty() {
        return None;
    }
    command_label(args)
}

#[cfg(unix)]
fn command_label(args: &str) -> Option<String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let exe = parts[0];
    let base = std::path::Path::new(exe)
        .file_name()
        .and_then(|name| name.to_str())?;
    if is_interpreter(base) {
        for part in parts.iter().skip(1) {
            if part.starts_with('-') {
                continue;
            }
            let name = std::path::Path::new(part)
                .file_name()
                .and_then(|name| name.to_str())?;
            if !name.starts_with('-') {
                return Some(name.to_string());
            }
        }
    }
    Some(base.to_string())
}

#[cfg(unix)]
fn is_interpreter(name: &str) -> bool {
    matches!(
        name,
        "node" | "nodejs" | "python" | "python3" | "ruby" | "deno" | "bun"
    )
}

#[cfg(unix)]
fn is_shell_name(name: &str) -> bool {
    matches!(
        name,
        "zsh" | "bash" | "sh" | "fish" | "nu" | "dash" | "ksh" | "tcsh" | "zsh5"
    )
}

#[cfg(all(test, unix))]
mod label_tests {
    use super::*;

    #[test]
    fn command_label_prefers_cli_over_interpreter() {
        assert_eq!(
            command_label("node /usr/local/bin/npm run build"),
            Some("npm".into())
        );
        assert_eq!(command_label("cargo build"), Some("cargo".into()));
    }

    #[test]
    fn shell_names_are_ignored() {
        assert!(is_shell_name("zsh"));
        assert!(!is_shell_name("npm"));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn login_args_for_common_shells() {
        assert_eq!(login_args("/bin/zsh"), &["-l"]);
        assert_eq!(login_args("/bin/bash"), &["-l"]);
        assert_eq!(login_args("/usr/bin/fish"), &["-l"]);
        assert_eq!(login_args("/usr/local/bin/nu"), &[] as &[&str]);
    }

    #[test]
    fn pty_flush_waits_for_a_full_chunk_or_the_coalesce_window() {
        assert!(!pty_should_flush(1, Duration::from_millis(1)));
        assert!(pty_should_flush(READ_CHUNK, Duration::from_millis(1)));
        assert!(pty_should_flush(1, PTY_COALESCE));
    }

    #[test]
    fn remove_if_pid_ignores_a_replaced_session() {
        let host = PtyHost::new();
        host.insert(
            "term".into(),
            Arc::new(LivePty {
                writer: Mutex::new(Box::new(std::io::sink())),
                master_fd: -1,
                pid: 42,
            }),
        );
        assert!(host.remove_if_pid("term", 7).is_none());
        assert!(host.get("term").is_some());
        assert!(host.remove_if_pid("term", 42).is_some());
        assert!(host.get("term").is_none());
    }
}
