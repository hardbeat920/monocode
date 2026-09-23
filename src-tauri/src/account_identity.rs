use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use crate::dirs_home;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountIdentity {
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: Option<String>,
    pub organization: Option<String>,
}

/// Read the signed-in identity a provider CLI already cached on disk, so
/// no token is sent anywhere. Returns `None` when the profile is not signed in.
#[tauri::command]
pub async fn provider_account_identity(
    app: AppHandle,
    provider: String,
    account_id: Option<String>,
) -> Result<Option<ProviderAccountIdentity>, String> {
    let dir = crate::harness::provider_account_dir(&app, &provider, account_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || match provider.as_str() {
        "claude" => Ok(claude_identity(dir)),
        "codex" => Ok(codex_identity(dir)),
        _ => Err("Account identity is not supported for this provider".into()),
    })
    .await
    .map_err(|e| e.to_string())?
}

fn home() -> Option<PathBuf> {
    dirs_home().map(PathBuf::from)
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn claude_identity(dir: Option<PathBuf>) -> Option<ProviderAccountIdentity> {
    let path = match dir {
        Some(dir) => dir.join(".claude.json"),
        None => home()?.join(".claude.json"),
    };
    let account = read_json(&path)?.get("oauthAccount")?.clone();
    // organizationType is e.g. "claude_max", "claude_pro", "claude_team".
    let plan = text(&account, "organizationType")
        .map(|kind| capitalize(kind.strip_prefix("claude_").unwrap_or(&kind)));
    Some(ProviderAccountIdentity {
        email: text(&account, "emailAddress"),
        name: text(&account, "displayName").or_else(|| text(&account, "fullName")),
        plan,
        organization: text(&account, "organizationName"),
    })
}

fn codex_identity(dir: Option<PathBuf>) -> Option<ProviderAccountIdentity> {
    let dir = match dir {
        Some(dir) => dir,
        None => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".codex")))?,
    };
    let auth = read_json(&dir.join("auth.json"))?;
    let id_token = auth.get("tokens")?.get("id_token")?.as_str()?;
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let openai = claims.get("https://api.openai.com/auth");
    let organization = openai
        .and_then(|auth| auth.get("organizations"))
        .and_then(Value::as_array)
        .and_then(|orgs| {
            orgs.iter()
                .find(|org| org.get("is_default").and_then(Value::as_bool) == Some(true))
        })
        .and_then(|org| text(org, "title"));
    Some(ProviderAccountIdentity {
        email: text(&claims, "email"),
        name: text(&claims, "name"),
        plan: openai
            .and_then(|auth| text(auth, "chatgpt_plan_type"))
            .map(|plan| capitalize(&plan)),
        organization,
    })
}
