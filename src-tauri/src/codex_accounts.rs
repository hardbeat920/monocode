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

// Only a missing file means an empty pool. Corrupt or future-version data
// must error rather than be overwritten by a later write.
fn read_pool(path: &Path) -> Result<Pool, String> {
    match std::fs::read(path) {
        Ok(raw) => {
            let pool = serde_json::from_slice::<Pool>(&raw)
                .map_err(|_| "Cannot parse Codex account store".to_string())?;
            if pool.version != 1 {
                return Err("Unsupported Codex account store version".into());
            }
            Ok(pool)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Pool::default()),
        Err(error) => Err(error.to_string()),
    }
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
        Ok(read_pool(&pool_path(&app)?)?
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
        let mut pool = read_pool(&path)?;
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
        let mut pool = read_pool(&path)?;
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
        let mut pool = read_pool(&path)?;
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
        read_pool(&pool_path(&app)?)?
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

// Permanent failures (rejected grants) may retire a credential; everything
// else is transient and must not disable a healthy account.
struct RefreshFailure {
    message: String,
    permanent: bool,
}

fn refresh_token(refresh: &str) -> Result<Value, RefreshFailure> {
    if refresh.is_empty() {
        return Err(RefreshFailure {
            message: "Missing refresh token".into(),
            permanent: true,
        });
    }
    // OMP's openai-codex.kdl declares body="form" (not JSON).
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build()
        .post("https://auth.openai.com/oauth/token")
        .send_form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh),
        ])
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => RefreshFailure {
                message: format!("Codex refresh failed (HTTP {status}); sign in again"),
                permanent: matches!(status, 400 | 401 | 403),
            },
            _ => RefreshFailure {
                message: "Codex refresh transport failed".into(),
                permanent: false,
            },
        })?;
    let raw = response.into_string().map_err(|_| RefreshFailure {
        message: "Cannot read refresh response".into(),
        permanent: false,
    })?;
    let value: Value = serde_json::from_str(&raw).map_err(|_| RefreshFailure {
        message: "Invalid refresh response".into(),
        permanent: false,
    })?;
    if token_field(&value, "access_token").is_none() {
        return Err(RefreshFailure {
            message: "Incomplete refresh response".into(),
            permanent: false,
        });
    }
    Ok(value)
}

fn token_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn jwt_claims(token: &str) -> Option<Value> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&raw).ok()
}

fn jwt_expiry_ms(token: &str) -> Option<i64> {
    jwt_claims(token)?
        .get("exp")?
        .as_i64()
        .map(|exp| exp.saturating_mul(1000))
}

fn apply_refresh(account: &mut Account, response: &Value) {
    account.access_token = response["access_token"].as_str().unwrap_or_default().into();
    // A rotated refresh token must be kept even without expiry metadata.
    if let Some(refresh) = token_field(response, "refresh_token") {
        account.refresh_token = refresh.into();
    }
    account.expires_at_ms = response
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|v| *v > 0)
        .map(|seconds| now_ms().saturating_add(seconds.saturating_mul(1000)))
        .or_else(|| jwt_expiry_ms(&account.access_token))
        .or(account.expires_at_ms);
    if let Some(claims) = token_field(response, "id_token").and_then(jwt_claims) {
        if let Some(email) = token_field(&claims, "email") {
            account.email = email.into();
        }
        if let Some(plan) = claims
            .get("https://api.openai.com/auth")
            .and_then(|auth| token_field(auth, "chatgpt_plan_type"))
        {
            account.plan_type = Some(plan.into());
        }
    }
    account.disabled_cause = None;
}

#[tauri::command]
pub async fn codex_account_refresh(app: tauri::AppHandle, id: String) -> Result<Account, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = pool_path(&app)?;
        // Hold the store lock only for reads/writes; HTTP IO runs unlocked so
        // independent accounts never serialize behind the network.
        let original_refresh = {
            let _guard = STORE_LOCK
                .lock()
                .map_err(|_| "Credential store unavailable")?;
            read_pool(&path)?
                .accounts
                .iter()
                .find(|a| a.id == id)
                .ok_or("Unknown Codex account")?
                .refresh_token
                .clone()
        };
        let result = refresh_token(&original_refresh);
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let mut pool = read_pool(&path)?;
        let account = pool
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("Unknown Codex account")?;
        match result {
            Ok(response) => {
                if account.refresh_token == original_refresh {
                    apply_refresh(account, &response);
                } else {
                    // A concurrent refresh already rotated the grant; keep the
                    // newer credentials but still adopt a fresh access token.
                    if let Some(access) = token_field(&response, "access_token") {
                        account.access_token = access.into();
                    }
                    account.disabled_cause = None;
                }
                let updated = account.clone();
                atomic_write(&path, &pool)?;
                Ok(updated)
            }
            Err(error) => {
                if error.permanent {
                    account.disabled_cause = Some(error.message.clone());
                    atomic_write(&path, &pool)?;
                    Err(format!("CODEX_PERMANENT:{}", error.message))
                } else {
                    Err(error.message)
                }
            }
        }
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
    let claims = token_field(tokens, "id_token").and_then(jwt_claims);
    Ok(json!({
        "accessToken": token_field(tokens, "access_token").ok_or("Missing Codex access token")?,
        "refreshToken": token_field(tokens, "refresh_token").unwrap_or_default(),
        "accountId": token_field(tokens, "account_id").ok_or("Missing Codex account id")?,
        "idToken": tokens.get("id_token"), "lastRefresh": blob.get("last_refresh"),
        "email": claims.as_ref().and_then(|c| token_field(c, "email")),
        "planType": claims
            .as_ref()
            .and_then(|c| c.get("https://api.openai.com/auth"))
            .and_then(|auth| token_field(auth, "chatgpt_plan_type")),
    }))
}

#[tauri::command]
pub fn codex_auth_json_read() -> Result<Value, String> {
    auth_projection(&read_auth(&auth_path()?)?)
}

/// Enroll the CLI's current auth.json identity into the pool so it becomes a
/// failover candidate after the user signs in elsewhere. Keyed by account id:
/// re-capturing refreshes stored tokens in place and preserves usage state.
#[tauri::command]
pub async fn codex_account_capture_current(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        let pool_file = pool_path(&app)?;
        let blob = read_auth(&auth_path()?)?;
        let tokens = &blob["tokens"];
        let account_id = token_field(tokens, "account_id")
            .ok_or("Missing Codex account id")?
            .to_string();
        let access = token_field(tokens, "access_token")
            .ok_or("Missing Codex access token")?
            .to_string();
        let refresh = token_field(tokens, "refresh_token")
            .ok_or("Missing Codex refresh token")?
            .to_string();
        let claims = token_field(tokens, "id_token").and_then(jwt_claims);
        let email = claims
            .as_ref()
            .and_then(|c| token_field(c, "email"))
            .ok_or("Cannot identify Codex account email")?
            .to_string();
        let plan = claims
            .as_ref()
            .and_then(|c| c.get("https://api.openai.com/auth"))
            .and_then(|auth| token_field(auth, "chatgpt_plan_type"))
            .map(str::to_string);
        let expiry = jwt_expiry_ms(&access);

        let mut pool = read_pool(&pool_file)?;
        let existing = pool
            .accounts
            .iter_mut()
            .find(|a| a.account_id == account_id);
        match existing {
            Some(account) => {
                let unchanged = account.access_token == access
                    && account.refresh_token == refresh
                    && account.email == email;
                account.access_token = access;
                account.refresh_token = refresh;
                account.email = email;
                account.plan_type = plan;
                account.expires_at_ms = expiry;
                if unchanged {
                    return Ok(redacted(account));
                }
                let projection = redacted(account);
                atomic_write(&pool_file, &pool)?;
                Ok(projection)
            }
            None => {
                let account = Account {
                    id: account_id.clone(),
                    email,
                    account_id,
                    access_token: access,
                    refresh_token: refresh,
                    plan_type: plan,
                    expires_at_ms: expiry,
                    blocked_until_ms: None,
                    disabled_cause: None,
                    added_at: now_ms(),
                    last_used_at: None,
                };
                let projection = redacted(&account);
                pool.accounts.push(account);
                atomic_write(&pool_file, &pool)?;
                Ok(projection)
            }
        }
    })
    .await
    .map_err(|_| "Codex account capture worker failed".to_string())?
}

// Only the external account may write auth.json. Preserve every unknown field.
#[tauri::command]
pub async fn codex_auth_json_refresh(account_id: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = auth_path()?;
        let original = {
            let _guard = STORE_LOCK
                .lock()
                .map_err(|_| "Credential store unavailable")?;
            let blob = read_auth(&path)?;
            if token_field(&blob["tokens"], "account_id") != Some(account_id.as_str()) {
                return Err("External Codex account changed".into());
            }
            blob
        };
        let response =
            refresh_token(token_field(&original["tokens"], "refresh_token").unwrap_or_default())
                .map_err(|error| error.message)?;
        // Do not overwrite a refresh/login performed by the CLI during HTTP IO.
        let _guard = STORE_LOCK
            .lock()
            .map_err(|_| "Credential store unavailable")?;
        if read_auth(&path)? != original {
            return Err("Codex auth.json changed during refresh; retry".into());
        }
        let mut blob = original;
        apply_auth_refresh(&mut blob, &response);
        atomic_write(&path, &blob)?;
        auth_projection(&blob)
    })
    .await
    .map_err(|_| "Codex refresh worker failed".to_string())?
}

fn apply_auth_refresh(blob: &mut Value, response: &Value) {
    for key in ["access_token", "refresh_token", "id_token"] {
        if let Some(value) = token_field(response, key) {
            blob["tokens"][key] = json!(value);
        }
    }
    blob["last_refresh"] = json!(rfc3339_now());
}

fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86_400;
    let secs_of_day = secs % 86_400;
    // Gregorian date from days-since-epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
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
        assert!(read_pool(&path).unwrap().accounts.is_empty());
        let pool = Pool {
            version: 1,
            accounts: vec![account()],
        };
        atomic_write(&path, &pool).unwrap();
        assert_eq!(read_pool(&path).unwrap().accounts[0].id, "test");
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
        assert!(read_pool(&path).unwrap().accounts.is_empty());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::write(&path, "broken json").unwrap();
        assert!(read_pool(&path).is_err());
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
        apply_auth_refresh(&mut blob, &json!({"access_token":"fake-new"}));
        assert_eq!(blob["custom"], 42);
        assert_eq!(blob["tokens"]["custom"], true);
        assert_eq!(blob["tokens"]["refresh_token"], "fake-refresh");
        assert_eq!(blob["tokens"]["account_id"], "workspace");
    }

    #[test]
    fn refresh_accepts_missing_expires_in_and_derives_jwt_expiry() {
        use base64::Engine;
        let mut account = account();
        // Rotated grant must survive even without expires_in.
        apply_refresh(
            &mut account,
            &json!({"access_token":"opaque-new", "refresh_token":"rotated"}),
        );
        assert_eq!(account.access_token, "opaque-new");
        assert_eq!(account.refresh_token, "rotated");
        assert_eq!(account.expires_at_ms, account.expires_at_ms);
        // JWT exp provides expiry when the response omits expires_in.
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"exp":4102444800}"#);
        let jwt = format!("{header}.{payload}.sig");
        let mut account2 = self::account();
        apply_refresh(&mut account2, &json!({"access_token": jwt}));
        assert_eq!(account2.expires_at_ms, Some(4_102_444_800_000));
    }

    #[test]
    fn refresh_failure_classification() {
        let transport = refresh_token("").unwrap_err();
        assert!(transport.permanent);
        assert_eq!(transport.message, "Missing refresh token");
    }
}
