//! Port of src/integrations/harness/core/acp.ts: the ACP JSON-RPC client, a
//! thin wrapper over [`JsonRpcClient`] that keeps the numeric request ids the
//! Cursor adapter was written against.

use std::sync::Arc;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::json_rpc::{
    JsonRpcClient, JsonRpcClientOptions, JsonRpcHandlers, JsonRpcId, LineWriter, RpcErrorBody,
};

pub use super::json_rpc::JsonRpcMessage;

type NotificationHandler = Arc<dyn Fn(&str, Value) + Send + Sync>;
type RequestHandler = Arc<dyn Fn(i64, &str, Value) + Send + Sync>;

/// `AcpHandlers`.
#[derive(Clone, Default)]
pub struct AcpHandlers {
    pub on_notification: Option<NotificationHandler>,
    pub on_request: Option<RequestHandler>,
}

impl AcpHandlers {
    pub fn on_notification(
        mut self,
        handler: impl Fn(&str, Value) + Send + Sync + 'static,
    ) -> Self {
        self.on_notification = Some(Arc::new(handler));
        self
    }

    pub fn on_request(
        mut self,
        handler: impl Fn(i64, &str, Value) + Send + Sync + 'static,
    ) -> Self {
        self.on_request = Some(Arc::new(handler));
        self
    }
}

/// `Number(id)` for an inbound request id.
fn numeric_id(id: &JsonRpcId) -> i64 {
    match id {
        JsonRpcId::Number(id) => *id,
        JsonRpcId::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return 0;
            }
            // TODO(port): `Number("abc")` is NaN in TypeScript, which then
            // serialized as `null` in a response. This reads it as 0.
            text.parse::<i64>()
                .ok()
                .or_else(|| text.parse::<f64>().ok().map(|value| value as i64))
                .unwrap_or(0)
        }
    }
}

/// `AcpClient`. Clones share one connection.
#[derive(Clone)]
pub struct AcpClient {
    rpc: JsonRpcClient,
}

impl AcpClient {
    pub fn new(session_id: &str, writer: Arc<dyn LineWriter>, handlers: AcpHandlers) -> Self {
        Self::with_options(
            session_id,
            writer,
            handlers,
            JsonRpcClientOptions::default(),
        )
    }

    /// Like [`AcpClient::new`] with explicit client options. The label is
    /// always `acp` and `jsonrpc` is always sent.
    pub fn with_options(
        session_id: &str,
        writer: Arc<dyn LineWriter>,
        handlers: AcpHandlers,
        options: JsonRpcClientOptions,
    ) -> Self {
        let mut rpc_handlers = JsonRpcHandlers::default();
        if let Some(on_notification) = handlers.on_notification {
            rpc_handlers.on_notification = Some(on_notification);
        }
        if let Some(on_request) = handlers.on_request {
            rpc_handlers.on_request = Some(Arc::new(move |id: JsonRpcId, method: &str, params| {
                on_request(numeric_id(&id), method, params)
            }));
        }
        let rpc = JsonRpcClient::new(
            session_id,
            writer,
            rpc_handlers,
            JsonRpcClientOptions {
                include_jsonrpc: true,
                label: "acp".into(),
                ..options
            },
        );
        Self { rpc }
    }

    /// The underlying client.
    pub fn rpc(&self) -> &JsonRpcClient {
        &self.rpc
    }

    pub fn push_line(&self, line: &str) {
        self.rpc.push_line(line);
    }

    pub fn close(&self, error: Option<&str>) {
        self.rpc.close(error);
    }

    pub fn reject_pending(&self, error: Option<&str>) {
        self.rpc.reject_pending(error);
    }

    pub fn is_closed(&self) -> bool {
        self.rpc.is_closed()
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_ms: i64,
    ) -> Result<T> {
        self.rpc.request(method, params, timeout_ms).await
    }

    pub async fn request_value(
        &self,
        method: &str,
        params: Option<Value>,
        timeout_ms: i64,
    ) -> Result<Value> {
        self.rpc.request_value(method, params, timeout_ms).await
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        self.rpc.notify(method, params).await
    }

    pub async fn respond(&self, id: i64, result: Value) -> Result<()> {
        self.rpc.respond(JsonRpcId::Number(id), result).await
    }

    pub async fn respond_error(&self, id: i64, error: RpcErrorBody) -> Result<()> {
        self.rpc.respond_error(JsonRpcId::Number(id), error).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::task::BoxFuture;
    use parking_lot::Mutex;

    struct Recorder(Mutex<Vec<String>>);

    impl LineWriter for Recorder {
        fn write_line(&self, _session_id: &str, line: String) -> BoxFuture<'static, Result<()>> {
            self.0.lock().push(line);
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn sends_jsonrpc_and_hands_numeric_request_ids() {
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let ids = Arc::new(Mutex::new(Vec::new()));
        let seen = ids.clone();
        let client = AcpClient::new(
            "acp-1",
            recorder.clone(),
            AcpHandlers::default()
                .on_request(move |id, method, _| seen.lock().push((id, method.to_string()))),
        );
        client.push_line(r#"{"jsonrpc":"2.0","id":"12","method":"session/request_permission"}"#);
        client.push_line(r#"{"jsonrpc":"2.0","id":4,"method":"fs/read_text_file"}"#);
        assert_eq!(
            *ids.lock(),
            vec![
                (12, "session/request_permission".to_string()),
                (4, "fs/read_text_file".to_string())
            ]
        );
        smol::block_on(client.respond(12, serde_json::json!({ "outcome": "allow" }))).unwrap();
        smol::block_on(client.notify(
            "session/cancel",
            Some(serde_json::json!({ "sessionId": "x" })),
        ))
        .unwrap();
        assert_eq!(
            *recorder.0.lock(),
            vec![
                r#"{"jsonrpc":"2.0","id":12,"result":{"outcome":"allow"}}"#.to_string(),
                r#"{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"x"}}"#
                    .to_string(),
            ]
        );
    }
}
