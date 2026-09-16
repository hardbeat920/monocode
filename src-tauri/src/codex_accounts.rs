use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Manager;

// Atomic replacement alone does not protect read/modify/write transactions.
static STORE_LOCK: Mutex<()> = Mutex::new(());
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub plan_type: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub blocked_until_ms: Option<i64>,
    pub disabled_cause: Option<String>,
    pub added_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct Pool {
    version: u32,
    accounts: Vec<Account>,
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
        }
    }
}

fn pool_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("codex-accounts.json"))
}

fn read_pool(path: &Path) -> Pool {
    std::fs::read(path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Pool>(&raw).ok())
        .filter(|pool| pool.version == 1)
        .unwrap_or_default()
}

fn atomic_write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing credential directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(".codex-credentials-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
        let raw = serde_json::to_vec(value).map_err(|_| "Cannot encode credentials")?;
        file.write_all(&raw).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

fn redacted(account: &Account) -> Value {
    json!({
        "id": account.id, "email": account.email, "accountId": account.account_id,
        "planType": account.plan_type, "expiresAtMs": account.expires_at_ms,
        "blockedUntilMs": account.blocked_until_ms, "disabledCause": account.disabled_cause,
        "addedAt": account.added_at, "lastUsedAt": account.last_used_at,
        "hasTokens": !account.access_token.is_empty() && !account.refresh_token.is_empty()
    })
}

// A refresh holds the transaction lock across HTTP IO. Never wait for it on
// Tauri's UI thread or async executor.
async fn store_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| "Codex credential worker failed".to_string())?
}

#[tauri::command]
pub async fn codex_accounts_list(app: tauri::AppHandle) -> Result<Vec<Value>, String> {
    store_task(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        Ok(read_pool(&pool_path(&app)?)
            .accounts
            .iter()
            .map(redacted)
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn codex_account_upsert(app: tauri::AppHandle, account: Account) -> Result<(), String> {
    store_task(move || {
        if account.id.is_empty() || account.id == "external" || account.account_id.is_empty() {
            return Err("Invalid pool account identity".into());
        }
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let path = pool_path(&app)?;
        let mut pool = read_pool(&path);
        if let Some(existing) = pool.accounts.iter_mut().find(|a| a.id == account.id) {
            *existing = account;
        } else {
            pool.accounts.push(account);
        }
        atomic_write(&path, &pool)
    })
    .await
}

#[tauri::command]
pub async fn codex_account_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    store_task(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let path = pool_path(&app)?;
        let mut pool = read_pool(&path);
        pool.accounts.retain(|a| a.id != id);
        atomic_write(&path, &pool)
    })
    .await
}

// Missing keys leave state unchanged; null explicitly clears a state field.
#[tauri::command]
pub async fn codex_account_update_state(
    app: tauri::AppHandle,
    id: String,
    state: Value,
) -> Result<(), String> {
    store_task(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let path = pool_path(&app)?;
        let mut pool = read_pool(&path);
        let account = pool
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("Unknown Codex account")?;
        if let Some(value) = state.get("blockedUntilMs") {
            account.blocked_until_ms =
                serde_json::from_value(value.clone()).map_err(|_| "Invalid blockedUntilMs")?;
        }
        if let Some(value) = state.get("disabledCause") {
            account.disabled_cause =
                serde_json::from_value(value.clone()).map_err(|_| "Invalid disabledCause")?;
        }
        if let Some(value) = state.get("lastUsedAt") {
            account.last_used_at =
                serde_json::from_value(value.clone()).map_err(|_| "Invalid lastUsedAt")?;
        }
        atomic_write(&path, &pool)
    })
    .await
}

#[tauri::command]
pub async fn codex_account_credentials(
    app: tauri::AppHandle,
    id: String,
) -> Result<Account, String> {
    store_task(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        read_pool(&pool_path(&app)?)
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "Unknown Codex account".into())
    })
    .await
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn refresh_token(refresh: &str) -> Result<Value, String> {
    if refresh.is_empty() {
        return Err("Missing refresh token".into());
    }
    // OMP's openai-codex.kdl declares body="form" (not JSON).
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .post("https://auth.openai.com/oauth/token")
        .send_form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh),
        ])
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                format!("Codex refresh failed (HTTP {status}); sign in again")
            }
            _ => "Codex refresh transport failed".into(),
        })?;
    let raw = response
        .into_string()
        .map_err(|_| "Cannot read refresh response")?;
    let value: Value = serde_json::from_str(&raw).map_err(|_| "Invalid refresh response")?;
    if token_field(&value, "access_token").is_none()
        || value
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|v| *v > 0)
            .is_none()
    {
        return Err("Incomplete refresh response".into());
    }
    Ok(value)
}

fn token_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn apply_refresh(account: &mut Account, response: &Value) {
    account.access_token = response["access_token"].as_str().unwrap_or_default().into();
    if let Some(refresh) = token_field(response, "refresh_token") {
        account.refresh_token = refresh.into();
    }
    account.expires_at_ms = Some(
        now_ms().saturating_add(
            response["expires_in"]
                .as_i64()
                .unwrap_or(0)
                .saturating_mul(1000),
        ),
    );
    account.disabled_cause = None;
}

#[tauri::command]
pub async fn codex_account_refresh(app: tauri::AppHandle, id: String) -> Result<Account, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let path = pool_path(&app)?;
        let mut pool = read_pool(&path);
        let account = pool
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("Unknown Codex account")?;
        let result = refresh_token(&account.refresh_token);
        match &result {
            Ok(response) => apply_refresh(account, response),
            Err(error) => account.disabled_cause = Some(error.clone()),
        }
        let updated = account.clone();
        atomic_write(&path, &pool)?;
        result.map(|_| updated)
    })
    .await
    .map_err(|_| "Codex refresh worker failed".to_string())?
}

fn auth_path() -> Result<PathBuf, String> {
    crate::dirs_home()
        .map(|home| PathBuf::from(home).join(".codex/auth.json"))
        .ok_or_else(|| "Home directory unavailable".into())
}

fn read_auth(path: &Path) -> Result<Value, String> {
    let raw = std::fs::read(path).map_err(|_| "Codex auth.json unavailable")?;
    serde_json::from_slice(&raw).map_err(|_| "Invalid Codex auth.json".into())
}

fn auth_projection(blob: &Value) -> Result<Value, String> {
    let tokens = &blob["tokens"];
    Ok(json!({
        "accessToken": token_field(tokens, "access_token").ok_or("Missing Codex access token")?,
        "refreshToken": token_field(tokens, "refresh_token").unwrap_or_default(),
        "accountId": token_field(tokens, "account_id").ok_or("Missing Codex account id")?,
        "idToken": tokens.get("id_token"), "lastRefresh": blob.get("last_refresh")
    }))
}

#[tauri::command]
pub fn codex_auth_json_read() -> Result<Value, String> {
    auth_projection(&read_auth(&auth_path()?)?)
}

// Only the external account may write auth.json. Preserve every unknown field.
#[tauri::command]
pub async fn codex_auth_json_refresh(
    account_id: String,
    last_refresh: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let path = auth_path()?;
        let original = read_auth(&path)?;
        if token_field(&original["tokens"], "account_id") != Some(account_id.as_str()) {
            return Err("External Codex account changed".into());
        }
        let response =
            refresh_token(token_field(&original["tokens"], "refresh_token").unwrap_or_default())?;
        // Do not overwrite a refresh/login performed by the CLI during HTTP IO.
        if read_auth(&path)? != original {
            return Err("Codex auth.json changed during refresh; retry".into());
        }
        let mut blob = original;
        apply_auth_refresh(&mut blob, &response, &last_refresh);
        atomic_write(&path, &blob)?;
        auth_projection(&blob)
    })
    .await
    .map_err(|_| "Codex refresh worker failed".to_string())?
}

fn apply_auth_refresh(blob: &mut Value, response: &Value, last_refresh: &str) {
    for key in ["access_token", "refresh_token", "id_token"] {
        if let Some(value) = token_field(response, key) {
            blob["tokens"][key] = json!(value);
        }
    }
    blob["last_refresh"] = json!(last_refresh);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        serde_json::from_value(json!({"id":"test", "email":"test@example.invalid", "accountId":"workspace", "accessToken":"fake-access", "refreshToken":"fake-refresh", "addedAt":1})).unwrap()
    }

    #[test]
    fn pool_roundtrip_redaction_and_atomic_replacement() {
        let dir = std::env::temp_dir().join(format!("monocode-codex-{}", uuid::Uuid::new_v4()));
        let path = dir.join("pool.json");
        assert!(read_pool(&path).accounts.is_empty());
        let pool = Pool {
            version: 1,
            accounts: vec![account()],
        };
        atomic_write(&path, &pool).unwrap();
        assert_eq!(read_pool(&path).accounts[0].id, "test");
        let projection = redacted(&pool.accounts[0]);
        assert_eq!(projection["hasTokens"], true);
        assert!(projection.get("accessToken").is_none());
        assert!(projection.get("refreshToken").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        atomic_write(&path, &Pool::default()).unwrap();
        assert!(read_pool(&path).accounts.is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::write(&path, "broken json").unwrap();
        assert!(read_pool(&path).accounts.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refresh_preserves_unrotated_tokens_and_external_fields() {
        let mut account = account();
        apply_refresh(
            &mut account,
            &json!({"access_token":"fake-new", "expires_in":3600}),
        );
        assert_eq!(account.refresh_token, "fake-refresh");
        assert!(account.expires_at_ms.unwrap() > now_ms());
        let mut blob = json!({"auth_mode":"chatgpt", "OPENAI_API_KEY":null, "custom":42, "tokens":{"account_id":"workspace", "refresh_token":"fake-refresh", "custom":true}});
        apply_auth_refresh(
            &mut blob,
            &json!({"access_token":"fake-new"}),
            "2030-01-01T00:00:00Z",
        );
        assert_eq!(blob["custom"], 42);
        assert_eq!(blob["tokens"]["custom"], true);
        assert_eq!(blob["tokens"]["refresh_token"], "fake-refresh");
        assert_eq!(blob["tokens"]["account_id"], "workspace");
    }
}
