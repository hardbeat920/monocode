//! Port of src/integrations/harness/core/jsonRpc.ts: a bidirectional
//! JSON-RPC client over one harness child's stdin and stdout, one JSON
//! message per line. Request ids are numbers or strings; the Codex app server
//! omits the `jsonrpc` field.
//!
//! The client does not read stdout itself. The adapter feeds it each line
//! with [`JsonRpcClient::push_line`], usually from a [`ChildEvent`] pump.
//!
//! [`ChildEvent`]: super::child::ChildEvent

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::child::Children;
use super::json_text::js_string;
use super::task::{self, BoxFuture};

/// `JsonRpcId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

impl JsonRpcId {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Number(number) => match number.as_i64() {
                Some(id) => JsonRpcId::Number(id),
                None => JsonRpcId::String(js_string(value)),
            },
            Value::String(id) => JsonRpcId::String(id.clone()),
            other => JsonRpcId::String(js_string(other)),
        }
    }

    /// `String(id)`, the pending-map key.
    fn key(&self) -> String {
        match self {
            JsonRpcId::Number(id) => id.to_string(),
            JsonRpcId::String(id) => id.clone(),
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "null".into())
    }
}

impl fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

impl From<i64> for JsonRpcId {
    fn from(id: i64) -> Self {
        JsonRpcId::Number(id)
    }
}

impl From<&str> for JsonRpcId {
    fn from(id: &str) -> Self {
        JsonRpcId::String(id.to_string())
    }
}

/// `JsonRpcMessage`, as parsed from one line. Absent fields are `None`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JsonRpcMessage {
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

impl JsonRpcMessage {
    /// Read the fields the client looks at. `null` counts as absent for
    /// `id`, as `msg.id != null` did, but `result: null` is a result.
    pub fn from_value(value: &Value) -> Self {
        let Value::Object(map) = value else {
            return Self::default();
        };
        Self {
            jsonrpc: map
                .get("jsonrpc")
                .and_then(Value::as_str)
                .map(str::to_string),
            id: map.get("id").filter(|id| !id.is_null()).cloned(),
            method: map
                .get("method")
                .and_then(Value::as_str)
                .filter(|method| !method.is_empty())
                .map(str::to_string),
            params: map.get("params").cloned(),
            result: map.get("result").cloned(),
            error: map.get("error").cloned(),
        }
    }
}

/// A JSON-RPC error response. The structured `code` and `data` stay, because
/// some agents (Factory Droid) put the user-facing detail in `data` behind a
/// generic `message`. Recover it with `error.downcast_ref::<RpcError>()`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub message: String,
    pub code: Option<i64>,
    pub data: Option<Value>,
}

/// The error object of `respondError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcErrorBody {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JavaScript truthiness of a parsed JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

type NotificationHandler = Arc<dyn Fn(&str, Value) + Send + Sync>;
type RequestHandler = Arc<dyn Fn(JsonRpcId, &str, Value) + Send + Sync>;

/// `JsonRpcHandlers`. Handlers run synchronously inside `push_line`; one that
/// needs to await (to respond, say) spawns its own task. `params` is
/// `Value::Null` when the message had none.
#[derive(Clone, Default)]
pub struct JsonRpcHandlers {
    pub on_notification: Option<NotificationHandler>,
    pub on_request: Option<RequestHandler>,
}

impl JsonRpcHandlers {
    pub fn on_notification(
        mut self,
        handler: impl Fn(&str, Value) + Send + Sync + 'static,
    ) -> Self {
        self.on_notification = Some(Arc::new(handler));
        self
    }

    pub fn on_request(
        mut self,
        handler: impl Fn(JsonRpcId, &str, Value) + Send + Sync + 'static,
    ) -> Self {
        self.on_request = Some(Arc::new(handler));
        self
    }
}

/// Writes one line to a session's child. [`Children`] is the real one.
pub trait LineWriter: Send + Sync {
    fn write_line(&self, session_id: &str, line: String) -> BoxFuture<'static, Result<()>>;
}

impl LineWriter for Children {
    fn write_line(&self, session_id: &str, line: String) -> BoxFuture<'static, Result<()>> {
        let children = self.clone();
        let session_id = session_id.to_string();
        Box::pin(async move { children.write_child(&session_id, &line).await })
    }
}

/// `WRITE_TIMEOUT_MS`. A child wedged hard enough to block stdin writes is
/// unrecoverable; the write deadline fails the request so the caller can
/// recycle the generation.
pub const WRITE_TIMEOUT_MS: i64 = 15_000;

/// `JsonRpcClientOptions`.
#[derive(Debug, Clone)]
pub struct JsonRpcClientOptions {
    /// Include `"jsonrpc":"2.0"` on outbound messages. Default true (ACP).
    /// Codex omits it.
    pub include_jsonrpc: bool,
    pub label: String,
    /// Defaults to [`WRITE_TIMEOUT_MS`]. Tests shorten it.
    pub write_timeout: Duration,
}

impl Default for JsonRpcClientOptions {
    fn default() -> Self {
        Self {
            include_jsonrpc: true,
            label: "rpc".into(),
            write_timeout: task::ms(WRITE_TIMEOUT_MS),
        }
    }
}

enum Failure {
    Rpc(RpcError),
    Other(String),
}

impl Failure {
    fn into_error(self) -> anyhow::Error {
        match self {
            Failure::Rpc(error) => anyhow::Error::new(error),
            Failure::Other(message) => anyhow!(message),
        }
    }
}

type Pending = oneshot::Sender<std::result::Result<Value, Failure>>;

struct Inner {
    session_id: String,
    handlers: JsonRpcHandlers,
    options: JsonRpcClientOptions,
    writer: Arc<dyn LineWriter>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<String, Pending>>,
    closed: AtomicBool,
}

/// `JsonRpcClient`. Clones share one connection.
#[derive(Clone)]
pub struct JsonRpcClient {
    inner: Arc<Inner>,
}

impl JsonRpcClient {
    pub fn new(
        session_id: &str,
        writer: Arc<dyn LineWriter>,
        handlers: JsonRpcHandlers,
        options: JsonRpcClientOptions,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                session_id: session_id.to_string(),
                handlers,
                options,
                writer,
                next_id: AtomicI64::new(1),
                pending: Mutex::new(HashMap::new()),
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    /// `pushLine`: one line of the child's stdout.
    pub fn push_line(&self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            let preview: String = trimmed.chars().take(200).collect();
            log::debug!(
                "[{} {}] non-json {preview}",
                self.inner.options.label,
                self.inner.session_id
            );
            return;
        };
        self.handle(JsonRpcMessage::from_value(&value));
    }

    /// `close`. Rejects every pending request with `error`, or "Harness
    /// process exited".
    pub fn close(&self, error: Option<&str>) {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.reject_pending(Some(error.unwrap_or("Harness process exited")));
    }

    /// `rejectPending`: drop in-flight client requests so a cancelled turn
    /// can unwind. The default error is "cancelled".
    pub fn reject_pending(&self, error: Option<&str>) {
        let message = error.unwrap_or("cancelled").to_string();
        let pending: Vec<Pending> = self.inner.pending.lock().drain().map(|(_, p)| p).collect();
        for entry in pending {
            let _ = entry.send(Err(Failure::Other(message.clone())));
        }
    }

    /// `isClosed`.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    fn envelope(&self, fields: &str) -> String {
        if self.inner.options.include_jsonrpc {
            format!("{{\"jsonrpc\":\"2.0\",{fields}}}")
        } else {
            format!("{{{fields}}}")
        }
    }

    fn json(value: &impl Serialize) -> String {
        serde_json::to_string(value).unwrap_or_else(|_| "null".into())
    }

    /// `request`, returning the raw `result`. `timeout_ms` of 0 waits forever.
    pub async fn request_value(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_ms: i64,
    ) -> Result<Value> {
        if self.is_closed() {
            return Err(anyhow!("Harness process is not running"));
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let key = id.to_string();
        let deadline = (timeout_ms > 0).then(|| Instant::now() + task::ms(timeout_ms));
        // Register before writing. A local harness can answer before the
        // write returns; adding the pending entry after the send would drop
        // that response.
        let (tx, mut rx) = oneshot::channel();
        self.inner.pending.lock().insert(key.clone(), tx);

        let mut fields = format!("\"id\":{id},\"method\":{}", Self::json(&method));
        if let Some(params) = &params {
            fields.push_str(&format!(",\"params\":{}", Self::json(params)));
        }
        let line = self.envelope(&fields);
        let timed_out = || anyhow!("{method} timed out");
        let expired = |deadline: Option<Instant>| deadline.is_some_and(|at| Instant::now() >= at);

        if let Err(error) = self.send(line).await {
            let still_pending = self.inner.pending.lock().remove(&key).is_some();
            if still_pending {
                // The request deadline may have passed while the write was
                // blocked. The TypeScript reported that deadline first.
                return Err(if expired(deadline) {
                    timed_out()
                } else {
                    error
                });
            }
        }

        let response = match deadline {
            None => (&mut rx).await.ok(),
            Some(at) => {
                let remaining = at.saturating_duration_since(Instant::now());
                match task::timeout(remaining, &mut rx).await {
                    Some(result) => result.ok(),
                    None => {
                        self.inner.pending.lock().remove(&key);
                        // A response can land between the timer and the removal.
                        match rx.try_recv() {
                            Ok(Some(result)) => Some(result),
                            _ => return Err(timed_out()),
                        }
                    }
                }
            }
        };
        match response {
            Some(Ok(value)) => Ok(value),
            Some(Err(failure)) => Err(failure.into_error()),
            // The client dropped the entry without settling it.
            None => Err(if expired(deadline) {
                timed_out()
            } else {
                anyhow!("cancelled")
            }),
        }
    }

    /// `request<T>`: the `result` deserialized as `T`.
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_ms: i64,
    ) -> Result<T> {
        let value = self.request_value(method, params, timeout_ms).await?;
        serde_json::from_value(value)
            .map_err(|error| anyhow!("{method} returned an unexpected result: {error}"))
    }

    /// `notify`. Does nothing once the client is closed.
    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        if self.is_closed() {
            return Ok(());
        }
        let mut fields = format!("\"method\":{}", Self::json(&method));
        if let Some(params) = &params {
            fields.push_str(&format!(",\"params\":{}", Self::json(params)));
        }
        self.send(self.envelope(&fields)).await
    }

    /// `respond`.
    pub async fn respond(&self, id: JsonRpcId, result: Value) -> Result<()> {
        let fields = format!("\"id\":{},\"result\":{}", id.to_json(), Self::json(&result));
        self.send(self.envelope(&fields)).await
    }

    /// `respondError`.
    pub async fn respond_error(&self, id: JsonRpcId, error: RpcErrorBody) -> Result<()> {
        let fields = format!("\"id\":{},\"error\":{}", id.to_json(), Self::json(&error));
        self.send(self.envelope(&fields)).await
    }

    /// Bound the write: a child that stops draining stdin must not let a
    /// blocked write outlive the request's own deadline, or wedge a
    /// cancellation waiting on the `session/cancel` notify.
    async fn send(&self, line: String) -> Result<()> {
        let write = self.inner.writer.write_line(&self.inner.session_id, line);
        match task::timeout(self.inner.options.write_timeout, write).await {
            Some(result) => result,
            None => Err(anyhow!("harness write timed out")),
        }
    }

    fn handle(&self, msg: JsonRpcMessage) {
        let has_error = msg.error.as_ref().is_some_and(truthy);
        if let Some(id) = &msg.id
            && (msg.result.is_some() || has_error)
        {
            let key = JsonRpcId::from_value(id).key();
            let Some(pending) = self.inner.pending.lock().remove(&key) else {
                return;
            };
            if has_error {
                let error = msg.error.unwrap_or(Value::Null);
                let code = error.get("code").and_then(|code| {
                    code.as_i64()
                        .or_else(|| code.as_f64().map(|value| value as i64))
                });
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .filter(|message| !message.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        let code = error.get("code").map(js_string).unwrap_or_default();
                        format!("{} error {code}", self.inner.options.label)
                    });
                let data = error.get("data").cloned();
                let _ = pending.send(Err(Failure::Rpc(RpcError {
                    message,
                    code,
                    data,
                })));
                return;
            }
            let _ = pending.send(Ok(msg.result.unwrap_or(Value::Null)));
            return;
        }

        if let (Some(method), Some(id)) = (&msg.method, &msg.id) {
            if let Some(on_request) = &self.inner.handlers.on_request {
                on_request(
                    JsonRpcId::from_value(id),
                    method,
                    msg.params.unwrap_or(Value::Null),
                );
            }
            return;
        }

        if let Some(method) = &msg.method
            && let Some(on_notification) = &self.inner.handlers.on_notification
        {
            on_notification(method, msg.params.unwrap_or(Value::Null));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    type WriteFn = dyn Fn(&str, String) -> BoxFuture<'static, Result<()>> + Send + Sync;

    struct FnWriter(Box<WriteFn>);

    impl LineWriter for FnWriter {
        fn write_line(&self, session_id: &str, line: String) -> BoxFuture<'static, Result<()>> {
            (self.0)(session_id, line)
        }
    }

    fn writer(
        write: impl Fn(&str, String) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    ) -> Arc<dyn LineWriter> {
        Arc::new(FnWriter(Box::new(write)))
    }

    fn options(write_timeout_ms: i64) -> JsonRpcClientOptions {
        JsonRpcClientOptions {
            write_timeout: task::ms(write_timeout_ms),
            ..Default::default()
        }
    }

    #[test]
    fn accepts_a_response_delivered_before_the_write_resolves() {
        let client_cell: Arc<OnceLock<JsonRpcClient>> = Arc::new(OnceLock::new());
        let cell = client_cell.clone();
        let client = JsonRpcClient::new(
            "fast",
            writer(move |_, line| {
                let outbound: Value = serde_json::from_str(&line).unwrap();
                let id = outbound["id"].as_i64().unwrap();
                cell.get().unwrap().push_line(
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "ok": true } })
                        .to_string(),
                );
                Box::pin(async { Ok(()) })
            }),
            JsonRpcHandlers::default(),
            JsonRpcClientOptions::default(),
        );
        let _ = client_cell.set(client.clone());
        let result = smol::block_on(client.request_value("session/set_mode", None, 0)).unwrap();
        assert_eq!(result, serde_json::json!({ "ok": true }));
    }

    #[test]
    fn rejects_and_removes_a_request_when_writing_fails() {
        let client = JsonRpcClient::new(
            "failed",
            writer(|_, _| Box::pin(async { Err(anyhow!("pipe closed")) })),
            JsonRpcHandlers::default(),
            JsonRpcClientOptions::default(),
        );
        let error = smol::block_on(client.request_value("initialize", None, 0)).unwrap_err();
        assert_eq!(error.to_string(), "pipe closed");
        assert!(client.inner.pending.lock().is_empty());
    }

    #[test]
    fn bounds_a_blocked_write_instead_of_outliving_the_request_deadline() {
        let client = JsonRpcClient::new(
            "wedged",
            writer(|_, _| Box::pin(futures::future::pending())),
            JsonRpcHandlers::default(),
            options(30),
        );
        let started = Instant::now();
        let error = smol::block_on(client.request_value("initialize", None, 60_000)).unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn reports_a_slow_request_deadline_once_the_blocked_write_returns() {
        let client = JsonRpcClient::new(
            "quiet",
            writer(|_, _| Box::pin(futures::future::pending())),
            JsonRpcHandlers::default(),
            options(80),
        );
        let started = Instant::now();
        let error = smol::block_on(client.request_value("initialize", None, 20)).unwrap_err();
        // The request deadline passes while the write is blocked; the call only
        // settles once the write bound returns it.
        assert!(started.elapsed() >= Duration::from_millis(80));
        assert_eq!(error.to_string(), "initialize timed out");
    }

    #[test]
    fn keeps_error_code_and_data() {
        let cell: Arc<OnceLock<JsonRpcClient>> = Arc::new(OnceLock::new());
        let inner = cell.clone();
        let client = JsonRpcClient::new(
            "droid",
            writer(move |_, line| {
                let outbound: Value = serde_json::from_str(&line).unwrap();
                inner.get().unwrap().push_line(
                    &serde_json::json!({
                        "jsonrpc": "2.0", "id": outbound["id"],
                        "error": { "code": -32000, "message": "", "data": { "detail": "quota" } }
                    })
                    .to_string(),
                );
                Box::pin(async { Ok(()) })
            }),
            JsonRpcHandlers::default(),
            JsonRpcClientOptions {
                label: "acp".into(),
                ..Default::default()
            },
        );
        let _ = cell.set(client.clone());
        let error = smol::block_on(client.request_value("session/prompt", None, 0)).unwrap_err();
        let rpc = error.downcast_ref::<RpcError>().unwrap();
        assert_eq!(rpc.message, "acp error -32000");
        assert_eq!(rpc.code, Some(-32000));
        assert_eq!(rpc.data, Some(serde_json::json!({ "detail": "quota" })));
    }

    #[test]
    fn writes_typescript_key_order_and_routes_inbound_messages() {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let notes = seen.clone();
        let requests = seen.clone();
        let client = JsonRpcClient::new(
            "s",
            writer(move |_, line| {
                sink.lock().push(line);
                Box::pin(async { Ok(()) })
            }),
            JsonRpcHandlers::default()
                .on_notification(move |method, params| {
                    notes.lock().push(format!("note {method} {params}"))
                })
                .on_request(move |id, method, _| {
                    requests.lock().push(format!("req {id} {method}"))
                }),
            JsonRpcClientOptions {
                include_jsonrpc: false,
                ..Default::default()
            },
        );
        smol::block_on(async {
            client.notify("initialized", None).await.unwrap();
            client
                .respond(JsonRpcId::from("abc"), serde_json::json!({ "ok": 1 }))
                .await
                .unwrap();
            client
                .respond_error(
                    7.into(),
                    RpcErrorBody {
                        code: -1,
                        message: "no".into(),
                        data: None,
                    },
                )
                .await
                .unwrap();
        });
        assert_eq!(
            *lines.lock(),
            vec![
                r#"{"method":"initialized"}"#.to_string(),
                r#"{"id":"abc","result":{"ok":1}}"#.to_string(),
                r#"{"id":7,"error":{"code":-1,"message":"no"}}"#.to_string(),
            ]
        );
        client.push_line("not json");
        client.push_line("  ");
        client.push_line(r#"{"method":"session/update","params":{"a":1}}"#);
        client.push_line(r#"{"id":3,"method":"session/request_permission"}"#);
        // A response nobody waits for is ignored.
        client.push_line(r#"{"id":99,"result":null}"#);
        assert_eq!(
            *seen.lock(),
            vec![
                "note session/update {\"a\":1}".to_string(),
                "req 3 session/request_permission".to_string(),
            ]
        );
    }

    #[test]
    fn close_rejects_pending_and_refuses_new_requests() {
        let client = JsonRpcClient::new(
            "closing",
            writer(|_, _| Box::pin(async { Ok(()) })),
            JsonRpcHandlers::default(),
            JsonRpcClientOptions::default(),
        );
        smol::block_on(async {
            let waiting = {
                let client = client.clone();
                smol::spawn(async move { client.request_value("slow", None, 0).await })
            };
            while client.inner.pending.lock().is_empty() {
                smol::future::yield_now().await;
            }
            client.close(None);
            assert_eq!(
                waiting.await.unwrap_err().to_string(),
                "Harness process exited"
            );
            assert!(client.is_closed());
            let error = client.request_value("again", None, 0).await.unwrap_err();
            assert_eq!(error.to_string(), "Harness process is not running");
            client.notify("ignored", None).await.unwrap();
        });
    }
}
