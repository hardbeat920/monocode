//! `monocode browser-mcp`: a stdio MCP server that lets an agent drive the
//! MonoCode built-in browser. It owns no state; every tool call is forwarded
//! over the authenticated loopback control channel (namespace `browser`) to
//! the window running the agent's session, which only answers during that
//! session's live turn.
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

const INSTRUCTIONS: &str = "This chat's web browser: the embedded, built-in, in-app browser in \
MonoCode, shown in the browser panel beside the chat. Requests to open a page in the embedded \
browser, evaluate JavaScript in it, or inspect its page refer to these tools; users do not need \
to name the server or tools. Honor requests for a different browser or browsing method. Prefer it over fetching pages another way whenever \
the result depends on a real rendered page: how something looks, JavaScript, console output, \
clicking or typing, signed-in pages, and local dev servers (localhost) you are building. \
Its tabs belong to this chat only and run in the background: the user can open the panel to \
watch or take over, and may already be using a tab, so prefer opening a new tab over navigating \
one you did not open. Sign-ins are shared with the user's other MonoCode browser tabs. Most \
tools act on the active tab unless you pass tabId. Use browser_read to see the page, \
browser_screenshot to see how it looks, browser_console for logs and errors, browser_eval for \
anything else.";

fn tab_id() -> Value {
    json!({"type": "string", "description": "Tab to act on. Defaults to the active tab."})
}

fn tools() -> Value {
    json!([
        {
            "name": "browser_tabs",
            "description": "List this chat's browser tabs with their id, URL, title, and which one is active.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "browser_open",
            "description": "Open a web page in the embedded browser (a new tab of this chat's in-app browser) and wait for it to load. Use this to view, test, or interact with a site or a local dev server. It does not open the panel. Returns the new tab id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "http(s) URL, or a bare host such as localhost:3000."}
                },
                "required": ["url"]
            }
        },
        {
            "name": "browser_navigate",
            "description": "Load a URL in an existing tab and wait for it to load.",
            "inputSchema": {
                "type": "object",
                "properties": {"url": {"type": "string"}, "tabId": tab_id()},
                "required": ["url"]
            }
        },
        {
            "name": "browser_history",
            "description": "Go back, go forward, or reload a tab.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["back", "forward", "reload"]},
                    "tabId": tab_id()
                },
                "required": ["action"]
            }
        },
        {
            "name": "browser_select",
            "description": "Make a tab the active one.",
            "inputSchema": {
                "type": "object",
                "properties": {"tabId": {"type": "string"}},
                "required": ["tabId"]
            }
        },
        {
            "name": "browser_close",
            "description": "Close a tab.",
            "inputSchema": {
                "type": "object",
                "properties": {"tabId": {"type": "string"}},
                "required": ["tabId"]
            }
        },
        {
            "name": "browser_read",
            "description": "Read the page: visible text (default) or HTML, for the whole document or the first element matching a CSS selector. Long output is truncated.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "format": {"type": "string", "enum": ["text", "html"]},
                    "selector": {"type": "string", "description": "CSS selector to read instead of the whole page."},
                    "maxChars": {"type": "integer", "minimum": 100, "maximum": 200000},
                    "tabId": tab_id()
                }
            }
        },
        {
            "name": "browser_eval",
            "description": "Evaluate JavaScript in a page in the embedded browser and return the result as JSON. The script is the body of an async function: use `return` to send a value back and `await` freely. A single expression without `return` is also accepted. DOM nodes come back as their outer HTML.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "script": {"type": "string"},
                    "timeoutMs": {"type": "integer", "minimum": 100, "maximum": 25000},
                    "tabId": tab_id()
                },
                "required": ["script"]
            }
        },
        {
            "name": "browser_console",
            "description": "Console messages, uncaught errors, and unhandled rejections the page logged since it loaded (most recent last).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                    "clear": {"type": "boolean", "description": "Clear the buffer after reading."},
                    "tabId": tab_id()
                }
            }
        },
        {
            "name": "browser_click",
            "description": "Click the first element matching a CSS selector, or the first link, button, or control whose text contains `text`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "tabId": tab_id()
                }
            }
        },
        {
            "name": "browser_type",
            "description": "Type into an input, textarea, select, or contenteditable element matching a CSS selector. Fires input and change events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "clear": {"type": "boolean", "description": "Replace the current value (default true)."},
                    "submit": {"type": "boolean", "description": "Submit the element's form afterwards."},
                    "tabId": tab_id()
                },
                "required": ["selector", "text"]
            }
        },
        {
            "name": "browser_wait",
            "description": "Wait until an element matching a CSS selector exists, or the page text contains `text`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "timeoutMs": {"type": "integer", "minimum": 100, "maximum": 25000},
                    "tabId": tab_id()
                }
            }
        },
        {
            "name": "browser_screenshot",
            "description": "See what a page in the embedded browser looks like: captures the tab's viewport as a PNG image, whether or not the browser panel is open.",
            "inputSchema": {"type": "object", "properties": {"tabId": tab_id()}}
        }
    ])
}

fn is_tool(name: &str) -> bool {
    tools()
        .as_array()
        .is_some_and(|list| list.iter().any(|tool| tool["name"] == name))
}

/// Turn an executor reply into MCP tool content.
fn tool_result(reply: Result<Value, String>) -> Value {
    let error =
        |message: String| json!({"content": [{"type": "text", "text": message}], "isError": true});
    let value = match reply {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("MonoCode browser request failed")
            .to_string();
        return error(message);
    }
    let result = value.get("result").cloned().unwrap_or(Value::Null);
    if let Some(data) = result.get("image").and_then(Value::as_str) {
        let mut content = vec![json!({
            "type": "image",
            "data": data,
            "mimeType": result.get("mimeType").and_then(Value::as_str).unwrap_or("image/png"),
        })];
        if let Some(text) = result.get("text").and_then(Value::as_str) {
            content.push(json!({"type": "text", "text": text}));
        }
        return json!({"content": content});
    }
    let text = match &result {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    json!({"content": [{"type": "text", "text": text}]})
}

fn call_tool(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if !is_tool(name) {
        return tool_result(Err(format!("Unknown tool: {name}")));
    }
    let input = match params.get("arguments") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        None | Some(Value::Null) => json!({}),
        Some(_) => return tool_result(Err("Tool arguments must be an object".into())),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    tool_result(
        crate::control_cli::send("browser", name, &input, &request_id)
            .map_err(|failure| failure.error),
    )
}

/// Answer one JSON-RPC message, or None for notifications.
fn handle(message: &Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            let version = if SUPPORTED_VERSIONS.contains(&requested) {
                requested
            } else {
                PROTOCOL_VERSION
            };
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "monocode-browser", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => call_tool(&params),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")},
            }))
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn run() -> i32 {
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let write = |out: &Arc<Mutex<std::io::Stdout>>, value: Value| {
        if let Ok(mut out) = out.lock() {
            let _ = writeln!(out, "{value}");
            let _ = out.flush();
        }
    };
    let mut calls = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            write(
                &stdout,
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}}),
            );
            continue;
        };
        // Tool calls can wait on page loads; keep answering pings meanwhile.
        if message.get("method").and_then(Value::as_str) == Some("tools/call") {
            let out = stdout.clone();
            calls.push(std::thread::spawn(move || {
                if let Some(reply) = handle(&message) {
                    write(&out, reply);
                }
            }));
            calls.retain(|call| !call.is_finished());
            continue;
        }
        if let Some(reply) = handle(&message) {
            write(&stdout, reply);
        }
    }
    // The client closed stdin; still answer the calls it already sent.
    for call in calls {
        let _ = call.join();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_a_supported_protocol() {
        let reply = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}})).unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        let reply = handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}})).unwrap();
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn lists_tools_and_ignores_notifications() {
        let reply = handle(&json!({"jsonrpc":"2.0","id":"a","method":"tools/list"})).unwrap();
        let names: Vec<&str> = reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"browser_eval"));
        assert!(names.iter().all(|name| name.starts_with("browser_")));
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let unknown = handle(&json!({"jsonrpc":"2.0","id":2,"method":"resources/list"})).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn maps_replies_to_tool_content() {
        let text = tool_result(Ok(json!({"ok": true, "result": {"title": "A"}})));
        assert!(text["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"title\""));
        let image = tool_result(Ok(
            json!({"ok": true, "result": {"image": "AAAA", "mimeType": "image/png"}}),
        ));
        assert_eq!(image["content"][0]["type"], "image");
        let failed = tool_result(Ok(json!({"ok": false, "error": "nope"})));
        assert_eq!(failed["isError"], true);
        assert_eq!(failed["content"][0]["text"], "nope");
        assert_eq!(tool_result(Err("down".into()))["isError"], true);
    }

    #[test]
    fn rejects_unknown_tools_without_calling_the_app() {
        let reply = call_tool(&json!({"name": "shell_exec", "arguments": {}}));
        assert_eq!(reply["isError"], true);
    }
}
