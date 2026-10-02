use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PiUsageProvider {
    Anthropic,
    OpenaiCodex,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageWindow {
    used_percent: f64,
    window_minutes: u64,
    resets_at: Option<u64>,
}
#[derive(Debug, Default, Serialize)]
pub struct UsageWindows {
    session: Option<UsageWindow>,
    weekly: Option<UsageWindow>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum PiUsageResult {
    Ok { windows: UsageWindows },
    Unavailable { message: String },
    Error { message: String },
}

#[derive(PartialEq)]
struct PiOAuth {
    access: String,
    expires: u64,
    account_id: Option<String>,
}

impl PiUsageProvider {
    fn key(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenaiCodex => "openai-codex",
        }
    }
    fn endpoint(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com/api/oauth/usage",
            Self::OpenaiCodex => "https://chatgpt.com/backend-api/wham/usage",
        }
    }
}

#[tauri::command]
pub async fn fetch_pi_usage(provider: PiUsageProvider) -> PiUsageResult {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(dir) = agent_dir(std::env::var_os("PI_CODING_AGENT_DIR"), crate::dirs_home())
        else {
            return unavailable(
                "Pi usage needs an absolute configuration directory. Check PI_CODING_AGENT_DIR.",
            );
        };
        read_usage(&dir, provider, |credential| {
            request_usage(provider, credential, provider.endpoint())
        })
    })
    .await
    .unwrap_or_else(|_| error("Could not read Pi usage. Try refreshing."))
}

fn agent_dir(override_dir: Option<std::ffi::OsString>, home: Option<String>) -> Option<PathBuf> {
    if let Some(value) = override_dir.filter(|v| !v.is_empty()) {
        let path = PathBuf::from(value);
        if let Ok(suffix) = path.strip_prefix("~") {
            return home.map(|h| PathBuf::from(h).join(suffix));
        }
        return path.is_absolute().then_some(path);
    }
    home.map(|h| PathBuf::from(h).join(".pi/agent"))
}

fn unavailable(message: &str) -> PiUsageResult {
    PiUsageResult::Unavailable {
        message: message.into(),
    }
}
fn error(message: &str) -> PiUsageResult {
    PiUsageResult::Error {
        message: message.into(),
    }
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn bounded_text(reader: impl Read) -> Result<String, ()> {
    let mut text = String::new();
    reader
        .take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| ())?;
    if text.len() as u64 > MAX_BYTES {
        return Err(());
    }
    Ok(text)
}
fn read_json(path: &Path) -> Result<Option<Value>, PiUsageResult> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(error(
                "Could not read Pi configuration. Check its file permissions.",
            ))
        }
    };
    let text =
        bounded_text(file).map_err(|_| error("Pi configuration is too large or unreadable."))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|_| error("Pi configuration contains invalid JSON."))
}
fn has_auth_override(value: &Value) -> bool {
    value.as_object().is_some_and(|obj| {
        ["apiKey", "baseUrl", "headers", "authHeader"]
            .iter()
            .any(|key| obj.contains_key(*key))
            || obj
                .get("models")
                .and_then(Value::as_array)
                .is_some_and(|models| models.iter().any(has_auth_override))
            || obj
                .get("modelOverrides")
                .and_then(Value::as_object)
                .is_some_and(|models| models.values().any(has_auth_override))
    })
}
fn credential(dir: &Path, provider: PiUsageProvider) -> Result<PiOAuth, PiUsageResult> {
    if let Some(config) = read_json(&dir.join("models.json"))? {
        if has_auth_override(&config["providers"][provider.key()]) {
            return Err(unavailable(
                "Usage is unavailable for custom Pi authentication or endpoints.",
            ));
        }
    }
    let value = read_json(&dir.join("auth.json"))?.unwrap_or(Value::Null);
    let entry = &value[provider.key()];
    if entry["type"].as_str() == Some("api_key") {
        return Err(unavailable(
            "Subscription usage is unavailable for Pi API keys.",
        ));
    }
    if entry["type"].as_str() != Some("oauth") {
        return Err(unavailable(
            "Sign in to this provider through Pi, then refresh usage.",
        ));
    }
    let access = entry["access"].as_str().filter(|v| valid_header(v));
    let expires = entry["expires"].as_u64();
    let (Some(access), Some(expires)) = (access, expires) else {
        return Err(error(
            "Pi credentials are invalid. Sign in through Pi again.",
        ));
    };
    if expires <= now_ms() {
        return Err(unavailable(
            "Pi sign-in expired. Resume or sign in through Pi, then refresh usage.",
        ));
    }
    let account_id = entry["accountId"]
        .as_str()
        .filter(|v| valid_header(v))
        .map(str::to_owned);
    if matches!(provider, PiUsageProvider::OpenaiCodex) && account_id.is_none() {
        return Err(unavailable(
            "Pi's Codex account could not be identified. Sign in through Pi again.",
        ));
    }
    Ok(PiOAuth {
        access: access.into(),
        expires,
        account_id,
    })
}
fn valid_header(value: &str) -> bool {
    !value.trim().is_empty() && value.bytes().all(|c| (0x21..=0x7e).contains(&c))
}
fn read_usage(
    dir: &Path,
    provider: PiUsageProvider,
    request: impl FnOnce(&PiOAuth) -> PiUsageResult,
) -> PiUsageResult {
    let credentials = match credential(dir, provider) {
        Ok(value) => value,
        Err(result) => return result,
    };
    let result = request(&credentials);
    // Pi owns token rotation. Never refresh or write its credentials here.
    if credential(dir, provider).ok().as_ref() != Some(&credentials) {
        return error("Pi credentials changed. Refresh usage to read the current account.");
    }
    result
}
fn request_usage(provider: PiUsageProvider, credentials: &PiOAuth, url: &str) -> PiUsageResult {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .redirects(0)
        .build();
    let mut request = agent
        .get(url)
        .set("Authorization", &format!("Bearer {}", credentials.access))
        .set("User-Agent", "monocode");
    match provider {
        PiUsageProvider::Anthropic => request = request.set("anthropic-beta", "oauth-2025-04-20"),
        PiUsageProvider::OpenaiCodex => {
            if let Some(id) = &credentials.account_id {
                request = request.set("ChatGPT-Account-Id", id);
            }
        }
    }
    let response = match request.call() {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(401, _)) => {
            return unavailable("Pi sign-in expired. Sign in through Pi, then refresh usage.")
        }
        Err(ureq::Error::Status(403, _)) => {
            return unavailable("Usage is unavailable for this Pi account.")
        }
        _ => return error("Could not fetch Pi usage. Try refreshing."),
    };
    match bounded_text(response.into_reader()) {
        Ok(body) => parse_usage(provider, &body),
        Err(_) => error("Pi usage response was too large or unreadable."),
    }
}
fn parse_usage(provider: PiUsageProvider, body: &str) -> PiUsageResult {
    let parsed = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| parse_windows(provider, &value));
    match parsed {
        Some(windows) if windows.session.is_some() || windows.weekly.is_some() => {
            PiUsageResult::Ok { windows }
        }
        _ => error("Pi usage response was unexpected. Try refreshing."),
    }
}
fn parse_windows(provider: PiUsageProvider, value: &Value) -> Option<UsageWindows> {
    let mut windows = UsageWindows::default();
    match provider {
        PiUsageProvider::Anthropic => {
            for (key, minutes) in [("five_hour", 300), ("seven_day", 10080)] {
                let raw = &value[key];
                if raw.is_null() {
                    continue;
                }
                let window = UsageWindow {
                    used_percent: percentage(&raw["utilization"])?,
                    window_minutes: minutes,
                    resets_at: if raw["resets_at"].is_null() {
                        None
                    } else {
                        let date = time::OffsetDateTime::parse(
                            raw["resets_at"].as_str()?,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .ok()?;
                        Some(u64::try_from(date.unix_timestamp_nanos() / 1_000_000).ok()?)
                    },
                };
                if minutes == 300 {
                    windows.session = Some(window);
                } else {
                    windows.weekly = Some(window);
                }
            }
        }
        PiUsageProvider::OpenaiCodex => {
            let limits = value.get("rate_limit")?.as_object()?;
            for key in ["primary_window", "secondary_window"] {
                let Some(raw) = limits.get(key).filter(|v| !v.is_null()) else {
                    continue;
                };
                let seconds = raw["limit_window_seconds"].as_u64()?;
                if seconds == 0 || seconds % 60 != 0 {
                    return None;
                }
                let window = UsageWindow {
                    used_percent: percentage(&raw["used_percent"])?,
                    window_minutes: seconds / 60,
                    resets_at: if raw["reset_at"].is_null() {
                        None
                    } else {
                        Some(raw["reset_at"].as_u64()?.checked_mul(1000)?)
                    },
                };
                let slot = if seconds >= 604800 {
                    &mut windows.weekly
                } else {
                    &mut windows.session
                };
                if slot.is_some() {
                    return None;
                }
                *slot = Some(window);
            }
        }
    }
    Some(windows)
}
fn percentage(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(value: serde_json::Value) -> Self {
            let dir =
                std::env::temp_dir().join(format!("monocode-pi-usage-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("auth.json"), value.to_string()).unwrap();
            Self(dir)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn auth() -> serde_json::Value {
        json!({"anthropic":{"type":"oauth","access":"pi-anthropic","expires":9999999999999_u64},
            "openai-codex":{"type":"oauth","access":"pi-codex","accountId":"pi-account","expires":9999999999999_u64}})
    }
    fn ok() -> PiUsageResult {
        PiUsageResult::Ok {
            windows: UsageWindows::default(),
        }
    }

    #[test]
    fn reads_only_the_selected_pi_credential_without_writing() {
        let f = Fixture::new(auth());
        let before = fs::read(f.0.join("auth.json")).unwrap();
        let result = read_usage(&f.0, PiUsageProvider::OpenaiCodex, |credential| {
            assert_eq!(credential.access, "pi-codex");
            assert_eq!(credential.account_id.as_deref(), Some("pi-account"));
            ok()
        });
        assert!(matches!(result, PiUsageResult::Ok { .. }));
        assert_eq!(fs::read(f.0.join("auth.json")).unwrap(), before);
        assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    }

    #[test]
    fn ineligible_credentials_never_make_a_request() {
        for entry in [
            json!(null),
            json!({"type":"api_key","key":"!do-not-run"}),
            json!({"type":"oauth","access":"expired","expires":1}),
            json!({"type":"oauth","access":"no-expiry"}),
            json!({"type":"oauth","access":"bad\r\nheader","expires":9999999999999_u64}),
        ] {
            let f = Fixture::new(json!({"anthropic":entry}));
            let result = read_usage(&f.0, PiUsageProvider::Anthropic, |_| {
                panic!("must not fetch")
            });
            assert!(!matches!(result, PiUsageResult::Ok { .. }));
        }
    }

    #[test]
    fn custom_directory_missing_auth_does_not_fall_back() {
        let f = Fixture::new(auth());
        fs::remove_file(f.0.join("auth.json")).unwrap();
        let result = read_usage(&f.0, PiUsageProvider::Anthropic, |_| {
            panic!("must not fetch")
        });
        assert!(matches!(result, PiUsageResult::Unavailable { .. }));
    }

    #[test]
    fn changed_credentials_discard_the_response() {
        let f = Fixture::new(auth());
        let result = read_usage(&f.0, PiUsageProvider::Anthropic, |_| {
            let mut replacement = auth();
            replacement["anthropic"]["access"] = json!("different-account");
            fs::write(f.0.join("auth.json"), replacement.to_string()).unwrap();
            ok()
        });
        assert!(matches!(result, PiUsageResult::Error { .. }));
    }

    #[test]
    fn known_provider_auth_overrides_are_not_mistaken_for_oauth() {
        let f = Fixture::new(auth());
        fs::write(
            f.0.join("models.json"),
            r#"{"providers":{"anthropic":{"apiKey":"other-account"}}}"#,
        )
        .unwrap();
        let result = read_usage(&f.0, PiUsageProvider::Anthropic, |_| {
            panic!("must not fetch")
        });
        assert!(matches!(result, PiUsageResult::Unavailable { .. }));
    }

    #[test]
    fn per_model_auth_overrides_are_not_mistaken_for_saved_oauth() {
        let f = Fixture::new(auth());
        fs::write(f.0.join("models.json"), r#"{"providers":{"anthropic":{"modelOverrides":{"claude-sonnet-5":{"headers":{"Authorization":"other-account"}}}}}}"#).unwrap();
        assert!(matches!(
            read_usage(&f.0, PiUsageProvider::Anthropic, |_| panic!(
                "must not fetch"
            )),
            PiUsageResult::Unavailable { .. }
        ));
    }

    #[test]
    fn relative_agent_directories_do_not_read_the_apps_working_directory() {
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(agent_dir(Some("relative-pi".into()), Some(home)), None);
    }

    #[test]
    fn codex_requires_its_own_account_id() {
        let mut value = auth();
        value["openai-codex"]
            .as_object_mut()
            .unwrap()
            .remove("accountId");
        let f = Fixture::new(value);
        let result = read_usage(&f.0, PiUsageProvider::OpenaiCodex, |_| {
            panic!("must not fetch")
        });
        assert!(!matches!(result, PiUsageResult::Ok { .. }));
    }

    #[test]
    fn parses_anthropic_zero_usage_and_never_returns_raw_identity() {
        let result = parse_usage(
            PiUsageProvider::Anthropic,
            r#"{"five_hour":{"utilization":0,"resets_at":"2026-09-25T13:00:00Z"},"seven_day":{"utilization":10,"resets_at":null},"email":"private@example.com"}"#,
        );
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["windows"]["session"]["usedPercent"], 0.0);
        assert_eq!(value["windows"]["weekly"]["windowMinutes"], 10080);
        assert!(!value.to_string().contains("private@example.com"));
    }

    #[test]
    fn codex_weekly_primary_is_not_labeled_five_hour() {
        let result = parse_usage(
            PiUsageProvider::OpenaiCodex,
            r#"{"rate_limit":{"primary_window":{"used_percent":32,"limit_window_seconds":604800,"reset_at":1790700198},"secondary_window":null}}"#,
        );
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "ok");
        assert!(value["windows"]["session"].is_null());
        assert_eq!(value["windows"]["weekly"]["usedPercent"], 32.0);
        assert_eq!(value["windows"]["weekly"]["resetsAt"], 1790700198000_u64);
    }

    #[test]
    fn agent_directory_matches_pi_override_semantics() {
        let home = std::env::temp_dir().to_string_lossy().into_owned();
        let home_path = PathBuf::from(&home);
        let custom_dir = home_path.join("custom").join("pi");
        assert!(custom_dir.is_absolute());

        assert_eq!(
            agent_dir(None, Some(home.clone())),
            Some(home_path.join(".pi").join("agent"))
        );
        assert_eq!(
            agent_dir(Some("~/custom/pi".into()), Some(home)),
            Some(custom_dir.clone())
        );
        assert_eq!(
            agent_dir(Some(custom_dir.clone().into_os_string()), None),
            Some(custom_dir)
        );
    }

    fn serve(
        status: &str,
        headers: &str,
        body: String,
    ) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usage", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                request.push_str(&line);
            }
            let _ = stream.write_all(response.as_bytes());
            request
        });
        (url, worker)
    }

    #[test]
    fn http_request_uses_pi_token_and_codex_account_header() {
        let (url, worker) = serve("200 OK", "", r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000,"reset_at":1790700198}}}"#.into());
        let creds = PiOAuth {
            access: "synthetic-pi-token".into(),
            account_id: Some("synthetic-pi-account".into()),
            expires: u64::MAX,
        };
        let result = request_usage(PiUsageProvider::OpenaiCodex, &creds, &url);
        assert!(matches!(result, PiUsageResult::Ok { .. }));
        let request = worker.join().unwrap().to_lowercase();
        assert!(request.contains("authorization: bearer synthetic-pi-token\r\n"));
        assert!(request.contains("chatgpt-account-id: synthetic-pi-account\r\n"));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic"));
    }

    #[test]
    fn redirects_never_forward_credentials_and_errors_do_not_forward_bodies() {
        let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let location = format!(
            "Location: http://{}/steal\r\n",
            target.local_addr().unwrap()
        );
        let (url, worker) = serve("302 Found", &location, "private upstream detail".into());
        let creds = PiOAuth {
            access: "synthetic-pi-token".into(),
            account_id: None,
            expires: u64::MAX,
        };
        let result = request_usage(PiUsageProvider::Anthropic, &creds, &url);
        assert!(matches!(result, PiUsageResult::Error { .. }));
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        worker.join().unwrap();
        let (url, worker) = serve("401 Unauthorized", "", "private upstream detail".into());
        let result = request_usage(PiUsageProvider::Anthropic, &creds, &url);
        assert!(matches!(result, PiUsageResult::Unavailable { .. }));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("private upstream detail"));
        worker.join().unwrap();
    }

    #[test]
    fn oversized_auth_and_responses_are_rejected() {
        let f = Fixture::new(auth());
        fs::write(f.0.join("auth.json"), " ".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert!(matches!(
            read_usage(&f.0, PiUsageProvider::Anthropic, |_| panic!(
                "must not fetch"
            )),
            PiUsageResult::Error { .. }
        ));
        let (url, worker) = serve("200 OK", "", " ".repeat(MAX_BYTES as usize + 1));
        let creds = PiOAuth {
            access: "synthetic".into(),
            account_id: None,
            expires: u64::MAX,
        };
        assert!(matches!(
            request_usage(PiUsageProvider::Anthropic, &creds, &url),
            PiUsageResult::Error { .. }
        ));
        worker.join().unwrap();
    }

    #[test]
    fn malformed_responses_are_not_zero_usage() {
        for body in [
            "{}",
            "not JSON",
            r#"{"five_hour":{"utilization":"30"}}"#,
            r#"{"five_hour":{"utilization":-1}}"#,
            r#"{"five_hour":{"utilization":25,"resets_at":"bad"}}"#,
        ] {
            assert!(matches!(
                parse_usage(PiUsageProvider::Anthropic, body),
                PiUsageResult::Error { .. }
            ));
        }
    }
}
