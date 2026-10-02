use serde_json::{json, Value};
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

const API_BASE: &str = "https://api.anthropic.com";
const CEDAR_READ: &str = "/api/oauth/usage?cedar_ember=1&skip_spend=1";
const WALL_READ: &str = "/api/oauth/usage?at_wall=1&skip_spend=1";
const MAX_BODY_BYTES: u64 = 1024 * 1024;
static REDEMPTION: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub(crate) enum RequestError {
    Http(u16),
    InvalidResponse,
    Transport,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(25))
        .redirects(0)
        .build()
}

fn request(agent: &ureq::Agent, method: &str, url: &str, token: &str) -> ureq::Request {
    agent
        .request(method, url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", "oauth-2025-04-20")
        .set("x-app", "cli")
        .set("User-Agent", "claude-cli/2.1.288 (external, cli)")
        .set("Content-Type", "application/json")
}

fn read_response(result: Result<ureq::Response, ureq::Error>) -> Result<Value, RequestError> {
    let response = match result {
        Ok(response) if (200..300).contains(&response.status()) => response,
        Ok(response) => return Err(RequestError::Http(response.status())),
        Err(ureq::Error::Status(status, _)) => return Err(RequestError::Http(status)),
        Err(_) => return Err(RequestError::Transport),
    };
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_string(&mut body)
        .map_err(|_| RequestError::InvalidResponse)?;
    if body.len() as u64 > MAX_BODY_BYTES {
        return Err(RequestError::InvalidResponse);
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| RequestError::InvalidResponse)?;
    if !value.is_object() {
        return Err(RequestError::InvalidResponse);
    }
    Ok(value)
}

fn get(agent: &ureq::Agent, base: &str, path: &str, token: &str) -> Result<Value, RequestError> {
    read_response(
        request(agent, "GET", &format!("{base}{path}"), token)
            .timeout(Duration::from_secs(10))
            .call(),
    )
}

/// Evaluate reset programs explicitly: the plain usage response can contain null placeholders.
pub(crate) fn fetch_usage(token: &str) -> Result<Value, RequestError> {
    fetch_usage_at(token, API_BASE)
}

fn fetch_usage_at(token: &str, base: &str) -> Result<Value, RequestError> {
    let agent = agent();
    let mut usage = match get(&agent, base, CEDAR_READ, token) {
        Ok(usage) => usage,
        Err(RequestError::Http(400 | 404 | 405 | 410 | 501)) => {
            let mut plain = get(&agent, base, "/api/oauth/usage", token)?;
            plain["reset_discovery_failed"] = json!(true);
            plain
        }
        Err(error) => return Err(error),
    };
    // The CLI evaluates Juniper only at a session wall. This read also supplies
    // Cedar eligibility for grants restricted to an exhausted limit.
    if usage["five_hour"]["utilization"]
        .as_f64()
        .is_some_and(|value| value >= 100.0)
    {
        match get(&agent, base, WALL_READ, token) {
            Ok(wall) => {
                for field in ["cedar_ember", "juniper_tide"] {
                    if wall[field].is_object() {
                        usage[field] = wall[field].clone();
                    }
                }
            }
            Err(_) => usage["reset_discovery_failed"] = json!(true),
        }
    }
    Ok(usage)
}

/// Redeem only the exact confirmed offer after a fresh, account-bound eligibility read.
#[tauri::command]
pub async fn consume_claude_rate_limit_reset(
    app: tauri::AppHandle,
    account_id: Option<String>,
    credit_id: String,
    request_id: String,
) -> Result<String, String> {
    let config = crate::harness::provider_account_dir(&app, "claude", account_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = REDEMPTION
            .lock()
            .map_err(|_| "Claude reset is unavailable. Try again.")?;
        let creds = crate::rate_limits::read_claude_credentials(config.as_deref())
            .ok_or("Claude not signed in")?;
        if crate::rate_limits::token_expired(creds.expires_at_ms, crate::rate_limits::now_ms()) {
            return Err("Claude sign-in expired. Sign in again before using a reset.".into());
        }
        redeem_at(
            &creds.access_token,
            API_BASE,
            &credit_id,
            &request_id,
            || {
                crate::rate_limits::read_claude_credentials(config.as_deref()).as_ref()
                    == Some(&creds)
                    && !crate::rate_limits::token_expired(
                        creds.expires_at_ms,
                        crate::rate_limits::now_ms(),
                    )
            },
        )
    })
    .await
    .map_err(|_| "Could not apply Claude reset. Refresh usage before trying again.".to_string())?
}

fn valid_id(value: &str, max: usize, lowercase: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|ch| {
            ch.is_ascii_digit()
                || ch == b'_'
                || ch == b'-'
                || if lowercase {
                    ch.is_ascii_lowercase()
                } else {
                    ch.is_ascii_alphabetic()
                }
        })
}

fn future_date(value: &Value, now: i64) -> Result<bool, String> {
    if value.is_null() {
        return Ok(false);
    }
    let date = value
        .as_str()
        .and_then(|raw| {
            time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).ok()
        })
        .ok_or("Claude returned an invalid reset expiry. Refresh usage.")?;
    Ok(date.unix_timestamp_nanos() / 1_000_000 > i128::from(now))
}

fn redeem_at(
    token: &str,
    base: &str,
    credit_id: &str,
    request_id: &str,
    credentials_current: impl FnOnce() -> bool,
) -> Result<String, String> {
    if !valid_id(request_id, 64, false) {
        return Err("Invalid Claude reset request.".into());
    }
    let (program, grant_id) = if let Some(id) = credit_id.strip_prefix("cedar_ember:") {
        if !valid_id(id, 40, true) {
            return Err("Invalid Claude reset offer.".into());
        }
        ("cedar_ember", Some(id))
    } else if credit_id == "juniper_tide" {
        ("juniper_tide", None)
    } else {
        return Err("Unknown Claude reset offer.".into());
    };
    let agent = agent();
    let usage = get(
        &agent,
        base,
        if grant_id.is_some() {
            CEDAR_READ
        } else {
            WALL_READ
        },
        token,
    )
    .map_err(discovery_error)?;
    validate_offer(&usage, program, grant_id, crate::rate_limits::now_ms())?;
    let profile = get(&agent, base, "/api/oauth/profile", token).map_err(discovery_error)?;
    let org = profile["organization"]["uuid"]
        .as_str()
        .filter(|id| valid_id(id, 128, false))
        .ok_or("Could not identify this Claude account's organization. Sign in again.")?;
    if !credentials_current() {
        return Err(
            "Claude account changed or sign-in expired. Refresh usage before using a reset.".into(),
        );
    }
    validate_offer(&usage, program, grant_id, crate::rate_limits::now_ms())?;
    let body = if let Some(id) = grant_id {
        json!({"program":program,"grant_id":id,"request_id":request_id})
    } else {
        json!({"program":program})
    };
    // A reset spends a finite grant. Never retry a POST after an uncertain result.
    let result = read_response(
        request(
            &agent,
            "POST",
            &format!("{base}/api/organizations/{org}/reset_rate_limits"),
            token,
        )
        .send_string(&body.to_string()),
    )
    .map_err(redemption_error)?;
    match result["result"].as_str() {
        Some("reset") => Ok("reset".into()),
        Some("not_limited") => Ok("nothingToReset".into()),
        Some("already_used") => Ok("alreadyRedeemed".into()),
        Some("cooldown") => {
            Err("Claude reset is cooling down. Refresh usage to check availability.".into())
        }
        Some("ineligible" | "unavailable") => {
            Err("This Claude reset is no longer usable. Refresh usage.".into())
        }
        _ => Err("Claude reset was not confirmed. Refresh usage before trying again.".into()),
    }
}

fn validate_offer(
    usage: &Value,
    program: &str,
    grant_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    let status = &usage[program];
    if status["eligible"].as_bool() != Some(true) {
        return Err(if status["ineligible_reason"].as_str() == Some("surface") {
            "This reset is only available in Claude Web or Desktop."
        } else {
            "No eligible Claude reset is available. Refresh usage."
        }
        .into());
    }
    if let Some(id) = grant_id {
        let grant = status["grants"]
            .as_array()
            .and_then(|grants| grants.iter().find(|g| g["id"].as_str() == Some(id)))
            .ok_or("This Claude reset is no longer available. Refresh usage.")?;
        let expired = !grant["ends_at"].is_null() && !future_date(&grant["ends_at"], now)?;
        if status["next_grant_id"].as_str() != Some(id)
            || grant["usable_now"].as_bool() != Some(true)
            || grant["paused"].as_bool().unwrap_or(false)
            || grant["resets_left"].as_u64().unwrap_or(0) == 0
            || expired
            || future_date(&grant["starts_at"], now)?
            || future_date(&status["cooldown_until"], now)?
        {
            return Err("This Claude reset cannot be used right now. Refresh usage.".into());
        }
    } else if status["arm"].as_str() != Some("reset")
        || status["available"].as_bool() != Some(true)
        || (!status["weekly_resets_at"].is_null()
            && !future_date(&status["weekly_resets_at"], now)?)
    {
        return Err("No 5-hour Claude reset is available right now. Refresh usage.".into());
    }
    Ok(())
}

fn discovery_error(error: RequestError) -> String {
    match error {
        RequestError::Http(401) => "Claude sign-in expired. Sign in again before using a reset.",
        RequestError::Http(403) => "Claude reset access is unavailable for this account.",
        RequestError::Http(429) => "Claude is rate limiting reset checks. Try again later.",
        _ => "Could not check this Claude reset. Refresh usage before trying again.",
    }
    .into()
}

fn redemption_error(error: RequestError) -> String {
    match error {
        RequestError::Http(401 | 403 | 429) => discovery_error(error),
        _ => "Claude reset was not confirmed. Refresh usage before trying again.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    fn cedar() -> Value {
        json!({"five_hour":{"utilization":100},"cedar_ember":{
        "eligible":true,"at_limit":true,"next_grant_id":"grant_1","grants":[{
            "id":"grant_1","label":"Full reset","resets_left":2,"usable_now":true,
            "paused":false,"ends_at":"2099-10-22T00:00:00Z","clears":["five_hour","seven_day"]
        }]}})
    }
    fn profile() -> Value {
        json!({"organization":{"uuid":"org-selected"}})
    }
    fn serve(responses: Vec<(u16, Value)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                    request.push_str(&line);
                }
                let mut data = vec![0; length];
                reader.read_exact(&mut data).unwrap();
                request.push_str(&String::from_utf8(data).unwrap());
                requests.push(request);
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                let _ = stream.write_all(response.as_bytes());
            }
            requests
        });
        (url, worker)
    }

    #[test]
    fn discovery_evaluates_null_programs_and_reads_wall_offers() {
        let mut wall = cedar();
        wall["juniper_tide"] = json!({"eligible":true,"arm":"reset","available":true});
        let (url, worker) = serve(vec![(200, cedar()), (200, wall)]);
        let usage = fetch_usage_at("synthetic-token", &url).unwrap();
        assert_eq!(usage["juniper_tide"]["available"], true);
        let requests = worker.join().unwrap();
        assert!(requests[0].starts_with("GET /api/oauth/usage?cedar_ember=1&skip_spend=1 "));
        assert!(requests[1].starts_with("GET /api/oauth/usage?at_wall=1&skip_spend=1 "));
        assert!(requests.iter().all(|request| request
            .to_lowercase()
            .contains("authorization: bearer synthetic-token")));
        assert!(requests.iter().all(|request| {
            let headers = request.to_lowercase();
            headers.contains("x-app: cli")
                && headers.contains("user-agent: claude-cli/2.1.288 (external, cli)")
        }));
    }

    #[test]
    fn unavailable_reset_discovery_keeps_plain_usage() {
        let (url, worker) = serve(vec![
            (404, json!({})),
            (200, json!({"five_hour":{"utilization":10}})),
        ]);
        let usage = fetch_usage_at("synthetic-token", &url).unwrap();
        assert_eq!(usage["five_hour"]["utilization"], 10);
        assert_eq!(usage["reset_discovery_failed"], true);
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[test]
    fn cedar_redemption_uses_selected_organization_grant_and_request_id() {
        let (url, worker) = serve(vec![
            (200, cedar()),
            (200, profile()),
            (
                200,
                json!({"result":"reset","cleared":["five_hour","seven_day"]}),
            ),
        ]);
        assert_eq!(
            redeem_at(
                "synthetic-token",
                &url,
                "cedar_ember:grant_1",
                "same-request",
                || true
            )
            .unwrap(),
            "reset"
        );
        let requests = worker.join().unwrap();
        assert!(requests[1].starts_with("GET /api/oauth/profile "));
        assert!(requests[2].starts_with("POST /api/organizations/org-selected/reset_rate_limits "));
        let body = &requests[2][requests[2].find('{').unwrap()..];
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            json!({"program":"cedar_ember","grant_id":"grant_1","request_id":"same-request"})
        );
    }

    #[test]
    fn juniper_redemption_never_sends_a_cedar_grant() {
        let (url, worker) = serve(vec![
            (
                200,
                json!({"juniper_tide":{"eligible":true,"arm":"reset","available":true}}),
            ),
            (200, profile()),
            (200, json!({"result":"not_limited"})),
        ]);
        assert_eq!(
            redeem_at(
                "synthetic-token",
                &url,
                "juniper_tide",
                "unused-request",
                || true
            )
            .unwrap(),
            "nothingToReset"
        );
        let requests = worker.join().unwrap();
        assert!(requests[0].contains("at_wall=1"));
        let body = &requests[2][requests[2].find('{').unwrap()..];
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            json!({"program":"juniper_tide"})
        );
    }

    #[test]
    fn unavailable_or_changed_grants_never_send_a_mutation() {
        let mut mutations = Vec::new();
        for (key, value) in [
            ("resets_left", json!(0)),
            ("paused", json!(true)),
            ("usable_now", json!(false)),
            ("ends_at", json!("2000-01-01T00:00:00Z")),
            ("ends_at", json!("bad")),
            ("starts_at", json!("2099-01-01T00:00:00Z")),
        ] {
            let mut usage = cedar();
            usage["cedar_ember"]["grants"][0][key] = value;
            mutations.push(usage);
        }
        let mut changed = cedar();
        changed["cedar_ember"]["next_grant_id"] = json!("different");
        mutations.push(changed);
        let mut cooldown = cedar();
        cooldown["cedar_ember"]["cooldown_until"] = json!("2099-01-01T00:00:00Z");
        mutations.push(cooldown);
        let mut surface = cedar();
        surface["cedar_ember"]["eligible"] = json!(false);
        surface["cedar_ember"]["ineligible_reason"] = json!("surface");
        mutations.push(surface);
        for usage in mutations {
            let (url, worker) = serve(vec![(200, usage)]);
            assert!(redeem_at(
                "synthetic-token",
                &url,
                "cedar_ember:grant_1",
                "request",
                || true
            )
            .is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn changed_credentials_prevent_redemption() {
        let (url, worker) = serve(vec![(200, cedar()), (200, profile())]);
        let error = redeem_at(
            "synthetic-token",
            &url,
            "cedar_ember:grant_1",
            "request",
            || false,
        )
        .unwrap_err();
        assert!(error.contains("account changed"));
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[test]
    fn unknown_or_unconfirmed_redemptions_never_report_success_or_retry() {
        for (status, body) in [
            (200, json!({})),
            (200, json!({"result":"surprise"})),
            (200, json!({"result":"cooldown"})),
            (500, json!({"private":"private@example.com"})),
        ] {
            let (url, worker) = serve(vec![(200, cedar()), (200, profile()), (status, body)]);
            let error = redeem_at(
                "synthetic-token",
                &url,
                "cedar_ember:grant_1",
                "request",
                || true,
            )
            .unwrap_err();
            assert!(!error.contains("private@example.com"));
            assert_eq!(worker.join().unwrap().len(), 3);
        }
    }

    #[test]
    fn malformed_identifiers_never_make_a_request() {
        for (credit, request_id) in [
            ("cedar_ember:../other", "request"),
            ("other:grant", "request"),
            ("juniper_tide", "bad request"),
        ] {
            assert!(redeem_at(
                "synthetic-token",
                "http://127.0.0.1:1",
                credit,
                request_id,
                || true
            )
            .is_err());
        }
    }
}
