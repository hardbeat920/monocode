// Userspace Tailscale (tsnet c-archive) for the iPad companion.
//
// MERGE NOTE: ios-gated module. The iPad joins login.tailscale.com as
// `monocode-ipad` and dials the Mac's tailnet IP without the Tailscale iOS
// app and without a Network Extension / VPN entitlement.

use std::ffi::{CStr, CString};
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::remote::{EmbedSnapshot, RemoteState};

const CONFIG_FILE: &str = "tailnet.json";

#[link(name = "monocode_tsnet")]
extern "C" {
    fn monocode_tsnet_new() -> i32;
    fn monocode_tsnet_start(sd: i32) -> i32;
    fn monocode_tsnet_close(sd: i32) -> i32;
    fn monocode_tsnet_set_dir(sd: i32, dir: *const std::os::raw::c_char) -> i32;
    fn monocode_tsnet_set_hostname(sd: i32, hostname: *const std::os::raw::c_char) -> i32;
    fn monocode_tsnet_set_authkey(sd: i32, key: *const std::os::raw::c_char) -> i32;
    fn monocode_tsnet_errmsg(sd: i32, buf: *mut std::os::raw::c_char, buflen: usize) -> i32;
    fn monocode_tsnet_status_json(
        sd: i32,
        json_out: *mut *mut std::os::raw::c_char,
    ) -> i32;
    fn monocode_tsnet_dial(
        sd: i32,
        network: *const std::os::raw::c_char,
        addr: *const std::os::raw::c_char,
        conn_out: *mut i32,
    ) -> i32;
}

static HANDLE: Mutex<Option<i32>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct EmbedConfig {
    #[serde(default)]
    auth_key: Option<String>,
    #[serde(default)]
    wanted: bool,
}

fn lock_handle() -> std::sync::MutexGuard<'static, Option<i32>> {
    HANDLE.lock().unwrap_or_else(|e| e.into_inner())
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("companion");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(CONFIG_FILE))
}

fn state_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("companion")
        .join("tsnet");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn load_config(app: &AppHandle) -> EmbedConfig {
    let Ok(path) = config_path(app) else {
        return EmbedConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return EmbedConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_config(app: &AppHandle, config: &EmbedConfig) -> Result<(), String> {
    let path = config_path(app)?;
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, raw).map_err(|e| e.to_string())?;
    Ok(())
}

fn set_wanted(app: &AppHandle, wanted: bool) {
    let mut saved = load_config(app);
    saved.wanted = wanted;
    let _ = save_config(app, &saved);
}

fn errmsg(sd: i32) -> String {
    let mut buf = [0u8; 512];
    unsafe {
        monocode_tsnet_errmsg(sd, buf.as_mut_ptr() as *mut _, buf.len());
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

fn cstr(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "tailnet string contained NUL".to_string())
}

fn status_json(sd: i32) -> Result<serde_json::Value, String> {
    let mut ptr: *mut std::os::raw::c_char = std::ptr::null_mut();
    let rc = unsafe { monocode_tsnet_status_json(sd, &mut ptr) };
    if rc != 0 || ptr.is_null() {
        return Err(errmsg(sd).if_empty("tailnet status unavailable"));
    }
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(ptr as *mut libc::c_void) };
    serde_json::from_str(&json).map_err(|e| format!("tailnet status JSON: {e}"))
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.trim().is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

fn snapshot_from_handle(sd: i32) -> EmbedSnapshot {
    match status_json(sd) {
        Ok(json) => crate::remote::embed_from_status_json(&json),
        Err(error) => EmbedSnapshot {
            running: true,
            error: Some(error),
            ..EmbedSnapshot::default()
        },
    }
}

fn close_handle() {
    if let Some(sd) = lock_handle().take() {
        unsafe {
            monocode_tsnet_close(sd);
        }
    }
}

/// Dial `host:port` through the userspace node. Blocking CGO; call from
/// `spawn_blocking`. Returns a connected Unix socketpair fd.
pub fn dial_blocking(addr: &str) -> Result<tokio::net::UnixStream, String> {
    let sd = lock_handle()
        .ok_or_else(|| "Sign in with Google in Companion settings first.".to_string())?;
    let network = cstr("tcp")?;
    let addr = cstr(addr)?;
    let mut fd: i32 = -1;
    let rc = unsafe { monocode_tsnet_dial(sd, network.as_ptr(), addr.as_ptr(), &mut fd) };
    if rc != 0 || fd < 0 {
        return Err(errmsg(sd).if_empty("tailnet dial failed"));
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let std_stream = std::os::unix::net::UnixStream::from(owned);
    std_stream
        .set_nonblocking(true)
        .map_err(|e| format!("tailnet socket: {e}"))?;
    tokio::net::UnixStream::from_std(std_stream).map_err(|e| format!("tailnet socket: {e}"))
}

pub async fn start(app: AppHandle, auth_key: Option<String>) -> Result<EmbedSnapshot, String> {
    let mut saved = load_config(&app);
    if auth_key.is_some() {
        saved.auth_key = auth_key.filter(|key| !key.trim().is_empty());
    }
    saved.wanted = true;
    save_config(&app, &saved)?;

    let dir = state_dir(&app)?;
    let dir_c = cstr(&dir.to_string_lossy())?;
    let host_c = cstr(&crate::remote::node_hostname())?;
    let key_c = saved
        .auth_key
        .as_deref()
        .filter(|key| !key.is_empty())
        .map(cstr)
        .transpose()?;

    let state: tauri::State<'_, RemoteState> = app.state();
    if let Some(previous) = state.embed_take_task() {
        previous.abort();
    }
    close_handle();
    state.embed_update_snapshot(EmbedSnapshot {
        running: true,
        ..EmbedSnapshot::default()
    });

    let sd = tauri::async_runtime::spawn_blocking(move || {
        let sd = unsafe { monocode_tsnet_new() };
        if sd == 0 {
            return Err("tailnet node could not be created".to_string());
        }
        if unsafe { monocode_tsnet_set_dir(sd, dir_c.as_ptr()) } != 0 {
            let err = errmsg(sd);
            unsafe { monocode_tsnet_close(sd) };
            return Err(err.if_empty("failed to set tailnet state dir"));
        }
        if unsafe { monocode_tsnet_set_hostname(sd, host_c.as_ptr()) } != 0 {
            let err = errmsg(sd);
            unsafe { monocode_tsnet_close(sd) };
            return Err(err.if_empty("failed to set tailnet hostname"));
        }
        if let Some(key) = key_c {
            if unsafe { monocode_tsnet_set_authkey(sd, key.as_ptr()) } != 0 {
                let err = errmsg(sd);
                unsafe { monocode_tsnet_close(sd) };
                return Err(err.if_empty("failed to set tailnet auth key"));
            }
        }
        if unsafe { monocode_tsnet_start(sd) } != 0 {
            let err = errmsg(sd);
            unsafe { monocode_tsnet_close(sd) };
            return Err(err.if_empty("tailnet node failed to start"));
        }
        Ok(sd)
    })
    .await
    .map_err(|e| e.to_string())??;

    *lock_handle() = Some(sd);
    let snapshot = snapshot_from_handle(sd);
    state.embed_update_snapshot(snapshot.clone());
    let task = tauri::async_runtime::spawn(poll_status(app.clone(), sd));
    state.embed_replace_task(task);
    Ok(snapshot)
}

pub async fn stop(app: AppHandle) -> Result<EmbedSnapshot, String> {
    let state: tauri::State<'_, RemoteState> = app.state();
    if let Some(task) = state.embed_take_task() {
        task.abort();
    }
    close_handle();
    set_wanted(&app, false);
    let snapshot = EmbedSnapshot::default();
    state.embed_update_snapshot(snapshot.clone());
    Ok(snapshot)
}

/// Wipe tsnet state so the next start asks for Google again.
pub async fn logout(app: AppHandle) -> Result<EmbedSnapshot, String> {
    let _ = stop(app.clone()).await;
    if let Ok(dir) = state_dir(&app) {
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
    }
    let mut saved = load_config(&app);
    saved.auth_key = None;
    saved.wanted = true;
    let _ = save_config(&app, &saved);
    start(app, None).await
}

pub fn autostart(app: &AppHandle) {
    if !load_config(app).wanted {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = start(app, None).await {
            eprintln!("companion: iPad tailnet autostart failed: {error}");
        }
    });
}

async fn poll_status(app: AppHandle, sd: i32) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let still = lock_handle().is_some_and(|live| live == sd);
        if !still {
            break;
        }
        let snapshot = match tauri::async_runtime::spawn_blocking(move || snapshot_from_handle(sd))
            .await
        {
            Ok(snapshot) => snapshot,
            Err(_) => break,
        };
        let state: tauri::State<'_, RemoteState> = app.state();
        state.embed_update_snapshot(snapshot);
    }
}


