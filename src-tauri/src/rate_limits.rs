use crate::jsonc::strip_jsonc_comments;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

pub(crate) const CMD_CREDITS_URL: &str = "https://api.commandcode.ai/alpha/billing/credits";

fn cmd_usage_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<String>,
) -> CmdUsageFetch {
    CmdUsageFetch {
        status: status.into(),
        http_status,
        body,
        error,
    }
}

/// Fetch Command Code 5-hour / weekly window limits and credits via ~/.commandcode/auth.json.
#[tauri::command]
pub async fn fetch_cmd_usage() -> Result<CmdUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_cmd_usage_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_cmd_usage_sync() -> Result<CmdUsageFetch, String> {
    let Some(api_key) = read_cmd_api_key() else {
        return Ok(cmd_usage_result(
            "unavailable",
            None,
            None,
            Some("Command Code not signed in".into()),
        ));
    };

    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(CMD_CREDITS_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("User-Agent", USER_AGENT)
        .call();

    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if (200..300).contains(&http_status) {
                Ok(cmd_usage_result("ok", Some(http_status), Some(body), None))
            } else {
                Ok(cmd_usage_result(
                    "error",
                    Some(http_status),
                    None,
                    Some(format!("Command Code usage request failed ({http_status})")),
                ))
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            let message = if status == 401 {
                "Command Code sign-in expired".into()
            } else {
                format!("Command Code usage request failed ({status})")
            };
            Ok(cmd_usage_result("error", Some(status), None, Some(message)))
        }
        Err(error) => Ok(cmd_usage_result(
            "error",
            None,
            None,
            Some(format!("Command Code usage request failed: {error}")),
        )),
    }
}

pub(crate) fn cmd_auth_path() -> Option<PathBuf> {
    let home = dirs_home().or_else(|| {
        std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
    })?;
    Some(PathBuf::from(home).join(".commandcode/auth.json"))
}

pub(crate) fn extract_cmd_api_key(raw: &str) -> Option<String> {
    let val: Value = serde_json::from_str(raw.trim()).ok()?;
    let key = val.get("apiKey").and_then(Value::as_str)?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn read_cmd_api_key() -> Option<String> {
    let path = cmd_auth_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    extract_cmd_api_key(&raw)
}

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

/// Resolve the OpenCode Go API key. Resolution mirrors OpenCode's own
/// precedence, with storage generations in order:
/// 1. the `OPENCODE_AUTH_CONTENT` blob,
/// 2. an explicit provider key in opencode config,
/// 3. the active credential in OpenCode 2's SQLite store,
/// 4. legacy `auth.json -> "opencode-go" -> "key"` credentials.
///
/// OpenCode 2 moved credentials into `opencode.db` and left `auth.json`
/// behind. Reading the old file first marked a live subscription as expired
/// while `opencode` itself kept working, so the database is authoritative
/// whenever it has an active credential.
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
    if let Some(key) = opencode_db_paths()
        .iter()
        .find_map(|path| read_opencode_go_db_key(path))
    {
        return Some(key);
    }
    // The database can be briefly locked while the CLI checkpoints its WAL.
    // Ask OpenCode itself before falling back to the stale v1 file.
    if let Some(key) = read_opencode_go_cli_key() {
        return Some(key);
    }
    let primary = opencode_data_dir()?.join("auth.json");
    let raw = std::fs::read_to_string(&primary)
        .or_else(|_| {
            // Legacy macOS location.
            legacy_macos_opencode_dir()
                .map(|dir| std::fs::read_to_string(dir.join("auth.json")))
                .unwrap_or_else(|| {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "no home directory",
                    ))
                })
        })
        .ok()?;
    extract_opencode_go_api_key(&raw)
}

fn legacy_macos_opencode_dir() -> Option<PathBuf> {
    dirs_home()
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|value| value.to_string_lossy().into_owned())
        })
        .map(|home| PathBuf::from(home).join("Library/Application Support/opencode"))
}

/// OpenCode 2 stores every credential in one SQLite file. The current data
/// directory wins; the legacy macOS location stays as a read-only fallback.
fn opencode_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = opencode_data_dir() {
        paths.push(dir.join("opencode.db"));
    }
    if let Some(dir) = legacy_macos_opencode_dir() {
        let legacy = dir.join("opencode.db");
        if !paths.contains(&legacy) {
            paths.push(legacy);
        }
    }
    paths
}

/// Read the active `opencode-go` credential from the OpenCode 2 database.
/// Read-only access keeps the CLI free to own and rotate the database; a
/// missing table (OpenCode 1 or an old database) simply falls through.
fn read_opencode_go_db_key(path: &std::path::Path) -> Option<String> {
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = connection.busy_timeout(Duration::from_millis(2_000));
    query_opencode_go_db_key(&connection)
}

fn query_opencode_go_db_key(connection: &rusqlite::Connection) -> Option<String> {
    let raw: String = connection
        .query_row(
            "SELECT value FROM credential \
             WHERE integration_id = 'opencode-go' AND active = 1 \
             ORDER BY time_updated DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let key = value.get("key")?.as_str()?.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// Ask the installed CLI for its active credential. `opencode auth export`
/// reads through the same store the running app uses, including any busy WAL
/// state that a direct read might miss.
fn read_opencode_go_cli_key() -> Option<String> {
    let path = crate::harness::resolve_opencode()?;
    let args = vec![
        "auth".to_string(),
        "export".to_string(),
        "opencode-go".to_string(),
    ];
    let output = crate::harness::exec_output(
        &path.to_string_lossy(),
        &args,
        None,
        Duration::from_secs(10),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    extract_active_opencode_go_key(&String::from_utf8_lossy(&output.stdout))
}

/// `opencode auth export` returns an array of credential records; choose the
/// active `opencode-go` entry rather than the first stale export.
fn extract_active_opencode_go_key(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    for item in value.as_array()? {
        let Some(record) = item.as_object() else {
            continue;
        };
        if record.get("integrationID").and_then(Value::as_str) != Some("opencode-go") {
            continue;
        }
        if record.get("active").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let key = record
            .get("value")
            .and_then(|value| value.get("key"))
            .and_then(Value::as_str)?
            .trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }
    None
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
#[tauri::command]
pub async fn fetch_claude_usage(
    app: AppHandle,
    account_id: Option<String>,
) -> Result<ClaudeUsageFetch, String> {
    let config_dir = crate::harness::provider_account_dir(&app, "claude", account_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || fetch_claude_usage_sync(config_dir))
        .await
        .map_err(|e| e.to_string())?
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
    fn query_opencode_go_db_key_prefers_the_active_credential() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE credential (
                    id TEXT PRIMARY KEY,
                    integration_id TEXT,
                    label TEXT NOT NULL,
                    value TEXT NOT NULL,
                    active INTEGER,
                    time_updated INTEGER NOT NULL
                );
                INSERT INTO credential VALUES
                    ('old', 'opencode-go', 'API key', '{\"type\":\"api\",\"key\":\"sk-stale\"}', 0, 1),
                    ('live', 'opencode-go', 'OpenCode Go', '{\"type\":\"api\",\"key\":\"oc_sk_live\"}', 1, 2),
                    ('other', 'opencode', 'Console', '{\"type\":\"api\",\"key\":\"sk-console\"}', 1, 3);",
            )
            .unwrap();
        assert_eq!(
            query_opencode_go_db_key(&connection).as_deref(),
            Some("oc_sk_live")
        );
    }

    #[test]
    fn query_opencode_go_db_key_ignores_inactive_and_other_providers() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE credential (
                    id TEXT PRIMARY KEY,
                    integration_id TEXT,
                    label TEXT NOT NULL,
                    value TEXT NOT NULL,
                    active INTEGER,
                    time_updated INTEGER NOT NULL
                );
                INSERT INTO credential VALUES
                    ('old', 'opencode-go', 'API key', '{\"type\":\"api\",\"key\":\"sk-stale\"}', 0, 1),
                    ('other', 'opencode', 'Console', '{\"type\":\"api\",\"key\":\"sk-console\"}', 1, 2);",
            )
            .unwrap();
        assert_eq!(query_opencode_go_db_key(&connection), None);
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
    fn extract_active_opencode_go_key_from_cli_export() {
        let raw = r#"[
            {"integrationID":"opencode-go","active":false,"value":{"type":"api","key":"sk-stale"}},
            {"integrationID":"opencode","active":true,"value":{"type":"api","key":"oc_sk_console"}},
            {"integrationID":"opencode-go","active":true,"value":{"type":"api","key":"oc_sk_live"}}
        ]"#;
        assert_eq!(
            extract_active_opencode_go_key(raw).as_deref(),
            Some("oc_sk_live")
        );
        assert_eq!(extract_active_opencode_go_key("[]"), None);
        assert_eq!(extract_active_opencode_go_key("not json"), None);
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
    fn extract_cmd_api_key_reads_key() {
        let raw = r#"{"apiKey":"user_123456","userName":"alice"}"#;
        assert_eq!(extract_cmd_api_key(raw).as_deref(), Some("user_123456"));
    }

    #[test]
    fn extract_cmd_api_key_rejects_empty() {
        let raw = r#"{"apiKey":"  ","userName":"alice"}"#;
        assert_eq!(extract_cmd_api_key(raw), None);
        assert_eq!(extract_cmd_api_key("not json"), None);
    }
}
