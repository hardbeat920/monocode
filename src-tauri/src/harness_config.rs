//! Read-only discovery for harness-owned runtime configuration.
//!
//! The UI needs to know how a CLI is configured without copying secrets into
//! webview state. This module deliberately returns only redacted metadata and
//! performs model discovery on the native side where the configured API key is
//! available.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::command;

use crate::dirs_home;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVariable {
    pub name: String,
    pub configured: bool,
    pub sensitive: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModel {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessRuntimeSnapshot {
    pub harness: String,
    pub binary_path: Option<String>,
    pub config_path: Option<String>,
    pub config_source: Option<String>,
    pub auth_mode: String,
    pub auth_status: String,
    pub provider_name: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub environment: Vec<RuntimeVariable>,
    pub models: Vec<RuntimeModel>,
    pub model_source: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct CodexConfig {
    path: Option<PathBuf>,
    provider_name: Option<String>,
    base_url: Option<String>,
    env_key: Option<String>,
    model: Option<String>,
    forced_login_method: Option<String>,
}

#[command]
pub async fn harness_runtime_inspect(
    harness: String,
    refresh_models: Option<bool>,
    override_config: Option<Value>,
) -> Result<HarnessRuntimeSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        inspect(&harness, refresh_models.unwrap_or(false), override_config)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn inspect(
    harness: &str,
    refresh_models: bool,
    override_config: Option<Value>,
) -> Result<HarnessRuntimeSnapshot, String> {
    let harness = harness.trim().to_ascii_lowercase();
    if harness.is_empty() {
        return Err("Harness name is required".into());
    }
    match harness.as_str() {
        "codex" => inspect_codex(refresh_models, override_config),
        _ => inspect_generic(&harness),
    }
}

fn inspect_codex(
    refresh_models: bool,
    override_config: Option<Value>,
) -> Result<HarnessRuntimeSnapshot, String> {
    let config = read_codex_config();
    let override_table = override_config.as_ref().and_then(Value::as_object);
    let override_base_url = override_table
        .and_then(|table| table.get("baseUrl"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let override_environment = override_table
        .and_then(|table| table.get("environment"))
        .and_then(Value::as_object);
    let binary_path = crate::harness::resolve_harness_binary_for_config("codex");
    let env_key = config
        .env_key
        .clone()
        .unwrap_or_else(|| "OPENAI_API_KEY".into());
    let key_present = override_environment
        .and_then(|values| values.get(&env_key))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .is_some()
        || env_value(&env_key).is_some();
    let has_auth_json = codex_home().is_some_and(|home| home.join("auth.json").is_file());
    let api_mode = config
        .forced_login_method
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("api"))
        || config.env_key.is_some()
        || config
            .base_url
            .as_deref()
            .is_some_and(|url| !is_official_openai_url(url));
    let auth_mode = if api_mode {
        "api"
    } else if has_auth_json {
        "oauth"
    } else {
        "unknown"
    };
    let auth_status = if api_mode {
        if key_present {
            "configured"
        } else {
            "missing-key"
        }
    } else if has_auth_json {
        "authenticated"
    } else {
        "not-configured"
    };
    let mut models = Vec::new();
    let mut model_source = None;
    let mut error = None;
    if refresh_models && api_mode {
        if let Some(base_url) = override_base_url.or(config.base_url.as_deref()) {
            match query_models_with_overrides(base_url, env_key.as_str(), override_environment) {
                Ok(found) => {
                    models = found;
                    model_source = Some("openai-compatible-api".into());
                }
                Err(cause) => error = Some(cause),
            }
        }
    }
    Ok(HarnessRuntimeSnapshot {
        harness: "codex".into(),
        binary_path,
        config_path: config.path.as_ref().map(path_string),
        config_source: config.path.as_ref().map(|_| "codex-config".into()),
        auth_mode: auth_mode.into(),
        auth_status: auth_status.into(),
        provider_name: config.provider_name,
        base_url: override_base_url.map(String::from).or(config.base_url),
        model: config.model,
        environment: vec![RuntimeVariable {
            name: env_key,
            configured: key_present,
            sensitive: true,
        }],
        models,
        model_source,
        error,
    })
}

fn inspect_generic(harness: &str) -> Result<HarnessRuntimeSnapshot, String> {
    let (config_path, variables): (Option<PathBuf>, &[&str]) = match harness {
        "claude" => (
            first_existing(&[".claude/settings.json", ".claude.json"]),
            &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ],
        ),
        "opencode" => (
            first_existing(&[
                ".config/opencode/opencode.json",
                ".config/opencode/opencode.jsonc",
            ]),
            &["OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENCODE_API_KEY"],
        ),
        "cursor" => (first_existing(&[".cursor/mcp.json"]), &["CURSOR_API_KEY"]),
        "grok" => (None, &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"]),
        "fx" => (
            None,
            &[
                "AI_GATEWAY_API_KEY",
                "FX_AI_GATEWAY_API_KEY",
                "VERCEL_OIDC_TOKEN",
            ],
        ),
        "pi" | "omp" | "hermes" | "antigravity" => (None, &[]),
        _ => return Err(format!("Unknown harness: {harness}")),
    };
    let environment = variables
        .iter()
        .map(|name| RuntimeVariable {
            name: (*name).into(),
            configured: env_value(name).is_some(),
            sensitive: true,
        })
        .collect();
    Ok(HarnessRuntimeSnapshot {
        harness: harness.into(),
        binary_path: crate::harness::resolve_harness_binary_for_config(harness),
        config_path: config_path.as_ref().map(path_string),
        config_source: config_path.as_ref().map(|_| "user-config".into()),
        auth_mode: "unknown".into(),
        auth_status: "unknown".into(),
        provider_name: None,
        base_url: None,
        model: None,
        environment,
        models: Vec::new(),
        model_source: None,
        error: None,
    })
}

fn read_codex_config() -> CodexConfig {
    let Some(path) = codex_home().map(|home| home.join("config.toml")) else {
        return CodexConfig::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return CodexConfig::default();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&raw) else {
        return CodexConfig {
            path: Some(path),
            ..CodexConfig::default()
        };
    };
    let profile_name = std::env::var("CODEX_PROFILE").unwrap_or_else(|_| "default".into());
    let root = value.as_table();
    let profile = root
        .and_then(|table| table.get("profiles"))
        .and_then(toml::Value::as_table)
        .and_then(|profiles| profiles.get(&profile_name))
        .and_then(toml::Value::as_table);
    let string = |table: Option<&toml::map::Map<String, toml::Value>>, key: &str| {
        table
            .and_then(|entry| entry.get(key))
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
    };
    let provider_id = string(profile, "model_provider").or_else(|| string(root, "model_provider"));
    let provider = provider_id.as_deref().and_then(|id| {
        root.and_then(|table| table.get("model_providers"))
            .and_then(toml::Value::as_table)
            .and_then(|providers| providers.get(id))
            .and_then(toml::Value::as_table)
    });
    CodexConfig {
        path: Some(path),
        provider_name: string(provider, "name").or(provider_id),
        base_url: string(provider, "base_url"),
        env_key: string(provider, "env_key"),
        model: string(profile, "model").or_else(|| string(root, "model")),
        // Codex accepts forced_login_method at the root, not inside profiles.
        forced_login_method: string(root, "forced_login_method"),
    }
}

fn query_models_with_overrides(
    base_url: &str,
    env_key: &str,
    override_environment: Option<&serde_json::Map<String, Value>>,
) -> Result<Vec<RuntimeModel>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let mut request = agent.get(&url).set("Accept", "application/json");
    if let Some(key) = override_environment
        .and_then(|values| values.get(env_key))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(String::from)
        .or_else(|| env_value(env_key))
    {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let response = request
        .call()
        .map_err(|error| format!("Model discovery failed: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("Could not read model discovery response: {error}"))?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("Invalid model discovery response: {error}"))?;
    let Some(rows) = value.get("data").and_then(Value::as_array) else {
        return Err("Model discovery response did not contain a data array".into());
    };
    let mut seen = HashMap::new();
    let mut models = rows
        .iter()
        .filter_map(|row| {
            let id = row.get("id")?.as_str()?.trim();
            if id.is_empty() || seen.insert(id.to_owned(), ()).is_some() {
                return None;
            }
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| row.get("display_name").and_then(Value::as_str))
                .unwrap_or(id)
                .trim();
            Some(RuntimeModel {
                id: id.into(),
                name: name.into(),
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_home().map(|home| PathBuf::from(home).join(".codex")))
}

fn first_existing(relative_paths: &[&str]) -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from)?;
    relative_paths
        .iter()
        .map(|relative| home.join(relative))
        .find(|path| path.is_file())
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| crate::harness::login_shell_env_value(name))
}

fn is_official_openai_url(url: &str) -> bool {
    url.trim_end_matches('/')
        .eq_ignore_ascii_case("https://api.openai.com/v1")
}

fn path_string(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_official_openai_endpoint() {
        assert!(is_official_openai_url("https://api.openai.com/v1/"));
        assert!(!is_official_openai_url("https://example.test/v1"));
    }
}
