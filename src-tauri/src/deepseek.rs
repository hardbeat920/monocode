//! DeepSeek OpenCode upstream provider: API key in OpenCode auth.json + balance fetch.
//! Official platform account login uses browser PKCE (loopback /oauth/callback) and
//! writes the grant into ~/.dsh/.credentials.yaml for live catalog + quota.
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::dirs_home;

const DEEPSEEK_BALANCE_URL: &str = "https://api.deepseek.com/user/balance";
const DEEPSEEK_MODELS_URL: &str = "https://api.deepseek.com/models";
const DSH_PLATFORM_ISSUER: &str = "https://platform.deepseek.com";
const DSH_GRANT_RECORD: &str = "deepseek-account-platform/default";
const MODELS_UA: &str = "MonoCode/0.1 (DeepSeek Harness catalog)";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const PROVIDER_ID: &str = "deepseek";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekKeyStatus {
    pub configured: bool,
    /// Last four characters of the key when configured; never the full secret.
    pub hint: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekBalanceFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekOfficialModel {
    pub id: String,
    pub name: Option<String>,
    pub effort_levels: Option<Vec<String>>,
    pub effort_default: Option<String>,
    pub context_window: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekOfficialModelsFetch {
    /// `ok` | `unauthenticated` | `unauthorized` | `error`
    pub status: String,
    pub http_status: Option<u16>,
    pub models: Vec<DeepseekOfficialModel>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn deepseek_key_status() -> Result<DeepseekKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(key_status_sync()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_save_api_key(key: String) -> Result<DeepseekKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = key.trim().to_string();
        if trimmed.is_empty() {
            return Err("API Key 不能为空".into());
        }
        write_deepseek_key(&trimmed)?;
        Ok(key_status_sync())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_clear_api_key() -> Result<DeepseekKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        clear_deepseek_key()?;
        Ok(key_status_sync())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Fetch DeepSeek account balance via the official API using the stored key.
/// Runs in the host process so the webview CORS policy does not apply.
#[tauri::command]
pub async fn fetch_deepseek_balance() -> Result<DeepseekBalanceFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_deepseek_balance_sync)
        .await
        .map_err(|e| e.to_string())?
}

/// List DeepSeek platform models with the `~/.dsh` account grant (not the API key).
/// Never returns the grant token to the frontend.
#[tauri::command]
pub async fn fetch_deepseek_official_models() -> Result<DeepseekOfficialModelsFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_deepseek_official_models_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn key_status_sync() -> DeepseekKeyStatus {
    match read_deepseek_key() {
        Some(key) => DeepseekKeyStatus {
            configured: true,
            hint: Some(mask_hint(&key)),
        },
        None => DeepseekKeyStatus {
            configured: false,
            hint: None,
        },
    }
}

fn mask_hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        "••••".into()
    } else {
        format!("••••{}", chars[chars.len() - 4..].iter().collect::<String>())
    }
}

fn fetch_deepseek_balance_sync() -> Result<DeepseekBalanceFetch, String> {
    let Some(api_key) = read_deepseek_key() else {
        return Ok(DeepseekBalanceFetch {
            status: "unavailable".into(),
            http_status: None,
            body: None,
            error: Some("未配置 DeepSeek API Key".into()),
        });
    };
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(DEEPSEEK_BALANCE_URL)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Accept", "application/json")
        .call();
    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if (200..300).contains(&http_status) {
                Ok(DeepseekBalanceFetch {
                    status: "ok".into(),
                    http_status: Some(http_status),
                    body: Some(body),
                    error: None,
                })
            } else {
                Ok(balance_http_error(http_status))
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            Ok(balance_http_error(status))
        }
        Err(error) => Ok(DeepseekBalanceFetch {
            status: "error".into(),
            http_status: None,
            body: None,
            error: Some(format!("余额请求失败：{error}")),
        }),
    }
}

fn balance_http_error(status: u16) -> DeepseekBalanceFetch {
    let message = match status {
        401 | 403 => "API Key 无效或已过期".into(),
        429 => "请求过于频繁，请稍后再试".into(),
        _ => format!("余额请求失败（{status}）"),
    };
    DeepseekBalanceFetch {
        status: "error".into(),
        http_status: Some(status),
        body: None,
        error: Some(message),
    }
}


fn fetch_deepseek_official_models_sync() -> Result<DeepseekOfficialModelsFetch, String> {
    let token = match read_dsh_platform_grant_token() {
        Ok(token) => token,
        Err(GrantReadError::Missing) => {
            return Ok(DeepseekOfficialModelsFetch {
                status: "unauthenticated".into(),
                http_status: None,
                models: Vec::new(),
                error: Some("未登录 DeepSeek 账号（缺少 ~/.dsh platform grant）".into()),
            });
        }
        Err(GrantReadError::Invalid(message)) => {
            return Ok(DeepseekOfficialModelsFetch {
                status: "unauthenticated".into(),
                http_status: None,
                models: Vec::new(),
                error: Some(message),
            });
        }
        Err(GrantReadError::Io(message)) => {
            return Ok(DeepseekOfficialModelsFetch {
                status: "error".into(),
                http_status: None,
                models: Vec::new(),
                error: Some(message),
            });
        }
    };

    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(DEEPSEEK_MODELS_URL)
        .set("x-dsh-auth-token", &token)
        .set("Accept", "application/json")
        .set("User-Agent", MODELS_UA)
        .call();
    // Drop token as soon as the request is sent.
    drop(token);

    match result {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if !(200..300).contains(&http_status) {
                return Ok(models_http_error(http_status));
            }
            match parse_official_models_body(&body) {
                Ok(models) if !models.is_empty() => Ok(DeepseekOfficialModelsFetch {
                    status: "ok".into(),
                    http_status: Some(http_status),
                    models,
                    error: None,
                }),
                Ok(_) => Ok(DeepseekOfficialModelsFetch {
                    status: "error".into(),
                    http_status: Some(http_status),
                    models: Vec::new(),
                    error: Some("模型列表为空".into()),
                }),
                Err(message) => Ok(DeepseekOfficialModelsFetch {
                    status: "error".into(),
                    http_status: Some(http_status),
                    models: Vec::new(),
                    error: Some(message),
                }),
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            Ok(models_http_error(status))
        }
        Err(error) => Ok(DeepseekOfficialModelsFetch {
            status: "error".into(),
            http_status: None,
            models: Vec::new(),
            error: Some(format!("模型列表请求失败：{error}")),
        }),
    }
}

fn models_http_error(status: u16) -> DeepseekOfficialModelsFetch {
    let (status_label, message) = match status {
        401 | 403 => (
            "unauthorized",
            "账号授权无效或已过期（401/403）".to_string(),
        ),
        429 => ("error", "请求过于频繁，请稍后再试".into()),
        _ => ("error", format!("模型列表请求失败（{status}）")),
    };
    DeepseekOfficialModelsFetch {
        status: status_label.into(),
        http_status: Some(status),
        models: Vec::new(),
        error: Some(message),
    }
}

fn parse_official_models_body(body: &str) -> Result<Vec<DeepseekOfficialModel>, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| "模型列表响应不是有效 JSON".to_string())?;
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .ok_or_else(|| "模型列表响应缺少 data".to_string())?;
    let mut models = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let Some(id) = id else { continue };
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let effort = item.get("effort");
        let effort_levels = effort
            .and_then(|e| e.get("supported_levels"))
            .and_then(Value::as_array)
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|level| level.as_str().map(str::trim))
                    .filter(|level| !level.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|levels| !levels.is_empty());
        let effort_default = effort
            .and_then(|e| e.get("default_level"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .map(str::to_string);
        let context_window = item.get("context_window").and_then(|v| {
            v.as_u64().or_else(|| {
                v.as_i64()
                    .filter(|n| *n >= 0)
                    .map(|n| n as u64)
            })
        });
        models.push(DeepseekOfficialModel {
            id: id.to_string(),
            name,
            effort_levels,
            effort_default,
            context_window,
        });
    }
    Ok(models)
}

enum GrantReadError {
    Missing,
    Invalid(String),
    Io(String),
}

/// Read `records.deepseek-account-platform/default` grant token from `~/.dsh/.credentials.yaml`.
/// Token is returned only to the caller for the HTTP request; never log it.
fn read_dsh_platform_grant_token() -> Result<String, GrantReadError> {
    let home = dirs_home().ok_or(GrantReadError::Missing)?;
    let path = PathBuf::from(home).join(".dsh/.credentials.yaml");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(GrantReadError::Missing);
        }
        Err(error) => {
            return Err(GrantReadError::Io(format!(
                "无法读取 ~/.dsh/.credentials.yaml：{error}"
            )));
        }
    };
    extract_dsh_platform_grant_token(&raw)
}

fn extract_dsh_platform_grant_token(yaml: &str) -> Result<String, GrantReadError> {
    let needle = format!("{DSH_GRANT_RECORD}:");
    let mut in_records = false;
    let mut in_target = false;
    let mut target_indent: Option<usize> = None;
    let mut kind: Option<String> = None;
    let mut token: Option<String> = None;
    let mut issuer: Option<String> = None;
    let mut in_payload = false;
    let mut payload_indent: Option<usize> = None;

    for line in yaml.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = line.trim();

        if !in_records {
            if trimmed == "records:" || trimmed.starts_with("records:") {
                in_records = true;
            }
            continue;
        }

        if !in_target {
            if trimmed == needle || trimmed.starts_with(&format!("{needle} ")) {
                in_target = true;
                target_indent = Some(indent);
            } else if indent == 0 && !trimmed.is_empty() {
                break;
            }
            continue;
        }

        let base = target_indent.unwrap_or(0);
        if indent <= base && !trimmed.is_empty() {
            break;
        }

        if let Some(rest) = trimmed.strip_prefix("kind:") {
            kind = Some(unquote_yaml_scalar(rest.trim()).to_string());
            continue;
        }
        if trimmed == "payload:" || trimmed.starts_with("payload:") {
            in_payload = true;
            payload_indent = Some(indent);
            continue;
        }
        if in_payload {
            let pbase = payload_indent.unwrap_or(base + 2);
            if indent <= pbase
                && !trimmed.starts_with("version:")
                && !trimmed.starts_with("token:")
                && !trimmed.starts_with("issuer:")
                && !trimmed.starts_with("id:")
            {
                in_payload = false;
            } else if let Some(rest) = trimmed.strip_prefix("token:") {
                let value = unquote_yaml_scalar(rest.trim());
                if !value.is_empty() {
                    token = Some(value.to_string());
                }
                continue;
            } else if let Some(rest) = trimmed.strip_prefix("issuer:") {
                issuer = Some(unquote_yaml_scalar(rest.trim()).to_string());
                continue;
            }
        }
    }

    if !in_target {
        return Err(GrantReadError::Missing);
    }
    if kind.as_deref() != Some("grant") {
        return Err(GrantReadError::Invalid(
            "DeepSeek 凭证不是 grant 类型".into(),
        ));
    }
    match issuer.as_deref() {
        Some(value) if value == DSH_PLATFORM_ISSUER => {}
        Some(_) => {
            return Err(GrantReadError::Invalid(
                "DeepSeek grant issuer 不是 platform.deepseek.com".into(),
            ));
        }
        None => {
            return Err(GrantReadError::Invalid(
                "DeepSeek grant 缺少 issuer".into(),
            ));
        }
    }
    match token {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(GrantReadError::Invalid(
            "DeepSeek grant 缺少 token".into(),
        )),
    }
}

fn unquote_yaml_scalar(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn read_deepseek_key() -> Option<String> {
    if let Some(blob) = env_nonempty("OPENCODE_AUTH_CONTENT") {
        if let Ok(value) = serde_json::from_str::<Value>(&blob) {
            if value.is_object() {
                if let Some(key) = extract_deepseek_key(&value) {
                    return Some(key);
                }
                // Explicit auth blob without deepseek means no key.
                return None;
            }
        }
    }
    if let Some(key) = read_deepseek_config_api_key() {
        return Some(key);
    }
    let raw = read_auth_json_raw()?;
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    extract_deepseek_key(&value)
}

fn extract_deepseek_key(value: &Value) -> Option<String> {
    let entry = value.get(PROVIDER_ID)?;
    // OpenCode stores API providers as { "type": "api", "key": "..." }.
    let key = entry
        .get("key")
        .and_then(Value::as_str)
        .or_else(|| entry.get("apiKey").and_then(Value::as_str))
        .or_else(|| entry.as_str())?
        .trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn read_deepseek_config_api_key() -> Option<String> {
    if let Some(content) = env_nonempty("OPENCODE_CONFIG_CONTENT") {
        if let Ok(value) = serde_json::from_str::<Value>(&content) {
            if let Some(key) = config_deepseek_api_key(&value) {
                return Some(key);
            }
        }
    }
    for path in opencode_config_paths() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<Value>(raw.trim()) {
                if let Some(key) = config_deepseek_api_key(&value) {
                    return Some(key);
                }
            }
        }
    }
    None
}

fn config_deepseek_api_key(value: &Value) -> Option<String> {
    let provider = value.get("provider")?.get(PROVIDER_ID)?;
    let key = provider
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .or_else(|| provider.get("apiKey").and_then(Value::as_str))?
        .trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn write_deepseek_key(key: &str) -> Result<(), String> {
    let path = auth_json_path_for_write()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 OpenCode 目录：{e}"))?;
    }
    let mut root = read_auth_object().unwrap_or_else(Map::new);
    root.insert(
        PROVIDER_ID.into(),
        json!({
            "type": "api",
            "key": key,
        }),
    );
    let pretty = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("无法序列化 auth.json：{e}"))?;
    fs::write(&path, format!("{pretty}\n")).map_err(|e| format!("无法写入 auth.json：{e}"))?;
    Ok(())
}

fn clear_deepseek_key() -> Result<(), String> {
    let path = auth_json_path_for_write()?;
    let Some(mut root) = read_auth_object() else {
        return Ok(());
    };
    if root.remove(PROVIDER_ID).is_none() {
        return Ok(());
    }
    if root.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let pretty = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("无法序列化 auth.json：{e}"))?;
    fs::write(&path, format!("{pretty}\n")).map_err(|e| format!("无法写入 auth.json：{e}"))?;
    Ok(())
}

fn read_auth_object() -> Option<Map<String, Value>> {
    let raw = read_auth_json_raw()?;
    serde_json::from_str::<Value>(raw.trim())
        .ok()?
        .as_object()
        .cloned()
}

fn read_auth_json_raw() -> Option<String> {
    let primary = opencode_data_dir()?.join("auth.json");
    fs::read_to_string(&primary)
        .or_else(|_| {
            dirs_home()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home"))
                .and_then(|home| {
                    fs::read_to_string(
                        PathBuf::from(home).join("Library/Application Support/opencode/auth.json"),
                    )
                })
        })
        .ok()
}

fn auth_json_path_for_write() -> Result<PathBuf, String> {
    let dir = opencode_data_dir().ok_or_else(|| "无法定位 OpenCode 数据目录".to_string())?;
    Ok(dir.join("auth.json"))
}

fn opencode_data_dir() -> Option<PathBuf> {
    if let Some(dir) = env_nonempty("OPENCODE_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    let app = env_nonempty("OPENCODE_APPNAME").unwrap_or_else(|| "opencode".into());
    if let Some(xdg) = env_nonempty("XDG_DATA_HOME") {
        return Some(PathBuf::from(xdg).join(app));
    }
    let home = dirs_home()?;
    Some(PathBuf::from(home).join(".local/share").join(app))
}

fn opencode_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(custom) = env_nonempty("OPENCODE_CONFIG") {
        paths.push(PathBuf::from(custom));
    }
    if let Some(xdg) = env_nonempty("XDG_CONFIG_HOME") {
        let root = PathBuf::from(xdg).join("opencode");
        paths.push(root.join("opencode.jsonc"));
        paths.push(root.join("opencode.json"));
    }
    if let Some(home) = dirs_home() {
        let root = PathBuf::from(home).join(".config/opencode");
        paths.push(root.join("opencode.jsonc"));
        paths.push(root.join("opencode.json"));
    }
    paths
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}


// ---- Official account OAuth (PKCE + loopback) --------------------------------

const AUTH_API: &str = "https://platform.deepseek.com/auth-api/v0/dsh";
const ACCOUNT_SUMMARY_URL: &str =
    "https://platform.deepseek.com/api/v0/users/get_user_summary";
const LOGOUT_URL: &str = "https://platform.deepseek.com/auth-api/v0/users/logout";
const DSH_GRANT_KEY: &str = "deepseek-account-platform/default";
const DSH_DEVICE_KEY: &str = "deepseek-account-platform/device";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const OAUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekAccountStatus {
    /// Official platform grant present in ~/.dsh (separate from API key).
    pub signed_in: bool,
    pub phase: String,
    pub error: Option<String>,
    pub attempt_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekOfficialLoginResult {
    /// `ok` | `cancelled` | `failed` | `expired`
    pub status: String,
    pub signed_in: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepseekAccountQuotaFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

struct LoginAttempt {
    id: String,
    cancelled: ArcAtomicBool,
}

/// Thin alias so the OnceLock type stays readable.
#[derive(Clone)]
struct ArcAtomicBool(std::sync::Arc<AtomicBool>);

impl ArcAtomicBool {
    fn new(v: bool) -> Self {
        Self(std::sync::Arc::new(AtomicBool::new(v)))
    }
    fn store(&self, v: bool) {
        self.0.store(v, Ordering::SeqCst);
    }
    fn load(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn login_slot() -> &'static Mutex<Option<LoginAttempt>> {
    static SLOT: OnceLock<Mutex<Option<LoginAttempt>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub async fn deepseek_account_status() -> Result<DeepseekAccountStatus, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(account_status_sync()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_start_official_login(
    timezone_offset_seconds: i32,
) -> Result<DeepseekOfficialLoginResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_official_login(timezone_offset_seconds)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_cancel_official_login() -> Result<DeepseekAccountStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if let Ok(slot) = login_slot().lock() {
            if let Some(attempt) = slot.as_ref() {
                attempt.cancelled.store(true);
            }
        }
        Ok(account_status_sync())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn deepseek_sign_out_official(
    timezone_offset_seconds: i32,
) -> Result<DeepseekAccountStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Cancel any in-flight login first.
        if let Ok(mut slot) = login_slot().lock() {
            if let Some(attempt) = slot.as_ref() {
                attempt.cancelled.store(true);
            }
            *slot = None;
        }
        let token = read_dsh_platform_grant_token().ok();
        clear_dsh_platform_grant()?;
        if let Some(token) = token {
            let _ = logout_remote(&token, timezone_offset_seconds);
        }
        Ok(account_status_sync())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Official account quota (grant token), separate from API-key balance.
#[tauri::command]
pub async fn fetch_deepseek_account_quota(
    timezone_offset_seconds: i32,
) -> Result<DeepseekAccountQuotaFetch, String> {
    tauri::async_runtime::spawn_blocking(move || {
        fetch_account_quota_sync(timezone_offset_seconds)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn account_status_sync() -> DeepseekAccountStatus {
    let phase = login_slot()
        .lock()
        .ok()
        .and_then(|slot| {
            slot.as_ref().map(|a| {
                if a.cancelled.load() {
                    "cancelled"
                } else {
                    "logging-in"
                }
            })
        })
        .unwrap_or("idle");
    let attempt_id = login_slot()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|a| a.id.clone()));
    match read_dsh_platform_grant_token() {
        Ok(_) => DeepseekAccountStatus {
            signed_in: true,
            phase: if phase == "logging-in" {
                phase.into()
            } else {
                "signed-in".into()
            },
            error: None,
            attempt_id,
        },
        Err(GrantReadError::Missing) | Err(GrantReadError::Invalid(_)) => DeepseekAccountStatus {
            signed_in: false,
            phase: phase.into(),
            error: None,
            attempt_id,
        },
        Err(GrantReadError::Io(message)) => DeepseekAccountStatus {
            signed_in: false,
            phase: "error".into(),
            error: Some(message),
            attempt_id,
        },
    }
}

fn run_official_login(timezone_offset_seconds: i32) -> Result<DeepseekOfficialLoginResult, String> {
    // Single-flight: reject overlapping logins.
    {
        let slot = login_slot().lock().map_err(|_| "登录状态锁失败".to_string())?;
        if let Some(existing) = slot.as_ref() {
            if !existing.cancelled.load() {
                return Ok(DeepseekOfficialLoginResult {
                    status: "failed".into(),
                    signed_in: read_dsh_platform_grant_token().is_ok(),
                    error: Some("已有登录进行中".into()),
                });
            }
        }
    }

    let cancelled = ArcAtomicBool::new(false);
    let attempt_id = uuid::Uuid::new_v4().to_string();
    {
        let mut slot = login_slot().lock().map_err(|_| "登录状态锁失败".to_string())?;
        *slot = Some(LoginAttempt {
            id: attempt_id.clone(),
            cancelled: cancelled.clone(),
        });
    }

    let outcome = (|| {
        if cancelled.load() {
            return Ok(cancelled_result());
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("无法绑定本机回调端口：{e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("回调端口设置失败：{e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("无法读取回调端口：{e}"))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");

        let verifier = random_base64url(32);
        let state = random_base64url(32);
        let challenge = pkce_challenge(&verifier);
        let headers = platform_client_headers(timezone_offset_seconds);

        let init = auth_init(&challenge, &state, &redirect_uri, &headers)?;
        if cancelled.load() {
            let _ = auth_cancel(&init.authorize_id, &verifier, &headers);
            return Ok(cancelled_result());
        }

        let authorize_url = init.authorize_url;
        if !(authorize_url.starts_with("https://platform.deepseek.com/dsh/authorize")
            || authorize_url.starts_with("https://platform.deepseek.com/dsh/authorize?"))
        {
            return Err("授权地址不是官方 /dsh/authorize".into());
        }

        open::that(&authorize_url).map_err(|e| format!("无法打开系统浏览器：{e}"))?;

        let deadline = Instant::now() + LOGIN_TIMEOUT;
        let code = wait_for_callback(
            &listener,
            &state,
            &cancelled,
            deadline,
            &init.authorize_id,
            &verifier,
            &headers,
            &redirect_uri,
            timezone_offset_seconds,
        )?;

        match code {
            CallbackOutcome::Cancelled => {
                let _ = auth_cancel(&init.authorize_id, &verifier, &headers);
                Ok(cancelled_result())
            }
            CallbackOutcome::Expired => {
                let _ = auth_cancel(&init.authorize_id, &verifier, &headers);
                Ok(DeepseekOfficialLoginResult {
                    status: "expired".into(),
                    signed_in: false,
                    error: Some("登录超时，请重试".into()),
                })
            }
            CallbackOutcome::Failed(message) => Ok(DeepseekOfficialLoginResult {
                status: "failed".into(),
                signed_in: false,
                error: Some(message),
            }),
            CallbackOutcome::Code { code, mut stream } => {
                if cancelled.load() {
                    let _ = write_http_response(&mut stream, 204, "text/plain", "");
                    let _ = auth_cancel(&init.authorize_id, &verifier, &headers);
                    return Ok(cancelled_result());
                }
                let device_id = ensure_device_id()?;
                let exchange = match auth_exchange(
                    &code,
                    &verifier,
                    &redirect_uri,
                    &device_id,
                    &headers,
                ) {
                    Ok(v) => v,
                    Err(err) => {
                        let msg = "登录失败，请关闭此页并在应用内重试。";
                        let _ = write_http_html(&mut stream, 200, msg);
                        return Ok(DeepseekOfficialLoginResult {
                            status: "failed".into(),
                            signed_in: false,
                            error: Some(err),
                        });
                    }
                };
                if cancelled.load() {
                    let _ = write_http_response(&mut stream, 204, "text/plain", "");
                    let _ = auth_cancel(&init.authorize_id, &verifier, &headers);
                    return Ok(cancelled_result());
                }
                write_dsh_platform_grant(&exchange.token)?;
                // Prefer redirect to platform completion page when present.
                if let Some(url) = exchange.authorized_url.as_deref() {
                    let mut completion = url.to_string();
                    if !completion.contains("login_source=") {
                        completion.push_str(if completion.contains('?') { "&" } else { "?" });
                        completion.push_str("login_source=desktop");
                    }
                    let _ = write_http_redirect(&mut stream, &completion);
                } else {
                    let _ = write_http_html(
                        &mut stream,
                        200,
                        "登录成功，可以关闭此页面返回 MonoCode。",
                    );
                }
                Ok(DeepseekOfficialLoginResult {
                    status: "ok".into(),
                    signed_in: true,
                    error: None,
                })
            }
        }
    })();

    if let Ok(mut slot) = login_slot().lock() {
        *slot = None;
    }

    match outcome {
        Ok(result) => Ok(result),
        Err(message) => Ok(DeepseekOfficialLoginResult {
            status: "failed".into(),
            signed_in: read_dsh_platform_grant_token().is_ok(),
            error: Some(message),
        }),
    }
}

fn cancelled_result() -> DeepseekOfficialLoginResult {
    DeepseekOfficialLoginResult {
        status: "cancelled".into(),
        signed_in: read_dsh_platform_grant_token().is_ok(),
        error: Some("已取消登录".into()),
    }
}

struct AuthInit {
    authorize_url: String,
    authorize_id: String,
}

struct AuthExchange {
    token: String,
    authorized_url: Option<String>,
}

enum CallbackOutcome {
    Code { code: String, stream: TcpStream },
    Cancelled,
    Expired,
    Failed(String),
}

fn platform_client_headers(timezone_offset_seconds: i32) -> Vec<(String, String)> {
    vec![
        ("User-Agent".into(), CHROME_UA.into()),
        ("Origin".into(), DSH_PLATFORM_ISSUER.into()),
        ("Referer".into(), format!("{DSH_PLATFORM_ISSUER}/")),
        ("Accept".into(), "application/json".into()),
        ("x-client-bundle-id".into(), "".into()),
        ("x-client-platform".into(), "desktop-mac".into()),
        ("x-client-version".into(), CLIENT_VERSION.into()),
        ("x-client-locale".into(), "zh_CN".into()),
        (
            "x-client-timezone-offset".into(),
            timezone_offset_seconds.to_string(),
        ),
    ]
}

fn apply_headers(req: ureq::Request, headers: &[(String, String)]) -> ureq::Request {
    let mut req = req;
    for (k, v) in headers {
        req = req.set(k, v);
    }
    req
}

fn auth_init(
    challenge: &str,
    state: &str,
    redirect_uri: &str,
    headers: &[(String, String)],
) -> Result<AuthInit, String> {
    let body = json!({
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "state": state,
        "redirect_uri": redirect_uri,
        "locale": "zh_CN",
        "login_source": "desktop",
    });
    let biz = post_auth_api("auth_init", &body, headers)?;
    let authorize_url = biz
        .get("authorize_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "auth_init 缺少 authorize_url".to_string())?
        .to_string();
    let authorize_id = biz
        .get("authorize_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "auth_init 缺少 authorize_id".to_string())?
        .to_string();
    Ok(AuthInit {
        authorize_url,
        authorize_id,
    })
}

fn auth_exchange(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    device_id: &str,
    headers: &[(String, String)],
) -> Result<AuthExchange, String> {
    let os_name = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    let device_model = format!("{os_name}-{arch}");
    let os_version = std::process::Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{os_name} {arch}"));
    let body = json!({
        "code": code,
        "code_verifier": verifier,
        "redirect_uri": redirect_uri,
        "device_id": device_id,
        "device_model": device_model,
        "os_version": os_version,
    });
    let biz = post_auth_api("auth_exchange", &body, headers)?;
    let token = biz
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "auth_exchange 缺少 token".to_string())?
        .to_string();
    let authorized_url = biz
        .get("authorized_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(AuthExchange {
        token,
        authorized_url,
    })
}

fn auth_cancel(authorize_id: &str, verifier: &str, headers: &[(String, String)]) -> Result<(), String> {
    let body = json!({
        "authorize_id": authorize_id,
        "code_verifier": verifier,
    });
    let _ = post_auth_api("auth_cancel", &body, headers);
    Ok(())
}

fn post_auth_api(
    method: &str,
    body: &Value,
    headers: &[(String, String)],
) -> Result<Value, String> {
    let url = format!("{AUTH_API}/{method}");
    let agent = ureq::AgentBuilder::new()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .redirects(0)
        .build();
    let req = apply_headers(
        agent
            .post(&url)
            .set("Content-Type", "application/json"),
        headers,
    );
    let payload = serde_json::to_string(body)
        .map_err(|e| format!("{method} 无法序列化请求：{e}"))?;
    let result = req.send_string(&payload);
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            return Err(platform_http_error(method, status));
        }
        Err(error) => {
            return Err(format!("{method} 请求失败：{error}"));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(platform_http_error(method, status));
    }
    parse_biz_data(&text, method)
}

fn platform_http_error(method: &str, status: u16) -> String {
    match status {
        401 | 403 => format!("{method} 被拒绝（{status}）"),
        429 => "请求过于频繁，请稍后再试".into(),
        _ => format!("{method} 失败（{status}）"),
    }
}

fn parse_biz_data(text: &str, method: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| format!("{method} 响应不是有效 JSON"))?;
    if value.get("code").and_then(Value::as_i64) == Some(40003) {
        return Err("账号授权已失效".into());
    }
    let code = value.get("code").and_then(Value::as_i64);
    if code != Some(0) {
        return Err(format!("{method} 返回异常 code"));
    }
    let data = value
        .get("data")
        .ok_or_else(|| format!("{method} 缺少 data"))?;
    let biz_code = data.get("biz_code").and_then(Value::as_i64);
    if biz_code != Some(0) {
        return Err("操作失败，请重试".into());
    }
    data.get("biz_data")
        .cloned()
        .ok_or_else(|| format!("{method} 缺少 biz_data"))
}

fn wait_for_callback(
    listener: &TcpListener,
    expected_state: &str,
    cancelled: &ArcAtomicBool,
    deadline: Instant,
    authorize_id: &str,
    verifier: &str,
    headers: &[(String, String)],
    _redirect_uri: &str,
    _timezone_offset_seconds: i32,
) -> Result<CallbackOutcome, String> {
    while Instant::now() < deadline {
        if cancelled.load() {
            return Ok(CallbackOutcome::Cancelled);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                match read_callback_request(&mut stream) {
                    Ok(CallbackRequest::Valid { code, state }) => {
                        if !constant_time_eq(state.as_bytes(), expected_state.as_bytes()) {
                            let _ = write_http_response(&mut stream, 400, "text/plain", "bad state");
                            continue;
                        }
                        return Ok(CallbackOutcome::Code { code, stream });
                    }
                    Ok(CallbackRequest::Ignore) => {
                        let _ = write_http_response(&mut stream, 404, "text/plain", "not found");
                    }
                    Ok(CallbackRequest::Bad) => {
                        let _ = write_http_response(&mut stream, 400, "text/plain", "bad request");
                    }
                    Err(_) => {
                        let _ = write_http_response(&mut stream, 400, "text/plain", "bad request");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                let _ = auth_cancel(authorize_id, verifier, headers);
                return Ok(CallbackOutcome::Failed(format!("回调监听失败：{error}")));
            }
        }
    }
    Ok(CallbackOutcome::Expired)
}

enum CallbackRequest {
    Valid { code: String, state: String },
    Bad,
    Ignore,
}

fn read_callback_request(stream: &mut TcpStream) -> Result<CallbackRequest, String> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let raw = String::from_utf8_lossy(&buf[..n]);
    let first = raw.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method != "GET" {
        return Ok(CallbackRequest::Bad);
    }
    let url = format!("http://127.0.0.1{path}");
    let parsed = url::Url::parse(&url).map_err(|_| "bad url".to_string())?;
    if parsed.path() != "/oauth/callback" {
        return Ok(CallbackRequest::Ignore);
    }
    let codes: Vec<_> = parsed
        .query_pairs()
        .filter(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .collect();
    let states: Vec<_> = parsed
        .query_pairs()
        .filter(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .collect();
    if codes.len() != 1 || states.len() != 1 || codes[0].is_empty() || states[0].is_empty() {
        return Ok(CallbackRequest::Bad);
    }
    Ok(CallbackRequest::Valid {
        code: codes[0].clone(),
        state: states[0].clone(),
    })
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        410 => "Gone",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    let _ = stream.flush();
    Ok(())
}

fn write_http_html(stream: &mut TcpStream, status: u16, message: &str) -> Result<(), String> {
    let body = format!(
        "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>{message}</title><body><p>{message}</p><script>window.close()</script></body></html>"
    );
    write_http_response(stream, status, "text/html; charset=utf-8", &body)
}

fn write_http_redirect(stream: &mut TcpStream, location: &str) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| e.to_string())?;
    let _ = stream.flush();
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn random_base64url(nbytes: usize) -> String {
    let mut bytes = vec![0u8; nbytes];
    // uuid v4 gives 16 bytes of randomness; fill by chunks.
    let mut offset = 0;
    while offset < nbytes {
        let chunk = uuid::Uuid::new_v4();
        let src = chunk.as_bytes();
        let n = (nbytes - offset).min(src.len());
        bytes[offset..offset + n].copy_from_slice(&src[..n]);
        offset += n;
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn credentials_yaml_path() -> Result<PathBuf, String> {
    let home = dirs_home().ok_or_else(|| "无法定位用户目录".to_string())?;
    Ok(PathBuf::from(home).join(".dsh/.credentials.yaml"))
}

fn ensure_device_id() -> Result<String, String> {
    if let Some(id) = read_device_id() {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    upsert_yaml_record(
        DSH_DEVICE_KEY,
        &format!(
            "  {DSH_DEVICE_KEY}:\n    kind: grant\n    payload:\n      id: {id}\n"
        ),
    )?;
    Ok(id)
}

fn read_device_id() -> Option<String> {
    let path = credentials_yaml_path().ok()?;
    let raw = fs::read_to_string(path).ok()?;
    let needle = format!("{DSH_DEVICE_KEY}:");
    let mut in_target = false;
    let mut base = 0usize;
    for line in raw.lines() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = line.trim();
        if !in_target {
            if trimmed == needle || trimmed.starts_with(&format!("{needle} ")) {
                in_target = true;
                base = indent;
            }
            continue;
        }
        if indent <= base && !trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("id:") {
            let value = unquote_yaml_scalar(rest.trim());
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn write_dsh_platform_grant(token: &str) -> Result<(), String> {
    let quoted = yaml_quote(token);
    let block = format!(
        "  {DSH_GRANT_KEY}:\n    kind: grant\n    payload:\n      version: 1\n      token: {quoted}\n      issuer: {DSH_PLATFORM_ISSUER}\n"
    );
    upsert_yaml_record(DSH_GRANT_KEY, &block)
}

fn clear_dsh_platform_grant() -> Result<(), String> {
    remove_yaml_record(DSH_GRANT_KEY)
}

fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn upsert_yaml_record(key: &str, block: &str) -> Result<(), String> {
    let path = credentials_yaml_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 ~/.dsh：{e}"))?;
    }
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let next = if existing.trim().is_empty() {
        format!("version: 1\nrecords:\n{block}")
    } else {
        replace_or_insert_record(&existing, key, block)?
    };
    atomic_write_restricted(&path, &next)
}

fn remove_yaml_record(key: &str) -> Result<(), String> {
    let path = credentials_yaml_path()?;
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("无法读取 ~/.dsh/.credentials.yaml：{error}")),
    };
    let next = match replace_or_insert_record(&existing, key, "") {
        Ok(text) => text,
        Err(_) => return Ok(()),
    };
    atomic_write_restricted(&path, &next)
}

fn replace_or_insert_record(yaml: &str, key: &str, block: &str) -> Result<String, String> {
    let needle = format!("{key}:");
    let lines: Vec<&str> = yaml.lines().collect();
    let mut records_line: Option<usize> = None;
    let mut target_start: Option<usize> = None;
    let mut target_end: Option<usize> = None;
    let mut in_records = false;
    let mut records_indent = 0usize;
    let mut target_indent = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = line.trim();
        if !in_records {
            if trimmed == "records:" || trimmed.starts_with("records:") {
                in_records = true;
                records_line = Some(idx);
                records_indent = indent;
            }
            continue;
        }
        if target_start.is_none() {
            if trimmed == needle || trimmed.starts_with(&format!("{needle} ")) {
                target_start = Some(idx);
                target_indent = indent;
                continue;
            }
            if !trimmed.is_empty() && indent <= records_indent {
                break;
            }
            continue;
        }
        if !trimmed.is_empty() && indent <= target_indent {
            target_end = Some(idx);
            break;
        }
    }
    if target_start.is_some() && target_end.is_none() {
        target_end = Some(lines.len());
    }

    if let (Some(start), Some(end)) = (target_start, target_end) {
        let mut out: Vec<String> = Vec::new();
        out.extend(lines[..start].iter().map(|s| (*s).to_string()));
        if !block.is_empty() {
            for bl in block.lines() {
                out.push(bl.to_string());
            }
        }
        out.extend(lines[end..].iter().map(|s| (*s).to_string()));
        let mut text = out.join("\n");
        if yaml.ends_with('\n') && !text.ends_with('\n') {
            text.push('\n');
        }
        return Ok(text);
    }

    if block.is_empty() {
        return Ok(yaml.to_string());
    }

    // Insert under records:
    if let Some(ridx) = records_line {
        let mut out: Vec<String> = Vec::new();
        out.extend(lines[..=ridx].iter().map(|s| (*s).to_string()));
        for bl in block.lines() {
            out.push(bl.to_string());
        }
        out.extend(lines[ridx + 1..].iter().map(|s| (*s).to_string()));
        let mut text = out.join("\n");
        if yaml.ends_with('\n') && !text.ends_with('\n') {
            text.push('\n');
        }
        return Ok(text);
    }

    // No records: section — append one.
    let mut text = yaml.trim_end().to_string();
    text.push_str("\nrecords:\n");
    text.push_str(block);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn atomic_write_restricted(path: &PathBuf, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("yaml.tmp-monocode");
    fs::write(&tmp, contents).map_err(|e| format!("无法写入凭证临时文件：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path).map_err(|e| format!("无法保存 ~/.dsh/.credentials.yaml：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn fetch_account_quota_sync(timezone_offset_seconds: i32) -> Result<DeepseekAccountQuotaFetch, String> {
    let token = match read_dsh_platform_grant_token() {
        Ok(token) => token,
        Err(GrantReadError::Missing) | Err(GrantReadError::Invalid(_)) => {
            return Ok(DeepseekAccountQuotaFetch {
                status: "unauthenticated".into(),
                http_status: None,
                body: None,
                error: Some("未登录 DeepSeek 官方账号".into()),
            });
        }
        Err(GrantReadError::Io(message)) => {
            return Ok(DeepseekAccountQuotaFetch {
                status: "error".into(),
                http_status: None,
                body: None,
                error: Some(message),
            });
        }
    };
    let headers = platform_client_headers(timezone_offset_seconds);
    let agent = ureq::AgentBuilder::new()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .redirects(0)
        .build();
    let mut req = agent
        .get(ACCOUNT_SUMMARY_URL)
        .set("x-dsh-auth-token", &token)
        .set("Accept", "application/json");
    req = apply_headers(req, &headers);
    drop(token);
    match req.call() {
        Ok(response) => {
            let http_status = response.status();
            let body = response.into_string().unwrap_or_default();
            if !(200..300).contains(&http_status) {
                return Ok(quota_http_error(http_status));
            }
            // Validate envelope lightly; return biz_data JSON for the UI.
            match parse_biz_data(&body, "get_user_summary") {
                Ok(biz) => Ok(DeepseekAccountQuotaFetch {
                    status: "ok".into(),
                    http_status: Some(http_status),
                    body: Some(biz.to_string()),
                    error: None,
                }),
                Err(message) => Ok(DeepseekAccountQuotaFetch {
                    status: "error".into(),
                    http_status: Some(http_status),
                    body: None,
                    error: Some(message),
                }),
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            Ok(quota_http_error(status))
        }
        Err(error) => Ok(DeepseekAccountQuotaFetch {
            status: "error".into(),
            http_status: None,
            body: None,
            error: Some(format!("额度请求失败：{error}")),
        }),
    }
}

fn quota_http_error(status: u16) -> DeepseekAccountQuotaFetch {
    let (label, message) = match status {
        401 | 403 => ("unauthorized", "账号授权无效或已过期".to_string()),
        429 => ("error", "请求过于频繁，请稍后再试".into()),
        _ => ("error", format!("额度请求失败（{status}）")),
    };
    DeepseekAccountQuotaFetch {
        status: label.into(),
        http_status: Some(status),
        body: None,
        error: Some(message),
    }
}

fn logout_remote(token: &str, timezone_offset_seconds: i32) -> Result<(), String> {
    let headers = platform_client_headers(timezone_offset_seconds);
    let agent = ureq::AgentBuilder::new()
        .timeout(OAUTH_HTTP_TIMEOUT)
        .redirects(0)
        .build();
    let mut req = agent
        .post(LOGOUT_URL)
        .set("x-dsh-auth-token", token)
        .set("Content-Type", "application/json");
    req = apply_headers(req, &headers);
    let _ = req.send_string("{}");
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_key_from_auth_object() {
        let value = serde_json::json!({
            "deepseek": { "type": "api", "key": "sk-test-abcd" },
            "opencode-go": { "type": "api", "key": "other" }
        });
        assert_eq!(
            extract_deepseek_key(&value).as_deref(),
            Some("sk-test-abcd")
        );
    }

    #[test]
    fn mask_keeps_last_four() {
        assert_eq!(mask_hint("sk-abcdefgh"), "••••efgh");
    }

    #[test]
    fn extracts_platform_grant_token() {
        let yaml = r#"
version: 1
records:
  deepseek-account-platform/default:
    kind: grant
    payload:
      version: 1
      token: test-token-value
      issuer: https://platform.deepseek.com
"#;
        assert_eq!(
            extract_dsh_platform_grant_token(yaml).ok().as_deref(),
            Some("test-token-value")
        );
    }

    #[test]
    fn rejects_wrong_issuer() {
        let yaml = r#"
records:
  deepseek-account-platform/default:
    kind: grant
    payload:
      token: test-token-value
      issuer: https://example.com
"#;
        assert!(matches!(
            extract_dsh_platform_grant_token(yaml),
            Err(GrantReadError::Invalid(_))
        ));
    }

    #[test]
    fn parses_models_json() {
        let body = r#"{
          "object":"list",
          "data":[
            {"id":"deepseek-flash","name":"DeepSeek-V4.1-Flash","effort":{"supported_levels":["low","high","max"],"default_level":"high"},"context_window":1048576},
            {"id":"deepseek-v4-pro","name":"DeepSeek-V4-Pro"}
          ]
        }"#;
        let models = parse_official_models_body(body).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "deepseek-flash");
        assert_eq!(models[0].effort_default.as_deref(), Some("high"));
        assert_eq!(models[1].id, "deepseek-v4-pro");
    }
}
