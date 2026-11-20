//! Session-scoped local app access. Provider children inherit only their own
//! capability; all requests are checked again against the Mono's project list.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub const HELP: &str = r#"MonoCode app access
Usage: monocode-gpui app ACTION --json '{...}' [--request-id ID]
Pass --input FILE or --input - to read a JSON object instead.
Actions:
models.list {}
sessions.list {project?}
sessions.read {sessionId,project?,before?,limit?,maxChars?,charOffset?}
sessions.start {prompt,project?,harness?,model?,modelSettings?,effort?,runtimeMode?,title?,draft?,reveal?,workspaceMode?,worktreeBase?,worktreeCwd?,placement?,besideSessionId?,notifyOnComplete?}
sessions.send {sessionId,prompt,project?,notifyOnComplete?}
sessions.draft {sessionId,prompt,project?}
sessions.stop / sessions.archive / sessions.delete {sessionId,project?}
worktrees.list {project?}
worktrees.create {branch,base?,existing?,project?}
folders.list {project?}
folders.move {sessionId,folderId?,newFolderName?,project?}
notes.list {limit?,offset?}; notes.read {id}; notes.write {id?,title?,body?,tags?}
artifacts.list {kind?,limit?,offset?}; artifacts.read {id}; artifacts.write {id?,kind:"document",title,body,summary?}
soul.read {}; soul.update {text,expectedHash}
memory.read {topic?}; memory.search {query,since?}; memory.add {fact,topic?,until?}; memory.replace {find,fact,topic?,until?}; memory.remove {find,topic?}
habits.list {}; habits.add {name,instructions,schedule:{kind,time?,minute?,dayOfWeek?}}; habits.update {id,name?,instructions?,schedule?,enabled?}; habits.run {id}; habits.remove {id}
chat.card {type:"pr",repo?,number,note?} / {type:"session",sessionId,note?} / {type:"choices",options:[text]} / {type:"habit",name,instructions,schedule}
Ordinary Operator sessions are visible in project history. Mono launches follow its Show started sessions preference. reveal defaults to false and controls navigation, not history visibility. placement right/down explicitly opens a split beside besideSessionId (or the caller).
sessions.read returns up to three prose exchanges and nextBefore for older turns; pass that turn ID as before. maxChars (200-6000) caps each message; a longer one has nextCharOffset, so read on by repeating the call with that value as charOffset (without limit, charOffset reads only the latest exchange).
notes.write requires body for a new note; existing notes accept title, body or tags independently.
Artifacts, memory, soul, habits, cards and notifyOnComplete:true require a Mono.
The CLI returns {ok,result,requestId}. Retry uncertain mutations with the same request ID and identical input.
Assigned projects alone may be addressed. Credentials are provided in the environment; never print them.
"#;
#[derive(Serialize, Deserialize)]
struct Wire {
    session: i64,
    token: String,
    request_id: String,
    action: String,
    input: Value,
}
pub struct Call {
    pub session: i64,
    pub request_id: String,
    pub action: String,
    pub input: Value,
    pub reply: mpsc::Sender<Result<Value, String>>,
}
pub struct Server {
    endpoint: String,
    secret: Arc<String>,
    stop: Arc<AtomicBool>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept so its thread sees the stop flag.
        let _ = TcpStream::connect(&self.endpoint);
    }
}
impl Server {
    pub fn start() -> Result<(Self, async_channel::Receiver<Call>), String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let endpoint = listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string();
        let secret = Arc::new(format!("{:032x}", rand::random::<u128>()) + &super::new_id());
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = async_channel::bounded(32);
        let owner = stop.clone();
        let key = secret.clone();
        std::thread::spawn(move || {
            while !owner.load(Ordering::Relaxed) && !tx.is_closed() {
                match listener.accept() {
                    Ok(_) if owner.load(Ordering::Relaxed) => break,
                    Ok((stream, _)) => {
                        if connections.fetch_add(1, Ordering::Relaxed) >= 16 {
                            connections.fetch_sub(1, Ordering::Relaxed);
                            continue;
                        }
                        let tx = tx.clone();
                        let key = key.clone();
                        let connections = connections.clone();
                        std::thread::spawn(move || {
                            let _ = serve(stream, &key, &tx);
                            connections.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
        Ok((
            Self {
                endpoint,
                secret,
                stop,
            },
            rx,
        ))
    }
    pub fn environment(&self, session: i64) -> HashMap<String, String> {
        HashMap::from([
            ("MONOCODE_APP_ENDPOINT".into(), self.endpoint.clone()),
            ("MONOCODE_APP_TOKEN".into(), token(&self.secret, session)),
            ("MONOCODE_APP_SESSION".into(), session.to_string()),
        ])
    }
}
fn token(secret: &str, session: i64) -> String {
    Sha256::digest(format!("{secret}:{session}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn serve(
    mut stream: TcpStream,
    secret: &str,
    tx: &async_channel::Sender<Call>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new((&stream).take(1_048_577));
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.len() > 1_048_576 || !line.ends_with('\n') {
        return Err("Invalid app request".into());
    }
    let wire: Wire = serde_json::from_str(&line).map_err(|_| "Invalid app request")?;
    let expected = token(secret, wire.session);
    let equal = wire.token.len() == expected.len()
        && wire
            .token
            .bytes()
            .zip(expected.bytes())
            .fold(0, |diff, (a, b)| diff | a ^ b)
            == 0;
    if !equal
        || !wire.input.is_object()
        || wire.request_id.len() > 128
        || wire.request_id.is_empty()
    {
        return Err("Invalid app capability".into());
    }
    let (reply, result) = mpsc::channel();
    let request_id = wire.request_id;
    tx.send_blocking(Call {
        session: wire.session,
        request_id: request_id.clone(),
        action: wire.action,
        input: wire.input,
        reply,
    })
    .map_err(|_| "App closed")?;
    let response = match result.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(value)) => json!({"ok":true,"result":value,"requestId":request_id}),
        Ok(Err(error)) => json!({"ok":false,"error":error,"requestId":request_id}),
        Err(_) => {
            json!({"ok":false,"error":"App request timed out; retry using the same request ID","requestId":request_id})
        }
    };
    writeln!(stream, "{response}").map_err(|e| e.to_string())
}
/// Runs before any GUI initialization, so a provider tool never opens a window.
pub fn client(args: &[String]) -> Result<Value, String> {
    if args
        .first()
        .is_none_or(|s| matches!(s.as_str(), "help" | "--help" | "-h"))
    {
        return Ok(json!({"help":HELP}));
    }
    let action = args[0].clone();
    let mut input = json!({});
    let mut request_id = super::new_id();
    let mut args = args[1..].iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => {
                input = serde_json::from_str(args.next().ok_or("--json needs JSON")?)
                    .map_err(|e| e.to_string())?
            }
            "--input" => {
                let path = args.next().ok_or("--input needs a path")?;
                let text = if path == "-" {
                    let mut text = String::new();
                    std::io::stdin()
                        .take(1_048_577)
                        .read_to_string(&mut text)
                        .map_err(|e| e.to_string())?;
                    text
                } else {
                    std::fs::read_to_string(path).map_err(|e| e.to_string())?
                };
                if text.len() > 1_048_576 {
                    return Err("Input too large".into());
                }
                input = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            }
            "--request-id" => request_id = args.next().ok_or("--request-id needs an ID")?.clone(),
            json if json.starts_with('{') => {
                input = serde_json::from_str(json).map_err(|e| e.to_string())?
            }
            _ => return Err(format!("Unknown app argument: {arg}")),
        }
    }
    call(action, input, request_id)
}

/// One authenticated request to the running app, with the capability this
/// provider process inherited.
fn call(action: String, input: Value, request_id: String) -> Result<Value, String> {
    let endpoint: SocketAddr = std::env::var("MONOCODE_APP_ENDPOINT")
        .map_err(|_| "App access is unavailable outside a Mono's provider process")?
        .parse()
        .map_err(|_| "Invalid app endpoint")?;
    if !endpoint.ip().is_loopback() {
        return Err("App access requires a local endpoint".into());
    }
    let token = std::env::var("MONOCODE_APP_TOKEN").map_err(|_| "App capability missing")?;
    let session = std::env::var("MONOCODE_APP_SESSION")
        .map_err(|_| "App session missing")?
        .parse()
        .map_err(|_| "Invalid app session")?;
    let wire = Wire {
        session,
        token,
        request_id,
        action,
        input,
    };
    let mut stream =
        TcpStream::connect_timeout(&endpoint, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(65)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&wire).map_err(|e| e.to_string())?
    )
    .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream.take(4_194_305))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.len() > 4_194_304 || line.is_empty() {
        return Err("App response unavailable".into());
    }
    serde_json::from_str(&line).map_err(|e| e.to_string())
}

/// The MCP server a Mono's provider reaches the app through: one long-lived
/// stdio child per provider process, instead of a shell and a fresh app
/// executable for every call.
pub const MCP_SERVER: &str = "monocode_app";
/// Claude's allowlist entry: the app checks every call against the Mono's own
/// capability and projects, so it needs no second permission check.
pub const CLAUDE_ALLOWED_TOOLS: &str = "mcp__monocode_app";
const ENV_KEYS: [&str; 3] = [
    "MONOCODE_APP_ENDPOINT",
    "MONOCODE_APP_TOKEN",
    "MONOCODE_APP_SESSION",
];

fn configured(environment: &HashMap<String, String>) -> bool {
    ENV_KEYS.iter().all(|key| environment.contains_key(*key))
}

/// Claude's stdio server entry for a Mono, or None without app access.
pub fn mcp_server(
    mono: Option<&crate::agent::MonoContext>,
    environment: &HashMap<String, String>,
) -> Option<Value> {
    let mono = mono.filter(|_| configured(environment))?;
    Some(json!({"type":"stdio","command":mono.executable,"args":["app-mcp"]}))
}

/// ACP's stdio server shape, which names the server and passes its env.
pub fn acp_server(
    mono: Option<&crate::agent::MonoContext>,
    environment: &HashMap<String, String>,
) -> Option<Value> {
    let mut server = mcp_server(mono, environment)?;
    server.as_object_mut()?.remove("type");
    server["name"] = MCP_SERVER.into();
    server["env"] = ENV_KEYS
        .iter()
        .map(|key| json!({"name":key,"value":environment[*key]}))
        .collect::<Vec<_>>()
        .into();
    Some(server)
}

pub fn configure_codex(
    command: &mut std::process::Command,
    mono: Option<&crate::agent::MonoContext>,
    environment: &HashMap<String, String>,
) {
    let Some(server) = mcp_server(mono, environment) else {
        return;
    };
    for (key, value) in [
        ("command", server["command"].clone()),
        ("args", server["args"].clone()),
        ("env_vars", json!(ENV_KEYS)),
        // The app checks every call against the Mono's own capability and
        // projects. Without this, Codex's auto-review spends a model call
        // (~3s) approving each one.
        ("default_tools_approval_mode", json!("approve")),
    ] {
        // JSON's strings and arrays are valid TOML literals for these values.
        command.args(["-c", &format!("mcp_servers.{MCP_SERVER}.{key}={value}")]);
    }
}

pub fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "name": "app",
        "description": format!("Run a MonoCode app action: sessions, worktrees, folders, notes, artifacts, soul, memory, habits and chat cards. Pass the action name and its JSON input. Retry an uncertain mutation with the same requestId and identical input.\n\n{HELP}"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "description": "For example memory.add or sessions.start"},
                "input": {"type": "object", "description": "The action's input object"},
                "requestId": {"type": "string", "maxLength": 128, "description": "Reuse when retrying an uncertain mutation"}
            },
            "required": ["action"],
            "additionalProperties": false
        },
        "annotations": {"openWorldHint": false}
    })]
}

fn mcp_response(request: Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": MCP_SERVER, "version": env!("CARGO_PKG_VERSION")}
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let arguments = &request["params"]["arguments"];
            let result = if request["params"]["name"].as_str() != Some("app") {
                Err("Unknown MonoCode app tool".to_string())
            } else if let Some(action) = arguments["action"].as_str() {
                let input = match &arguments["input"] {
                    Value::Null => json!({}),
                    input => input.clone(),
                };
                let request_id = arguments["requestId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map_or_else(super::new_id, str::to_owned);
                call(action.to_owned(), input, request_id)
            } else {
                Err("action is required".to_string())
            };
            Ok(match result {
                Ok(value) => {
                    let failed = value.get("ok") == Some(&Value::Bool(false));
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":failed})
                }
                Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
            })
        }
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
    })
}

/// Runs before GUI initialization. stdout carries MCP JSON-RPC frames only.
pub fn mcp_stdio() -> Result<(), String> {
    const MAX_FRAME: usize = 4 * 1024 * 1024;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut line = String::new();
        (&mut input)
            .take((MAX_FRAME + 1) as u64)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Ok(());
        }
        if line.len() > MAX_FRAME || !line.ends_with('\n') {
            return Err("Oversized or incomplete MCP frame".into());
        }
        let response = match serde_json::from_str(&line) {
            Ok(request) => mcp_response(request),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")
                .and_then(|_| output.flush())
                .map_err(|e| e.to_string())?;
        }
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    fn send(
        server: &Server,
        session: i64,
        credential: String,
    ) -> std::thread::JoinHandle<Option<Value>> {
        let endpoint = server.endpoint.clone();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(endpoint).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let wire = Wire {
                session,
                token: credential,
                request_id: "read-1".into(),
                action: "models.list".into(),
                input: json!({}),
            };
            writeln!(stream, "{}", serde_json::to_string(&wire).unwrap()).unwrap();
            let mut result = String::new();
            let _ = BufReader::new(stream).read_line(&mut result);
            serde_json::from_str(&result).ok()
        })
    }
    #[test]
    fn mcp_server_offers_one_app_tool_only_to_a_mono_with_app_access() {
        let mono = crate::agent::MonoContext {
            mono: Default::default(),
            root: "/monos".into(),
            executable: "/bin/monocode-gpui".into(),
        };
        let (server, _rx) = Server::start().unwrap();
        let environment = server.environment(7);
        assert!(mcp_server(None, &environment).is_none());
        assert!(mcp_server(Some(&mono), &HashMap::new()).is_none());
        let entry = mcp_server(Some(&mono), &environment).unwrap();
        assert_eq!(entry["args"], json!(["app-mcp"]));
        let mut codex = std::process::Command::new("codex");
        configure_codex(&mut codex, Some(&mono), &environment);
        let args: Vec<_> = codex
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(
            &r#"mcp_servers.monocode_app.default_tools_approval_mode="approve""#.to_string()
        ));
        let acp = acp_server(Some(&mono), &environment).unwrap();
        assert_eq!(acp["name"], MCP_SERVER);
        assert_eq!(acp["env"].as_array().unwrap().len(), 3);

        let list = mcp_response(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "app");
        let unknown = mcp_response(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"other","arguments":{}}})).unwrap();
        assert_eq!(unknown["result"]["isError"], true);
        let missing = mcp_response(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"app","arguments":{}}})).unwrap();
        assert_eq!(missing["result"]["isError"], true);
        assert!(
            mcp_response(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none()
        );
    }
    #[test]
    fn capability_is_bound_to_its_session_and_only_authenticated_calls_reach_the_app() {
        let (server, rx) = Server::start().unwrap();
        let credential = server.environment(7)["MONOCODE_APP_TOKEN"].clone();
        assert_ne!(credential, server.environment(8)["MONOCODE_APP_TOKEN"]);
        assert!(send(&server, 8, credential.clone())
            .join()
            .unwrap()
            .is_none());
        assert!(rx.try_recv().is_err());
        let request = send(&server, 7, credential);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let call = loop {
            if let Ok(call) = rx.try_recv() {
                break call;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authenticated request was not dispatched"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(call.session, 7);
        call.reply.send(Ok(json!({"models":[]}))).unwrap();
        assert_eq!(request.join().unwrap().unwrap()["ok"], true);
    }
}
