use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
use tauri::AppHandle;
#[cfg(target_os = "macos")]
use unicode_normalization::UnicodeNormalization;

use crate::dirs_home;

const OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str = "claude-code/2.1.0";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(target_os = "macos")]
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const LEGACY_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
#[cfg(target_os = "macos")]
const KEYCHAIN_FALLBACK_USER: &str = "claude-code-user";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeGoUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

const OPENCODE_GO_USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";

/// Fetch OpenCode Go 5h / weekly / monthly usage via the local Go API key.
/// Runs in the host process so the webview CORS policy does not apply.
/// The key never leaves the host process.
#[tauri::command]
pub async fn fetch_opencode_go_usage() -> Result<OpencodeGoUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_opencode_go_usage_sync)
        .await
        .map_err(|e| e.to_string())?
}

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

fn fetch_opencode_go_usage_sync() -> Result<OpencodeGoUsageFetch, String> {
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

fn strip_jsonc_comments(raw: &str) -> Option<String> {
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

        match (ch, chars.peek().copied()) {
            ('"', _) => {
                in_string = true;
                out.push(ch);
            }
            ('/', Some('/')) => {
                let _ = chars.next();
                out.push(' ');
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                let _ = chars.next();
                out.push(' ');
                let mut closed = false;
                while let Some(next) = chars.next() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if next == '*' && chars.next_if_eq(&'/').is_some() {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return None;
                }
            }
            _ => out.push(ch),
        }
    }
    Some(out)
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

struct ClaudeCredentials {
    access_token: String,
    expires_at_ms: Option<i64>,
}

fn usage_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<String>,
) -> ClaudeUsageFetch {
    ClaudeUsageFetch {
        status: status.into(),
        http_status,
        body,
        error,
    }
}

/// Fetch Claude Code 5-hour / weekly usage via the local OAuth token.
/// The token never leaves the host process.
///
/// `config_dir_override` is the CLAUDE_CONFIG_DIR the Claude launch resolves
/// to. It wins over the account-derived dir, matching `harness_spawn`, so
/// usage reads the same credentials the CLI does.
#[tauri::command]
pub async fn fetch_claude_usage(
    app: AppHandle,
    account_id: Option<String>,
    config_dir_override: Option<String>,
) -> Result<ClaudeUsageFetch, String> {
    let config_dir = match claude_usage_config_override(config_dir_override) {
        Some(dir) => Some(dir),
        None => crate::harness::provider_account_dir(&app, "claude", account_id.as_deref())?,
    };
    tauri::async_runtime::spawn_blocking(move || fetch_claude_usage_sync(config_dir))
        .await
        .map_err(|e| e.to_string())?
}

fn claude_usage_config_override(config_dir_override: Option<String>) -> Option<PathBuf> {
    config_dir_override
        .filter(|dir| !dir.trim().is_empty())
        .map(|dir| crate::fs::expand_home(dir.trim()))
}

fn fetch_claude_usage_sync(config_dir: Option<PathBuf>) -> Result<ClaudeUsageFetch, String> {
    let Some(creds) = read_claude_credentials(config_dir.as_deref()) else {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Claude not signed in".into()),
        ));
    };

    // Claude Code owns this credential and rotates its refresh token. The
    // usage footer must remain read-only: independently refreshing here can
    // race a live CLI (or another MonoCode window) and leave one process with
    // a spent refresh token, which forces the user through sign-in again.
    if token_expired(creds.expires_at_ms, now_ms()) {
        return Ok(usage_error(401));
    }

    Ok(fetch_usage_with_token(&creds.access_token))
}

fn fetch_usage_with_token(token: &str) -> ClaudeUsageFetch {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(OAUTH_USAGE_URL)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", OAUTH_BETA)
        .set("User-Agent", USER_AGENT)
        .call();

    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if (200..300).contains(&http_status) {
                usage_result("ok", Some(http_status), Some(body), None)
            } else {
                usage_error(http_status)
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            usage_error(status)
        }
        Err(error) => usage_result(
            "error",
            None,
            None,
            Some(format!("Claude usage request failed: {error}")),
        ),
    }
}

fn usage_error(status: u16) -> ClaudeUsageFetch {
    let message = if status == 401 {
        "Claude sign-in expired".into()
    } else if status == 403 {
        "Claude usage is unavailable for this account".into()
    } else {
        format!("Claude usage request failed ({status})")
    };
    usage_result("error", Some(status), None, Some(message))
}

fn read_claude_credentials(config_dir: Option<&std::path::Path>) -> Option<ClaudeCredentials> {
    #[cfg(target_os = "macos")]
    {
        let service = claude_keychain_service(config_dir);
        if let Some(creds) = read_macos_keychain_credentials(&service) {
            return Some(creds);
        }
    }
    read_credentials_file(config_dir)
}

fn read_credentials_file(config_dir: Option<&std::path::Path>) -> Option<ClaudeCredentials> {
    let path = claude_credentials_path(config_dir)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    credentials_from_blob(&raw)
}

fn claude_credentials_path(config_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    if let Some(dir) = config_dir {
        return Some(dir.join(".credentials.json"));
    }
    let home = dirs_home().or_else(|| {
        std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
    })?;
    Some(PathBuf::from(home).join(".claude/.credentials.json"))
}

fn credentials_from_blob(raw: &str) -> Option<ClaudeCredentials> {
    let blob: Value = serde_json::from_str(raw.trim()).ok()?;
    let access_token = extract_access_token(raw)?;
    Some(ClaudeCredentials {
        access_token,
        expires_at_ms: oauth_expires_at_ms(&blob),
    })
}

pub(crate) fn extract_access_token(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let token = value
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("accessToken"))
        .or_else(|| value.get("accessToken"))
        .and_then(Value::as_str)?
        .trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn oauth_expires_at_ms(blob: &Value) -> Option<i64> {
    let value = blob
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("expiresAt"))
        .or_else(|| blob.get("expiresAt"))?;
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number.as_f64().and_then(|float| {
                if float.is_finite() {
                    Some(float as i64)
                } else {
                    None
                }
            })
        }),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// An unknown expiry is treated as usable: the usage request itself will 401
/// if it is not, which produces the same user-facing result without mutating
/// credentials owned by another process.
pub(crate) fn token_expired(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    expires_at_ms.is_some_and(|expires| now_ms >= expires)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_credentials(service: &str) -> Option<ClaudeCredentials> {
    let candidates = [
        {
            let mut args = keychain_find_args(service);
            args.push("-w".into());
            args
        },
        {
            let mut args = keychain_find_args(service);
            args.extend(["-a".into(), keychain_user(), "-w".into()]);
            args
        },
        {
            let mut args = keychain_find_args(service);
            args.extend(["-a".into(), KEYCHAIN_FALLBACK_USER.into(), "-w".into()]);
            args
        },
    ];
    for args in candidates {
        if let Some(secret) = security_output(&args) {
            if let Some(creds) = credentials_from_blob(&secret) {
                return Some(creds);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn keychain_find_args(service: &str) -> Vec<String> {
    vec!["find-generic-password".into(), "-s".into(), service.into()]
}

#[cfg(target_os = "macos")]
fn claude_keychain_service(config_dir: Option<&std::path::Path>) -> String {
    let Some(config_dir) = config_dir else {
        return LEGACY_KEYCHAIN_SERVICE.into();
    };
    // Claude Code hashes the exact, NFC-normalized selector string and uses
    // the first eight lowercase hex characters as its Keychain service suffix.
    let selector: String = config_dir.to_string_lossy().nfc().collect();
    let digest = Sha256::digest(selector.as_bytes());
    let suffix = format!("{digest:x}");
    format!("{LEGACY_KEYCHAIN_SERVICE}-{}", &suffix[..8])
}

#[cfg(target_os = "macos")]
pub(crate) fn delete_claude_keychain_credentials(
    config_dir: &std::path::Path,
) -> Result<(), String> {
    let service = claude_keychain_service(Some(config_dir));
    let args = vec![
        "delete-generic-password".into(),
        "-s".into(),
        service.clone(),
    ];
    security_delete(&args).map_err(|error| {
        format!("Could not remove the Claude credentials from Keychain ({service}): {error}")
    })
}

#[cfg(target_os = "macos")]
fn keychain_user() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    if user
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        && !user.is_empty()
    {
        user
    } else {
        KEYCHAIN_FALLBACK_USER.into()
    }
}

#[cfg(target_os = "macos")]
fn security_output(args: &[String]) -> Option<String> {
    security_run(args)
}

#[cfg(target_os = "macos")]
fn security_run(args: &[String]) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("security");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    run_with_timeout(&mut cmd, KEYCHAIN_TIMEOUT)
}

#[cfg(target_os = "macos")]
fn security_delete(args: &[String]) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut error = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut error);
                }
                if status.success()
                    || error.contains("could not be found")
                    || error.contains("specified item could not be found")
                {
                    return Ok(());
                }
                let detail = error.trim();
                return Err(if detail.is_empty() {
                    format!("security exited with {status}")
                } else {
                    detail.to_string()
                });
            }
            Ok(None) if started.elapsed() < KEYCHAIN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("security timed out".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(target_os = "macos")]
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::time::Instant;
    let mut child = cmd.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = child.stdout.take()?;
                let mut out = String::new();
                stdout.read_to_string(&mut out).ok()?;
                let trimmed = out.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_access_token_from_claude_credentials() {
        let raw = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-abc","refreshToken":"r"}}"#;
        assert_eq!(extract_access_token(raw).as_deref(), Some("sk-ant-oat-abc"));
    }

    #[test]
    fn extract_access_token_from_flat_object() {
        assert_eq!(
            extract_access_token(r#"{"accessToken":"token-1"}"#).as_deref(),
            Some("token-1")
        );
    }

    #[test]
    fn extract_access_token_rejects_empty() {
        assert_eq!(
            extract_access_token(r#"{"claudeAiOauth":{"accessToken":"  "}}"#),
            None
        );
        assert_eq!(extract_access_token("not json"), None);
    }

    #[test]
    fn extract_opencode_go_api_key_from_auth_json() {
        let raw = r#"{"openai":{"type":"oauth"},"opencode-go":{"type":"api","key":"sk-go-abc"}}"#;
        assert_eq!(
            extract_opencode_go_api_key(raw).as_deref(),
            Some("sk-go-abc")
        );
    }

    #[test]
    fn opencode_go_forbidden_maps_to_unavailable() {
        // A valid key without a Go subscription hides the chip
        // instead of rendering an error.
        let fetch = opencode_go_error(403);
        assert_eq!(fetch.status, "unavailable");
        assert_eq!(fetch.http_status, Some(403));

        let fetch = opencode_go_error(500);
        assert_eq!(fetch.status, "error");
    }

    #[test]
    fn extract_opencode_go_api_key_rejects_missing_or_empty() {
        assert_eq!(
            extract_opencode_go_api_key(r#"{"openai":{"type":"oauth"}}"#),
            None
        );
        assert_eq!(
            extract_opencode_go_api_key(r#"{"opencode-go":{"key":"  "}}"#),
            None
        );
        assert_eq!(extract_opencode_go_api_key("not json"), None);
    }

    #[test]
    fn opencode_data_dir_prefers_explicit_override() {
        std::env::set_var("OPENCODE_DATA_DIR", "/tmp/custom-data");
        assert_eq!(opencode_data_dir(), Some(PathBuf::from("/tmp/custom-data")));
        std::env::remove_var("OPENCODE_DATA_DIR");
    }

    #[test]
    fn parse_opencode_config_accepts_jsonc() {
        let value = parse_opencode_config(
            r#"{
              // URLs inside strings must not be treated as comments.
              "provider": {
                /* OpenCode Go credentials */
                "opencode-go": {
                  "options": {
                    "apiKey": "sk-go-jsonc",
                    "baseURL": "https://opencode.ai/v1",
                  },
                },
              },
            }"#,
        )
        .unwrap();
        assert_eq!(config_go_api_key(&value).as_deref(), Some("sk-go-jsonc"));
    }

    #[test]
    fn config_go_api_key_reads_provider_options() {
        let value: Value = serde_json::from_str(
            r#"{"provider":{"anthropic":{"options":{"apiKey":"sk-ant-x"}},"opencode-go":{"options":{"apiKey":"sk-go-cfg"}}}}"#,
        )
        .unwrap();
        assert_eq!(config_go_api_key(&value).as_deref(), Some("sk-go-cfg"));
    }

    #[test]
    fn config_go_api_key_falls_through_to_opencode_provider() {
        let value: Value = serde_json::from_str(
            r#"{"provider":{"opencode":{"options":{"apiKey":"sk-go-opencode"}}}}"#,
        )
        .unwrap();
        assert_eq!(config_go_api_key(&value).as_deref(), Some("sk-go-opencode"));
    }

    #[test]
    fn config_go_api_key_ignores_other_providers_and_supports_env() {
        let value: Value =
            serde_json::from_str(r#"{"provider":{"anthropic":{"options":{"apiKey":"sk-ant-x"}}}}"#)
                .unwrap();
        assert_eq!(config_go_api_key(&value), None);

        std::env::set_var("MONOCODE_TEST_GO_KEY", "sk-go-env");
        let value: Value = serde_json::from_str(
            r#"{"provider":{"opencode-go":{"options":{"apiKey":"{env:MONOCODE_TEST_GO_KEY}"}}}}"#,
        )
        .unwrap();
        assert_eq!(config_go_api_key(&value).as_deref(), Some("sk-go-env"));
        std::env::remove_var("MONOCODE_TEST_GO_KEY");
    }

    #[test]
    fn auth_content_blob_without_key_stays_authoritative() {
        // A valid blob without opencode-go means "no key", even when disk
        // credentials exist: no fallback to auth.json.
        std::env::set_var(
            "OPENCODE_AUTH_CONTENT",
            r#"{"openai":{"type":"api","key":"sk-openai-x"}}"#,
        );
        assert_eq!(read_opencode_go_api_key(), None);
        std::env::remove_var("OPENCODE_AUTH_CONTENT");
    }

    #[test]
    fn token_expired_uses_actual_expiry() {
        let now = 1_000_000;
        assert!(!token_expired(Some(now + 1), now));
        assert!(token_expired(Some(now), now));
        assert!(token_expired(Some(now - 1), now));
        assert!(!token_expired(None, now));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn custom_config_dir_selects_claudes_hashed_keychain_service() {
        assert_eq!(
            claude_keychain_service(Some(std::path::Path::new("/tmp/profile"))),
            "Claude Code-credentials-902e721c"
        );
        assert_eq!(claude_keychain_service(None), "Claude Code-credentials");
    }

    #[test]
    fn claude_usage_config_override_uses_non_blank_value() {
        assert_eq!(
            claude_usage_config_override(Some(" /custom/claude-home ".into())),
            Some(PathBuf::from("/custom/claude-home")),
        );
    }

    #[test]
    fn claude_usage_config_override_ignores_blank_value() {
        assert_eq!(claude_usage_config_override(Some("   ".into())), None);
        assert_eq!(claude_usage_config_override(None), None);
    }
}
