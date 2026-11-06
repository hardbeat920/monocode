//! Ephemeral, session-scoped publication of native visuals. No user MCP settings
//! are changed. Stable provider capabilities can publish only during a live turn.
use crate::visual::{self, VisualDocument};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const MAX_WIRE_BYTES: usize = 1_048_576;
const MAX_SOURCE_BYTES: usize = visual::MAX_SOURCE_BYTES;
const ENV_ENDPOINT: &str = "MONOCODE_VISUAL_ENDPOINT";
const ENV_TOKEN: &str = "MONOCODE_VISUAL_TOKEN";
const ENV_SESSION: &str = "MONOCODE_VISUAL_SESSION";
const ENV_EXECUTABLE: &str = "MONOCODE_VISUAL_EXECUTABLE";

/// Loaded through visual_docs or the CLI only when authoring a visual. Keep the
/// SDK out of ordinary prompts, server instructions and tool descriptions; only
/// the short USAGE trigger is part of the system prompt.
const DOCUMENTATION: &str = r#"Native session visuals
Use visual_check to validate a visual, then visual_show to publish it into this
conversation. They accept {source}; visual_show also accepts {title?,id?}.
The source is a JavaScript function body with a `visual` SDK. Always return a render function. Use closure state for interactions. Example:
const state = visual.state({count: 0});
return () => visual.column([
  visual.text('Counter', {size: 'title'}),
  visual.stat('Count', String(state.count)),
  visual.button('Add', () => { state.count += 1; })
], {gap: 12});

Answers that tell a story: publish one small visual per idea (a single chart,
a stat row, a gauge) rather than one dashboard holding everything. Each
visual_show returns an embed line such as ![Deploys per week](visual:deploys).
Write your final reply as prose and put each embed line on its own line
directly after the sentence it supports, so text and visuals alternate. The
title becomes the caption; do not restate what the visual already shows.
Visuals you never place appear above your reply. Choose short, readable ids.

Native components:
visual.column(children,{gap?}), row(children,{gap?}), grid(children,{columns?,gap?})
visual.card(children,{gap?}) groups a chart or stats on a raised panel.
visual.text(text,{size?,color?})
visual.stat(label,value,{detail?,delta?,trend?,better?,color?}): delta is a signed
change such as '+12.4%'; trend is an array of numbers drawn as a sparkline;
better:'lower' colors a falling delta as good (latency, errors, cost).
visual.table(columns,rows)
visual.chart('bar'|'line'|'area',[{label,values,color?}],{labels?,height?})
A single-series bar chart labels its latest bar; give each series its own color.
visual.gauge(label,value,{min?,max?,unit?,color?}) for a reading against a range.
visual.donut([{label,value,color?}],{label?,value?}) for parts of a whole;
label and value are written in the middle (default: the total).
visual.heatmap(rows,{rows?,columns?,color?}): rows is an array of equal-length
number arrays, such as weekdays by weeks; labels are optional.
visual.progress(label,value,{max?,detail?,color?})
visual.button(label,onClick,{disabled?}), toggle(label,value,onChange)
visual.slider(label,value,min,max,onChange,{step?}), tabs(labels,selected,onChange)
visual.input(label,value,onChange,{placeholder?}), select(label,options,selectedIndex,onChange)
Input handlers receive a string (up to 4096 bytes); select handlers receive an index.
Select and input labels are not shown: make options and placeholders self-explanatory.
visual.canvas(shapes,{width?,height?})
Canvas shapes: visual.rect(x,y,w,h,{color?}), circle(x,y,r,{color?}),
line(x1,y1,x2,y2,{color?,width?}), polyline([[x,y],...],{color?,width?}),
label(x,y,text,{color?}).
visual.scene(objects,{width?,height?,camera?})
Scene objects: visual.object({kind:'box'|'sphere'|'cylinder'|'cone'|'plane'},
{position:[x,y,z],rotation:[x,y,z],scale:[x,y,z],color:'accent'}).
Custom geometry: {kind:'mesh',vertices:[[x,y,z],...],indices:[[a,b,c],...]}
or {kind:'line',points:[[x,y,z],...],width:1}. Rotations are radians.
Camera: {eye:[5,4,6],target:[0,0,0],fov:45}. Use controls and state to change
geometry or camera; every handler rerenders. State is local to this visual.

Style is inherited from the app. Use semantic colors only: ink, muted, faint,
accent, positive, negative, warning, series1 through series6. Text sizes are
caption, small, body, subtitle, title, hero. Choose clear labels and restrained
headings, use row/column/grid layouts, and allow the host width to determine
layout. Use gaps around 8–16 and chart/canvas/scene heights around 220–360.
Do not draw custom buttons, simulate a whole webpage, add a page background, or
hardcode font families or CSS. Native controls and the app theme own those styles.
Include units, chart legends, readable labels, and useful empty states. Keep
supporting prose in the conversation.
The sandbox has no DOM, network, files, timers, imports, or external libraries.
No HTML, SVG markup, or shader code. Source is limited to 64 KiB; keep
scripts compact. Check validates execution, the initial component tree, and representative controls;
it does not capture a screenshot. Fix errors before publishing. Set a concise
title for the published visual. Never expose capability environment variables.
"#;

/// The system-prompt rule for when to reach for a visual. Only the trigger
/// lives here; the SDK stays behind visual_docs.
const USAGE: &str = "### Native visuals\n\nAnswer data with a trend, comparison or breakdown with an inline visual, and show any chart, diagram or visualizer the user asks for the same way: read `visual_docs` once, then `visual_check` and `visual_show`. Never start a server, open a browser or write an HTML file to show it. Ordinary coding answers need no visual.";

/// Usage guidance for a provider that received the visual MCP server. Claude
/// Code may defer MCP tools behind ToolSearch, so it is told how to load them.
pub fn instructions(environment: &HashMap<String, String>, deferred: bool) -> Option<String> {
    configured(environment).then(|| {
        if deferred {
            format!("{USAGE} The monocode_visual tools may be deferred: load them with ToolSearch before you call them.")
        } else {
            USAGE.to_owned()
        }
    })
}

/// The explicit composer command requires a publication even when the ordinary
/// automatic trigger would leave the answer in prose. Keep the SDK on demand.
pub fn requested_prompt(request: &str) -> String {
    let request = if request.trim().is_empty() {
        "Visualize the current topic using the conversation so far."
    } else {
        request
    };
    format!(
        "Create and publish at least one native inline visual in this conversation during this turn. The user invoked the visualization command, so a visual is required even if you would normally answer only in prose.\n\n\
         Read `visual_docs`, build the visual, run `visual_check`, fix any errors, and call `visual_show` before finishing. The monocode_visual MCP tools may be deferred; load them with ToolSearch if needed. If these tools are unavailable, use the executable in MONOCODE_VISUAL_EXECUTABLE: run `\"$MONOCODE_VISUAL_EXECUTABLE\" visual docs`, write the JavaScript source to a file, then run `\"$MONOCODE_VISUAL_EXECUTABLE\" visual check --input FILE` and `\"$MONOCODE_VISUAL_EXECUTABLE\" visual show --input FILE --title TITLE`.\n\n\
         Use the native visual SDK, never a server, browser or HTML file. Include the embed line returned by visual_show in your reply, alongside concise supporting prose. If publication fails, explain the failure; do not claim a visual was shown. Never print capability environment variables.\n\n\
         User request:\n{request}"
    )
}

/// Claude permission rule that pre-approves every visual tool. Publishing
/// only affects this conversation, so read-only and plan turns may use it.
pub const CLAUDE_ALLOWED_TOOLS: &str = "mcp__monocode_visual";

#[derive(Serialize, Deserialize)]
struct Wire {
    session: i64,
    token: String,
    request_id: String,
    document: VisualDocument,
}

pub struct Publication {
    pub document: VisualDocument,
    pub reply: mpsc::Sender<Result<Value, String>>,
    generation: u64,
}

struct ActiveTurn {
    generation: u64,
    send: mpsc::SyncSender<Publication>,
    publications: usize,
}

struct SessionCapability {
    token: String,
    generation: u64,
    active: Option<ActiveTurn>,
    receipts: HashMap<String, (String, Option<Value>)>,
}

struct Hub {
    endpoint: String,
    sessions: Mutex<HashMap<i64, SessionCapability>>,
}

/// One process-owned listener serves stable session capabilities. Only the
/// currently attached turn receiver can accept a publication.
fn hub() -> Result<Arc<Hub>, String> {
    static HUB: OnceLock<Mutex<Option<Arc<Hub>>>> = OnceLock::new();
    let mut slot = HUB
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Visual bridge unavailable")?;
    if let Some(hub) = &*slot {
        return Ok(hub.clone());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let endpoint = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    let hub = Arc::new(Hub {
        endpoint,
        sessions: Mutex::new(HashMap::new()),
    });
    let owner = hub.clone();
    let connections = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                break;
            };
            if connections.fetch_add(1, Ordering::Relaxed) >= 16 {
                connections.fetch_sub(1, Ordering::Relaxed);
                continue;
            }
            let owner = owner.clone();
            let connections = connections.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, &owner);
                connections.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    *slot = Some(hub.clone());
    Ok(hub)
}

pub struct Server {
    hub: Arc<Hub>,
    token: String,
    session: i64,
    generation: u64,
    publications: mpsc::Receiver<Publication>,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.hub.sessions.lock() {
            if let Some(session) = sessions.get_mut(&self.session) {
                if session.generation == self.generation {
                    session.active = None;
                }
            }
        }
    }
}

impl Server {
    pub fn start(session: i64) -> Result<Self, String> {
        let hub = hub()?;
        let (send, publications) = mpsc::sync_channel(16);
        let (token, generation) = {
            let mut sessions = hub
                .sessions
                .lock()
                .map_err(|_| "Visual session unavailable")?;
            let capability = sessions
                .entry(session)
                .or_insert_with(|| SessionCapability {
                    token: format!(
                        "{:032x}{:032x}",
                        rand::random::<u128>(),
                        rand::random::<u128>()
                    ),
                    generation: 0,
                    active: None,
                    receipts: HashMap::new(),
                });
            if capability.active.is_some() {
                return Err("A visual session turn is already active".into());
            }
            capability.generation = capability.generation.wrapping_add(1);
            capability.active = Some(ActiveTurn {
                generation: capability.generation,
                send,
                publications: 0,
            });
            (capability.token.clone(), capability.generation)
        };
        Ok(Self {
            hub,
            token,
            session,
            generation,
            publications,
        })
    }

    pub fn environment(&self, executable: &std::path::Path) -> HashMap<String, String> {
        HashMap::from([
            (ENV_ENDPOINT.into(), self.hub.endpoint.clone()),
            (ENV_TOKEN.into(), self.token.clone()),
            (ENV_SESSION.into(), self.session.to_string()),
            (
                ENV_EXECUTABLE.into(),
                executable.to_string_lossy().into_owned(),
            ),
        ])
    }

    pub fn drain(&self) -> Vec<Publication> {
        self.publications
            .try_iter()
            .filter_map(|publication| {
                if publication.generation == self.generation
                    && active_generation(&self.hub, self.session) == Some(self.generation)
                {
                    Some(publication)
                } else {
                    let _ = publication
                        .reply
                        .send(Err("The session turn has ended".into()));
                    None
                }
            })
            .collect()
    }
}

fn valid_document(document: &VisualDocument) -> Result<(), String> {
    if document.source.len() > MAX_SOURCE_BYTES {
        return Err("Visual source exceeds 64 KiB".into());
    }
    if document.id.is_empty()
        || document.id.len() > 96
        || !document
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(
            "Visual id must be 1–96 ASCII letters, numbers, underscores, or hyphens".into(),
        );
    }
    if document.title.chars().count() > 160 {
        return Err("Visual title exceeds 160 characters".into());
    }
    document.validate()?;
    visual::check(&document.source)?;
    Ok(())
}

fn active_generation(hub: &Hub, session: i64) -> Option<u64> {
    hub.sessions
        .lock()
        .ok()?
        .get(&session)?
        .active
        .as_ref()
        .map(|active| active.generation)
}

fn serve(mut stream: TcpStream, hub: &Hub) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new((&stream).take((MAX_WIRE_BYTES + 1) as u64))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.len() > MAX_WIRE_BYTES || !line.ends_with('\n') {
        return Err("Invalid visual request".into());
    }
    let wire: Wire = serde_json::from_str(&line).map_err(|_| "Invalid visual request")?;
    // Authenticate and capture the active generation before executing source.
    let generation = {
        let sessions = hub
            .sessions
            .lock()
            .map_err(|_| "Visual session unavailable")?;
        let capability = sessions
            .get(&wire.session)
            .ok_or("Invalid visual capability")?;
        let secret = &capability.token;
        let equal = wire.token.len() == secret.len()
            && wire
                .token
                .bytes()
                .zip(secret.bytes())
                .fold(0u8, |diff, (a, b)| diff | a ^ b)
                == 0;
        if !equal {
            return Err("Invalid visual capability".into());
        }
        capability.active.as_ref().map(|active| active.generation)
    };
    let response = (|| -> Result<Value, String> {
        if wire.request_id.is_empty() || wire.request_id.len() > 128 {
            return Err("Invalid request id".into());
        }
        let signature =
            Sha256::digest(serde_json::to_vec(&wire.document).map_err(|e| e.to_string())?)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
        {
            let sessions = hub
                .sessions
                .lock()
                .map_err(|_| "Visual session unavailable")?;
            let capability = sessions
                .get(&wire.session)
                .ok_or("Invalid visual capability")?;
            if let Some((previous, result)) = capability.receipts.get(&wire.request_id) {
                if previous != &signature {
                    return Err("Request id already used for another visual".into());
                }
                return result.clone().ok_or_else(|| {
                    "Visual publication is still in progress; retry the same request id".into()
                });
            }
        }
        let generation = generation.ok_or("Visual publication requires an active session turn")?;
        valid_document(&wire.document)?;
        let (reply, result) = mpsc::channel();
        {
            let mut sessions = hub
                .sessions
                .lock()
                .map_err(|_| "Visual session unavailable")?;
            let capability = sessions
                .get_mut(&wire.session)
                .ok_or("Invalid visual capability")?;
            let active = capability
                .active
                .as_mut()
                .filter(|active| active.generation == generation)
                .ok_or("The session turn has ended")?;
            // A concurrent retry may have reserved this request during validation.
            if capability.receipts.contains_key(&wire.request_id) {
                return Err("Visual publication is in progress; retry the same request id".into());
            }
            if active.publications >= 64 {
                return Err("This turn has reached its visual publication limit".into());
            }
            // Bound remembered receipts across a long-lived session. Pending
            // requests are never evicted; older settled receipt ids may expire.
            if capability.receipts.len() >= 512 {
                let expired = capability
                    .receipts
                    .iter()
                    .find(|(_, (_, receipt))| receipt.is_some())
                    .map(|(key, _)| key.clone());
                if let Some(expired) = expired {
                    capability.receipts.remove(&expired);
                } else {
                    return Err("Visual receipt capacity reached".into());
                }
            }
            active
                .send
                .try_send(Publication {
                    document: wire.document,
                    reply,
                    generation,
                })
                .map_err(|_| "Visual publication queue unavailable")?;
            active.publications += 1;
            capability
                .receipts
                .insert(wire.request_id.clone(), (signature, None));
        }
        let started = Instant::now();
        let result = loop {
            if active_generation(hub, wire.session) != Some(generation) {
                break Err("The session turn has ended".into());
            }
            match result.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => {
                    if active_generation(hub, wire.session) != Some(generation) {
                        break Err("The session turn has ended".into());
                    }
                    break result;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err("Visual publication was not saved".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout)
                    if started.elapsed() < Duration::from_secs(30) => {}
                Err(_) => break Err("Visual publication timed out".into()),
            }
        };
        let value = match result {
            Ok(value) => json!({"ok":true,"result":value,"requestId":wire.request_id}),
            Err(error) => json!({"ok":false,"error":error,"requestId":wire.request_id}),
        };
        if let Ok(mut sessions) = hub.sessions.lock() {
            if let Some(capability) = sessions.get_mut(&wire.session) {
                if let Some((_, receipt)) = capability.receipts.get_mut(&wire.request_id) {
                    *receipt = Some(value.clone());
                }
            }
        }
        Ok(value)
    })()
    .unwrap_or_else(|error| json!({"ok":false,"error":error,"requestId":wire.request_id}));
    writeln!(stream, "{response}").map_err(|e| e.to_string())
}

/// Local validation never requires an active session capability.
pub fn check(source: &str) -> Result<Value, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("Visual source exceeds 64 KiB".into());
    }
    let tree = visual::check(source)?;
    Ok(json!({"ok":true,"tree":tree}))
}

fn publish_with_environment(
    input: Value,
    environment: &HashMap<String, String>,
) -> Result<Value, String> {
    if !input.is_object() {
        return Err("Visual input must be an object".into());
    }
    for field in ["title", "id", "requestId"] {
        if input.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("{field} must be a string"));
        }
    }
    let source = input["source"]
        .as_str()
        .ok_or("source must be a JavaScript string")?;
    let request_id = input["requestId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>()));
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("Request id must contain 1–128 bytes".into());
    }
    let document_id = Sha256::digest(request_id.as_bytes())
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let document = VisualDocument {
        id: input["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("visual-{document_id}")),
        title: input["title"].as_str().unwrap_or("Visual").to_owned(),
        source: source.to_owned(),
    };
    valid_document(&document)?;
    let get = |key| {
        environment
            .get(key)
            .ok_or("Visual publication requires a running MonoCode session")
    };
    let endpoint: SocketAddr = get(ENV_ENDPOINT)?
        .parse()
        .map_err(|_| "Invalid visual endpoint")?;
    if !endpoint.ip().is_loopback() {
        return Err("Visual endpoint must be loopback".into());
    }
    let wire = Wire {
        session: get(ENV_SESSION)?
            .parse()
            .map_err(|_| "Invalid visual session")?,
        token: get(ENV_TOKEN)?.clone(),
        request_id,
        document,
    };
    let mut stream =
        TcpStream::connect_timeout(&endpoint, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(35)))
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
    let mut response = String::new();
    BufReader::new(stream.take(MAX_WIRE_BYTES as u64))
        .read_line(&mut response)
        .map_err(|e| e.to_string())?;
    let response: Value =
        serde_json::from_str(&response).map_err(|_| "Visual session closed without a response")?;
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Visual publication failed")
            .into());
    }
    let mut response = response;
    response["embed"] = json!(embed(&wire.document));
    response["placement"] = json!(
        "Put the embed line on its own line in your final reply, where this visual belongs. Do not describe or restate the visual around it."
    );
    Ok(response)
}

/// The Markdown line that places a published visual in an answer's prose.
fn embed(document: &VisualDocument) -> String {
    let caption: String = document
        .title
        .chars()
        .map(|c| {
            if matches!(c, '[' | ']' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    format!("![{}](visual:{})", caption.trim(), document.id)
}

fn current_environment() -> HashMap<String, String> {
    [ENV_ENDPOINT, ENV_TOKEN, ENV_SESSION, ENV_EXECUTABLE]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect()
}

pub fn client(args: &[String]) -> Result<Value, String> {
    let Some(action) = args
        .first()
        .filter(|a| !matches!(a.as_str(), "help" | "--help" | "-h"))
    else {
        return Ok(
            json!({"help":format!("Usage: monocode-gpui visual docs\n       monocode-gpui visual check|show --input FILE [--title TITLE] [--id ID]\nFILE is JavaScript source, or JSON with source/title/id/requestId. Use - for stdin.\n{DOCUMENTATION}")}),
        );
    };
    if action == "docs" {
        if args.len() != 1 {
            return Err("Usage: monocode-gpui visual docs".into());
        }
        return Ok(documentation());
    }
    if !matches!(action.as_str(), "check" | "show") {
        return Err("Expected visual docs, visual check or visual show".into());
    }
    let mut input = None;
    let mut title = None;
    let mut id = None;
    let mut request_id = None;
    let mut args = args[1..].iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                let path = args.next().ok_or("--input needs a path")?;
                let reader: Box<dyn Read> = if path == "-" {
                    Box::new(std::io::stdin())
                } else {
                    Box::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
                };
                let mut text = String::new();
                reader
                    .take((MAX_WIRE_BYTES + 1) as u64)
                    .read_to_string(&mut text)
                    .map_err(|e| e.to_string())?;
                if text.len() > MAX_WIRE_BYTES {
                    return Err("Visual input exceeds 1 MiB".into());
                }
                input = Some(
                    serde_json::from_str::<Value>(&text)
                        .ok()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({"source":text})),
                );
            }
            "--json" => {
                input = Some(
                    serde_json::from_str(args.next().ok_or("--json needs JSON")?)
                        .map_err(|e| e.to_string())?,
                )
            }
            "--title" => title = Some(args.next().ok_or("--title needs a value")?.clone()),
            "--id" => id = Some(args.next().ok_or("--id needs a value")?.clone()),
            "--request-id" => {
                request_id = Some(args.next().ok_or("--request-id needs a value")?.clone())
            }
            _ => return Err(format!("Unknown visual argument: {arg}")),
        }
    }
    let mut input = input.ok_or("Pass --input FILE or --json JSON")?;
    if !input.is_object() {
        return Err("Visual input must be an object".into());
    }
    if let Some(title) = title {
        input["title"] = title.into();
    }
    if let Some(id) = id {
        input["id"] = id.into();
    }
    if let Some(request_id) = request_id {
        input["requestId"] = request_id.into();
    }
    if action == "check" {
        check(
            input["source"]
                .as_str()
                .ok_or("source must be a JavaScript string")?,
        )
    } else {
        publish_with_environment(input, &current_environment())
    }
}

fn documentation() -> Value {
    json!({
        "documentation": DOCUMENTATION,
        "cli": {
            "check": "monocode-gpui visual check --input FILE",
            "show": "monocode-gpui visual show --input FILE --title TITLE"
        }
    })
}

pub fn configured(environment: &HashMap<String, String>) -> bool {
    [ENV_ENDPOINT, ENV_TOKEN, ENV_SESSION, ENV_EXECUTABLE]
        .iter()
        .all(|key| environment.contains_key(*key))
}

/// Provider-specific adapters use this ephemeral stdio server definition.
pub fn mcp_server(environment: &HashMap<String, String>) -> Option<Value> {
    configured(environment).then(|| {
        json!({
            "type":"stdio", "command":environment[ENV_EXECUTABLE], "args":["visual-mcp"]
        })
    })
}

pub fn acp_servers(environment: &HashMap<String, String>) -> Value {
    let Some(mut server) = mcp_server(environment) else {
        return json!([]);
    };
    // ACP's stdio server shape omits the transport discriminator.
    server.as_object_mut().unwrap().remove("type");
    server["name"] = "monocode_visual".into();
    server["env"] = [ENV_ENDPOINT, ENV_TOKEN, ENV_SESSION, ENV_EXECUTABLE]
        .into_iter()
        .map(|key| json!({"name":key,"value":environment[key]}))
        .collect::<Vec<_>>()
        .into();
    json!([server])
}

pub fn configure_codex(command: &mut std::process::Command, environment: &HashMap<String, String>) {
    if !configured(environment) {
        return;
    }
    for (key, value) in [
        ("command", json!(environment[ENV_EXECUTABLE])),
        ("args", json!(["visual-mcp"])),
        (
            "env_vars",
            json!([ENV_ENDPOINT, ENV_TOKEN, ENV_SESSION, ENV_EXECUTABLE]),
        ),
    ] {
        // JSON's strings and arrays are valid TOML literals for these values.
        command.args(["-c", &format!("mcp_servers.monocode_visual.{key}={value}")]);
    }
}

pub fn tool_definitions() -> Vec<Value> {
    let properties = json!({"source":{"type":"string","description":"JavaScript function body using the native visual SDK"}});
    let mut show_properties = properties.clone();
    show_properties["title"] = json!({"type":"string","maxLength":160});
    show_properties["id"] = json!({"type":"string","maxLength":96,"description":"Optional document identifier. A new request appends a new visual; reuse requestId to retry a publication."});
    show_properties["requestId"] = json!({"type":"string","maxLength":128,"description":"Reuse when retrying an uncertain publication"});
    vec![
        json!({"name":"visual_docs","description":"Read the native visual SDK reference, examples and styling rules before building an inline chart, diagram or visualizer. Does not publish.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}),
        json!({"name":"visual_check","description":"Validate a native visual program and representative controls without publishing. Returns its component tree. Read visual_docs for the SDK reference.","inputSchema":{"type":"object","properties":properties,"required":["source"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}),
        json!({"name":"visual_show","description":"Publish a native visual (chart, stat row, diagram, interactive explainer) inline in this conversation. Use it for data with a trend, comparison or breakdown, and whenever the user asks for a visual. Returns an embed line: put it on its own line in your final reply where the visual belongs, between the sentences it supports. Read visual_docs first; validate with visual_check.","inputSchema":{"type":"object","properties":show_properties,"required":["source"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}),
    ]
}

fn mcp_response(request: Value, environment: &HashMap<String, String>) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(
            json!({"protocolVersion":request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"monocode_visual","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":tool_definitions()})),
        "tools/call" => {
            let input = request["params"]["arguments"].clone();
            let result = match request["params"]["name"].as_str().unwrap_or("") {
                "visual_docs" => Ok(documentation()),
                "visual_check" => input["source"]
                    .as_str()
                    .ok_or("source must be a JavaScript string".into())
                    .and_then(check),
                "visual_show" => publish_with_environment(input, environment),
                _ => Err("Unknown native visual tool".into()),
            };
            Ok(match result {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
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

/// Runs before GUI initialization. stdout contains MCP JSON-RPC frames only.
pub fn mcp_stdio() -> Result<(), String> {
    let environment = current_environment();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut line = String::new();
        (&mut input)
            .take((MAX_WIRE_BYTES + 1) as u64)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Ok(());
        }
        if line.len() > MAX_WIRE_BYTES || !line.ends_with('\n') {
            return Err("Oversized or incomplete MCP frame".into());
        }
        let response = match serde_json::from_str(&line) {
            Ok(request) => mcp_response(request, &environment),
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
mod tests {
    use super::*;

    fn input() -> Value {
        json!({"source":"return () => visual.text('Hello');","title":"Hello","requestId":"test-request","id":"hello"})
    }

    #[test]
    fn visualize_prompt_uses_native_tools_without_invoking_a_same_named_skill() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join(".agents/skills/visualize");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: visualize\n---\nUse a different visualization system.",
        )
        .unwrap();
        let cwd = root.path().to_str().unwrap();
        for request in ["", "Compare the two approaches"] {
            let prompt = requested_prompt(request);
            assert!(prompt.contains("a visual is required"));
            assert!(prompt.contains("visual_docs"));
            assert!(prompt.contains("visual_check"));
            assert!(prompt.contains("visual_show"));
            assert!(prompt.contains("MONOCODE_VISUAL_EXECUTABLE"));
            assert!(prompt.ends_with(if request.is_empty() {
                "Visualize the current topic using the conversation so far."
            } else {
                request
            }));
            for kind in [
                crate::provider::Kind::Codex,
                crate::provider::Kind::Omp,
                crate::provider::Kind::Devin,
            ] {
                assert_eq!(
                    crate::provider::prepare_prompt(kind, &prompt, cwd, &[]).unwrap(),
                    prompt,
                    "The local command must not expand a same-named skill or reach a provider as a raw slash command"
                );
            }
        }
    }

    #[test]
    fn scoped_publication_acknowledges_after_save_and_deduplicates() {
        let server = Server::start(42).unwrap();
        let environment = server.environment(std::path::Path::new("/test/monocode-gpui"));
        let child_environment = environment.clone();
        let worker =
            std::thread::spawn(move || publish_with_environment(input(), &child_environment));
        let publication = server
            .publications
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(publication.document.title, "Hello");
        assert!(!worker.is_finished());
        publication.reply.send(Ok(json!({"id":"hello"}))).unwrap();
        assert_eq!(worker.join().unwrap().unwrap()["result"]["id"], "hello");
        assert!(publish_with_environment(input(), &environment).is_ok());
        assert!(server.drain().is_empty());
        let mut changed = input();
        changed["source"] = "return () => visual.text('Changed');".into();
        assert!(publish_with_environment(changed, &environment)
            .unwrap_err()
            .contains("already used"));
    }

    #[test]
    fn wrong_session_and_expired_capabilities_cannot_publish() {
        let server = Server::start(7).unwrap();
        let mut environment = server.environment(std::path::Path::new("/test/app"));
        environment.insert(ENV_SESSION.into(), "8".into());
        assert!(publish_with_environment(input(), &environment).is_err());
        assert!(server.drain().is_empty());
        environment.insert(ENV_SESSION.into(), "7".into());
        drop(server);
        std::thread::sleep(Duration::from_millis(30));
        assert!(publish_with_environment(input(), &environment).is_err());
    }

    #[test]
    fn publication_reports_persistence_failure() {
        let server = Server::start(3).unwrap();
        let environment = server.environment(std::path::Path::new("/test/app"));
        let worker = std::thread::spawn(move || publish_with_environment(input(), &environment));
        let publication = server
            .publications
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        publication.reply.send(Err("Saving failed".into())).unwrap();
        assert!(worker
            .join()
            .unwrap()
            .unwrap_err()
            .contains("Saving failed"));
    }

    #[test]
    fn local_validation_and_mcp_notifications_need_no_capability() {
        assert!(check("return () => visual.text('Local');").is_ok());
        assert!(check("while (true) {}").is_err());
        assert!(check(&" ".repeat(MAX_SOURCE_BYTES + 1)).is_err());
        let environment = HashMap::new();
        assert!(
            mcp_response(json!({"method":"notifications/initialized"}), &environment).is_none()
        );
        let response = mcp_response(json!({"id":2,"method":"tools/call","params":{"name":"visual_show","arguments":input()}}), &environment).unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(tool_definitions().len(), 3);
        assert_eq!(acp_servers(&environment), json!([]));
    }

    #[test]
    fn visual_reference_is_loaded_on_demand_without_global_instructions() {
        let environment = HashMap::new();
        let initialization = mcp_response(
            json!({"id":1,"method":"initialize","params":{}}),
            &environment,
        )
        .unwrap();
        assert!(initialization["result"].get("instructions").is_none());
        let catalog = mcp_response(json!({"id":2,"method":"tools/list"}), &environment).unwrap();
        let metadata = catalog["result"].to_string();
        assert!(
            metadata.len() < 2_500,
            "Discovery must stay small: {} bytes",
            metadata.len()
        );
        assert!(!metadata.contains("visual.column"));
        assert!(!metadata.contains("hardcode font"));
        let response = mcp_response(
            json!({"id":3,"method":"tools/call","params":{"name":"visual_docs","arguments":{}}}),
            &environment,
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], false);
        let reference: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert!(reference["documentation"]
            .as_str()
            .unwrap()
            .contains("visual.scene"));
        assert!(reference["documentation"]
            .as_str()
            .unwrap()
            .contains("Style is inherited from the app"));
        assert_eq!(reference, client(&["docs".into()]).unwrap());
        assert!(client(&["docs".into(), "--input".into(), "unused".into()]).is_err());
    }

    #[test]
    fn stable_session_configuration_rejects_old_in_flight_requests() {
        let server = Server::start(83).unwrap();
        let environment = server.environment(std::path::Path::new("/test/app"));
        let child_environment = environment.clone();
        let worker =
            std::thread::spawn(move || publish_with_environment(input(), &child_environment));
        let old = server
            .publications
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        drop(server);
        let next = Server::start(83).unwrap();
        assert_eq!(
            environment,
            next.environment(std::path::Path::new("/test/app"))
        );
        let _ = old.reply.send(Ok(json!({"id":"old"})));
        assert!(worker.join().unwrap().unwrap_err().contains("ended"));
        assert!(next.drain().is_empty());
    }

    #[test]
    fn provider_configuration_forwards_capability_without_token_in_arguments() {
        let server = Server::start(84).unwrap();
        let environment = server.environment(std::path::Path::new("/test/app"));
        let mut command = std::process::Command::new("provider");
        configure_codex(&mut command, &environment);
        let arguments = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(arguments.contains("visual-mcp"));
        assert!(arguments.contains(ENV_TOKEN));
        assert!(!arguments.contains(&environment[ENV_TOKEN]));
        let acp = acp_servers(&environment);
        assert_eq!(acp[0]["name"], "monocode_visual");
        assert_eq!(acp[0]["env"].as_array().unwrap().len(), 4);
        assert_eq!(mcp_server(&environment).unwrap()["command"], "/test/app");
    }
}
