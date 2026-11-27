//! GUI-launch environment for git and gh, ported from src-tauri harness.rs.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
#[cfg(not(windows))]
use std::{process::Stdio, sync::mpsc, time::Duration};

#[cfg(not(windows))]
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);

static LOGIN_SHELL_ENV: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub(crate) fn dirs_home() -> Option<String> {
    #[cfg(windows)]
    let keys = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let keys = ["HOME", "USERPROFILE"];
    for key in keys {
        if let Some(home) = std::env::var_os(key) {
            let home = home.to_string_lossy().into_owned();
            if !home.is_empty() {
                return Some(home);
            }
        }
    }
    match (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
        (Ok(drive), Ok(path)) if !drive.is_empty() && !path.is_empty() => {
            Some(format!("{drive}{path}"))
        }
        _ => passwd_identity().map(|id| id.home),
    }
}

pub(crate) fn expand_home(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if text == "~" {
        return dirs_home()
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
    }
    let rest = text.strip_prefix("~/").or_else(|| {
        if cfg!(windows) {
            text.strip_prefix("~\\")
        } else {
            None
        }
    });
    if let Some(rest) = rest {
        if let Some(home) = dirs_home() {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}

pub(crate) fn path_to_js(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

struct PasswdIdentity {
    home: String,
    user: String,
    shell: String,
}

fn passwd_identity() -> Option<PasswdIdentity> {
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        let mut buf = vec![0u8; 4096];
        let mut pwd = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut::<libc::passwd>();
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                &mut pwd,
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
                &mut result,
            )
        };
        if rc != 0 || result.is_null() {
            return None;
        }
        unsafe {
            let user = std::ffi::CStr::from_ptr(pwd.pw_name)
                .to_string_lossy()
                .into_owned();
            let home = std::ffi::CStr::from_ptr(pwd.pw_dir)
                .to_string_lossy()
                .into_owned();
            let shell = std::ffi::CStr::from_ptr(pwd.pw_shell)
                .to_string_lossy()
                .into_owned();
            if user.is_empty() || home.is_empty() {
                return None;
            }
            Some(PasswdIdentity { home, user, shell })
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Resolve `name` the way a terminal would, then fall back to common install
/// dirs. Finder-launched apps inherit launchd's bare PATH.
pub(crate) fn resolve_gui_binary(name: &str) -> Option<PathBuf> {
    which_in_path(&gui_search_path(), name)
}

pub(crate) fn gui_search_path() -> String {
    gui_search_path_from(login_shell_path(), dirs_home(), std::env::var("PATH").ok())
}

/// `gui_search_path` without ever starting the login shell: its PATH is used
/// only when something already read it.
pub(crate) fn cached_gui_search_path() -> String {
    gui_search_path_from(
        peek_login_shell_path(&LOGIN_SHELL_ENV),
        dirs_home(),
        std::env::var("PATH").ok(),
    )
}

fn gui_search_path_from(
    login_path: Option<String>,
    home: Option<String>,
    existing: Option<String>,
) -> String {
    let mut parts: Vec<PathBuf> = Vec::new();
    // Login-shell PATH first, inherited PATH next, fixed dirs last.
    if let Some(path) = login_path {
        parts.extend(std::env::split_paths(&path));
    }
    if let Some(existing) = existing {
        parts.extend(std::env::split_paths(&existing));
    }
    if let Some(home) = home {
        for dir in [
            ".local/bin",
            ".cargo/bin",
            ".claude/local",
            ".local/share/claude",
            ".opencode/bin",
            ".grok/bin",
            ".npm-global/bin",
            ".bun/bin",
            "AppData/Roaming/npm",
            "AppData/Local/Yarn/bin",
            "scoop/shims",
        ] {
            parts.push(format!("{home}/{dir}").into());
        }
    }
    for dir in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/snap/bin",
    ] {
        parts.push(dir.into());
    }
    #[cfg(windows)]
    {
        parts.push(r"C:\Program Files\Git\cmd".into());
        parts.push(r"C:\Program Files\nodejs".into());
    }
    std::env::join_paths(parts)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Login-shell PATH, HOME/USER/SHELL from passwd, and a UTF-8 locale.
pub(crate) fn apply_gui_env(cmd: &mut Command) {
    cmd.env("PATH", gui_search_path());
    if let Some(id) = passwd_identity() {
        if std::env::var_os("HOME").is_none() {
            cmd.env("HOME", &id.home);
        }
        if std::env::var_os("USER").is_none() {
            cmd.env("USER", &id.user);
            cmd.env("LOGNAME", &id.user);
        }
        if std::env::var_os("SHELL").is_none() && !id.shell.is_empty() {
            cmd.env("SHELL", &id.shell);
        }
    } else if let Some(home) = dirs_home() {
        cmd.env("HOME", &home);
        if std::env::var_os("USERPROFILE").is_none() {
            cmd.env("USERPROFILE", &home);
        }
        if std::env::var_os("USER").is_none() {
            if let Ok(username) = std::env::var("USERNAME") {
                cmd.env("USER", username);
            }
        }
    }
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
}

fn which_in_path(path: &str, name: &str) -> Option<PathBuf> {
    std::env::split_paths(std::ffi::OsStr::new(path)).find_map(|dir| {
        if dir.as_os_str().is_empty() {
            return None;
        }
        existing_binary(dir.join(name))
    })
}

fn existing_binary(path: PathBuf) -> Option<PathBuf> {
    #[cfg(windows)]
    if path.extension().is_none() {
        for ext in ["exe", "cmd", "bat", "com"] {
            let candidate = path.with_extension(ext);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    is_executable_file(&path).then_some(path)
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ["exe", "cmd", "bat", "com"]
                        .iter()
                        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                })
    }
}

fn login_shell_path() -> Option<String> {
    let mut cache = LOGIN_SHELL_ENV.lock().ok()?;
    if cache.is_none() {
        *cache = Some(load_login_shell_env());
    }
    cache
        .as_ref()
        .and_then(|map| map.get("PATH").cloned())
        .filter(|value| !value.is_empty())
}

/// `try_lock`, not `lock`: the read holds the lock while the shell runs.
fn peek_login_shell_path(cache: &Mutex<Option<HashMap<String, String>>>) -> Option<String> {
    let cache = cache.try_lock().ok()?;
    cache
        .as_ref()?
        .get("PATH")
        .cloned()
        .filter(|value| !value.is_empty())
}

fn load_login_shell_env() -> HashMap<String, String> {
    #[cfg(windows)]
    {
        std::env::var("PATH")
            .ok()
            .filter(|value| !value.is_empty())
            .map(|value| HashMap::from([("PATH".to_string(), value)]))
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    load_unix_login_shell_env()
}

/// `-lic`, not `-lc`: zsh reads `.zshrc` only for interactive shells, and
/// version managers initialize from there.
#[cfg(not(windows))]
fn load_unix_login_shell_env() -> HashMap<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    });
    let mut cmd = Command::new(&shell);
    cmd.args(["-lic", "printenv"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::process::isolate(&mut cmd);
    let Ok(child) = cmd.spawn() else {
        return HashMap::new();
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = match rx.recv_timeout(LOGIN_SHELL_TIMEOUT) {
        Ok(Ok(output)) => output,
        _ => {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            return HashMap::new();
        }
    };
    let mut map = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "PATH" && !value.is_empty() {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_search_path_puts_login_path_ahead_of_fallbacks() {
        let path = gui_search_path_from(
            Some("/login/bin".into()),
            Some("/home/me".into()),
            Some("/inherited/bin".into()),
        );
        let parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(parts[0], PathBuf::from("/login/bin"));
        assert_eq!(parts[1], PathBuf::from("/inherited/bin"));
        assert!(parts.contains(&PathBuf::from("/home/me/.local/bin")));
        assert!(parts.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_unix_backslash_filenames() {
        assert_eq!(path_to_js(Path::new(r"/tmp/a\b.txt")), r"/tmp/a\b.txt");
        assert_eq!(
            expand_home(Path::new(r"~\literal")),
            PathBuf::from(r"~\literal")
        );
    }
}
