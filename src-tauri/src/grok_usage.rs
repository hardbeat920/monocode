use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::dirs_home;

const BILLING_CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const USER_AGENT: &str = "MonoCode";
const TOKEN_AUTH: &str = "xai-grok-cli";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

/// Fetch Grok credit usage via the signed-in Grok CLI session.
/// The access token never leaves the host process.
#[tauri::command]
pub async fn fetch_grok_usage() -> Result<GrokUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_grok_usage_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_grok_usage_sync() -> Result<GrokUsageFetch, String> {
    let Some(token) = read_grok_access_token() else {
        return Ok(usage_result(
            "unavailable",
            None,
            None,
            Some("Grok not signed in".into()),
        ));
    };
    Ok(fetch_usage_with_token(&token))
}

fn usage_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<String>,
) -> GrokUsageFetch {
    GrokUsageFetch {
        status: status.into(),
        http_status,
        body,
        error,
    }
}

fn fetch_usage_with_token(token: &str) -> GrokUsageFetch {
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let result = agent
        .get(BILLING_CREDITS_URL)
        .set("Accept", "application/json")
        .set("Authorization", &format!("Bearer {token}"))
        .set("x-xai-token-auth", TOKEN_AUTH)
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
            Some(format!("Grok usage request failed: {error}")),
        ),
    }
}

fn usage_error(status: u16) -> GrokUsageFetch {
    let (kind, message) = if status == 401 || status == 403 {
        ("unavailable", "Grok not signed in".into())
    } else {
        ("error", format!("Grok usage request failed ({status})"))
    };
    usage_result(kind, Some(status), None, Some(message))
}

fn read_grok_access_token() -> Option<String> {
    let path = grok_auth_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    extract_grok_access_token(&raw)
}

fn grok_auth_path() -> Option<PathBuf> {
    Some(Path::new(&dirs_home()?).join(".grok/auth.json"))
}

pub(crate) fn extract_grok_access_token(raw: &str) -> Option<String> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let object = value.as_object()?;
    let mut best: Option<(String, String)> = None;
    for entry in object.values() {
        let Some(key) = entry.get("key").and_then(Value::as_str) else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let expires = entry
            .get("expires_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let replace = match &best {
            Some((current, _)) => expires > *current,
            None => true,
        };
        if replace {
            best = Some((expires, key.to_string()));
        }
    }
    best.map(|(_, key)| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_grok_access_token_picks_the_newest_entry() {
        let raw = r#"{
          "https://auth.x.ai::old": {
            "key": "old-token",
            "expires_at": "2026-01-01T00:00:00Z"
          },
          "https://auth.x.ai::new": {
            "key": "new-token",
            "expires_at": "2026-09-09T01:42:07Z"
          }
        }"#;
        assert_eq!(extract_grok_access_token(raw).as_deref(), Some("new-token"));
    }

    #[test]
    fn extract_grok_access_token_skips_empty_keys() {
        let raw = r#"{"https://auth.x.ai::a":{"key":"  ","expires_at":"2099-01-01T00:00:00Z"}}"#;
        assert_eq!(extract_grok_access_token(raw), None);
    }

    #[test]
    fn extract_grok_access_token_rejects_garbage() {
        assert_eq!(extract_grok_access_token("not json"), None);
    }
}
