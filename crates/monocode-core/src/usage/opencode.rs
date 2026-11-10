use crate::provider::devin::jsonc::strip_jsonc_comments;
use serde::Serialize;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
fn dirs_home() -> Option<String> {
    crate::accounts::home().map(|p| p.to_string_lossy().into_owned())
}
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Serialize)]
pub struct OpencodeGoUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}
const OPENCODE_GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
fn opencode_go_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<String>,
) -> OpencodeGoUsageFetch {
    OpencodeGoUsageFetch {
        status: status.into(),
        http_status,
        body,
        error,
    }
}

pub(super) fn fetch() -> Result<OpencodeGoUsageFetch, String> {
    let Some(api_key) = read_opencode_go_api_key() else {
        return Ok(opencode_go_result(
            "unavailable",
            None,
            None,
            Some("OpenCode Go not connected".into()),
        ));
    };
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(OPENCODE_GO_USAGE_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .call();
    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if (200..300).contains(&http_status) {
                Ok(opencode_go_result(
                    "ok",
                    Some(http_status),
                    Some(body),
                    None,
                ))
            } else {
                Ok(opencode_go_error(http_status))
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            Ok(opencode_go_error(status))
        }
        Err(error) => Ok(opencode_go_result(
            "error",
            None,
            None,
            Some(format!("OpenCode Go usage request failed: {error}")),
        )),
    }
}

fn opencode_go_error(status: u16) -> OpencodeGoUsageFetch {
    // 403 means a valid key without a Go subscription — not a failure,
    // so the footer can hide the chip instead of showing an error.
    if status == 403 {
        return opencode_go_result(
            "unavailable",
            Some(status),
            None,
            Some("No OpenCode Go subscription".into()),
        );
    }
    let message = if status == 401 {
        "OpenCode Go sign-in expired".into()
    } else {
        format!("OpenCode Go usage request failed ({status})")
    };
    opencode_go_result("error", Some(status), None, Some(message))
}

/// Resolve the OpenCode data directory the same way OpenCode does:
/// `OPENCODE_DATA_DIR`, then `$XDG_DATA_HOME/<app>`, then the default
/// `~/.local/share/<app>`, where `<app>` is `OPENCODE_APPNAME` or "opencode".
fn opencode_data_dir() -> Option<PathBuf> {
    if let Some(dir) = env_var("OPENCODE_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    let app = env_var("OPENCODE_APPNAME").unwrap_or_else(|| "opencode".into());
    if let Some(xdg) = env_var("XDG_DATA_HOME") {
        return Some(PathBuf::from(xdg).join(app));
    }
    let home = dirs_home().or_else(|| {
        std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
    })?;
    Some(PathBuf::from(home).join(".local/share").join(app))
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The Go key lives at `auth.json -> "opencode-go" -> "key"` inside the
/// OpenCode data directory. Resolution mirrors OpenCode's own precedence:
/// the `OPENCODE_AUTH_CONTENT` blob, then an explicit provider key in
/// opencode config, then stored credentials on disk.
fn read_opencode_go_api_key() -> Option<String> {
    // Env-injected auth blob is authoritative when it parses: a valid blob
    // without opencode-go means "no key", not "look elsewhere".
    if let Some(blob) = env_var("OPENCODE_AUTH_CONTENT") {
        if let Ok(value) = serde_json::from_str::<Value>(&blob) {
            if value.is_object() {
                return extract_opencode_go_key(&value);
            }
        }
    }
    if let Some(key) = read_opencode_config_api_key() {
        return Some(key);
    }
    let primary = opencode_data_dir()?.join("auth.json");
    let raw = std::fs::read_to_string(&primary)
        .or_else(|_| {
            // Legacy macOS location.
            dirs_home()
                .or_else(|| {
                    std::env::var_os("USERPROFILE")
                        .map(|value| value.to_string_lossy().into_owned())
                })
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory")
                })
                .and_then(|home| {
                    std::fs::read_to_string(
                        PathBuf::from(home).join("Library/Application Support/opencode/auth.json"),
                    )
                })
        })
        .ok()?;
    extract_opencode_go_api_key(&raw)
}

/// Explicit `provider.options.apiKey` for the Go provider in opencode
/// config: `OPENCODE_CONFIG_CONTENT`, then `OPENCODE_CONFIG`, then the
/// global `opencode.json`. Only the Go provider IDs are considered so keys
/// for unrelated providers are never picked up.
fn read_opencode_config_api_key() -> Option<String> {
    if let Some(content) = env_var("OPENCODE_CONFIG_CONTENT") {
        if let Some(value) = parse_opencode_config(&content) {
            if let Some(key) = config_go_api_key(&value) {
                return Some(key);
            }
        }
    }
    opencode_config_paths()
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|raw| parse_opencode_config(&raw))
        .find_map(|value| config_go_api_key(&value))
}

fn opencode_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(custom) = env_var("OPENCODE_CONFIG") {
        paths.push(PathBuf::from(custom));
    }
    if let Some(xdg) = env_var("XDG_CONFIG_HOME") {
        let root = PathBuf::from(xdg).join("opencode");
        paths.push(root.join("opencode.jsonc"));
        paths.push(root.join("opencode.json"));
    }
    if let Some(home) = dirs_home().or_else(|| {
        std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
    }) {
        let root = PathBuf::from(home).join(".config/opencode");
        paths.push(root.join("opencode.jsonc"));
        paths.push(root.join("opencode.json"));
    }
    paths
}

/// Parse OpenCode configuration using JSONC semantics: comments and trailing
/// commas are accepted, while ordinary JSON stays on serde_json's fast path.
fn parse_opencode_config(raw: &str) -> Option<Value> {
    serde_json::from_str(raw.trim()).ok().or_else(|| {
        let without_comments = strip_jsonc_comments(raw)?;
        let normalized = strip_jsonc_trailing_commas(&without_comments);
        serde_json::from_str(&normalized).ok()
    })
}

fn strip_jsonc_trailing_commas(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            ',' if matches!(
                chars.clone().find(|next| !next.is_whitespace()),
                Some('}' | ']')
            ) => {}
            _ => out.push(ch),
        }
    }
    out
}

fn config_go_api_key(value: &Value) -> Option<String> {
    let providers = value.get("provider")?.as_object()?;
    for id in ["opencode-go", "opencode"] {
        let Some(api_key) = providers
            .get(id)
            .and_then(|entry| entry.get("options"))
            .and_then(|options| options.get("apiKey"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            continue;
        };
        if let Some(var) = api_key
            .strip_prefix("{env:")
            .and_then(|rest| rest.strip_suffix('}'))
        {
            if let Some(resolved) = env_var(var) {
                return Some(resolved);
            }
            continue;
        }
        return Some(api_key.to_string());
    }
    None
}

fn extract_opencode_go_key(value: &Value) -> Option<String> {
    let key = value.get("opencode-go")?.get("key")?.as_str()?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

pub(crate) fn extract_opencode_go_api_key(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let key = value.get("opencode-go")?.get("key")?.as_str()?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}
