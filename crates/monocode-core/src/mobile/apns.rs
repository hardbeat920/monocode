//! Live Activity pushes straight to Apple, signed with the user's own key.
//! Unlike the silent pushes that ask the app to redraw, iOS applies these
//! itself, so the activity stays current with the app closed. Off until a key
//! is set in Settings.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Apple turns a signing token away once it is an hour old.
const TOKEN_LIFETIME: Duration = Duration::from_secs(50 * 60);

/// An APNs auth key from the Apple Developer account: the `.p8` file, its
/// key ID and the team that made it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApnsKey {
    pub team_id: String,
    pub key_id: String,
    pub key_path: PathBuf,
}

impl ApnsKey {
    fn validate(&self) -> Result<(), String> {
        let id =
            |value: &str| value.len() == 10 && value.chars().all(|c| c.is_ascii_alphanumeric());
        if !id(&self.team_id) {
            return Err("The team ID is 10 letters and digits".into());
        }
        if !id(&self.key_id) {
            return Err("The key ID is 10 letters and digits".into());
        }
        Ok(())
    }
}

pub(super) struct Client {
    http: reqwest::Client,
    key: ApnsKey,
    signer: EcdsaKeyPair,
    token: Mutex<Option<(String, Instant)>>,
}

pub(super) enum ApnsError {
    /// The activity ended or the app went away; forget the token.
    Unregistered,
    Other(String),
}

impl Client {
    pub(super) fn new(key: &ApnsKey) -> Result<Self, String> {
        key.validate()?;
        let pem = std::fs::read_to_string(&key.key_path)
            .map_err(|e| format!("Cannot read the key file: {e}"))?;
        let der: String = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(der.trim())
            .map_err(|_| "The key file isn't a .p8 key")?;
        let signer =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &SystemRandom::new())
                .map_err(|_| "The key file isn't a .p8 key")?;
        // HTTP/2 is negotiated over TLS, which is all APNs accepts.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            key: key.clone(),
            signer,
            token: Mutex::new(None),
        })
    }

    /// The provider token, reused for most of its hour as Apple asks.
    fn token(&self) -> Result<String, String> {
        let mut cached = self.token.lock().map_err(|_| "Push key unavailable")?;
        if let Some((token, made)) = cached.as_ref() {
            if made.elapsed() < TOKEN_LIFETIME {
                return Ok(token.clone());
            }
        }
        let token = jwt(&self.key, &self.signer, unix_now())?;
        *cached = Some((token.clone(), Instant::now()));
        Ok(token)
    }

    pub(super) async fn send(
        &self,
        target: &Delivery,
        payload: &Value,
        urgent: bool,
    ) -> Result<(), ApnsError> {
        let host = if target.sandbox {
            "api.sandbox.push.apple.com"
        } else {
            "api.push.apple.com"
        };
        let token = self.token().map_err(ApnsError::Other)?;
        let response = self
            .http
            .post(format!("https://{host}/3/device/{}", target.token))
            .bearer_auth(token)
            .header("apns-push-type", "liveactivity")
            .header(
                "apns-topic",
                format!("{}.push-type.liveactivity", target.bundle_id),
            )
            // Low priority is cheaper against the activity's update budget;
            // a session that needs the user can't wait.
            .header("apns-priority", if urgent { "10" } else { "5" })
            .header("content-type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| ApnsError::Other(e.to_string()))?;
        let status = response.status().as_u16();
        if status == 200 {
            return Ok(());
        }
        let reason = response
            .text()
            .await
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|body| body["reason"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {status}"));
        if matches!(
            reason.as_str(),
            "ExpiredProviderToken" | "InvalidProviderToken"
        ) {
            if let Ok(mut cached) = self.token.lock() {
                *cached = None;
            }
        }
        match (status, reason.as_str()) {
            (410, _) | (400, "BadDeviceToken") => Err(ApnsError::Unregistered),
            _ => Err(ApnsError::Other(reason)),
        }
    }
}

/// Where one push goes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Delivery {
    pub token: String,
    pub bundle_id: String,
    pub sandbox: bool,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// An ES256 provider token: the key ID in the header, the team and the time
/// it was made in the claims.
fn jwt(key: &ApnsKey, signer: &EcdsaKeyPair, issued: u64) -> Result<String, String> {
    let header = URL_SAFE_NO_PAD.encode(json!({"alg": "ES256", "kid": key.key_id}).to_string());
    let claims = URL_SAFE_NO_PAD.encode(json!({"iss": key.team_id, "iat": issued}).to_string());
    let input = format!("{header}.{claims}");
    let signature = signer
        .sign(&SystemRandom::new(), input.as_bytes())
        .map_err(|_| "Cannot sign with the push key")?;
    Ok(format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

/// How long a finished activity stays on the Lock Screen, as the app does.
const FINISHED_FOR: u64 = 15 * 60;
/// Past this without news the widget says so, as the app does.
const STALE_AFTER: u64 = 10 * 60;

/// Redraws a running activity.
pub(super) fn update(state: &Value, now: u64) -> Value {
    json!({"aps": {
        "timestamp": now,
        "event": "update",
        "content-state": state,
        "stale-date": now + STALE_AFTER,
    }})
}

/// Ends the activity: shown for a while once everything finished, or gone at
/// once when its sessions closed.
pub(super) fn end(state: &Value, finished: bool, now: u64) -> Value {
    json!({"aps": {
        "timestamp": now,
        "event": "end",
        "content-state": state,
        "dismissal-date": if finished { now + FINISHED_FOR } else { now },
    }})
}

/// Starts an activity with the app closed (iOS 17.2+). Apple wants an alert
/// with it; it lights the Lock Screen rather than showing a banner.
pub(super) fn start(desktop: &str, state: &Value, alert: (&str, &str), now: u64) -> Value {
    json!({"aps": {
        "timestamp": now,
        "event": "start",
        "content-state": state,
        "attributes-type": "MonocodeActivityAttributes",
        "attributes": {"desktop": desktop},
        "alert": {"title": alert.0, "body": alert.1},
        "stale-date": now + STALE_AFTER,
    }})
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::mobile) fn key_file(dir: &std::path::Path) -> (ApnsKey, Vec<u8>) {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .unwrap();
        let body = base64::engine::general_purpose::STANDARD.encode(pkcs8.as_ref());
        let path = dir.join("AuthKey_ABCDE12345.p8");
        std::fs::write(
            &path,
            format!("-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n"),
        )
        .unwrap();
        let key = ApnsKey {
            team_id: "TEAM123456".into(),
            key_id: "ABCDE12345".into(),
            key_path: path,
        };
        (key, pkcs8.as_ref().to_vec())
    }

    use ring::signature::{KeyPair, UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};

    #[test]
    fn signs_a_provider_token_apple_can_verify() {
        let dir = tempfile::tempdir().unwrap();
        let (key, pkcs8) = key_file(dir.path());
        let client = Client::new(&key).unwrap();
        let token = client.token().unwrap();
        // Reused rather than re-signed on every push.
        assert_eq!(client.token().unwrap(), token);
        let parts: Vec<&str> = token.split('.').collect();
        let decode = |part: &str| -> Value {
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
        };
        assert_eq!(
            decode(parts[0]),
            json!({"alg": "ES256", "kid": "ABCDE12345"})
        );
        assert_eq!(decode(parts[1])["iss"], "TEAM123456");
        let public = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &pkcs8,
            &SystemRandom::new(),
        )
        .unwrap()
        .public_key()
        .as_ref()
        .to_vec();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn a_wrong_id_or_file_says_which() {
        let dir = tempfile::tempdir().unwrap();
        let (key, _) = key_file(dir.path());
        let short = ApnsKey {
            team_id: "TEAM".into(),
            ..key.clone()
        };
        assert!(Client::new(&short).err().unwrap().contains("team ID"));
        let missing = ApnsKey {
            key_path: dir.path().join("none.p8"),
            ..key.clone()
        };
        assert!(Client::new(&missing).err().unwrap().contains("Cannot read"));
        std::fs::write(&key.key_path, "not a key").unwrap();
        assert!(Client::new(&key).err().unwrap().contains(".p8"));
    }

    #[test]
    fn finished_activities_linger_and_closed_ones_go_at_once() {
        let state = json!({"sessions": []});
        assert_eq!(
            end(&state, true, 100)["aps"]["dismissal-date"],
            100 + FINISHED_FOR
        );
        assert_eq!(end(&state, false, 100)["aps"]["dismissal-date"], 100);
        let started = start("Mac", &state, ("Fix push", "Working"), 100);
        assert_eq!(
            started["aps"]["attributes-type"],
            "MonocodeActivityAttributes"
        );
        assert_eq!(started["aps"]["attributes"]["desktop"], "Mac");
        assert_eq!(update(&state, 100)["aps"]["stale-date"], 100 + STALE_AFTER);
    }
}
