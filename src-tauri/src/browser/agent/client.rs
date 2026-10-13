//! Transport shared by `monocode browser` and `monocode browser-mcp`: one JSON
//! line to the loopback control endpoint, one JSON line back (spec 6, 12.1).
//!
//! Every way a call can end is folded into an [`ActionResult`], so the CLI and
//! the MCP server report transport trouble in the same envelope as browser
//! outcomes. The only thing left as an error is "this process has no
//! credentials", which each front end words for its own audience.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::browser::protocol::{
    ActionKind, ActionResult, CommonInput, Outcome, Reason, ENV_ENDPOINT, ENV_TOKEN, NAMESPACE,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Margin over the action's own time limit: queueing behind a busy tab (5 s),
/// admission and audit happen inside the host before the action clock starts.
const REPLY_MARGIN: Duration = Duration::from_secs(15);
/// Full-page screenshots arrive as base64 inside the JSON line.
const MAX_RESPONSE: u64 = 48 * 1024 * 1024;
/// The control server rejects larger request lines.
const MAX_REQUEST: usize = 256 * 1024;

/// Endpoint and session token from the provider's environment.
#[derive(Clone)]
pub struct Credentials {
    endpoint: SocketAddr,
    token: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never let the token reach a log line.
        f.debug_struct("Credentials")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// Why this process cannot reach the browser module at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissingCredentials {
    /// Neither variable is set: not started by MonoCode, or the provider did
    /// not pass its environment on to this child.
    Absent,
    /// Set but unusable (a non-loopback endpoint, an empty token, ...).
    Invalid(String),
}

impl MissingCredentials {
    pub fn message(&self) -> String {
        match self {
            MissingCredentials::Absent => format!(
                "This process has no MonoCode browser credential ({ENV_ENDPOINT} and {ENV_TOKEN} are not set). Browser tools only work in a MonoCode session; if this is one, the provider did not pass MonoCode's environment to this tool, so ask the user to restart the session from MonoCode."
            ),
            MissingCredentials::Invalid(why) => {
                format!("The MonoCode browser credential in this environment is invalid: {why}.")
            }
        }
    }
}

impl Credentials {
    pub fn new(endpoint: &str, token: &str) -> Result<Credentials, MissingCredentials> {
        let endpoint: SocketAddr = endpoint.trim().parse().map_err(|_| {
            MissingCredentials::Invalid(format!("{ENV_ENDPOINT} is not an address"))
        })?;
        if !endpoint.ip().is_loopback() {
            return Err(MissingCredentials::Invalid(
                "MonoCode only listens on localhost".into(),
            ));
        }
        let token = token.trim();
        if token.is_empty() || token.len() > 256 || token.chars().any(char::is_control) {
            return Err(MissingCredentials::Invalid(format!(
                "{ENV_TOKEN} is malformed"
            )));
        }
        Ok(Credentials {
            endpoint,
            token: token.to_string(),
        })
    }

    pub fn from_env() -> Result<Credentials, MissingCredentials> {
        let endpoint = std::env::var(ENV_ENDPOINT).ok();
        let token = std::env::var(ENV_TOKEN).ok();
        match (endpoint, token) {
            (None, None) => Err(MissingCredentials::Absent),
            (Some(endpoint), Some(token)) => Credentials::new(&endpoint, &token),
            (None, Some(_)) => Err(MissingCredentials::Invalid(format!(
                "{ENV_ENDPOINT} is not set"
            ))),
            (Some(_), None) => Err(MissingCredentials::Invalid(format!(
                "{ENV_TOKEN} is not set"
            ))),
        }
    }
}

/// A fresh request id. The host deduplicates by id (spec 11.4), so a retry
/// after an `unknown` outcome must reuse the id it reported, never a new one.
pub fn new_request_id() -> String {
    format!("req_{}", uuid::Uuid::new_v4().simple())
}

/// Whether `id` is acceptable as a request id: what the control server takes
/// (1-128 bytes), restricted to characters that survive shells and logs.
pub fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

/// How long to wait for the host's answer to one action.
pub fn reply_timeout(action: &str, input: &Value) -> Duration {
    let timeout_ms = serde_json::from_value::<CommonInput>(input.clone())
        .ok()
        .and_then(|common| common.timeout_ms);
    ActionKind::parse(action)
        .map(|kind| kind.effective_timeout(timeout_ms))
        .unwrap_or(Duration::from_secs(15))
        + REPLY_MARGIN
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    result: Option<ActionResult>,
    #[serde(default)]
    error: Option<String>,
}

/// Send one action and return exactly one result for it.
///
/// - The host's own result is passed through unchanged.
/// - Nothing written yet (cannot connect, request too large): `blocked`
///   without a reason from the closed set, since no browser state is involved.
/// - Written but no usable answer: `unknown`; the host may have run it.
/// - Token refused by the control server: `blocked` / `revoked`.
pub fn call(creds: &Credentials, action: &str, input: &Value, request_id: &str) -> ActionResult {
    let line = json!({
        "token": creds.token,
        "namespace": NAMESPACE,
        "action": action,
        "input": input,
        "requestId": request_id,
    })
    .to_string();
    if line.len() > MAX_REQUEST {
        return not_sent(
            request_id,
            "The request is larger than MonoCode accepts (256 KiB); send less text per call.",
        );
    }
    let mut stream = match TcpStream::connect_timeout(&creds.endpoint, CONNECT_TIMEOUT) {
        Ok(stream) => stream,
        Err(error) => {
            return not_sent(
                request_id,
                format!(
                    "Cannot reach MonoCode ({error}). The app may have quit or restarted, or a sandbox blocks localhost; nothing was sent to the browser."
                ),
            )
        }
    };
    let _ = stream.set_nodelay(true);
    if stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
        || stream
            .set_read_timeout(Some(reply_timeout(action, input)))
            .is_err()
    {
        return not_sent(request_id, "Cannot configure the MonoCode connection.");
    }
    if let Err(error) = stream
        .write_all(line.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
    {
        return lost(request_id, &format!("Sending to MonoCode failed ({error})"));
    }
    let mut raw = String::new();
    let read = BufReader::new(stream)
        .take(MAX_RESPONSE + 1)
        .read_line(&mut raw);
    match read {
        Err(error) => lost(request_id, &format!("No answer from MonoCode ({error})")),
        Ok(0) => lost(
            request_id,
            "MonoCode closed the connection without an answer",
        ),
        Ok(_) if raw.len() as u64 > MAX_RESPONSE => {
            lost(request_id, "MonoCode's answer was too large to read")
        }
        Ok(_) => decode(request_id, &raw),
    }
}

fn decode(request_id: &str, raw: &str) -> ActionResult {
    match serde_json::from_str::<Reply>(raw) {
        Ok(Reply {
            ok: true,
            result: Some(result),
            ..
        }) => result,
        Ok(Reply {
            ok: false, error, ..
        }) => {
            let detail = error.unwrap_or_else(|| "request refused".into());
            ActionResult::from_reason(request_id, Reason::Revoked).with_message(format!(
                "MonoCode refused this session's browser credential ({detail}). Nothing was sent to the browser. The app may have restarted since this session started; ask the user to restart the session from MonoCode."
            ))
        }
        _ => lost(
            request_id,
            "MonoCode returned an answer this tool cannot read",
        ),
    }
}

/// Nothing reached the host, so nothing reached the browser.
pub fn not_sent(request_id: &str, message: impl Into<String>) -> ActionResult {
    ActionResult {
        request_id: request_id.to_string(),
        outcome: Outcome::Blocked,
        reason: None,
        message: message.into(),
        retryable: true,
        observed: Default::default(),
        data: None,
        timing_ms: 0,
    }
}

/// The request reached the host but its answer did not reach us.
fn lost(request_id: &str, what: &str) -> ActionResult {
    ActionResult {
        request_id: request_id.to_string(),
        outcome: Outcome::Unknown,
        reason: None,
        message: format!(
            "{what}; the action may have happened. Take a snapshot before retrying, and retry only with request_id {request_id}."
        ),
        retryable: false,
        observed: Default::default(),
        data: None,
        timing_ms: 0,
    }
}

/// The result an action gets when this process has no credentials (spec
/// 12.1: tools stay listed and every call answers `not_granted`).
pub fn without_credentials(request_id: &str, missing: &MissingCredentials) -> ActionResult {
    ActionResult::from_reason(request_id, Reason::NotGranted).with_message(missing.message())
}

/// Text payload of a snapshot or read result: `data.text`, or `data` itself
/// when the semantic layer returns a bare string.
pub fn data_text(data: Option<&Value>) -> Option<&str> {
    let data = data?;
    data.as_str().or_else(|| {
        ["text", "snapshot", "content"]
            .iter()
            .find_map(|key| data.get(key).and_then(Value::as_str))
    })
}

/// Base64 image of a screenshot result and its media type.
pub fn data_image(data: Option<&Value>) -> Option<(&str, String)> {
    let data = data?;
    let image = data.as_str().or_else(|| {
        ["image", "data", "base64"]
            .iter()
            .find_map(|key| data.get(key).and_then(Value::as_str))
    })?;
    let declared = ["mime_type", "mimeType", "format"]
        .iter()
        .find_map(|key| data.get(key).and_then(Value::as_str))
        .unwrap_or("png");
    let mime = match declared.trim_start_matches("image/") {
        "jpeg" | "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    };
    Some((image, mime.to_string()))
}

/// `data` with the keys [`data_text`] / [`data_image`] read removed, so the
/// envelope that travels next to the extracted content does not repeat it.
pub fn data_without(data: Option<&Value>, keys: &[&str]) -> Option<Value> {
    match data? {
        Value::String(_) => None,
        Value::Object(map) => {
            let rest: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(key, _)| !keys.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            (!rest.is_empty()).then_some(Value::Object(rest))
        }
        other => Some(other.clone()),
    }
}

pub const TEXT_KEYS: [&str; 3] = ["text", "snapshot", "content"];
pub const IMAGE_KEYS: [&str; 3] = ["image", "data", "base64"];

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    /// A one-shot control server: hands the received line to the test and
    /// answers with whatever `answer` returns (`None` closes without a reply).
    pub(crate) fn fake_control(
        answer: impl Fn(&Value) -> Option<String> + Send + 'static,
    ) -> (Credentials, mpsc::Receiver<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                BufReader::new(&mut stream).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let reply = answer(&request);
                let _ = tx.send(request);
                if let Some(reply) = reply {
                    let _ = writeln!(stream, "{reply}");
                }
            }
        });
        (Credentials::new(&address, &"ab".repeat(32)).unwrap(), rx)
    }

    #[test]
    fn sends_one_line_in_the_browser_namespace_and_passes_results_through() {
        let (creds, requests) = fake_control(|request| {
            let result = ActionResult::done(request["requestId"].as_str().unwrap(), "Clicked.");
            Some(json!({"ok": true, "result": result}).to_string())
        });
        let input = json!({"tab": "t_0a1b2c3d", "ref": "e3"});
        let result = call(&creds, "click", &input, "req_1");
        assert_eq!(result.outcome, Outcome::Done);
        assert_eq!(result.message, "Clicked.");
        let sent = requests.recv().unwrap();
        assert_eq!(sent["namespace"], "browser");
        assert_eq!(sent["action"], "click");
        assert_eq!(sent["input"], input);
        assert_eq!(sent["requestId"], "req_1");
        assert_eq!(sent["token"], "ab".repeat(32));
    }

    #[test]
    fn refused_tokens_lost_answers_and_dead_endpoints_each_get_one_result() {
        let (creds, _) = fake_control(|_| {
            Some(json!({"ok": false, "error": "Connection revoked or unauthorized"}).to_string())
        });
        let refused = call(&creds, "tabs.list", &json!({}), "req_2");
        assert_eq!(refused.outcome, Outcome::Blocked);
        assert_eq!(refused.reason, Some(Reason::Revoked));
        assert!(refused.message.contains("Connection revoked"));

        let (creds, _) = fake_control(|_| None);
        let lost = call(&creds, "click", &json!({}), "req_3");
        assert_eq!(lost.outcome, Outcome::Unknown);
        assert!(lost.message.contains("req_3"), "{}", lost.message);
        assert!(!lost.retryable);

        let (creds, _) = fake_control(|_| Some("not json".into()));
        assert_eq!(
            call(&creds, "click", &json!({}), "req_4").outcome,
            Outcome::Unknown
        );

        // Nothing listens on a port we just released.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dead = Credentials::new(&format!("127.0.0.1:{port}"), "tok").unwrap();
        let unreachable = call(&dead, "click", &json!({}), "req_5");
        assert_eq!(unreachable.outcome, Outcome::Blocked);
        assert_eq!(unreachable.reason, None);
        assert!(unreachable.message.contains("nothing was sent"));
    }

    #[test]
    fn oversized_requests_are_refused_before_connecting() {
        let dead = Credentials::new("127.0.0.1:9", "tok").unwrap();
        let input = json!({"tab": "t_0a1b2c3d", "ref": "e1", "text": "x".repeat(300_000)});
        let result = call(&dead, "type", &input, "req_6");
        assert_eq!(result.outcome, Outcome::Blocked);
        assert!(result.message.contains("256 KiB"));
    }

    #[test]
    fn credentials_accept_only_loopback_and_never_print_the_token() {
        assert!(Credentials::new("127.0.0.1:4000", "secret").is_ok());
        assert!(Credentials::new("[::1]:4000", "secret").is_ok());
        assert!(matches!(
            Credentials::new("10.0.0.2:4000", "secret"),
            Err(MissingCredentials::Invalid(_))
        ));
        assert!(Credentials::new("localhost", "secret").is_err());
        assert!(Credentials::new("127.0.0.1:4000", " ").is_err());
        let creds = Credentials::new("127.0.0.1:4000", "secret").unwrap();
        assert!(!format!("{creds:?}").contains("secret"));
        let blocked = without_credentials("req_7", &MissingCredentials::Absent);
        assert_eq!(blocked.reason, Some(Reason::NotGranted));
        assert!(blocked.message.contains(ENV_TOKEN));
    }

    #[test]
    fn reply_waits_cover_each_actions_own_limit() {
        assert_eq!(
            reply_timeout("navigate", &json!({"timeout_ms": 600_000})),
            Duration::from_secs(120) + REPLY_MARGIN
        );
        assert_eq!(
            reply_timeout("click", &json!({})),
            Duration::from_secs(10) + REPLY_MARGIN
        );
        assert_eq!(
            reply_timeout("nonsense", &json!({"timeout_ms": "x"})),
            Duration::from_secs(15) + REPLY_MARGIN
        );
    }

    #[test]
    fn request_ids_are_fresh_and_checked() {
        let id = new_request_id();
        assert!(valid_request_id(&id));
        assert_ne!(id, new_request_id());
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("a b"));
        assert!(!valid_request_id(&"a".repeat(129)));
        assert!(valid_request_id("retry-1.2:x_y"));
    }

    #[test]
    fn payloads_are_found_in_canonical_and_bare_shapes() {
        let snapshot = json!({"text": "- button \"Go\" [ref=e1]", "version": 3});
        assert_eq!(data_text(Some(&snapshot)), Some("- button \"Go\" [ref=e1]"));
        assert_eq!(
            data_without(Some(&snapshot), &TEXT_KEYS),
            Some(json!({"version": 3}))
        );
        assert_eq!(data_text(Some(&json!("bare"))), Some("bare"));
        assert_eq!(data_without(Some(&json!("bare")), &TEXT_KEYS), None);
        assert_eq!(data_text(Some(&json!({"tabs": []}))), None);

        let shot = json!({"image": "iVBO", "mime_type": "image/jpeg", "width": 10});
        assert_eq!(
            data_image(Some(&shot)),
            Some(("iVBO", "image/jpeg".to_string()))
        );
        assert_eq!(
            data_image(Some(&json!({"data": "AAAA", "format": "png"}))),
            Some(("AAAA", "image/png".to_string()))
        );
        assert_eq!(
            data_without(Some(&shot), &IMAGE_KEYS),
            Some(json!({"mime_type": "image/jpeg", "width": 10}))
        );
    }
}
