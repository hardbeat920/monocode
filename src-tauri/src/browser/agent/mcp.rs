//! `monocode browser-mcp`: a hand-written MCP stdio server (spec 12.1,
//! decision D9). JSON-RPC 2.0, one message per line; `initialize`, `ping`,
//! `tools/list`, `tools/call` and the `initialized` / `cancelled`
//! notifications. No resources, no prompts, no state beyond credentials.
//!
//! Tool calls run on their own threads so a long `navigate` neither blocks
//! `ping` nor other tabs; responses share one locked stdout. Logs go to
//! stderr only: stdout carries protocol messages and nothing else.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::client::{self, Credentials, MissingCredentials};
use super::tools;
use crate::browser::protocol::{ActionKind, ActionResult, Outcome, Reason};

/// Newest first; an unknown client version gets the first entry.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const SERVER_NAME: &str = "monocode-browser";
/// Calls beyond this many at once are answered `busy` without being sent;
/// the host allows 4 concurrent actions per session anyway.
const MAX_IN_FLIGHT: usize = 16;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub struct Server {
    credentials: Result<Credentials, MissingCredentials>,
    out: Mutex<Box<dyn Write + Send>>,
    in_flight: AtomicUsize,
    /// Worker threads that have not written their reply yet.
    workers: AtomicUsize,
    cancelled: Mutex<HashSet<String>>,
}

pub fn run() -> i32 {
    let server = Arc::new(Server::new(
        Credentials::from_env(),
        Box::new(std::io::stdout()),
    ));
    if let Err(missing) = &server.credentials {
        eprintln!("monocode browser-mcp: {}", missing.message());
    }
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        Server::dispatch(&server, &line);
    }
    // Clients may close stdin right after their last call; answer the calls
    // still running before exiting (each ends within its reply timeout).
    while server.workers.load(Ordering::SeqCst) > 0 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    0
}

impl Server {
    pub fn new(
        credentials: Result<Credentials, MissingCredentials>,
        out: Box<dyn Write + Send>,
    ) -> Server {
        Server {
            credentials,
            out: Mutex::new(out),
            in_flight: AtomicUsize::new(0),
            workers: AtomicUsize::new(0),
            cancelled: Mutex::new(HashSet::new()),
        }
    }

    /// Handle one input line: answer cheap messages inline, run tool calls
    /// (and batches, which may contain them) on a worker thread.
    pub fn dispatch(server: &Arc<Server>, line: &str) {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(_) => {
                server.write(&error(Value::Null, PARSE_ERROR, "Parse error"));
                return;
            }
        };
        let slow = match &message {
            Value::Array(_) => true,
            message => message["method"] == "tools/call",
        };
        if !slow {
            if let Some(reply) = server.handle(&message) {
                server.write(&reply);
            }
            return;
        }
        let server = Arc::clone(server);
        server.workers.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            if let Some(reply) = server.handle(&message) {
                server.write(&reply);
            }
            server.workers.fetch_sub(1, Ordering::SeqCst);
        });
    }

    /// The reply to one message or batch, if it needs one. Synchronous; used
    /// directly by tests.
    pub fn handle(&self, message: &Value) -> Option<Value> {
        if let Value::Array(batch) = message {
            if batch.is_empty() {
                return Some(error(Value::Null, INVALID_REQUEST, "Empty batch"));
            }
            let replies: Vec<Value> = batch.iter().filter_map(|m| self.handle_one(m)).collect();
            return (!replies.is_empty()).then_some(Value::Array(replies));
        }
        self.handle_one(message)
    }

    fn handle_one(&self, message: &Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error(Value::Null, INVALID_REQUEST, "Invalid Request"));
        };
        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to a request we never send, or garbage: a reply
            // only if it carries an id we could address.
            return match id {
                Some(id) if !object.contains_key("result") && !object.contains_key("error") => {
                    Some(error(id, INVALID_REQUEST, "Invalid Request"))
                }
                _ => None,
            };
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            self.notification(method, &params);
            return None;
        };
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tools::TOOLS.into_iter().map(tools::definition).collect::<Vec<_>>()
            })),
            "tools/call" => self.call_tool(&params),
            "resources/list" => Ok(json!({"resources": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            _ => Err((METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        };
        let reply = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => error(id, code, &message),
        };
        // A cancelled request gets no response (MCP cancellation).
        if self.cancelled.lock().ok()?.remove(&id_key(&reply["id"])) {
            return None;
        }
        Some(reply)
    }

    fn notification(&self, method: &str, params: &Value) {
        if method == "notifications/cancelled" {
            if let (Some(id), Ok(mut cancelled)) = (params.get("requestId"), self.cancelled.lock())
            {
                // Ids of calls that finished first are never removed; bound them.
                if cancelled.len() >= 1024 {
                    cancelled.clear();
                }
                cancelled.insert(id_key(id));
            }
        }
        // notifications/initialized and anything else: nothing to do.
    }

    fn call_tool(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_string()))?;
        if tools::find(name).is_none() {
            return Err((INVALID_PARAMS, format!("Unknown tool: {name}")));
        }
        let mapped = match tools::map_call(name, params.get("arguments")) {
            Ok(mapped) => mapped,
            Err(message) => {
                let result =
                    ActionResult::from_reason(&client::new_request_id(), Reason::InvalidRequest)
                        .with_message(message);
                return Ok(tool_result(None, &result));
            }
        };
        let request_id = mapped.request_id.unwrap_or_else(client::new_request_id);
        let result = match &self.credentials {
            Err(missing) => client::without_credentials(&request_id, missing),
            Ok(credentials) => {
                if self.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                    self.in_flight.fetch_sub(1, Ordering::SeqCst);
                    ActionResult::from_reason(&request_id, Reason::Busy)
                } else {
                    let result =
                        client::call(credentials, mapped.kind.name(), &mapped.input, &request_id);
                    self.in_flight.fetch_sub(1, Ordering::SeqCst);
                    result
                }
            }
        };
        Ok(tool_result(Some(mapped.kind), &result))
    }

    fn write(&self, message: &Value) {
        let Ok(mut out) = self.out.lock() else { return };
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|version| PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "MonoCode browser",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": tools::section("overview").unwrap_or_default()
    })
}

/// Map one action result onto an MCP `CallToolResult` (spec 12.4).
///
/// - done: the message and the compact envelope; snapshot and read put their
///   text first, screenshot adds an image block, and the page snapshot of
///   an action follows the message as its own text block.
/// - blocked, failed, unknown: `isError`, with the reason and whether a retry
///   can help spelled out so the model need not parse JSON.
pub fn tool_result(kind: Option<ActionKind>, result: &ActionResult) -> Value {
    let mut content = Vec::new();
    let mut envelope = result.clone();
    if result.outcome == Outcome::Done {
        match kind {
            Some(
                ActionKind::Snapshot | ActionKind::Read | ActionKind::Console | ActionKind::Network,
            ) => {
                if let Some(text) = client::data_text(result.data.as_ref()) {
                    content.push(json!({"type": "text", "text": text}));
                    envelope.data = client::data_without(result.data.as_ref(), &client::TEXT_KEYS);
                }
            }
            Some(ActionKind::Screenshot) => {
                if let Some((image, mime)) = client::data_image(result.data.as_ref()) {
                    content.push(json!({"type": "image", "data": image, "mimeType": mime}));
                    envelope.data = client::data_without(result.data.as_ref(), &client::IMAGE_KEYS);
                }
            }
            // An action's page snapshot follows the summary as its own text.
            _ => {
                if let Some(text) = client::data_snapshot(result.data.as_ref()) {
                    content.push(json!({"type": "text", "text": text}));
                    envelope.data = client::data_without(result.data.as_ref(), &["snapshot"]);
                }
            }
        }
        let summary = format!("{}\n{}", result.message, compact(&envelope));
        // The text the agent asked for leads; the envelope follows it. An
        // image goes after the text that explains it.
        match kind {
            Some(
                ActionKind::Snapshot | ActionKind::Read | ActionKind::Console | ActionKind::Network,
            ) => content.push(json!({"type": "text", "text": summary})),
            _ => content.insert(0, json!({"type": "text", "text": summary})),
        }
        return json!({"content": content, "isError": false});
    }
    let reason = result.reason.map(Reason::as_str).unwrap_or("none");
    let advice = match (result.outcome, result.retryable) {
        (Outcome::Unknown, _) => format!(
            "The action may have happened: take a snapshot before retrying, and retry only with request_id {}.",
            result.request_id
        ),
        (_, true) => "Retrying can help once the cause is gone.".to_string(),
        (_, false) => "Retrying the same call will not help.".to_string(),
    };
    let text = format!(
        "{}\noutcome: {}; reason: {}; retryable: {}. {}\n{}",
        result.message,
        result.outcome.as_str(),
        reason,
        result.retryable,
        advice,
        compact(&envelope)
    );
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

/// The envelope on one line, without the message (already the first line of
/// the text) and without empty parts.
fn compact(result: &ActionResult) -> String {
    let mut value = serde_json::to_value(result).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.retain(|key, value| match key.as_str() {
            "message" => false,
            "data" | "reason" => !value.is_null(),
            "observed" => value
                .as_object()
                .is_some_and(|o| o.iter().any(|(_, v)| v != &json!(false))),
            _ => true,
        });
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::agent::client::tests::fake_control;
    use crate::browser::protocol::{LoadState, Observed};

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Sink {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    fn server(credentials: Result<Credentials, MissingCredentials>) -> (Arc<Server>, Sink) {
        let sink = Sink::default();
        (
            Arc::new(Server::new(credentials, Box::new(sink.clone()))),
            sink,
        )
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    #[test]
    fn negotiates_the_protocol_and_lists_every_tool() {
        let (server, _) = server(Err(MissingCredentials::Absent));
        let reply = server
            .handle(&request(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}})))
            .unwrap();
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(
            reply["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
        assert!(reply["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("snapshot"));
        let future = server
            .handle(&request(
                2,
                "initialize",
                json!({"protocolVersion": "2099-01-01"}),
            ))
            .unwrap();
        assert_eq!(future["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);

        assert!(server
            .handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .is_none());
        assert_eq!(
            server.handle(&request(3, "ping", Value::Null)).unwrap()["result"],
            json!({})
        );
        let list = server.handle(&request(4, "tools/list", json!({}))).unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), tools::TOOLS.len());
        assert!(names.contains(&"browser_click") && names.contains(&"browser_tabs"));
    }

    #[test]
    fn protocol_errors_follow_json_rpc() {
        let (server, sink) = server(Err(MissingCredentials::Absent));
        Server::dispatch(&server, "{not json");
        assert_eq!(sink.lines()[0]["error"]["code"], PARSE_ERROR);
        assert_eq!(
            server
                .handle(&request(5, "sampling/createMessage", json!({})))
                .unwrap()["error"]["code"],
            METHOD_NOT_FOUND
        );
        assert_eq!(
            server
                .handle(&request(
                    6,
                    "tools/call",
                    json!({"name": "browser_evaluate"})
                ))
                .unwrap()["error"]["code"],
            INVALID_PARAMS
        );
        assert_eq!(
            server.handle(&json!(42)).unwrap()["error"]["code"],
            INVALID_REQUEST
        );
        // Responses to requests we never sent are ignored.
        assert!(server
            .handle(&json!({"jsonrpc": "2.0", "id": 9, "result": {}}))
            .is_none());
        let batch = server
            .handle(&json!([
                request(7, "ping", Value::Null),
                {"jsonrpc": "2.0", "method": "notifications/initialized"}
            ]))
            .unwrap();
        assert_eq!(batch.as_array().unwrap().len(), 1);
        assert_eq!(batch[0]["id"], 7);
    }

    #[test]
    fn without_credentials_tools_stay_listed_and_answer_not_granted() {
        let (server, _) = server(Err(MissingCredentials::Absent));
        let reply = server
            .handle(&request(
                8,
                "tools/call",
                json!({"name": "browser_tabs", "arguments": {"action": "list"}}),
            ))
            .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("reason: not_granted"), "{text}");
        assert!(text.contains("retryable: false"));
    }

    #[test]
    fn calls_reach_the_host_with_the_mapped_action_and_the_callers_request_id() {
        let (credentials, sent) = fake_control(|request| {
            let id = request["requestId"].as_str().unwrap();
            let result = ActionResult::done(id, "Navigated.").with_observed(Observed {
                url: Some("https://example.com/".into()),
                navigated: true,
                load_state: Some(LoadState::Load),
                ..Observed::default()
            });
            Some(json!({"ok": true, "result": result}).to_string())
        });
        let (server, _) = server(Ok(credentials));
        let reply = server
            .handle(&request(
                9,
                "tools/call",
                json!({"name": "browser_back", "arguments": {"tab": "t_0a1b2c3d", "direction": "forward", "request_id": "retry-7"}}),
            ))
            .unwrap();
        let forwarded = sent.recv().unwrap();
        assert_eq!(forwarded["action"], "forward");
        assert_eq!(forwarded["requestId"], "retry-7");
        assert_eq!(forwarded["input"], json!({"tab": "t_0a1b2c3d"}));
        assert_eq!(reply["result"]["isError"], false);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Navigated.\n"));
        let envelope: Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
        assert_eq!(envelope["observed"]["url"], "https://example.com/");
        assert_eq!(envelope["request_id"], "retry-7");

        // Without a request_id each call gets a fresh one.
        let _ = server.handle(&request(
            10,
            "tools/call",
            json!({"name": "browser_tabs", "arguments": {"action": "list"}}),
        ));
        let generated = sent.recv().unwrap();
        assert_eq!(generated["action"], "tabs.list");
        assert!(generated["requestId"].as_str().unwrap().starts_with("req_"));
    }

    #[test]
    fn bad_arguments_are_tool_errors_not_protocol_errors() {
        let (server, _) = server(Err(MissingCredentials::Absent));
        let reply = server
            .handle(&request(
                11,
                "tools/call",
                json!({"name": "browser_tabs", "arguments": {"action": "steal"}}),
            ))
            .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("invalid_request"));
    }

    #[test]
    fn snapshot_text_and_screenshot_images_become_their_own_content_blocks() {
        let snapshot = ActionResult::done("r1", "Snapshot taken.")
            .with_data(json!({"text": "- button \"Go\" [ref=e1]", "version": 2}));
        let mapped = tool_result(Some(ActionKind::Snapshot), &snapshot);
        assert_eq!(mapped["content"][0]["text"], "- button \"Go\" [ref=e1]");
        let second = mapped["content"][1]["text"].as_str().unwrap();
        assert!(second.contains("\"version\":2") && !second.contains("ref=e1"));

        let shot = ActionResult::done("r2", "Captured.")
            .with_data(json!({"image": "iVBORw0KGgo=", "mime_type": "image/png"}));
        let mapped = tool_result(Some(ActionKind::Screenshot), &shot);
        assert_eq!(mapped["content"][0]["type"], "text");
        assert!(!mapped["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("iVBOR"));
        assert_eq!(mapped["content"][1]["type"], "image");
        assert_eq!(mapped["content"][1]["mimeType"], "image/png");

        let unknown = ActionResult::from_reason("r3", Reason::CdpTimeout);
        let mapped = tool_result(Some(ActionKind::Click), &unknown);
        assert_eq!(mapped["isError"], true);
        let text = mapped["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("outcome: unknown; reason: cdp_timeout"));
        assert!(text.contains("request_id r3"));
    }

    #[test]
    fn cancelled_calls_get_no_response() {
        let (credentials, _) = fake_control(|request| {
            let result = ActionResult::done(request["requestId"].as_str().unwrap(), "ok");
            Some(json!({"ok": true, "result": result}).to_string())
        });
        let (server, _) = server(Ok(credentials));
        assert!(server
            .handle(&json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 12}}))
            .is_none());
        assert!(server
            .handle(&request(
                12,
                "tools/call",
                json!({"name": "browser_tabs", "arguments": {"action": "list"}})
            ))
            .is_none());
        assert!(server
            .handle(&request(
                13,
                "tools/call",
                json!({"name": "browser_tabs", "arguments": {"action": "list"}})
            ))
            .is_some());
    }

    #[test]
    fn slow_calls_do_not_block_pings() {
        let (credentials, _) = fake_control(|request| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let result = ActionResult::done(request["requestId"].as_str().unwrap(), "ok");
            Some(json!({"ok": true, "result": result}).to_string())
        });
        let (server, sink) = server(Ok(credentials));
        Server::dispatch(
            &server,
            &request(
                14,
                "tools/call",
                json!({"name": "browser_wait", "arguments": {"tab": "t_0a1b2c3d", "for": "load"}}),
            )
            .to_string(),
        );
        Server::dispatch(&server, &request(15, "ping", Value::Null).to_string());
        assert_eq!(sink.lines()[0]["id"], 15);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while sink.lines().len() < 2 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(sink.lines()[1]["id"], 14);
    }
}
