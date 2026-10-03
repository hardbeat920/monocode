//! The GitHub host MonoCode talks to through `gh`: github.com or a GitHub Enterprise Server.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const DEFAULT_HOST: &str = "github.com";

static HOST: RwLock<Option<String>> = RwLock::new(None);

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

/// The configured host, such as `github.com` or `github.example.com`.
pub fn host() -> String {
    HOST.read()
        .ok()
        .and_then(|host| host.clone())
        .unwrap_or_else(|| DEFAULT_HOST.to_string())
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

/// Points `gh` at the configured host for commands that do not name one.
pub fn apply_gh_env(cmd: &mut Command) {
    if is_enterprise() {
        cmd.env("GH_HOST", host());
    }
}

#[tauri::command(async)]
pub fn github_host_get() -> String {
    host()
}

#[tauri::command(async)]
pub fn github_host_set(app: AppHandle, host: String) -> Result<String, String> {
    let host = if host.trim().is_empty() {
        DEFAULT_HOST.to_string()
    } else {
        normalize_github_host(&host)?
    };
    let path = config_path(&app)?;
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
    let host = match host.as_str() {
        "www.github.com" | "api.github.com" => DEFAULT_HOST.to_string(),
        _ => host,
    };
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
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
        assert_eq!(
            normalize_github_host("github.example.com:8443").unwrap(),
            "github.example.com:8443"
        );
        assert!(normalize_github_host("http://github.example.com").is_err());
        assert!(normalize_github_host("https://user@github.example.com").is_err());
        assert!(normalize_github_host("").is_err());
    }
}
