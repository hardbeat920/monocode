use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::dirs_home;

const DEFAULT_API_SERVER: &str = "https://server.codeium.com";
const USER_STATUS_PATH: &str = "/exa.seat_management_pb.SeatManagementService/GetUserStatus";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevinUsage {
    pub name: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
    /// Remaining share of each window, 0–100, or None when the plan hides it.
    pub daily_remaining_percent: Option<f64>,
    pub daily_resets_at: Option<i64>,
    pub weekly_remaining_percent: Option<f64>,
    pub weekly_resets_at: Option<i64>,
    /// Extra (overage) usage balance in millionths of a US dollar; negative
    /// once usage beyond the quota has been billed.
    pub extra_usage_balance_micros: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevinUsageFetch {
    pub status: String,
    pub http_status: Option<u16>,
    pub usage: Option<DevinUsage>,
    pub error: Option<String>,
}

/// Read the plan, identity and daily/weekly quota of the `devin auth login`
/// account from Devin's own API server. The key never leaves this process.
/// `cli_version` is the installed Devin CLI's version, which the server
/// requires alongside the client's own name and version.
#[tauri::command]
pub async fn fetch_devin_usage(
    app: AppHandle,
    cli_version: String,
) -> Result<DevinUsageFetch, String> {
    let app_version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || fetch_sync(&app_version, &cli_version))
        .await
        .map_err(|e| e.to_string())?
}

fn result(
    status: &str,
    http_status: Option<u16>,
    usage: Option<DevinUsage>,
    error: Option<&str>,
) -> DevinUsageFetch {
    DevinUsageFetch {
        status: status.into(),
        http_status,
        usage,
        error: error.map(String::from),
    }
}

fn fetch_sync(app_version: &str, cli_version: &str) -> Result<DevinUsageFetch, String> {
    let cli_version = cli_version.trim();
    if !valid_version(cli_version) {
        return Ok(result(
            "error",
            None,
            None,
            Some("Devin CLI version unknown"),
        ));
    }
    let Some(credentials) = read_credentials() else {
        return Ok(result(
            "unavailable",
            None,
            None,
            Some("Devin not signed in"),
        ));
    };
    let body = json!({
        "metadata": {
            "apiKey": credentials.api_key,
            "ideName": "monocode",
            "ideVersion": app_version,
            "extensionName": "devin-cli",
            "extensionVersion": cli_version,
        }
    });
    // The key rides in the body: refuse any redirect that drops to plain HTTP.
    let agent = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .https_only(true)
        .build();
    let response = agent
        .post(&format!("{}{USER_STATUS_PATH}", credentials.api_server))
        .set("Content-Type", "application/json")
        .set("Connect-Protocol-Version", "1")
        .send_string(&body.to_string());
    match response {
        Ok(response) => {
            let http_status = response.status();
            let text = response.into_string().unwrap_or_default();
            match serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| parse_user_status(&v))
            {
                Some(usage) => Ok(result("ok", Some(http_status), Some(usage), None)),
                None => Ok(result(
                    "error",
                    Some(http_status),
                    None,
                    Some("Devin usage response was unexpected"),
                )),
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let _ = response.into_string();
            let message = if status == 401 || status == 403 {
                "Devin sign-in expired".to_string()
            } else {
                format!("Devin usage request failed ({status})")
            };
            Ok(result("error", Some(status), None, Some(&message)))
        }
        Err(error) => Ok(result(
            "error",
            None,
            None,
            Some(&format!("Devin usage request failed: {error}")),
        )),
    }
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

struct Credentials {
    api_key: String,
    api_server: String,
}

fn credentials_candidates() -> Vec<PathBuf> {
    let env = |key: &str| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    candidate_paths(
        cfg!(windows),
        env("APPDATA"),
        env("XDG_DATA_HOME"),
        env("XDG_CONFIG_HOME"),
        dirs_home().map(PathBuf::from),
    )
}

/// Where `devin auth login` keeps credentials.toml: `%APPDATA%\devin` on
/// Windows, the XDG data directory (`~/.local/share/devin`) elsewhere. The
/// config directory is read last for logins made by older CLIs.
fn candidate_paths(
    windows: bool,
    app_data: Option<PathBuf>,
    xdg_data: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let file = |dir: PathBuf| dir.join("devin").join("credentials.toml");
    let mut dirs = Vec::new();
    if windows {
        dirs.extend(app_data);
    } else {
        dirs.extend(xdg_data);
        dirs.extend(home.as_ref().map(|home| home.join(".local").join("share")));
        dirs.extend(xdg_config);
    }
    dirs.extend(home.map(|home| home.join(".config")));
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in dirs.into_iter().map(file) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn read_credentials() -> Option<Credentials> {
    credentials_candidates()
        .into_iter()
        .find_map(|path| parse_credentials(&std::fs::read_to_string(path).ok()?))
}

fn parse_credentials(raw: &str) -> Option<Credentials> {
    let table: toml::Table = raw.parse().ok()?;
    let field = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let api_key = ["windsurf_api_key", "devin_api_key", "api_key"]
        .into_iter()
        .find_map(field)?
        .to_string();
    // Only send the key to an https server the login itself recorded.
    let api_server = field("api_server_url")
        .filter(|url| url.starts_with("https://"))
        .unwrap_or(DEFAULT_API_SERVER)
        .trim_end_matches('/')
        .to_string();
    Some(Credentials {
        api_key,
        api_server,
    })
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Proto3 JSON sends int64 as a string and omits zero values.
fn number(value: &Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A window exists when its reset time does; a missing remaining percent
/// then means zero, because proto3 JSON omits zero values.
fn quota_window(plan: &Value, info: &Value, prefix: &str) -> (Option<f64>, Option<i64>) {
    if flag(info, &format!("hide{prefix}Quota")) || flag(plan, &format!("hide{prefix}Quota")) {
        return (None, None);
    }
    let lower = prefix.to_lowercase();
    let Some(resets_at) = number(plan, &format!("{lower}QuotaResetAtUnix")) else {
        return (None, None);
    };
    let remaining = number(plan, &format!("{lower}QuotaRemainingPercent"))
        .unwrap_or(0.0)
        .clamp(0.0, 100.0);
    (Some(remaining), Some(resets_at as i64))
}

fn parse_user_status(body: &Value) -> Option<DevinUsage> {
    let status = body.get("userStatus")?;
    let plan = status.get("planStatus").unwrap_or(&Value::Null);
    let info = plan.get("planInfo").unwrap_or(&Value::Null);
    let (daily_remaining_percent, daily_resets_at) = quota_window(plan, info, "Daily");
    let (weekly_remaining_percent, weekly_resets_at) = quota_window(plan, info, "Weekly");
    Some(DevinUsage {
        name: text(status, "name"),
        email: text(status, "email"),
        plan: text(info, "planName"),
        daily_remaining_percent,
        daily_resets_at,
        weekly_remaining_percent,
        weekly_resets_at,
        extra_usage_balance_micros: extra_usage_balance(plan, info),
    })
}

/// Quota plans always carry a balance, so an omitted one is proto3's zero;
/// plans billed another way have none to show.
fn extra_usage_balance(plan: &Value, info: &Value) -> Option<i64> {
    match plan.get("overageBalanceMicros") {
        Some(Value::String(text)) => text.trim().parse().ok(),
        Some(Value::Number(number)) => number.as_i64(),
        _ if text(info, "billingStrategy").as_deref() == Some("BILLING_STRATEGY_QUOTA") => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quota_windows_and_identity() {
        let body = json!({
            "userStatus": {
                "name": "Ada",
                "email": "ada@example.com",
                "planStatus": {
                    "planInfo": { "planName": "Pro", "billingStrategy": "BILLING_STRATEGY_QUOTA" },
                    "weeklyQuotaRemainingPercent": 42,
                    "overageBalanceMicros": "-1673099",
                    "dailyQuotaResetAtUnix": "1791273600",
                    "weeklyQuotaResetAtUnix": "1791705600"
                }
            }
        });
        assert_eq!(
            parse_user_status(&body),
            Some(DevinUsage {
                name: Some("Ada".into()),
                email: Some("ada@example.com".into()),
                plan: Some("Pro".into()),
                // Omitted remaining percent is proto3's zero.
                daily_remaining_percent: Some(0.0),
                daily_resets_at: Some(1_791_273_600),
                weekly_remaining_percent: Some(42.0),
                weekly_resets_at: Some(1_791_705_600),
                extra_usage_balance_micros: Some(-1_673_099),
            })
        );
    }

    #[test]
    fn extra_usage_balance_defaults_to_zero_only_on_quota_plans() {
        let quota = json!({ "planInfo": { "billingStrategy": "BILLING_STRATEGY_QUOTA" } });
        assert_eq!(extra_usage_balance(&quota, &quota["planInfo"]), Some(0));
        let other = json!({ "planInfo": {} });
        assert_eq!(extra_usage_balance(&other, &other["planInfo"]), None);
        let paid = json!({ "overageBalanceMicros": 2_500_000 });
        assert_eq!(extra_usage_balance(&paid, &Value::Null), Some(2_500_000));
    }

    #[test]
    fn hidden_or_absent_windows_are_dropped() {
        let body = json!({
            "userStatus": {
                "planStatus": {
                    "planInfo": { "hideDailyQuota": true },
                    "dailyQuotaRemainingPercent": 80,
                    "dailyQuotaResetAtUnix": "1791273600"
                }
            }
        });
        let usage = parse_user_status(&body).unwrap();
        assert_eq!(usage.daily_remaining_percent, None);
        assert_eq!(usage.weekly_remaining_percent, None);
        assert_eq!(
            parse_user_status(&json!({ "code": "unauthenticated" })),
            None
        );
    }

    #[test]
    fn credentials_require_a_key_and_an_https_server() {
        let creds = parse_credentials(
            "windsurf_api_key = \"k\"\napi_server_url = \"https://server.example/\"\n",
        )
        .unwrap();
        assert_eq!(creds.api_key, "k");
        assert_eq!(creds.api_server, "https://server.example");
        let plain = parse_credentials(
            "windsurf_api_key = \"k\"\napi_server_url = \"http://evil.example\"\n",
        )
        .unwrap();
        assert_eq!(plain.api_server, DEFAULT_API_SERVER);
        assert!(parse_credentials("api_server_url = \"https://x\"\n").is_none());
    }

    #[test]
    fn unix_credentials_live_in_the_xdg_data_directory() {
        let home = PathBuf::from("/home/me");
        let file = |dir: &str| PathBuf::from(dir).join("devin").join("credentials.toml");
        assert_eq!(
            candidate_paths(false, None, None, None, Some(home.clone())),
            vec![file("/home/me/.local/share"), file("/home/me/.config")]
        );
        assert_eq!(
            candidate_paths(
                false,
                None,
                Some("/data".into()),
                Some("/cfg".into()),
                Some(home.clone())
            ),
            vec![
                file("/data"),
                file("/home/me/.local/share"),
                file("/cfg"),
                file("/home/me/.config"),
            ]
        );
        assert_eq!(
            candidate_paths(true, Some("/appdata".into()), None, None, Some(home)),
            vec![file("/appdata"), file("/home/me/.config")]
        );
    }

    #[test]
    fn versions_are_plain_tokens() {
        assert!(valid_version("3000.6.7"));
        assert!(!valid_version(""));
        assert!(!valid_version("1.0\nX-Header: y"));
    }
}
