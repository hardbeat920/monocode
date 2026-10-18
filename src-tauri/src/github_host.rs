//! The GitHub host MonoCode talks to through `gh`: github.com or a GitHub Enterprise Server.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

pub const DEFAULT_HOST: &str = "github.com";
pub const HOST_CHANGED_EVENT: &str = "github-host-changed";

static HOST: RwLock<Option<String>> = RwLock::new(None);

thread_local! {
    /// The host one command acts on, so an item keeps its own server even
    /// after the configured host changes.
    static SCOPED_HOST: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[derive(Serialize, Deserialize, Default)]
struct GithubHostConfig {
    host: String,
}

pub fn init(app: &AppHandle) {
    let host = config_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<GithubHostConfig>(&raw).ok())
        .and_then(|config| normalize_github_host(&config.host).ok());
    store(host);
}

/// The host in effect: a command's scoped host, else the configured one.
pub fn host() -> String {
    if let Some(scoped) = SCOPED_HOST.with(|scoped| scoped.borrow().clone()) {
        return scoped;
    }
    HOST.read()
        .ok()
        .and_then(|host| host.clone())
        .unwrap_or_else(|| DEFAULT_HOST.to_string())
}

struct ScopeGuard(Option<String>);

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPED_HOST.with(|scoped| *scoped.borrow_mut() = previous);
    }
}

/// Runs `run` against `host` (an item's own server) instead of the configured host.
/// `None` or an empty host keeps the configured host.
pub fn with_host<T>(
    host: Option<&str>,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let Some(host) = host.map(str::trim).filter(|host| !host.is_empty()) else {
        return run();
    };
    let host = normalize_github_host(host)?;
    let previous = SCOPED_HOST.with(|scoped| scoped.replace(Some(host)));
    let _guard = ScopeGuard(previous);
    run()
}

pub fn is_enterprise() -> bool {
    host() != DEFAULT_HOST
}

/// REST API root for the configured host.
pub fn api_base() -> String {
    let host = host();
    if host == DEFAULT_HOST {
        "https://api.github.com".into()
    } else {
        format!("https://{host}/api/v3")
    }
}

/// Points `gh` at the host in effect. Always set, so an inherited `GH_HOST`
/// cannot redirect commands meant for github.com.
pub fn apply_gh_env(cmd: &mut Command) {
    cmd.env("GH_HOST", host());
}

#[tauri::command(async)]
pub fn github_host_get() -> String {
    host()
}

#[tauri::command(async)]
pub fn github_host_set(app: AppHandle, host: String) -> Result<String, String> {
    let host = save_host(&app, host)?;
    // Every window mirrors the host for its synchronous URL helpers.
    let _ = app.emit(HOST_CHANGED_EVENT, host.clone());
    Ok(host)
}

fn save_host(app: &AppHandle, host: String) -> Result<String, String> {
    let host = if host.trim().is_empty() {
        DEFAULT_HOST.to_string()
    } else {
        normalize_github_host(&host)?
    };
    let path = config_path(app)?;
    if host == DEFAULT_HOST {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        store(None);
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let value = serde_json::to_string(&GithubHostConfig { host: host.clone() })
            .map_err(|error| error.to_string())?;
        fs::write(&path, value).map_err(|error| error.to_string())?;
        store(Some(host.clone()));
    }
    Ok(host)
}

fn store(host: Option<String>) {
    if let Ok(mut current) = HOST.write() {
        *current = host.filter(|host| host != DEFAULT_HOST);
    }
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("github-config.json"))
}

/// Accepts `github.example.com`, `https://github.example.com/org/repo` and the like.
fn normalize_github_host(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("Enter your GitHub host (e.g. github.example.com)".into());
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| "GitHub host is invalid".to_string())?;
    if url.scheme() != "https" {
        return Err("GitHub host must use HTTPS".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("GitHub host is invalid".into());
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| "GitHub host is invalid".to_string())?
        .to_ascii_lowercase();
    // `gh` addresses hosts by name, and link matching compares hostnames.
    if url.port().is_some() {
        return Err("GitHub host must not include a port".into());
    }
    Ok(match host.as_str() {
        "www.github.com" | "api.github.com" => DEFAULT_HOST.to_string(),
        _ => host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_github_hosts() {
        assert_eq!(normalize_github_host("github.com").unwrap(), "github.com");
        assert_eq!(
            normalize_github_host("https://www.github.com/acme").unwrap(),
            "github.com"
        );
        assert_eq!(
            normalize_github_host(" https://GitHub.Example.com/acme/web/ ").unwrap(),
            "github.example.com"
        );
        assert!(normalize_github_host("github.example.com:8443").is_err());
        assert!(normalize_github_host("http://github.example.com").is_err());
        assert!(normalize_github_host("https://user@github.example.com").is_err());
        assert!(normalize_github_host("").is_err());
    }

    fn gh_host_env(cmd: &Command) -> Option<String> {
        cmd.get_envs()
            .find(|(key, _)| *key == "GH_HOST")
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn gh_host_is_set_explicitly_for_github_com() {
        let mut cmd = Command::new("gh");
        apply_gh_env(&mut cmd);
        assert_eq!(gh_host_env(&cmd).as_deref(), Some(DEFAULT_HOST));
    }

    #[test]
    fn scoped_host_applies_only_inside_the_command() {
        let inside = with_host(Some("https://GitHub.Example.com/acme/web"), || {
            let mut cmd = Command::new("gh");
            apply_gh_env(&mut cmd);
            Ok((host(), api_base(), gh_host_env(&cmd)))
        })
        .unwrap();
        assert_eq!(inside.0, "github.example.com");
        assert_eq!(inside.1, "https://github.example.com/api/v3");
        assert_eq!(inside.2.as_deref(), Some("github.example.com"));
        assert_eq!(host(), DEFAULT_HOST);
        assert_eq!(with_host(None, || Ok(host())).unwrap(), DEFAULT_HOST);
        assert!(with_host(Some("github.example.com:8443"), || Ok(())).is_err());
        let failed: Result<(), String> =
            with_host(Some("github.example.com"), || Err("boom".into()));
        assert!(failed.is_err());
        assert_eq!(host(), DEFAULT_HOST);
    }
}
