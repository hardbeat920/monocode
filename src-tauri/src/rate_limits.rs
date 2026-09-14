use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

use crate::dirs_home;

const OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str = "claude-code/2.1.0";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;

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

enum ClaudeCredStore {
    #[cfg(target_os = "macos")]
    Keychain {
        service: String,
        account: String,
    },
    File {
        path: PathBuf,
    },
}

struct ClaudeCredentials {
    access_token: String,
    refresh_token: String,
    expires_at_ms: Option<i64>,
    blob: Value,
    store: ClaudeCredStore,
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
/// `config_dir` is the CLAUDE_CONFIG_DIR the caller's session is actually
/// running under (None for the default, no-override account) — each
/// profile's login is stored separately (a suffixed Keychain service, or a
/// credentials file inside that config dir), so reading the wrong one
/// silently reports a different account's usage than the one in use.
#[tauri::command]
pub async fn fetch_claude_usage(config_dir: Option<String>) -> Result<ClaudeUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_claude_usage_sync(config_dir.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_claude_usage_sync(config_dir: Option<&str>) -> Result<ClaudeUsageFetch, String> {
    let Some(mut creds) = read_claude_credentials(config_dir) else {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Claude not signed in".into()),
        ));
    };

    if token_needs_refresh(creds.expires_at_ms, now_ms()) {
        refresh_claude_credentials(&mut creds);
    }

    let first = fetch_usage_with_token(&creds.access_token);
    if first.http_status != Some(401) {
        return Ok(first);
    }
    if !refresh_claude_credentials(&mut creds) {
        return Ok(first);
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

fn refresh_claude_credentials(creds: &mut ClaudeCredentials) -> bool {
    if creds.refresh_token.is_empty() {
        return false;
    }
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": creds.refresh_token,
        "client_id": OAUTH_CLIENT_ID,
    });
    let Ok(request_body) = serde_json::to_string(&body) else {
        return false;
    };
    let result = agent
        .post(OAUTH_TOKEN_URL)
        .set("Content-Type", "application/json")
        .set("User-Agent", USER_AGENT)
        .send_string(&request_body);
    let response = match result {
        Ok(response) if (200..300).contains(&response.status()) => response,
        _ => return false,
    };
    let Ok(text) = response.into_string() else {
        return false;
    };
    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(access) = apply_refresh_response(&mut creds.blob, &payload, now_ms()) else {
        return false;
    };
    creds.access_token = access;
    if let Some(refresh) = string_field(&payload, "refresh_token") {
        creds.refresh_token = refresh;
    }
    creds.expires_at_ms = oauth_expires_at_ms(&creds.blob);
    persist_claude_credentials(creds)
}

fn persist_claude_credentials(creds: &ClaudeCredentials) -> bool {
    let Ok(raw) = serde_json::to_string(&creds.blob) else {
        return false;
    };
    match &creds.store {
        #[cfg(target_os = "macos")]
        ClaudeCredStore::Keychain { service, account } => {
            write_macos_keychain_blob(service, account, &raw)
        }
        ClaudeCredStore::File { path } => write_credentials_file(path, &raw),
    }
}

fn write_credentials_file(path: &Path, raw: &str) -> bool {
    if std::fs::write(path, raw).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    true
}

fn read_claude_credentials(config_dir: Option<&str>) -> Option<ClaudeCredentials> {
    #[cfg(target_os = "macos")]
    {
        if let Some(creds) = read_macos_keychain_credentials(config_dir) {
            return Some(creds);
        }
    }
    read_credentials_file(config_dir)
}

fn read_credentials_file(config_dir: Option<&str>) -> Option<ClaudeCredentials> {
    let path = claude_credentials_path(config_dir)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    credentials_from_blob(&raw, ClaudeCredStore::File { path })
}

fn claude_credentials_path(config_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(dir) = config_dir {
        return Some(crate::fs::expand_home(dir).join(".credentials.json"));
    }
    let home = dirs_home().or_else(|| {
        std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
    })?;
    Some(PathBuf::from(home).join(".claude/.credentials.json"))
}

fn credentials_from_blob(raw: &str, store: ClaudeCredStore) -> Option<ClaudeCredentials> {
    let blob: Value = serde_json::from_str(raw.trim()).ok()?;
    let access_token = extract_access_token(raw)?;
    Some(ClaudeCredentials {
        access_token,
        refresh_token: extract_refresh_token(&blob).unwrap_or_default(),
        expires_at_ms: oauth_expires_at_ms(&blob),
        blob,
        store,
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

fn extract_refresh_token(blob: &Value) -> Option<String> {
    let token = blob
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("refreshToken"))
        .or_else(|| blob.get("refreshToken"))
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

fn oauth_object_mut(blob: &mut Value) -> Option<&mut Value> {
    if blob.get("claudeAiOauth").is_some() {
        blob.get_mut("claudeAiOauth")
    } else {
        Some(blob)
    }
}

pub(crate) fn apply_refresh_response(
    blob: &mut Value,
    response: &Value,
    now_ms: i64,
) -> Option<String> {
    let access = string_field(response, "access_token")?;
    let oauth = oauth_object_mut(blob)?;
    oauth["accessToken"] = json!(access);
    if let Some(refresh) = string_field(response, "refresh_token") {
        oauth["refreshToken"] = json!(refresh);
    }
    if let Some(expires_in) = int_field(response, "expires_in") {
        oauth["expiresAt"] = json!(now_ms.saturating_add(expires_in.saturating_mul(1000)));
    }
    if let Some(refresh_expires_in) = int_field(response, "refresh_token_expires_in") {
        oauth["refreshTokenExpiresAt"] =
            json!(now_ms.saturating_add(refresh_expires_in.saturating_mul(1000)));
    }
    Some(access)
}

pub(crate) fn token_needs_refresh(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    let Some(expires) = expires_at_ms else {
        return false;
    };
    now_ms.saturating_add(TOKEN_REFRESH_BUFFER_MS) >= expires
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn int_field(value: &Value, key: &str) -> Option<i64> {
    match value.get(key)? {
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn read_macos_keychain_credentials(config_dir: Option<&str>) -> Option<ClaudeCredentials> {
    let service = keychain_service_for(config_dir);
    let user = keychain_user();
    let candidates = [
        (user.clone(), {
            let mut args = keychain_find_args(&service);
            args.push("-w".into());
            args
        }),
        (user.clone(), {
            let mut args = keychain_find_args(&service);
            args.extend(["-a".into(), user.clone(), "-w".into()]);
            args
        }),
        (KEYCHAIN_FALLBACK_USER.into(), {
            let mut args = keychain_find_args(&service);
            args.extend(["-a".into(), KEYCHAIN_FALLBACK_USER.into(), "-w".into()]);
            args
        }),
    ];
    for (account, args) in candidates {
        if let Some(secret) = security_output(&args) {
            if let Some(creds) = credentials_from_blob(
                &secret,
                ClaudeCredStore::Keychain {
                    service: service.clone(),
                    account,
                },
            ) {
                return Some(creds);
            }
        }
    }
    None
}

/// Claude CLI keeps the default account's login under the plain
/// "Claude Code-credentials" Keychain service, and every other
/// CLAUDE_CONFIG_DIR's login under that same name suffixed with the first
/// 8 hex chars of SHA-256(absolute config dir path) — e.g.
/// `~/.claude-personal` → `Claude Code-credentials-d8a6e19b`. Verified
/// against this machine's real Keychain entries.
#[cfg(target_os = "macos")]
fn keychain_service_for(config_dir: Option<&str>) -> String {
    let Some(dir) = config_dir else {
        return LEGACY_KEYCHAIN_SERVICE.to_string();
    };
    let absolute = crate::fs::expand_home(dir);
    let mut hasher = Sha256::new();
    hasher.update(absolute.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let mut suffix = String::with_capacity(8);
    for byte in digest.iter().take(4) {
        suffix.push_str(&format!("{byte:02x}"));
    }
    format!("{LEGACY_KEYCHAIN_SERVICE}-{suffix}")
}

#[cfg(target_os = "macos")]
fn write_macos_keychain_blob(service: &str, account: &str, raw: &str) -> bool {
    let args = vec![
        "add-generic-password".into(),
        "-U".into(),
        "-s".into(),
        service.into(),
        "-a".into(),
        account.into(),
        "-w".into(),
        raw.into(),
    ];
    security_ok(&args)
}

#[cfg(target_os = "macos")]
fn keychain_find_args(service: &str) -> Vec<String> {
    vec!["find-generic-password".into(), "-s".into(), service.into()]
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
    security_run(args, true)
}

#[cfg(target_os = "macos")]
fn security_ok(args: &[String]) -> bool {
    security_run(args, false).is_some()
}

#[cfg(target_os = "macos")]
fn security_run(args: &[String], require_stdout: bool) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("security");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    run_with_timeout(&mut cmd, KEYCHAIN_TIMEOUT, require_stdout)
}

#[cfg(target_os = "macos")]
fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
    require_stdout: bool,
) -> Option<String> {
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
                if require_stdout && trimmed.is_empty() {
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

    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_service_for_matches_observed_claude_cli_scheme() {
        // Claude CLI suffixes the default Keychain service with the first 8
        // hex chars of SHA-256(absolute config dir) for a non-default
        // CLAUDE_CONFIG_DIR — verified against real Keychain entries. These
        // paths are generic examples; the algorithm itself is what's tested.
        assert_eq!(keychain_service_for(None), LEGACY_KEYCHAIN_SERVICE);
        assert_eq!(
            keychain_service_for(Some("/Users/alice/.claude-work")),
            "Claude Code-credentials-be865d75"
        );
        assert_eq!(
            keychain_service_for(Some("/Users/alice/.claude-client")),
            "Claude Code-credentials-a90ccfe6"
        );
    }

    #[test]
    fn token_needs_refresh_uses_five_minute_buffer() {
        let now = 1_000_000;
        assert!(!token_needs_refresh(
            Some(now + TOKEN_REFRESH_BUFFER_MS + 1),
            now
        ));
        assert!(token_needs_refresh(
            Some(now + TOKEN_REFRESH_BUFFER_MS),
            now
        ));
        assert!(token_needs_refresh(Some(now - 1), now));
        assert!(!token_needs_refresh(None, now));
    }

    #[test]
    fn apply_refresh_response_updates_oauth_blob() {
        let mut blob = json!({
            "claudeAiOauth": {
                "accessToken": "old-access",
                "refreshToken": "old-refresh",
                "expiresAt": 1,
                "subscriptionType": "pro"
            }
        });
        let response = json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "expires_in": 28800,
            "refresh_token_expires_in": 2592000
        });
        let now = 1_700_000_000_000i64;
        assert_eq!(
            apply_refresh_response(&mut blob, &response, now).as_deref(),
            Some("new-access")
        );
        let oauth = blob.get("claudeAiOauth").unwrap();
        assert_eq!(oauth["accessToken"], "new-access");
        assert_eq!(oauth["refreshToken"], "new-refresh");
        assert_eq!(oauth["expiresAt"], now + 28_800_000);
        assert_eq!(oauth["refreshTokenExpiresAt"], now + 2_592_000_000i64);
        assert_eq!(oauth["subscriptionType"], "pro");
    }
}
