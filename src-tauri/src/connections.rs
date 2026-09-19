//! SSH connection profiles: the servers (and server+container targets)
//! remote projects point at. Persisted as JSON in the app data dir with
//! owner-only permissions, following the GitLab/Linear settings pattern.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::remote;

/// A remote development target. `container: None` means "develop directly on
/// the host"; `Some(name)` means "docker exec into this container on the
/// host". Everything else (paths, agent installs, logins) lives on the target.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    /// Flat charset id (`conn_` + hex) — it is embedded in `ssh://` URIs.
    pub id: String,
    /// Display name shown in the rail and pickers.
    pub name: String,
    /// Host, IP, or a ~/.ssh/config alias — the system ssh resolves it.
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub container: Option<String>,
    /// Free-text hint about how this server authenticates.
    pub auth_note: Option<String>,
    pub created_at: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
    /// Remote $HOME — seeds the path browser in the connect dialog.
    pub home: Option<String>,
    /// `uname -sm` for the success line in the UI.
    pub uname: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ConnectionStore {
    #[serde(default)]
    connections: Vec<ConnectionProfile>,
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("connections.json"))
}

// ---------------------------------------------------------------------------
// Global mirror.
//
// Deep git/fs helpers need profiles without an AppHandle in scope. The app
// records its data + cache dirs once at setup; profile lookups then read
// through this small cache (writes keep it in sync). Reading a tiny JSON per
// ssh command would also be fine — the ssh round-trip dwarfs it — but the
// cache keeps local git paths allocation-free.
// ---------------------------------------------------------------------------

struct GlobalDirs {
    data_dir: PathBuf,
    cache_dir: Option<PathBuf>,
}

static GLOBAL_DIRS: std::sync::OnceLock<GlobalDirs> = std::sync::OnceLock::new();
static PROFILE_CACHE: Mutex<Option<Vec<ConnectionProfile>>> = Mutex::new(None);

/// Record the app's directories for AppHandle-free lookups. Called once from
/// app setup; also implicitly by any `require_profile(app, …)` call.
pub(crate) fn init(app: &AppHandle) {
    if let Ok(data_dir) = app.path().app_data_dir() {
        let _ = GLOBAL_DIRS.set(GlobalDirs {
            data_dir,
            cache_dir: app.path().app_cache_dir().ok(),
        });
    }
}

fn global_store_path() -> Result<PathBuf, String> {
    let dirs = GLOBAL_DIRS
        .get()
        .ok_or("Connections are not initialized yet")?;
    Ok(dirs.data_dir.join("connections.json"))
}

pub(crate) fn global_cache_dir() -> Option<std::path::PathBuf> {
    GLOBAL_DIRS.get().and_then(|dirs| dirs.cache_dir.clone())
}

fn read_profiles_global() -> Result<Vec<ConnectionProfile>, String> {
    if let Some(cached) = PROFILE_CACHE.lock().ok().and_then(|guard| guard.clone()) {
        return Ok(cached);
    }
    let path = global_store_path()?;
    let profiles = match fs::read_to_string(path) {
        Ok(raw) => {
            serde_json::from_str::<ConnectionStore>(&raw)
                .map_err(|_| "Server settings are invalid".to_string())?
                .connections
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.to_string()),
    };
    if let Ok(mut cache) = PROFILE_CACHE.lock() {
        *cache = Some(profiles.clone());
    }
    Ok(profiles)
}

/// Look up a profile by id for a remote operation.
pub(crate) fn require_profile(app: &AppHandle, id: &str) -> Result<ConnectionProfile, String> {
    init(app);
    profile_by_id(id)
}

/// AppHandle-free lookup for deep git/fs helpers.
pub(crate) fn profile_by_id(id: &str) -> Result<ConnectionProfile, String> {
    read_profiles_global()?
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| format!("Unknown connection “{id}”. It may have been removed in Settings."))
}

fn read_profiles(app: &AppHandle) -> Result<Vec<ConnectionProfile>, String> {
    init(app);
    read_profiles_global()
}

fn write_profiles(app: &AppHandle, connections: &[ConnectionProfile]) -> Result<(), String> {
    let path = store_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let store = ConnectionStore {
        connections: connections.to_vec(),
    };
    let value = serde_json::to_string(&store).map_err(|error| error.to_string())?;
    crate::gitlab::write_secret_file(&path, &value)?;
    if let Ok(mut cache) = PROFILE_CACHE.lock() {
        *cache = Some(connections.to_vec());
    }
    Ok(())
}

fn new_connection_id() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("conn_{}", &hex[..8])
}

/// Field-level validation; does not touch the network.
pub(crate) fn validate_profile(profile: &ConnectionProfile) -> Result<(), String> {
    let host = profile.host.trim();
    if host.is_empty() {
        return Err("Host is required".into());
    }
    if host.starts_with('-')
        || profile
            .user
            .as_deref()
            .is_some_and(|user| user.starts_with('-'))
    {
        return Err("Host and user cannot start with a dash".into());
    }
    if host.chars().any(char::is_whitespace) {
        return Err("Host cannot contain spaces".into());
    }
    if let Some(user) = profile.user.as_deref() {
        let user = user.trim();
        if !user.is_empty() && user.chars().any(char::is_whitespace) {
            return Err("User cannot contain spaces".into());
        }
    }
    if let Some(container) = profile.container.as_deref() {
        let container = container.trim();
        if container.is_empty() {
            return Err("Container name cannot be blank when set".into());
        }
        if container
            .chars()
            .any(|c| c.is_whitespace() || c == '\'' || c == '"')
        {
            return Err("Container name contains characters Docker does not allow".into());
        }
    }
    Ok(())
}

/// Connectivity probe: prints the remote $HOME and `uname -sm`.
fn probe(profile: &ConnectionProfile, app: &AppHandle) -> Result<ConnectionTestResult, String> {
    let argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "printf '%s\\n' \"$HOME\"; uname -sm 2>/dev/null || true".to_string(),
    ];
    let mut command = remote::remote_exec(app, profile, None, false, &argv)?;
    let output = remote::capture_timeout(&mut command, Duration::from_secs(20))?;
    if !output.success {
        return Ok(ConnectionTestResult {
            ok: false,
            latency_ms: 0,
            error: Some(remote::ssh_failure(profile, &output)),
            home: None,
            uname: None,
        });
    }
    let mut lines = output.stdout.lines().filter(|line| !line.trim().is_empty());
    let home = lines
        .next()
        .map(|line| line.trim().to_string())
        .filter(|home| !home.is_empty());
    let uname = lines
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty());
    Ok(ConnectionTestResult {
        ok: true,
        latency_ms: 0,
        error: None,
        home,
        uname,
    })
}

#[tauri::command]
pub async fn connections_list(app: AppHandle) -> Result<Vec<ConnectionProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || read_profiles(&app))
        .await
        .map_err(|error| error.to_string())?
}

/// Validate the profile by connecting before it is saved, mirroring the
/// GitLab/Linear connect flows — a typo'd host should never persist.
#[tauri::command]
pub async fn connections_save(
    app: AppHandle,
    profile: ConnectionProfile,
) -> Result<ConnectionProfile, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut profile = profile;
        profile.host = profile.host.trim().to_string();
        profile.user = profile
            .user
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty())
            .map(str::to_string);
        profile.container = profile
            .container
            .as_deref()
            .map(str::trim)
            .filter(|container| !container.is_empty())
            .map(str::to_string);
        validate_profile(&profile)?;

        let started = Instant::now();
        let mut result = probe(&profile, &app)?;
        result.latency_ms = started.elapsed().as_millis() as u64;
        if !result.ok {
            return Err(result
                .error
                .unwrap_or_else(|| "Could not connect to this server".into()));
        }

        let mut profiles = read_profiles(&app)?;
        if profile.id.trim().is_empty() {
            profile.id = new_connection_id();
            profile.created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0);
        } else {
            remote::invalidate_connection(&profile.id);
            if !profiles.iter().any(|existing| existing.id == profile.id) {
                return Err(format!("Unknown connection “{}”", profile.id));
            }
        }
        if profile.name.trim().is_empty() {
            profile.name = match profile.container.as_deref() {
                Some(container) => format!("{} · {container}", profile.host),
                None => profile.host.clone(),
            };
        }
        profiles.retain(|existing| existing.id != profile.id);
        profiles.push(profile.clone());
        write_profiles(&app, &profiles)?;
        Ok(profile)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn connections_remove(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut profiles = read_profiles(&app)?;
        let before = profiles.len();
        profiles.retain(|profile| profile.id != id);
        if profiles.len() == before {
            return Err(format!("Unknown connection “{id}”"));
        }
        write_profiles(&app, &profiles)?;
        remote::invalidate_connection(&id);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Run the connectivity probe without persisting anything.
#[tauri::command]
pub async fn connections_test(
    app: AppHandle,
    profile: ConnectionProfile,
) -> Result<ConnectionTestResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut profile = profile;
        profile.host = profile.host.trim().to_string();
        validate_profile(&profile)?;
        let started = Instant::now();
        let mut result = probe(&profile, &app)?;
        result.latency_ms = started.elapsed().as_millis() as u64;
        Ok(result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(container: Option<&str>) -> ConnectionProfile {
        ConnectionProfile {
            id: "conn_abc12345".into(),
            name: "Build box".into(),
            host: "build.example.com".into(),
            user: Some("deploy".into()),
            port: Some(2222),
            container: container.map(str::to_string),
            auth_note: None,
            created_at: 0,
        }
    }

    #[test]
    fn connection_ids_stay_uri_safe() {
        let id = new_connection_id();
        assert!(remote::valid_connection_id(&id));
    }

    #[test]
    fn rejects_blank_and_spaced_hosts() {
        let mut p = profile(None);
        p.host = "   ".into();
        assert!(validate_profile(&p).is_err());
        p.host = "two words".into();
        assert!(validate_profile(&p).is_err());
        p.host = "build.example.com".into();
        assert!(validate_profile(&p).is_ok());
    }

    #[test]
    fn rejects_unusable_container_names() {
        let mut p = profile(Some("web app"));
        assert!(validate_profile(&p).is_err());
        p.container = Some("web-1".into());
        assert!(validate_profile(&p).is_ok());
        p.container = Some("  ".into());
        assert!(validate_profile(&p).is_err());
    }
}
