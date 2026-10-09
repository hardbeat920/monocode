// OAuth and project discovery adapted from cortexkit/antigravity-auth (MIT).
use super::storage::Store;
use super::transport::{
    checked, Cancel, NativeError, Result, Transport, CLIENT_ID, CLIENT_SECRET, REDIRECT, SCOPES,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub email: String,
    pub project: Option<String>,
    pub project_checked_at: u64,
}

#[derive(Default)]
struct AuthState {
    loaded: bool,
    account: Option<Account>,
}

#[derive(Default)]
pub struct Auth {
    state: Mutex<AuthState>,
}

impl Auth {
    async fn loaded(&self, store: &Store) -> Result<tokio::sync::MutexGuard<'_, AuthState>> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            state.account = store.read("account.dpapi")?;
            state.loaded = true;
        }
        Ok(state)
    }
    pub async fn summary(&self, store: &Store) -> Result<Option<String>> {
        Ok(self
            .loaded(store)
            .await?
            .account
            .as_ref()
            .map(|account| account.email.clone()))
    }
    pub async fn save(&self, store: &Store, account: Account, cancel: &Cancel) -> Result<()> {
        let mut state = cancel
            .run(Duration::from_secs(180), async {
                Ok(self.state.lock().await)
            })
            .await?;
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        store.write("account.dpapi", &account)?;
        state.account = Some(account);
        state.loaded = true;
        Ok(())
    }
    pub async fn logout(&self, store: &Store) -> Result<()> {
        let mut state = self.state.lock().await;
        store.remove("account.dpapi")?;
        state.account = None;
        state.loaded = true;
        Ok(())
    }
    pub async fn account(
        &self,
        store: &Store,
        transport: &Transport,
        cancel: &Cancel,
    ) -> Result<Account> {
        // Keep the mutex through refresh and project discovery. All callers share
        // one refresh; logout cannot race a late token write back to disk.
        let mut state = cancel
            .run(Duration::from_secs(180), self.loaded(store))
            .await?;
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        let account = state.account.as_mut().ok_or_else(NativeError::auth)?;
        if account.expires_at <= now_ms() + 60_000 {
            let payload = transport
                .form(
                    &[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", &account.refresh_token),
                        ("client_id", CLIENT_ID),
                        ("client_secret", CLIENT_SECRET),
                    ],
                    cancel,
                )
                .await;
            let payload = match payload {
                Ok(payload) => payload,
                Err(error) if error.code == "auth" => {
                    store.remove("account.dpapi")?;
                    state.account = None;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            apply_tokens(account, &payload, false)?;
            if cancel.is_cancelled() {
                return Err(NativeError::cancelled());
            }
            store.write("account.dpapi", account)?;
        }
        if account.project.is_none()
            || now_ms().saturating_sub(account.project_checked_at) > 30 * 60_000
        {
            account.project =
                Some(discover_project(transport, &account.access_token, cancel).await?);
            account.project_checked_at = now_ms();
            if cancel.is_cancelled() {
                return Err(NativeError::cancelled());
            }
            store.write("account.dpapi", account)?;
        }
        Ok(account.clone())
    }
}

pub fn apply_tokens(account: &mut Account, payload: &Value, require_refresh: bool) -> Result<()> {
    let access = payload["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(NativeError::auth)?;
    let expires = payload["expires_in"]
        .as_u64()
        .filter(|&s| s > 0 && s <= 86400)
        .ok_or_else(NativeError::auth)?;
    let refresh = payload["refresh_token"].as_str().filter(|s| !s.is_empty());
    if require_refresh && refresh.is_none() {
        return Err(NativeError::auth());
    }
    account.access_token = access.into();
    if let Some(refresh) = refresh {
        account.refresh_token = refresh.into();
    }
    account.expires_at = now_ms() + expires * 1000;
    Ok(())
}

pub struct Pkce {
    pub verifier: String,
    pub state: String,
    pub url: String,
}

impl Pkce {
    pub fn new() -> Self {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let state = uuid::Uuid::new_v4().to_string();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url =
            url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("OAuth URL");
        url.query_pairs_mut().extend_pairs([
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("scope", SCOPES),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ]);
        Self {
            verifier,
            state,
            url: url.into(),
        }
    }
    pub fn callback(&self, target: &str) -> Result<String> {
        let url = url::Url::parse(&format!("http://localhost:51121{target}"))
            .map_err(|_| NativeError::new("oauth", "Invalid OAuth callback."))?;
        if url.path() != "/oauth-callback" {
            return Err(NativeError::new("oauth", "Invalid OAuth callback path."));
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        let values = |key: &str| {
            pairs
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_ref())
                .collect::<Vec<_>>()
        };
        if values("state") != vec![self.state.as_str()] {
            return Err(NativeError::new(
                "oauth",
                "Google sign-in state did not match.",
            ));
        }
        if !values("error").is_empty() {
            return Err(NativeError::new(
                "oauth_declined",
                "Google sign-in was declined. Please try connecting again.",
            ));
        }
        let codes = values("code");
        if codes.len() != 1 || codes[0].is_empty() {
            return Err(NativeError::new(
                "oauth",
                "Google sign-in returned no authorization code.",
            ));
        }
        Ok(codes[0].into())
    }
}

pub async fn login(
    transport: &Transport,
    cancel: &Cancel,
    open_browser: impl FnOnce(&str) -> Result<()>,
) -> Result<Account> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 51121)).await
        .map_err(|_| NativeError::new("oauth", "Google sign-in could not open localhost port 51121. Close the other sign-in flow and retry."))?;
    let pkce = Pkce::new();
    if cancel.is_cancelled() {
        return Err(NativeError::cancelled());
    }
    open_browser(&pkce.url)?;
    let code = wait_callback(listener, &pkce, cancel, Duration::from_secs(10 * 60)).await?;
    let payload = transport
        .form(
            &[
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("code", &code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", REDIRECT),
                ("code_verifier", &pkce.verifier),
            ],
            cancel,
        )
        .await?;
    let mut account = Account {
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: 0,
        email: String::new(),
        project: None,
        project_checked_at: 0,
    };
    apply_tokens(&mut account, &payload, true)?;
    let userinfo: Value = cancel
        .run(Duration::from_secs(30), async {
            checked(
                transport
                    .client
                    .get(&transport.userinfo_url)
                    .bearer_auth(&account.access_token)
                    .send()
                    .await,
            )
            .await?
            .json()
            .await
            .map_err(|_| NativeError::new("protocol", "Google account identity could not be read."))
        })
        .await?;
    account.email = userinfo["email"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(NativeError::auth)?
        .into();
    Ok(account)
}

pub(super) async fn wait_callback(
    listener: TcpListener,
    pkce: &Pkce,
    cancel: &Cancel,
    timeout: Duration,
) -> Result<String> {
    // Own the listener in this future so every terminal path releases the port.
    cancel.run(timeout, async {
        loop {
            let (mut socket, peer) = listener.accept().await.map_err(|_| NativeError::new("oauth", "Google callback listener failed."))?;
            if !peer.ip().is_loopback() { continue; }
            let request = cancel.run(Duration::from_secs(5), async {
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0u8; 1024];
                    let n = socket.read(&mut buffer).await.map_err(|_| NativeError::new("oauth", "Invalid Google callback."))?;
                    if n == 0 { break; }
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > 16 * 1024 { return Err(NativeError::new("oauth", "Invalid Google callback.")); }
                    if bytes.windows(4).any(|b| b == b"\r\n\r\n") { break; }
                }
                String::from_utf8(bytes).map_err(|_| NativeError::new("oauth", "Invalid Google callback."))
            }).await;
            let callback = request.ok().and_then(|request| {
                let mut parts = request.lines().next()?.split_whitespace();
                if parts.next()? != "GET" { return None; }
                Some(pkce.callback(parts.next()?))
            });
            let success = matches!(&callback, Some(Ok(_)));
            let text = if success { "Signed in. Return to MonoCode." } else { "Sign-in callback rejected. Return to the Google sign-in window." };
            let response = format!("HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", if success { "200 OK" } else { "400 Bad Request" }, text.len());
            let _ = cancel.run(Duration::from_secs(5), async {
                socket.write_all(response.as_bytes()).await.map_err(|_| NativeError::new("oauth", "Google callback response failed."))
            }).await;
            match callback {
                Some(Ok(code)) => return Ok(code),
                Some(Err(error)) if error.code == "oauth_declined" => return Err(error),
                _ => {},
            }
        }
    }).await
}

pub async fn discover_project(
    transport: &Transport,
    token: &str,
    cancel: &Cancel,
) -> Result<String> {
    let payload = transport
        .json(
            "loadCodeAssist",
            token,
            &json!({"metadata":{"ideType":"ANTIGRAVITY"}}),
            cancel,
            false,
        )
        .await?;
    if let Some(project) = project_id(&payload["cloudaicompanionProject"]) {
        return Ok(project);
    }
    let tiers = payload["allowedTiers"].as_array();
    let tier = tiers
        .and_then(|ts| {
            ts.iter()
                .find(|t| t["isDefault"] == true)
                .or_else(|| ts.first())
        })
        .and_then(|t| t["id"].as_str())
        .unwrap_or("free-tier");
    for _ in 0..10 {
        let onboard = transport
            .json("onboardUser", token, &json!({"tierId":tier}), cancel, true)
            .await?;
        if onboard["done"] == true {
            return project_id(&onboard["response"]["cloudaicompanionProject"]).ok_or_else(|| {
                NativeError::new(
                    "ineligible",
                    "Antigravity did not provision a project for this Google account.",
                )
            });
        }
        cancel
            .run(Duration::from_secs(6), async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(())
            })
            .await?;
    }
    Err(NativeError::new(
        "ineligible",
        "Antigravity project provisioning did not complete. Try refreshing models later.",
    ))
}

fn project_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value["id"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}
