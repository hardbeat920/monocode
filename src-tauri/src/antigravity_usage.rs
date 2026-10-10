//! Antigravity usage: per-model quota pools from Cloud Code's
//! `fetchAvailableModels`, the same call `agy` makes.
//!
//! `agy` refreshes its Google access token in memory and never writes it
//! back, so the token on disk is usually past its one-hour expiry. MonoCode
//! refreshes it here, in memory only, with `agy`'s own installed-app client.
//! Google refresh tokens do not rotate, so this cannot spend a token `agy`
//! still needs, and the credential file is never modified.
//!
//! The client is not hardcoded: its id is the `aud` claim of the stored
//! `id_token`, and its secret is read from the installed `agy` binary on
//! first use. Google rejects a wrong secret with 401 `invalid_client`, so
//! each candidate found there can be tried until one is accepted.

use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};

use crate::dirs_home;
use crate::session_store::now_millis;

const TOKEN_FILES: [&str; 2] = [
    ".gemini/jetski-standalone-oauth-token",
    ".gemini/antigravity-cli/antigravity-oauth-token",
];
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Google installed-app client secrets: this prefix, then 28 URL-safe chars.
const SECRET_PREFIX: &[u8] = b"GOCSPX-";
const SECRET_LEN: usize = 28;
const SCAN_CHUNK: usize = 4 << 20;
const LOAD_CODE_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const FETCH_MODELS_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const USER_AGENT: &str = "antigravity";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// Refresh this long before expiry so a token cannot lapse mid-request.
const EXPIRY_SKEW_MS: i64 = 60_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub body: Option<String>,
    pub error: Option<String>,
}

struct StoredToken {
    access_token: String,
    refresh_token: String,
    expires_at_ms: Option<i64>,
    /// `aud` of the stored `id_token`: the OAuth client `agy` signed in with.
    client_id: Option<String>,
}

struct OAuthClient {
    id: String,
    secret: String,
}

struct State {
    session: Option<Session>,
    /// Outlives sessions: a new sign-in still uses the same `agy` client.
    client: Option<OAuthClient>,
}

/// Session state reused across polls until the user signs in again.
struct Session {
    refresh_token: String,
    access_token: String,
    expires_at_ms: i64,
    project: Option<String>,
}

enum Failure {
    Status(u16),
    Transport(String),
    /// No secret in the installed `agy` is accepted for its client id.
    NoClient,
}

static STATE: Mutex<State> = Mutex::new(State {
    session: None,
    client: None,
});

#[tauri::command]
pub async fn fetch_antigravity_usage() -> Result<AntigravityUsageFetch, String> {
    tauri::async_runtime::spawn_blocking(fetch_antigravity_usage_sync)
        .await
        .map_err(|e| e.to_string())
}

fn fetch_result(
    status: &str,
    http_status: Option<u16>,
    body: Option<String>,
    error: Option<&str>,
) -> AntigravityUsageFetch {
    AntigravityUsageFetch {
        status: status.into(),
        http_status,
        body,
        error: error.map(Into::into),
    }
}

fn fetch_antigravity_usage_sync() -> AntigravityUsageFetch {
    let Some(stored) = read_stored_token() else {
        return fetch_result("unavailable", None, None, Some("Antigravity not signed in"));
    };
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    // Holding the lock serializes overlapping polls onto one refresh.
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let State { session, client } = &mut *guard;
    // A different refresh token means `agy` signed in again, maybe as
    // someone else: drop the cached token and project.
    if session
        .as_ref()
        .is_some_and(|session| session.refresh_token != stored.refresh_token)
    {
        *session = None;
    }
    let session = session.get_or_insert_with(|| Session {
        refresh_token: stored.refresh_token.clone(),
        access_token: stored.access_token.clone(),
        // No access token on disk: refresh before the first request.
        expires_at_ms: if stored.access_token.is_empty() {
            0
        } else {
            stored.expires_at_ms.unwrap_or(0)
        },
        project: None,
    });

    // A token can be revoked before its expiry, so one 401 earns one
    // refresh and retry within this call.
    let mut refreshed = false;
    loop {
        if session.expires_at_ms <= now_millis() + EXPIRY_SKEW_MS {
            match refresh_access_token(
                &agent,
                client,
                stored.client_id.as_deref(),
                &session.refresh_token,
            ) {
                Ok((token, expires_at_ms)) => {
                    session.access_token = token;
                    session.expires_at_ms = expires_at_ms;
                    refreshed = true;
                }
                // invalid_grant: the refresh token itself is not accepted.
                Err(Failure::Status(status @ 400)) => {
                    return fetch_result(
                        "error",
                        Some(status),
                        None,
                        Some("Antigravity sign-in expired. Run `agy` once in Terminal to sign in."),
                    );
                }
                Err(failure) => return failure_result(failure),
            }
        }
        match fetch_models(&agent, session) {
            Ok(Some(body)) => return fetch_result("ok", Some(200), Some(body), None),
            Ok(None) => {
                return fetch_result(
                    "unavailable",
                    None,
                    None,
                    Some("Antigravity has no Code Assist project for this account"),
                );
            }
            Err(Failure::Status(401)) if !refreshed => session.expires_at_ms = 0,
            Err(failure) => {
                if matches!(failure, Failure::Status(401)) {
                    session.expires_at_ms = 0;
                }
                return failure_result(failure);
            }
        }
    }
}

/// `None` when the account has no Code Assist project to read quotas from.
fn fetch_models(agent: &ureq::Agent, session: &mut Session) -> Result<Option<String>, Failure> {
    let project = match session.project.clone() {
        Some(project) => project,
        None => match load_project(agent, &session.access_token)? {
            Some(project) => session.project.insert(project).clone(),
            None => return Ok(None),
        },
    };
    post_json(
        agent,
        FETCH_MODELS_URL,
        &session.access_token,
        &json!({ "project": project }),
    )
    .map(Some)
}

fn failure_result(failure: Failure) -> AntigravityUsageFetch {
    match failure {
        Failure::Status(401) => fetch_result(
            "error",
            Some(401),
            None,
            Some("Antigravity sign-in expired"),
        ),
        Failure::Status(403) => fetch_result(
            "error",
            Some(403),
            None,
            Some("Antigravity usage is unavailable for this account"),
        ),
        Failure::Status(status) => fetch_result(
            "error",
            Some(status),
            None,
            Some(&format!("Antigravity usage request failed ({status})")),
        ),
        Failure::Transport(error) => fetch_result(
            "error",
            None,
            None,
            Some(&format!("Antigravity usage request failed: {error}")),
        ),
        Failure::NoClient => fetch_result(
            "error",
            None,
            None,
            Some(
                "Couldn’t renew the Antigravity sign-in. Update agy, then run it once in Terminal.",
            ),
        ),
    }
}

/// Refresh with the cached client, or find `agy`'s secret and remember it.
fn refresh_access_token(
    agent: &ureq::Agent,
    client: &mut Option<OAuthClient>,
    client_id: Option<&str>,
    refresh_token: &str,
) -> Result<(String, i64), Failure> {
    let client_id = client_id.ok_or(Failure::NoClient)?;
    if let Some(known) = client.as_ref().filter(|known| known.id == client_id) {
        match request_token(agent, &known.id, &known.secret, refresh_token) {
            // An updated `agy` may ship a new secret: search again.
            Err(Failure::Status(401)) => *client = None,
            result => return result,
        }
    }
    for path in agy_binaries() {
        for secret in client_secrets_in_file(&path) {
            match request_token(agent, client_id, &secret, refresh_token) {
                Err(Failure::Status(401)) => continue,
                Ok(token) => {
                    *client = Some(OAuthClient {
                        id: client_id.to_string(),
                        secret,
                    });
                    return Ok(token);
                }
                Err(failure) => return Err(failure),
            }
        }
    }
    Err(Failure::NoClient)
}

fn request_token(
    agent: &ureq::Agent,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<(String, i64), Failure> {
    let response = agent
        .post(TOKEN_URL)
        .send_form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .map_err(failure_from)?;
    let body = response
        .into_string()
        .map_err(|error| Failure::Transport(error.to_string()))?;
    let value: Value =
        serde_json::from_str(&body).map_err(|error| Failure::Transport(error.to_string()))?;
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Failure::Transport("token response had no access token".into()))?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    Ok((token.to_string(), now_millis() + expires_in * 1000))
}

/// The `agy` CLI first (smaller), then the ACP server MonoCode launches.
fn agy_binaries() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs_home() {
        paths.push(PathBuf::from(home).join(".local/bin/agy"));
    }
    paths.extend(crate::harness::which_via_login_shell("agy"));
    paths.extend(crate::harness::resolve_antigravity());
    let mut seen = Vec::new();
    paths.retain(|path| {
        let Ok(real) = std::fs::canonicalize(path) else {
            return false;
        };
        let fresh = !seen.contains(&real);
        seen.push(real);
        fresh
    });
    paths
}

/// Streams the file so an 800 MB ACP archive is never held in memory.
fn client_secrets_in_file(path: &std::path::Path) -> Vec<String> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let overlap = SECRET_PREFIX.len() + SECRET_LEN - 1;
    let mut found = Vec::new();
    let mut buffer = Vec::with_capacity(SCAN_CHUNK + overlap);
    let mut chunk = vec![0; SCAN_CHUNK];
    loop {
        let read = match file.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        buffer.extend_from_slice(&chunk[..read]);
        client_secrets_in(&buffer, &mut found);
        // Keep a tail too short to hold a whole secret, so none is lost at
        // a chunk boundary and none is found twice.
        let keep = buffer.len().min(overlap);
        buffer.drain(..buffer.len() - keep);
    }
    found
}

fn client_secrets_in(bytes: &[u8], found: &mut Vec<String>) {
    let total = SECRET_PREFIX.len() + SECRET_LEN;
    let mut start = 0;
    while let Some(offset) = bytes[start..].iter().position(|&b| b == SECRET_PREFIX[0]) {
        let at = start + offset;
        if at + total > bytes.len() {
            break;
        }
        let body = &bytes[at + SECRET_PREFIX.len()..at + total];
        if bytes[at..].starts_with(SECRET_PREFIX)
            && body
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            let secret = String::from_utf8_lossy(&bytes[at..at + total]).into_owned();
            if !found.contains(&secret) {
                found.push(secret);
            }
        }
        start = at + 1;
    }
}

fn load_project(agent: &ureq::Agent, token: &str) -> Result<Option<String>, Failure> {
    let body = post_json(
        agent,
        LOAD_CODE_ASSIST_URL,
        token,
        &json!({ "metadata": { "ideType": "ANTIGRAVITY" } }),
    )?;
    let value: Value =
        serde_json::from_str(&body).map_err(|error| Failure::Transport(error.to_string()))?;
    Ok(project_from_load_response(&value))
}

fn project_from_load_response(value: &Value) -> Option<String> {
    let project = value.get("cloudaicompanionProject")?;
    project
        .as_str()
        .or_else(|| project.get("id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|project| !project.is_empty())
        .map(Into::into)
}

fn post_json(agent: &ureq::Agent, url: &str, token: &str, body: &Value) -> Result<String, Failure> {
    let response = agent
        .post(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", USER_AGENT)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(failure_from)?;
    response
        .into_string()
        .map_err(|error| Failure::Transport(error.to_string()))
}

fn failure_from(error: ureq::Error) -> Failure {
    match error {
        ureq::Error::Status(status, response) => {
            let _ = response.into_string();
            Failure::Status(status)
        }
        ureq::Error::Transport(transport) => Failure::Transport(transport.to_string()),
    }
}

/// The most recently written credential file wins: `agy` has used both.
fn read_stored_token() -> Option<StoredToken> {
    let home = PathBuf::from(dirs_home()?);
    TOKEN_FILES
        .iter()
        .map(|file| home.join(file))
        .filter_map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()?;
            let token = token_from_blob(&std::fs::read_to_string(&path).ok()?)?;
            Some((modified, token))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, token)| token)
}

fn token_from_blob(raw: &str) -> Option<StoredToken> {
    let value: Value = serde_json::from_str(raw.trim()).ok()?;
    let token = value.get("token")?;
    let field = |key: &str| {
        token
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    Some(StoredToken {
        access_token: field("access_token").unwrap_or_default().to_string(),
        refresh_token: field("refresh_token")?.to_string(),
        expires_at_ms: field("expiry").and_then(parse_expiry_ms),
        client_id: value
            .get("id_token")
            .and_then(Value::as_str)
            .and_then(audience),
    })
}

/// The `aud` claim of an unverified JWT; it only picks which client to use.
fn audience(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let aud = claims.get("aud")?;
    aud.as_str()
        .or_else(|| aud.get(0).and_then(Value::as_str))
        .filter(|aud| !aud.is_empty())
        .map(Into::into)
}

fn parse_expiry_ms(text: &str) -> Option<i64> {
    let date =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(date.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_agy_token_file() {
        let token = token_from_blob(
            r#"{"token":{"access_token":"ya29.a","token_type":"Bearer","refresh_token":"1//r","expiry":"2026-10-09T17:30:48.805695+07:00"},"auth_method":"consumer","id_token":"x"}"#,
        )
        .unwrap();
        assert_eq!(token.access_token, "ya29.a");
        assert_eq!(token.refresh_token, "1//r");
        assert_eq!(token.expires_at_ms, Some(1_791_541_848_805));
        assert_eq!(token.client_id, None);
    }

    #[test]
    fn client_id_comes_from_the_id_token_audience() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"aud":"123-abc.apps.googleusercontent.com","iss":"x"}"#);
        let raw = json!({
            "token": { "refresh_token": "1//r" },
            "id_token": format!("header.{payload}.signature"),
        })
        .to_string();
        assert_eq!(
            token_from_blob(&raw).unwrap().client_id.as_deref(),
            Some("123-abc.apps.googleusercontent.com")
        );
    }

    fn fake_secret(fill: u8) -> Vec<u8> {
        [SECRET_PREFIX, &[fill; SECRET_LEN]].concat()
    }

    #[test]
    fn finds_adjacent_secrets_once_each() {
        let a = fake_secret(b'a');
        let b = fake_secret(b'b');
        let bytes = [b"\0junk".as_slice(), &a, &b, b"\0", &a, b"GOCSPX-short\0"].concat();
        let mut found = Vec::new();
        client_secrets_in(&bytes, &mut found);
        assert_eq!(
            found,
            vec![String::from_utf8(a).unwrap(), String::from_utf8(b).unwrap()]
        );
    }

    #[test]
    fn finds_a_secret_split_across_chunks() {
        let secret = fake_secret(b'z');
        let mut bytes = vec![0; SCAN_CHUNK - 10];
        bytes.extend_from_slice(&secret);
        bytes.extend_from_slice(&[0; 64]);
        let path = std::env::temp_dir().join(format!("monocode-agy-scan-{}", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let found = client_secrets_in_file(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(found, vec![String::from_utf8(secret).unwrap()]);
    }

    #[test]
    fn token_without_refresh_token_is_unusable() {
        assert!(token_from_blob(r#"{"token":{"access_token":"ya29.a"}}"#).is_none());
        assert!(token_from_blob("not json").is_none());
    }

    #[test]
    fn unparseable_expiry_forces_refresh() {
        let token =
            token_from_blob(r#"{"token":{"refresh_token":"1//r","expiry":"soon"}}"#).unwrap();
        assert_eq!(token.expires_at_ms, None);
        assert_eq!(token.access_token, "");
    }

    #[test]
    fn project_accepts_string_or_object() {
        assert_eq!(
            project_from_load_response(&json!({ "cloudaicompanionProject": "aicode-consumers" }))
                .as_deref(),
            Some("aicode-consumers")
        );
        assert_eq!(
            project_from_load_response(&json!({ "cloudaicompanionProject": { "id": "p-1" } }))
                .as_deref(),
            Some("p-1")
        );
        assert_eq!(
            project_from_load_response(&json!({ "currentTier": {} })),
            None
        );
    }
}
