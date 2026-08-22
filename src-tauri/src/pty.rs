use std::collections::HashMap;
#[cfg(unix)]
use std::io::Read;
use std::io::Write;
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dirs_home;
use crate::fs::expand_home;

const DATA_EVENT: &str = "pty-data";
const EXIT_EVENT: &str = "pty-exit";
#[cfg(unix)]
const READ_CHUNK: usize = 32 * 1024;
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
        for live in kids {
            terminate(live.pid);
            #[cfg(unix)]
            close_fd(live.master_fd);
        }
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

    #[cfg(not(unix))]
    {
        let _ = (app, cwd, cols, rows);
        return Err("Terminals are supported on macOS and Linux.".into());
    }

    #[cfg(unix)]
    {
        spawn_unix(app, host, id, cwd, cols.max(2), rows.max(2))
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
    #[cfg(not(unix))]
    {
        let _ = (live, cols, rows);
        Err("Terminals are supported on macOS and Linux.".into())
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PtyStatus {
    foreground: Option<String>,
}

#[tauri::command]
pub fn pty_status(host: State<PtyHost>, id: String) -> Result<PtyStatus, String> {
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

#[tauri::command]
pub fn pty_kill_all(host: State<PtyHost>) -> Result<(), String> {
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
        .env("TERM_PROGRAM", "MonoCode");
    apply_path(&mut cmd);
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
        let mut buf = vec![0_u8; READ_CHUNK];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        &buf[..n],
                    );
                    let _ = data_app.emit(
                        DATA_EVENT,
                        PtyData {
                            id: data_id.clone(),
                            data,
                        },
                    );
                }
                Err(_) => break,
            }
        }
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

#[cfg(unix)]
fn working_dir(cwd: &str) -> std::path::PathBuf {
    let path = expand_home(cwd);
    if path.is_dir() {
        return path;
    }
    dirs_home().map(std::path::PathBuf::from).unwrap_or(path)
}

#[cfg(unix)]
fn default_shell() -> (String, Vec<String>) {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
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

#[cfg(unix)]
fn login_args(shell: &str) -> &'static [&'static str] {
    match std::path::Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
    {
        "zsh" | "bash" | "sh" | "fish" => &["-l"],
        _ => &[],
    }
}

#[cfg(unix)]
fn apply_path(cmd: &mut std::process::Command) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(home) = dirs_home() {
        parts.push(format!("{home}/.local/bin"));
        parts.push(format!("{home}/.cargo/bin"));
        parts.push(format!("{home}/.claude/local"));
        parts.push(format!("{home}/.local/share/claude"));
        parts.push(format!("{home}/.opencode/bin"));
        parts.push(format!("{home}/.npm-global/bin"));
    }
    parts.push("/opt/homebrew/bin".into());
    parts.push("/usr/local/bin".into());
    parts.push("/usr/bin".into());
    parts.push("/bin".into());
    parts.push("/snap/bin".into());
    if let Ok(existing) = std::env::var("PATH") {
        parts.push(existing);
    }
    cmd.env("PATH", parts.join(":"));
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
    #[cfg(not(unix))]
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
