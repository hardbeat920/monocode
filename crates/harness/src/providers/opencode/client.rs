//! Port of src/integrations/harness/providers/opencode/opencodeClient.ts: the
//! HTTP and SSE client for one `opencode serve` process, over the
//! framework's [`Children`] handle (`harness_http`, `harness_sse_open`).
//!
//! Request bodies serialize from structs, because the TypeScript wrote keys
//! in a fixed order and this workspace's `serde_json::Map` sorts them.

use std::collections::HashMap;
use std::fmt;

use anyhow::{Result, anyhow};
use monocode_core::js;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::protocol::{
    OpenCodePermissionRule, OpenCodePromptPart, ParsedOpenCodeModelSlug, PermissionReply, Record,
};
use crate::core::child::{Children, HttpRequest, SseEvents};

/// `OpenCodeHttpError`: a response with status 400 or above.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenCodeHttpError {
    pub status: u16,
    /// The parsed body, or the raw text when it was not JSON.
    pub body: Value,
    pub message: String,
}

impl OpenCodeHttpError {
    /// `error.name`.
    pub fn name(&self) -> &'static str {
        if self.status == 404 {
            "NotFoundError"
        } else {
            "OpenCodeHttpError"
        }
    }
}

impl fmt::Display for OpenCodeHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OpenCodeHttpError {}

/// `isOpenCodeNotFound(error) || isHttpNotFound(error)` for an error the
/// client returned. Every client error that carries a status is an
/// [`OpenCodeHttpError`], so a 404 is the only way to match.
pub fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<OpenCodeHttpError>()
        .is_some_and(|error| error.status == 404)
}

/// `OpenCodeSession`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OpenCodeSession {
    pub id: String,
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// `OpenCodeMessage`. A missing or malformed `info` reads as `None`, and
/// malformed `parts` as empty.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenCodeMessage {
    pub info: Option<Record>,
    pub parts: Vec<Value>,
}

impl OpenCodeMessage {
    fn from_value(value: &Value) -> Self {
        Self {
            info: value.get("info").and_then(Value::as_object).cloned(),
            parts: value
                .get("parts")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

/// What `prompt` returns.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PromptResult {
    pub info: Option<Record>,
    pub parts: Option<Vec<Value>>,
}

/// The input of `promptAsync` and `prompt`.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptInput {
    pub session_id: String,
    pub model: ParsedOpenCodeModelSlug,
    pub agent: Option<String>,
    pub variant: Option<String>,
    pub parts: Vec<OpenCodePromptPart>,
}

#[derive(Serialize)]
struct PromptBody<'a> {
    model: &'a ParsedOpenCodeModelSlug,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<&'a str>,
    parts: &'a [OpenCodePromptPart],
}

impl<'a> PromptBody<'a> {
    fn new(input: &'a PromptInput) -> Self {
        // `...(input.agent ? { agent } : {})`: an empty string is left out.
        let present =
            |value: &'a Option<String>| value.as_deref().filter(|value| !value.is_empty());
        Self {
            model: &input.model,
            agent: present(&input.agent),
            variant: present(&input.variant),
            parts: &input.parts,
        }
    }
}

#[derive(Serialize)]
struct CreateSessionBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission: Option<&'a [OpenCodePermissionRule]>,
}

/// `{ permission }`, the body ensureLive and resolveSession send to
/// `updateSession`.
#[derive(Serialize)]
pub struct PermissionUpdate<'a> {
    pub permission: &'a [OpenCodePermissionRule],
}

#[derive(Serialize)]
struct Empty {}

#[derive(Serialize)]
struct RevertBody<'a> {
    #[serde(rename = "messageID")]
    message_id: &'a str,
}

#[derive(Serialize)]
struct ReplyBody {
    reply: PermissionReply,
}

#[derive(Serialize)]
struct AnswersBody<'a> {
    answers: &'a [Vec<String>],
}

/// Options for one request.
#[derive(Default)]
struct RequestOptions<'a> {
    /// Already serialized JSON. `None` sends no body.
    body: Option<String>,
    query: &'a [(&'a str, &'a str)],
    timeout_ms: Option<i64>,
}

fn json_body(body: &impl Serialize) -> Option<String> {
    Some(serde_json::to_string(body).unwrap_or_else(|_| "{}".into()))
}

/// `OpenCodeClient`: one server, scoped to one working directory.
#[derive(Clone)]
pub struct OpenCodeClient {
    pub base_url: String,
    pub directory: String,
    children: Children,
}

impl fmt::Debug for OpenCodeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenCodeClient")
            .field("base_url", &self.base_url)
            .field("directory", &self.directory)
            .finish()
    }
}

impl OpenCodeClient {
    pub fn new(base_url: &str, directory: &str, children: Children) -> Self {
        Self {
            base_url: base_url.to_string(),
            directory: directory.to_string(),
            children,
        }
    }

    pub async fn get_session(&self, session_id: &str) -> Result<OpenCodeSession> {
        let value = self
            .request(
                "GET",
                &format!("/session/{}", enc(session_id)),
                RequestOptions::default(),
            )
            .await?;
        session_from(value)
    }

    /// `getMessages`. `None` when the server did not return a list.
    pub async fn get_messages(&self, session_id: &str) -> Result<Option<Vec<OpenCodeMessage>>> {
        let value = self
            .request(
                "GET",
                &format!("/session/{}/message", enc(session_id)),
                RequestOptions::default(),
            )
            .await?;
        Ok(value
            .as_ref()
            .and_then(Value::as_array)
            .map(|messages| messages.iter().map(OpenCodeMessage::from_value).collect()))
    }

    pub async fn create_session(
        &self,
        title: Option<&str>,
        permission: Option<&[OpenCodePermissionRule]>,
    ) -> Result<OpenCodeSession> {
        // `...(input.permission ? { permission } : {})`: an array is always truthy.
        let body = CreateSessionBody {
            title: title.filter(|title| !title.is_empty()),
            permission,
        };
        let value = self
            .request(
                "POST",
                "/session",
                RequestOptions {
                    body: json_body(&body),
                    ..Default::default()
                },
            )
            .await?;
        session_from(value)
    }

    /// `updateSession`. The TypeScript typed the reply as a session but no
    /// caller read it, so it is returned as parsed.
    pub async fn update_session(
        &self,
        session_id: &str,
        body: &impl Serialize,
    ) -> Result<Option<Value>> {
        self.request(
            "PATCH",
            &format!("/session/{}", enc(session_id)),
            RequestOptions {
                body: json_body(body),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn fork_session(&self, session_id: &str, directory: &str) -> Result<OpenCodeSession> {
        let value = self
            .request(
                "POST",
                &format!("/session/{}/fork", enc(session_id)),
                RequestOptions {
                    body: json_body(&Empty {}),
                    query: &[("directory", directory)],
                    ..Default::default()
                },
            )
            .await?;
        session_from(value)
    }

    /// `abortSession`. Failures are ignored.
    pub async fn abort_session(&self, session_id: &str) {
        let _ = self
            .request(
                "POST",
                &format!("/session/{}/abort", enc(session_id)),
                RequestOptions {
                    body: json_body(&Empty {}),
                    ..Default::default()
                },
            )
            .await;
    }

    pub async fn revert_session(&self, session_id: &str, message_id: &str) -> Result<()> {
        self.request(
            "POST",
            &format!("/session/{}/revert", enc(session_id)),
            RequestOptions {
                body: json_body(&RevertBody { message_id }),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    /// `summarizeSession`: OpenCode's own compaction. It answers only after
    /// the pass, so it waits up to 30 minutes.
    pub async fn summarize_session(
        &self,
        session_id: &str,
        model: &ParsedOpenCodeModelSlug,
    ) -> Result<()> {
        self.request(
            "POST",
            &format!("/session/{}/summarize", enc(session_id)),
            RequestOptions {
                body: json_body(model),
                timeout_ms: Some(30 * 60_000),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    /// `promptAsync`: start a turn. The reply has no body; the event stream
    /// reports the turn.
    pub async fn prompt_async(&self, input: &PromptInput) -> Result<()> {
        self.request(
            "POST",
            &format!("/session/{}/prompt_async", enc(&input.session_id)),
            RequestOptions {
                body: json_body(&PromptBody::new(input)),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    /// `prompt`: run a turn and wait for the finished message.
    pub async fn prompt(
        &self,
        input: &PromptInput,
        timeout_ms: Option<i64>,
    ) -> Result<PromptResult> {
        let value = self
            .request(
                "POST",
                &format!("/session/{}/message", enc(&input.session_id)),
                RequestOptions {
                    body: json_body(&PromptBody::new(input)),
                    timeout_ms,
                    ..Default::default()
                },
            )
            .await?;
        Ok(PromptResult {
            info: value
                .as_ref()
                .and_then(|value| value.get("info"))
                .and_then(Value::as_object)
                .cloned(),
            parts: value
                .as_ref()
                .and_then(|value| value.get("parts"))
                .and_then(Value::as_array)
                .cloned(),
        })
    }

    pub async fn reply_permission(&self, request_id: &str, reply: PermissionReply) -> Result<()> {
        self.request(
            "POST",
            &format!("/permission/{}/reply", enc(request_id)),
            RequestOptions {
                body: json_body(&ReplyBody { reply }),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn reply_question(&self, request_id: &str, answers: &[Vec<String>]) -> Result<()> {
        self.request(
            "POST",
            &format!("/question/{}/reply", enc(request_id)),
            RequestOptions {
                body: json_body(&AnswersBody { answers }),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    pub async fn reject_question(&self, request_id: &str) -> Result<()> {
        self.request(
            "POST",
            &format!("/question/{}/reject", enc(request_id)),
            RequestOptions {
                body: json_body(&Empty {}),
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    /// `subscribeEvents`: watch the stream for `stream_id`, then open it.
    /// The caller reads the returned channel; the TypeScript took `onEvent`
    /// and `onEnd` callbacks instead. Frames that are not JSON objects are
    /// for the caller to drop, as `parse_event` does.
    pub async fn subscribe_events(&self, stream_id: &str) -> Result<SseEvents> {
        let url = self.url("/event", &[])?;
        let events = self.children.watch_sse(stream_id);
        self.children
            .open_harness_sse(stream_id, &url, Some(self.headers(false)))
            .await?;
        Ok(events)
    }

    /// `closeEvents`. Failures are ignored.
    pub async fn close_events(&self, stream_id: &str) {
        let _ = self.children.close_harness_sse(stream_id).await;
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        opts: RequestOptions<'_>,
    ) -> Result<Option<Value>> {
        let url = self.url(path, opts.query)?;
        let has_body = opts.body.is_some();
        let response = self
            .children
            .harness_http(HttpRequest {
                url,
                method: method.to_string(),
                headers: Some(self.headers(has_body)),
                body: opts.body,
                timeout_ms: opts.timeout_ms,
            })
            .await?;
        if response.status == 204 || js::trim(&response.body).is_empty() {
            if response.status >= 400 {
                return Err(OpenCodeHttpError {
                    status: response.status,
                    body: Value::String(response.body.clone()),
                    message: http_error_message(response.status, &response.body, None),
                }
                .into());
            }
            return Ok(None);
        }
        let parsed = parse_json(&response.body);
        if response.status >= 400 {
            let message = http_error_message(response.status, &response.body, Some(&parsed));
            return Err(OpenCodeHttpError {
                status: response.status,
                body: parsed,
                message,
            }
            .into());
        }
        Ok(Some(unwrap_data(parsed)))
    }

    /// `url(path, query)`: the path on this server, with `directory` and any
    /// extra query parameters set the way `URLSearchParams.set` sets them.
    fn url(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let base = format!(
            "{}/",
            self.base_url.strip_suffix('/').unwrap_or(&self.base_url)
        );
        let mut url = Url::parse(&base)
            .and_then(|base| base.join(path))
            .map_err(|error| anyhow!("Invalid OpenCode URL {base}: {error}"))?;
        let mut pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        set_param(&mut pairs, "directory", &self.directory);
        for (key, value) in query {
            set_param(&mut pairs, key, value);
        }
        url.query_pairs_mut().clear().extend_pairs(pairs);
        Ok(url.to_string())
    }

    fn headers(&self, json: bool) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        if json {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        headers.insert(
            "x-opencode-directory".into(),
            js::encode_uri_component(&self.directory),
        );
        headers
    }
}

/// `URLSearchParams.set`: replace the first value and drop the rest, or append.
fn set_param(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    match pairs.iter().position(|(name, _)| name == key) {
        Some(index) => {
            pairs[index].1 = value.to_string();
            let mut seen = 0;
            pairs.retain(|(name, _)| {
                if name != key {
                    return true;
                }
                seen += 1;
                seen == 1
            });
        }
        None => pairs.push((key.to_string(), value.to_string())),
    }
}

fn session_from(value: Option<Value>) -> Result<OpenCodeSession> {
    // TODO(port): the TypeScript read `.id` off whatever came back, so a reply
    // without a session (OpenCode 2 serves its web app at these paths) failed
    // later with an undefined id. Here it fails at once.
    let value = value.ok_or_else(|| anyhow!("OpenCode returned no session"))?;
    serde_json::from_value::<OpenCodeSession>(value).map_err(|_| {
        anyhow!(
            "OpenCode returned an unexpected session. MonoCode supports the OpenCode 1 server API."
        )
    })
}

fn enc(value: &str) -> String {
    js::encode_uri_component(value)
}

/// `parseJson`: the parsed value, or the raw text when it is not JSON.
fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

/// `parseJson` for an SSE frame: the event when it is a JSON object.
pub fn parse_event(data: &str) -> Option<Record> {
    match parse_json(data) {
        Value::Object(record) => Some(record),
        _ => None,
    }
}

/// `unwrapData`: OpenCode wraps some replies in `{ data }`.
fn unwrap_data(value: Value) -> Value {
    match value {
        Value::Object(mut record) if record.get("data").is_some() => {
            record.remove("data").unwrap_or(Value::Null)
        }
        other => other,
    }
}

/// `httpErrorMessage`.
fn http_error_message(status: u16, raw: &str, parsed: Option<&Value>) -> String {
    let parsed = parsed.cloned().unwrap_or_else(|| parse_json(raw));
    let rec = parsed.as_object();
    let nested = rec
        .and_then(|rec| rec.get("error"))
        .and_then(Value::as_object)
        .or_else(|| {
            rec.and_then(|rec| rec.get("data"))
                .and_then(Value::as_object)
        });
    let message_of = |rec: Option<&Record>| {
        rec.and_then(|rec| rec.get("message"))
            .and_then(Value::as_str)
            .map(js::trim)
            .filter(|message| !message.is_empty())
            .map(str::to_string)
    };
    let message = message_of(rec)
        .or_else(|| message_of(nested))
        .unwrap_or_else(|| js::trim(raw).to_string());
    if message.is_empty() {
        format!("OpenCode HTTP {status}")
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::opencode::test_support::FakeHost;
    use serde_json::json;

    // describe("OpenCodeClient.summarizeSession")

    #[test]
    fn calls_the_native_session_summarize_endpoint_with_the_selected_model() {
        let host = FakeHost::new();
        host.respond_with(|_| (200, "true".into()));
        let client = OpenCodeClient::new("http://127.0.0.1:4096", "/repo", host.children());
        smol::block_on(client.summarize_session(
            "session/a",
            &ParsedOpenCodeModelSlug {
                provider_id: "openai".into(),
                model_id: "gpt-5.4".into(),
            },
        ))
        .unwrap();
        let calls = host.http_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            HttpRequest {
                url: "http://127.0.0.1:4096/session/session%2Fa/summarize?directory=%2Frepo".into(),
                method: "POST".into(),
                headers: Some(HashMap::from([
                    ("Content-Type".to_string(), "application/json".to_string()),
                    ("x-opencode-directory".to_string(), "%2Frepo".to_string()),
                ])),
                body: Some(r#"{"providerID":"openai","modelID":"gpt-5.4"}"#.into()),
                timeout_ms: Some(30 * 60_000),
            }
        );
    }

    #[test]
    fn sets_the_fork_directory_over_the_client_directory() {
        let host = FakeHost::new();
        host.respond_with(|_| (200, r#"{"data":{"id":"forked"}}"#.into()));
        let client = OpenCodeClient::new("http://127.0.0.1:4096/", "/repo", host.children());
        let forked = smol::block_on(client.fork_session("s1", "/other dir")).unwrap();
        assert_eq!(forked.id, "forked");
        let calls = host.http_calls();
        assert_eq!(
            calls[0].url,
            "http://127.0.0.1:4096/session/s1/fork?directory=%2Fother+dir"
        );
        assert_eq!(calls[0].body.as_deref(), Some("{}"));
    }

    #[test]
    fn reads_error_messages_from_json_and_plain_bodies() {
        let host = FakeHost::new();
        let client = OpenCodeClient::new("http://127.0.0.1:4096", "/repo", host.children());
        host.respond_with(|_| {
            (
                404,
                r#"{"name":"NotFoundError","data":{"message":"Session not found"}}"#.into(),
            )
        });
        let error = smol::block_on(client.get_session("missing")).unwrap_err();
        assert_eq!(error.to_string(), "Session not found");
        assert!(is_not_found_error(&error));
        let http = error.downcast_ref::<OpenCodeHttpError>().unwrap();
        assert_eq!(http.name(), "NotFoundError");
        assert_eq!(http.body["name"], json!("NotFoundError"));

        host.respond_with(|_| (500, "  Permission reply failed \n".into()));
        let error =
            smol::block_on(client.reply_permission("p1", PermissionReply::Once)).unwrap_err();
        assert_eq!(error.to_string(), "Permission reply failed");
        assert!(!is_not_found_error(&error));

        host.respond_with(|_| (502, "".into()));
        let error = smol::block_on(client.reject_question("q1")).unwrap_err();
        assert_eq!(error.to_string(), "OpenCode HTTP 502");
    }

    #[test]
    fn writes_bodies_in_the_typescript_key_order() {
        let host = FakeHost::new();
        host.respond_with(|_| (204, String::new()));
        let client = OpenCodeClient::new("http://127.0.0.1:4096", "/repo", host.children());
        let input = PromptInput {
            session_id: "s1".into(),
            model: ParsedOpenCodeModelSlug {
                provider_id: "openai".into(),
                model_id: "gpt-5.4".into(),
            },
            agent: Some("build".into()),
            variant: Some(String::new()),
            parts: vec![OpenCodePromptPart::Text { text: "hi".into() }],
        };
        smol::block_on(client.prompt_async(&input)).unwrap();
        smol::block_on(client.revert_session("s1", "m1")).unwrap();
        smol::block_on(client.reply_question("q1", &[vec!["Repo".into()]])).unwrap();
        let bodies: Vec<String> = host
            .http_calls()
            .into_iter()
            .map(|call| call.body.unwrap())
            .collect();
        assert_eq!(
            bodies,
            [
                r#"{"model":{"providerID":"openai","modelID":"gpt-5.4"},"agent":"build","parts":[{"type":"text","text":"hi"}]}"#,
                r#"{"messageID":"m1"}"#,
                r#"{"answers":[["Repo"]]}"#,
            ]
        );
    }

    #[test]
    fn rejects_a_reply_that_is_not_a_session() {
        let host = FakeHost::new();
        host.respond_with(|_| (200, "<!doctype html><html></html>".into()));
        let client = OpenCodeClient::new("http://127.0.0.1:4096", "/repo", host.children());
        let error = smol::block_on(client.create_session(None, None)).unwrap_err();
        assert!(error.to_string().contains("OpenCode 1 server API"));
    }
}
