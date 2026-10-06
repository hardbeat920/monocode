use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tungstenite::handshake::server::{Callback, ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{HandshakeError, Message, WebSocket};

pub const FIGMA_BRIDGE_PORT: u16 = 3056;
const BRIDGE_EVENT: &str = "monocode-figma-bridge";
const GENERATION_EVENT: &str = "monocode-figma-generation";
const TOKEN_PLACEHOLDER: &str = "__MONOCODE_FIGMA_BRIDGE_TOKEN__";
const PORT_PLACEHOLDER: &str = "__MONOCODE_FIGMA_BRIDGE_PORT__";
const PLUGIN_MANIFEST: &str = include_str!("../figma-plugin/manifest.json");
const PLUGIN_CODE: &str = include_str!("../figma-plugin/code.js");
const PLUGIN_UI: &str = include_str!("../figma-plugin/ui.html");
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;
const MAX_PENDING: usize = 16;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(20);
const READ_CHUNK_BYTES: usize = 64 * 1024;
const EVENT_QUEUE: usize = 256;
const MAX_HANDSHAKE_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const BUNDLE_TIMEOUT: Duration = Duration::from_secs(150);
const PREVIEW_MAX_BYTES: usize = 8 * 1024 * 1024;
const BUNDLE_PREVIEW_MAX_BYTES: usize = 16 * 1024 * 1024;
const ASSET_MAX_BYTES: usize = 32 * 1024 * 1024;
const ASSETS_MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ASSETS: usize = 50;
const MAX_DIAGNOSTICS: usize = 40;
const MAX_TEXT_CHARS: usize = 512;
const MAX_DIAGNOSTIC_CHARS: usize = 200;
const MAX_ERROR_CHARS: usize = 400;
const MAX_ID_CHARS: usize = 128;
const MAX_NODE_TYPE_CHARS: usize = 32;
const MAX_DIMENSION: f64 = 100_000.0;
const GENERATIONS_KEPT: usize = 20;
const PREVIEW_ROOT: &str = ".monocode";
const PREVIEW_IGNORE: &str = "*\n";
const PREVIEW_FOLDER_IGNORE: &str = "figma/";
const PREVIEW_IGNORE_RULES: [&str; 8] = [
    "*",
    "**",
    "figma",
    "figma/",
    "/figma",
    "/figma/",
    "figma/**",
    "/figma/**",
];
const PREVIEW_COPY_DEPTH: usize = 4;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FigmaDocument {
    pub id: String,
    pub name: String,
    pub file_key: Option<String>,
    pub page_id: String,
    pub page_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSource {
    pub node_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FigmaSelection {
    pub selection_count: u32,
    pub document: FigmaDocument,
    pub source: Option<FigmaSource>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FigmaConnection {
    pub id: String,
    pub connected_at: u64,
    pub selection: Option<FigmaSelection>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FigmaBridgeStatus {
    pub enabled: bool,
    pub listening: bool,
    pub port: u16,
    pub error: Option<String>,
    pub plugin_directory: String,
    pub plugin_installed: bool,
    pub connections: Vec<FigmaConnection>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FigmaPreviewWorkspace {
    pub directory: String,
    pub relative_directory: String,
    pub preview_path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FigmaGeneration {
    pub id: String,
    pub preview_bytes: u64,
    pub source: FigmaSource,
    pub document: FigmaDocument,
    pub diagnostics: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FigmaConfig {
    enabled: bool,
    token: String,
}

enum Notice {
    Changed,
    Generation(Box<FigmaGeneration>),
}

type Notify = Arc<dyn Fn(Notice) -> bool + Send + Sync>;

enum ClientEvent {
    Bytes(Vec<u8>),
    Disconnected,
    Send(String),
    Close,
}

struct ChannelStream {
    incoming: Rc<RefCell<VecDeque<u8>>>,
    writer: TcpStream,
}

impl Read for ChannelStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut incoming = self.incoming.borrow_mut();
        if incoming.is_empty() {
            return Err(std::io::Error::from(ErrorKind::WouldBlock));
        }
        let count = buffer.len().min(incoming.len());
        for (slot, byte) in buffer.iter_mut().zip(incoming.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

impl Write for ChannelStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writer.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

struct Client {
    outgoing: mpsc::SyncSender<ClientEvent>,
    channel: Option<String>,
    session_token: String,
    connected_at: u64,
    selection: Option<FigmaSelection>,
}

struct Running {
    stop: Arc<AtomicBool>,
    port: u16,
}

struct Pending {
    client: String,
    reply: mpsc::Sender<Result<Value, String>>,
}

struct Inner {
    enabled: bool,
    token: String,
    running: Option<Running>,
    error: Option<String>,
    clients: HashMap<String, Client>,
    pending: HashMap<String, Pending>,
}

struct Shared {
    inner: Mutex<Inner>,
    connections: AtomicUsize,
    generations: PathBuf,
    notify: Notify,
}

#[derive(Clone)]
pub struct FigmaBridge {
    shared: Arc<Shared>,
}

struct PairingCheck {
    token: String,
}

impl Callback for PairingCheck {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let origin = request
            .headers()
            .get("origin")
            .and_then(|value| value.to_str().ok());
        if client_allowed(origin, query_token(request.uri().query()), &self.token) {
            Ok(response)
        } else {
            Err(forbidden())
        }
    }
}

struct ConnectionSlot(Arc<Shared>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FigmaBridge {
    fn new(generations: PathBuf, enabled: bool, token: String, notify: Notify) -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    enabled,
                    token,
                    running: None,
                    error: None,
                    clients: HashMap::new(),
                    pending: HashMap::new(),
                }),
                connections: AtomicUsize::new(0),
                generations,
                notify,
            }),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, String> {
        self.shared
            .inner
            .lock()
            .map_err(|_| "The Figma bridge is unavailable".to_string())
    }

    fn notify(&self, notice: Notice) -> bool {
        (self.shared.notify)(notice)
    }

    fn start(&self, port: u16) -> Result<u16, String> {
        let mut inner = self.lock()?;
        if let Some(running) = &inner.running {
            return Ok(running.port);
        }
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => listener,
            Err(error) => {
                let message = if error.kind() == ErrorKind::AddrInUse {
                    format!("Port {port} is already in use by another app")
                } else {
                    error.to_string()
                };
                inner.error = Some(message.clone());
                return Err(message);
            }
        };
        let bound = listener.local_addr().map_err(|e| e.to_string())?.port();
        let stop = Arc::new(AtomicBool::new(false));
        inner.running = Some(Running {
            stop: stop.clone(),
            port: bound,
        });
        inner.error = None;
        drop(inner);
        let bridge = self.clone();
        std::thread::spawn(move || bridge.accept(listener, &stop));
        Ok(bound)
    }

    fn stop(&self) {
        let (running, clients) = match self.lock() {
            Ok(mut inner) => (
                inner.running.take(),
                inner
                    .clients
                    .values()
                    .map(|client| client.outgoing.clone())
                    .collect::<Vec<_>>(),
            ),
            Err(_) => return,
        };
        if let Some(running) = running {
            running.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect_timeout(
                &SocketAddr::from((Ipv4Addr::LOCALHOST, running.port)),
                Duration::from_millis(300),
            );
        }
        for client in clients {
            let _ = client.send(ClientEvent::Close);
        }
    }

    fn set_enabled(&self, enabled: bool, port: u16) {
        if let Ok(mut inner) = self.lock() {
            inner.enabled = enabled;
            if !enabled {
                inner.error = None;
            }
        }
        if enabled {
            let _ = self.start(port);
        } else {
            self.stop();
        }
    }

    fn replace_token(&self, token: String) {
        let clients = match self.lock() {
            Ok(mut inner) => {
                inner.token = token;
                inner
                    .clients
                    .values()
                    .map(|client| client.outgoing.clone())
                    .collect::<Vec<_>>()
            }
            Err(_) => return,
        };
        for client in clients {
            let _ = client.send(ClientEvent::Close);
        }
    }

    fn ensure_token(&self) -> Result<(String, bool), String> {
        let mut inner = self.lock()?;
        if valid_token(&inner.token) {
            return Ok((inner.token.clone(), false));
        }
        inner.token = random_token();
        Ok((inner.token.clone(), true))
    }

    fn connections(&self) -> Vec<FigmaConnection> {
        let Ok(inner) = self.lock() else {
            return Vec::new();
        };
        let mut connections: Vec<FigmaConnection> = inner
            .clients
            .iter()
            .filter(|(_, client)| client.channel.is_some())
            .map(|(id, client)| FigmaConnection {
                id: id.clone(),
                connected_at: client.connected_at,
                selection: client.selection.clone(),
            })
            .collect();
        connections.sort_by(|a, b| a.connected_at.cmp(&b.connected_at).then(a.id.cmp(&b.id)));
        connections
    }

    fn status(&self, plugin_directory: &Path) -> Result<FigmaBridgeStatus, String> {
        let (enabled, listening, port, error, token) = {
            let inner = self.lock()?;
            (
                inner.enabled,
                inner.running.is_some(),
                inner
                    .running
                    .as_ref()
                    .map_or(FIGMA_BRIDGE_PORT, |running| running.port),
                inner.error.clone(),
                inner.token.clone(),
            )
        };
        Ok(FigmaBridgeStatus {
            enabled,
            listening,
            port,
            error,
            plugin_directory: crate::fs::path_to_js(plugin_directory),
            plugin_installed: plugin_installed(plugin_directory, &token),
            connections: self.connections(),
        })
    }

    fn accept(&self, listener: TcpListener, stop: &AtomicBool) {
        for stream in listener.incoming() {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if self.shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                self.shared.connections.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let slot = ConnectionSlot(self.shared.clone());
            let bridge = self.clone();
            std::thread::spawn(move || {
                let _slot = slot;
                bridge.serve(stream);
            });
        }
    }

    fn serve(&self, stream: TcpStream) {
        let Ok(token) = self.lock().map(|inner| inner.token.clone()) else {
            return;
        };
        let (Ok(mut reader), Ok(writer)) = (stream.try_clone(), stream.try_clone()) else {
            return;
        };
        if writer.set_write_timeout(Some(WRITE_TIMEOUT)).is_err() {
            return;
        }
        let (events, queue) = mpsc::sync_channel(EVENT_QUEUE);
        let feed = events.clone();
        std::thread::spawn(move || {
            let mut buffer = vec![0u8; READ_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => {
                        let _ = feed.send(ClientEvent::Disconnected);
                        return;
                    }
                    Ok(count) => {
                        if feed
                            .send(ClientEvent::Bytes(buffer[..count].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        });
        self.run_client(writer, events, &queue, PairingCheck { token });
        let _ = stream.shutdown(Shutdown::Both);
    }

    fn run_client(
        &self,
        writer: TcpStream,
        events: mpsc::SyncSender<ClientEvent>,
        queue: &mpsc::Receiver<ClientEvent>,
        check: PairingCheck,
    ) {
        let incoming = Rc::new(RefCell::new(VecDeque::new()));
        let stream = ChannelStream {
            incoming: incoming.clone(),
            writer,
        };
        let Some(mut socket) = accept_plugin(stream, check, &incoming, queue) else {
            return;
        };
        let client_id = uuid::Uuid::new_v4().simple().to_string();
        let session_token = random_token();
        match self.lock() {
            Ok(mut inner) => {
                inner.clients.insert(
                    client_id.clone(),
                    Client {
                        outgoing: events,
                        channel: None,
                        session_token: session_token.clone(),
                        connected_at: unix_millis(),
                        selection: None,
                    },
                );
            }
            Err(_) => return,
        }
        let hello = json!({ "type": "hello", "version": 1, "sessionToken": session_token });
        if socket.send(Message::text(hello.to_string())).is_ok() {
            self.client_loop(&client_id, &mut socket, &incoming, queue);
        }
        self.remove_client(&client_id);
    }

    fn client_loop(
        &self,
        client_id: &str,
        socket: &mut WebSocket<ChannelStream>,
        incoming: &Rc<RefCell<VecDeque<u8>>>,
        queue: &mpsc::Receiver<ClientEvent>,
    ) {
        while let Ok(event) = queue.recv() {
            match event {
                ClientEvent::Bytes(bytes) => {
                    incoming.borrow_mut().extend(bytes);
                    if !self.read_messages(client_id, socket) {
                        return;
                    }
                }
                ClientEvent::Send(text) => {
                    if socket.send(Message::text(text)).is_err() {
                        return;
                    }
                }
                ClientEvent::Close => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return;
                }
                ClientEvent::Disconnected => return,
            }
        }
    }

    fn read_messages(&self, client_id: &str, socket: &mut WebSocket<ChannelStream>) -> bool {
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    for reply in self.handle_message(client_id, text.as_str()) {
                        if socket.send(Message::text(reply)).is_err() {
                            return false;
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    let _ = socket.flush();
                    return false;
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    return true;
                }
                Err(_) => return false,
            }
        }
    }

    fn remove_client(&self, client_id: &str) {
        let removed = match self.lock() {
            Ok(mut inner) => {
                let removed = inner.clients.remove(client_id).is_some();
                let orphaned: Vec<String> = inner
                    .pending
                    .iter()
                    .filter(|(_, pending)| pending.client == client_id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in orphaned {
                    if let Some(pending) = inner.pending.remove(&id) {
                        let _ = pending
                            .reply
                            .send(Err("The Figma plugin disconnected".into()));
                    }
                }
                removed
            }
            Err(_) => false,
        };
        if removed {
            self.notify(Notice::Changed);
        }
    }

    fn handle_message(&self, client_id: &str, raw: &str) -> Vec<String> {
        let Ok(payload) = serde_json::from_str::<Value>(raw) else {
            return vec![error_frame(None, "MonoCode received an invalid message")];
        };
        match payload.get("type").and_then(Value::as_str) {
            Some("join") => self.join(client_id, &payload),
            Some("message") => self.client_message(client_id, &payload),
            _ => Vec::new(),
        }
    }

    fn join(&self, client_id: &str, payload: &Value) -> Vec<String> {
        let channel = text(payload.get("channel")).filter(|channel| safe_id(channel));
        let session_token = payload.get("sessionToken").and_then(Value::as_str);
        let well_formed = payload.get("v").and_then(Value::as_u64) == Some(1)
            && payload.get("role").and_then(Value::as_str) == Some("figma-plugin");
        let joined = match self.lock() {
            Ok(mut inner) => match (inner.clients.get_mut(client_id), channel) {
                (Some(client), Some(channel))
                    if well_formed && session_token == Some(client.session_token.as_str()) =>
                {
                    client.channel = Some(channel.clone());
                    Some(channel)
                }
                _ => None,
            },
            Err(_) => None,
        };
        let Some(channel) = joined else {
            return vec![error_frame(None, "The Figma plugin handshake is invalid")];
        };
        self.notify(Notice::Changed);
        vec![
            json!({ "v": 1, "type": "system", "channel": channel, "message": { "result": true } })
                .to_string(),
        ]
    }

    fn client_message(&self, client_id: &str, payload: &Value) -> Vec<String> {
        let channel = match self.lock() {
            Ok(inner) => inner
                .clients
                .get(client_id)
                .and_then(|client| client.channel.clone()),
            Err(_) => None,
        };
        let Some(channel) = channel else {
            return Vec::new();
        };
        let Some(message) = payload.get("message").filter(|value| value.is_object()) else {
            return Vec::new();
        };
        let Some(id) = text(message.get("id")).filter(|id| safe_id(id)) else {
            return Vec::new();
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match message.get("command").and_then(Value::as_str) {
            None => {
                self.resolve(client_id, &id, message);
                Vec::new()
            }
            Some("selection_changed") => match parse_selection(params.get("selection")) {
                Ok(selection) => {
                    self.store_selection(client_id, selection);
                    vec![result_frame(&channel, &id, json!({ "accepted": true }))]
                }
                Err(error) => vec![command_error_frame(&channel, &id, &error)],
            },
            Some("generate_code_from_selection") => {
                match stage_generation(
                    &self.shared.generations,
                    params.get("bundle").unwrap_or(&Value::Null),
                ) {
                    Ok(generation) => {
                        let generation_id = generation.id.clone();
                        if !self.notify(Notice::Generation(Box::new(generation))) {
                            let _ =
                                fs::remove_dir_all(self.shared.generations.join(&generation_id));
                            return vec![command_error_frame(
                                &channel,
                                &id,
                                "Open a MonoCode window, then generate the component again.",
                            )];
                        }
                        vec![result_frame(
                            &channel,
                            &id,
                            json!({
                                "accepted": true,
                                "generationId": generation_id,
                                "message": "MonoCode is generating this component.",
                            }),
                        )]
                    }
                    Err(error) => vec![command_error_frame(&channel, &id, &error)],
                }
            }
            Some(other) => vec![command_error_frame(
                &channel,
                &id,
                &format!("Unsupported command: {}", truncate(other, MAX_ID_CHARS)),
            )],
        }
    }

    fn resolve(&self, client_id: &str, id: &str, message: &Value) {
        let Ok(mut inner) = self.lock() else { return };
        if inner
            .pending
            .get(id)
            .is_none_or(|pending| pending.client != client_id)
        {
            return;
        }
        let Some(pending) = inner.pending.remove(id) else {
            return;
        };
        let result = match text(message.get("error")) {
            Some(error) => Err(truncate(&error, MAX_ERROR_CHARS)),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = pending.reply.send(result);
    }

    fn store_selection(&self, client_id: &str, selection: FigmaSelection) {
        let changed = match self.lock() {
            Ok(mut inner) => match inner.clients.get_mut(client_id) {
                Some(client) if client.selection.as_ref() != Some(&selection) => {
                    client.selection = Some(selection);
                    true
                }
                _ => false,
            },
            Err(_) => false,
        };
        if changed {
            self.notify(Notice::Changed);
        }
    }

    fn request(
        &self,
        connection_id: &str,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = format!("monocode-{}", uuid::Uuid::new_v4().simple());
        let (reply, response) = mpsc::channel();
        let (outgoing, frame) = {
            let mut inner = self.lock()?;
            if inner.pending.len() >= MAX_PENDING {
                return Err("Too many Figma requests are waiting".into());
            }
            let client = inner
                .clients
                .get(connection_id)
                .filter(|client| client.channel.is_some())
                .ok_or("The Figma plugin is not connected")?;
            let frame = json!({
                "v": 1,
                "id": id,
                "type": "message",
                "channel": client.channel,
                "message": { "id": id, "command": command, "params": params },
            })
            .to_string();
            let outgoing = client.outgoing.clone();
            inner.pending.insert(
                id.clone(),
                Pending {
                    client: connection_id.to_string(),
                    reply,
                },
            );
            (outgoing, frame)
        };
        if outgoing.send(ClientEvent::Send(frame)).is_err() {
            if let Ok(mut inner) = self.lock() {
                inner.pending.remove(&id);
            }
            return Err("The Figma plugin is not connected".into());
        }
        let result = response.recv_timeout(timeout);
        if let Ok(mut inner) = self.lock() {
            inner.pending.remove(&id);
        }
        result.unwrap_or_else(|_| Err(format!("Figma did not answer in time ({command})")))
    }
}

fn accept_plugin(
    stream: ChannelStream,
    check: PairingCheck,
    incoming: &Rc<RefCell<VecDeque<u8>>>,
    queue: &mpsc::Receiver<ClientEvent>,
) -> Option<WebSocket<ChannelStream>> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut received = 0usize;
    let mut attempt = tungstenite::accept_hdr_with_config(
        stream,
        check,
        Some(
            WebSocketConfig::default()
                .max_message_size(Some(MAX_MESSAGE_BYTES))
                .max_frame_size(Some(MAX_MESSAGE_BYTES)),
        ),
    );
    loop {
        match attempt {
            Ok(socket) => return Some(socket),
            Err(HandshakeError::Interrupted(pending)) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match queue.recv_timeout(remaining) {
                    Ok(ClientEvent::Bytes(bytes)) => {
                        received += bytes.len();
                        if received > MAX_HANDSHAKE_BYTES {
                            return None;
                        }
                        incoming.borrow_mut().extend(bytes);
                        attempt = pending.handshake();
                    }
                    _ => return None,
                }
            }
            Err(HandshakeError::Failure(_)) => return None,
        }
    }
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let data_dir = app_data_dir(app)?;
    let (config, error) = match read_config(app) {
        Ok(config) => (config, None),
        Err(error) => (FigmaConfig::default(), Some(error)),
    };
    let handle = app.clone();
    let notify: Notify = Arc::new(move |notice| emit_notice(&handle, notice));
    let bridge = FigmaBridge::new(
        data_dir.join("figma").join("generations"),
        config.enabled,
        config.token,
        notify,
    );
    if let Some(error) = error {
        if let Ok(mut inner) = bridge.lock() {
            inner.error = Some(error);
        }
    }
    app.manage(bridge.clone());
    if config.enabled {
        let _ = bridge.start(FIGMA_BRIDGE_PORT);
    }
    Ok(())
}

fn emit_notice(app: &AppHandle, notice: Notice) -> bool {
    match notice {
        Notice::Changed => {
            let Some(bridge) = app.try_state::<FigmaBridge>() else {
                return false;
            };
            let Ok(directory) = plugin_directory(app) else {
                return false;
            };
            bridge
                .status(&directory)
                .is_ok_and(|status| app.emit(BRIDGE_EVENT, status).is_ok())
        }
        Notice::Generation(generation) => {
            let windows = crate::window::workspace_windows(app);
            let target = windows
                .iter()
                .find(|window| window.is_focused().unwrap_or(false))
                .or_else(|| {
                    windows
                        .iter()
                        .find(|window| window.is_visible().unwrap_or(false))
                })
                .or(windows.first());
            target.is_some_and(|window| {
                app.emit_to(window.label(), GENERATION_EVENT, *generation)
                    .is_ok()
            })
        }
    }
}

#[tauri::command(async)]
pub fn figma_bridge_status(
    app: AppHandle,
    bridge: State<'_, FigmaBridge>,
) -> Result<FigmaBridgeStatus, String> {
    bridge.status(&plugin_directory(&app)?)
}

#[tauri::command(async)]
pub fn figma_bridge_set_enabled(
    app: AppHandle,
    bridge: State<'_, FigmaBridge>,
    enabled: bool,
) -> Result<FigmaBridgeStatus, String> {
    let (token, _) = bridge.ensure_token()?;
    write_config(&app, &FigmaConfig { enabled, token })?;
    bridge.set_enabled(enabled, FIGMA_BRIDGE_PORT);
    bridge.notify(Notice::Changed);
    bridge.status(&plugin_directory(&app)?)
}

#[tauri::command(async)]
pub fn figma_bridge_reset_pairing(
    app: AppHandle,
    bridge: State<'_, FigmaBridge>,
) -> Result<FigmaBridgeStatus, String> {
    let directory = plugin_directory(&app)?;
    let enabled = bridge.lock()?.enabled;
    let token = random_token();
    write_config(
        &app,
        &FigmaConfig {
            enabled,
            token: token.clone(),
        },
    )?;
    bridge.replace_token(token.clone());
    if directory.exists() {
        install_plugin(&directory, &token, FIGMA_BRIDGE_PORT)?;
    }
    bridge.notify(Notice::Changed);
    bridge.status(&directory)
}

#[tauri::command(async)]
pub fn figma_plugin_install(
    app: AppHandle,
    bridge: State<'_, FigmaBridge>,
) -> Result<FigmaBridgeStatus, String> {
    let (token, created) = bridge.ensure_token()?;
    if created {
        let enabled = bridge.lock()?.enabled;
        write_config(
            &app,
            &FigmaConfig {
                enabled,
                token: token.clone(),
            },
        )?;
    }
    let directory = plugin_directory(&app)?;
    install_plugin(&directory, &token, FIGMA_BRIDGE_PORT)?;
    bridge.notify(Notice::Changed);
    bridge.status(&directory)
}

#[tauri::command]
pub async fn figma_selection_preview(
    bridge: State<'_, FigmaBridge>,
    connection_id: String,
    node_id: String,
) -> Result<String, String> {
    let bridge = bridge.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let node_id = node_id.trim().to_string();
        if node_id.is_empty() || node_id.chars().count() > MAX_ID_CHARS {
            return Err("The Figma layer id is invalid".to_string());
        }
        let result = bridge.request(
            &connection_id,
            "export_selection_preview",
            json!({ "nodeId": node_id }),
            COMMAND_TIMEOUT,
        )?;
        preview_data_url(&result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn figma_prepare_preview(
    bridge: State<'_, FigmaBridge>,
    generation_id: String,
    cwd: String,
) -> Result<FigmaPreviewWorkspace, String> {
    let generations = bridge.shared.generations.clone();
    tauri::async_runtime::spawn_blocking(move || {
        prepare_preview(&generations, &generation_id, &crate::fs::expand_home(&cwd))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn figma_generate(
    bridge: State<'_, FigmaBridge>,
    connection_id: String,
    node_id: Option<String>,
) -> Result<FigmaGeneration, String> {
    let bridge = bridge.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bundle = bridge.request(
            &connection_id,
            "get_codegen_bundle",
            capture_params(node_id.as_deref())?,
            BUNDLE_TIMEOUT,
        )?;
        stage_generation(&bridge.shared.generations, &bundle)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn capture_params(node_id: Option<&str>) -> Result<Value, String> {
    let Some(node_id) = node_id.map(str::trim) else {
        return Ok(json!({}));
    };
    if !safe_id(node_id) {
        return Err("The Figma layer id is invalid".into());
    }
    Ok(json!({ "nodeId": node_id }))
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

fn plugin_directory(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("figma-plugin"))
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("figma-bridge.json"))
}

fn read_config(app: &AppHandle) -> Result<FigmaConfig, String> {
    match fs::read_to_string(config_path(app)?) {
        Ok(raw) => {
            let config: FigmaConfig = serde_json::from_str(&raw)
                .map_err(|_| "Figma bridge settings are invalid".to_string())?;
            if !config.token.is_empty() && !valid_token(&config.token) {
                return Err("The Figma bridge pairing token is invalid".into());
            }
            Ok(config)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(FigmaConfig::default()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_config(app: &AppHandle, config: &FigmaConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let value = serde_json::to_string(config).map_err(|error| error.to_string())?;
    write_secret_file(&path, &value)
}

fn write_secret_file(path: &Path, value: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())?;
        file.write_all(value.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::write(path, value).map_err(|error| error.to_string())
    }
}

fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None | Some("null") => true,
        Some(origin) => url::Url::parse(origin).is_ok_and(|url| {
            url.scheme() == "https"
                && url.port().is_none()
                && url
                    .host_str()
                    .is_some_and(|host| host == "figma.com" || host.ends_with(".figma.com"))
        }),
    }
}

fn query_token(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

fn client_allowed(origin: Option<&str>, token: Option<&str>, expected: &str) -> bool {
    origin_allowed(origin)
        && valid_token(expected)
        && token.is_some_and(|token| constant_time_eq(token.as_bytes(), expected.as_bytes()))
}

fn forbidden() -> ErrorResponse {
    let mut response = ErrorResponse::new(Some("Forbidden".into()));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn text(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?.trim();
    (!value.is_empty()).then(|| truncate(value, MAX_TEXT_CHARS))
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b';' | b'.')
        })
}

fn node_type(value: &str) -> bool {
    value.len() <= MAX_NODE_TYPE_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
}

fn dimension(value: Option<&Value>) -> Option<f64> {
    value?
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= MAX_DIMENSION)
}

fn error_frame(channel: Option<&str>, message: &str) -> String {
    json!({ "v": 1, "type": "error", "channel": channel, "message": message }).to_string()
}

fn result_frame(channel: &str, id: &str, result: Value) -> String {
    json!({ "v": 1, "id": id, "type": "message", "channel": channel, "message": { "id": id, "result": result } })
        .to_string()
}

fn command_error_frame(channel: &str, id: &str, error: &str) -> String {
    json!({ "v": 1, "id": id, "type": "message", "channel": channel, "message": { "id": id, "error": error } })
        .to_string()
}

fn parse_document(value: Option<&Value>) -> Result<FigmaDocument, String> {
    let value = value
        .filter(|value| value.is_object())
        .ok_or("The Figma selection has no document")?;
    let file_key = match value.get("fileKey") {
        None | Some(Value::Null) => None,
        other => Some(text(other).ok_or("The Figma file key is invalid")?),
    };
    Ok(FigmaDocument {
        id: text(value.get("id")).ok_or("The Figma document has no id")?,
        name: text(value.get("name")).ok_or("The Figma document has no name")?,
        file_key,
        page_id: text(value.get("pageId")).ok_or("The Figma page has no id")?,
        page_name: text(value.get("pageName")).ok_or("The Figma page has no name")?,
    })
}

fn parse_source(value: Option<&Value>) -> Result<FigmaSource, String> {
    let value = value
        .filter(|value| value.is_object())
        .ok_or("The Figma layer is missing")?;
    Ok(FigmaSource {
        node_id: text(value.get("nodeId"))
            .filter(|id| safe_id(id))
            .ok_or("The Figma layer id is invalid")?,
        name: text(value.get("name")).ok_or("The Figma layer has no name")?,
        node_type: text(value.get("type"))
            .filter(|kind| node_type(kind))
            .ok_or("The Figma layer type is invalid")?,
        width: dimension(value.get("width")).ok_or("The Figma layer width is invalid")?,
        height: dimension(value.get("height")).ok_or("The Figma layer height is invalid")?,
    })
}

fn parse_selection(value: Option<&Value>) -> Result<FigmaSelection, String> {
    let value = value
        .filter(|value| value.is_object())
        .ok_or("The Figma selection is invalid")?;
    let selection_count = value
        .get("selectionCount")
        .and_then(Value::as_u64)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or("The Figma selection count is invalid")?;
    let source = match value.get("source") {
        None | Some(Value::Null) => None,
        other => Some(parse_source(other)?),
    };
    if source.is_some() && selection_count != 1 {
        return Err("The Figma selection count does not match its layer".into());
    }
    Ok(FigmaSelection {
        selection_count,
        document: parse_document(value.get("document"))?,
        source,
    })
}

fn decode_base64(value: Option<&Value>, limit: usize) -> Result<Vec<u8>, String> {
    let encoded = value
        .and_then(Value::as_str)
        .ok_or("The Figma image has no data")?;
    if encoded.len() > limit.div_ceil(3) * 4 + 4 {
        return Err("The Figma image exceeds the size limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "The Figma image data is invalid".to_string())?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err("The Figma image exceeds the size limit".into());
    }
    Ok(bytes)
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

fn image_extension(mime_type: &str, bytes: &[u8]) -> Option<&'static str> {
    match mime_type {
        "image/png" if is_png(bytes) => Some("png"),
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Some("jpg"),
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Some("gif"),
        "image/webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => {
            Some("webp")
        }
        _ => None,
    }
}

fn preview_data_url(result: &Value) -> Result<String, String> {
    if result.get("mimeType").and_then(Value::as_str) != Some("image/png") {
        return Err("Figma returned a preview that is not a PNG".into());
    }
    let bytes = decode_base64(result.get("imageData"), PREVIEW_MAX_BYTES)?;
    if !is_png(&bytes) {
        return Err("Figma returned a preview that is not a PNG".into());
    }
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

struct StagedAsset {
    hash: String,
    mime_type: String,
    path: String,
    bytes: Vec<u8>,
    metadata: Map<String, Value>,
}

fn parse_assets(value: Option<&Value>) -> Result<Vec<StagedAsset>, String> {
    let entries = match value {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(entries)) => entries,
        Some(_) => return Err("The Figma assets are invalid".into()),
    };
    if entries.len() > MAX_ASSETS {
        return Err(format!("The selection uses more than {MAX_ASSETS} images"));
    }
    let mut seen = HashSet::new();
    let mut total = 0usize;
    let mut assets = Vec::new();
    for entry in entries {
        let object = entry.as_object().ok_or("A Figma asset is invalid")?;
        let hash = object
            .get("hash")
            .and_then(Value::as_str)
            .filter(|hash| {
                (32..=128).contains(&hash.len())
                    && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or("A Figma asset has an invalid hash")?
            .to_ascii_lowercase();
        let mime_type = object
            .get("mimeType")
            .and_then(Value::as_str)
            .ok_or("A Figma asset has no type")?
            .to_string();
        let bytes = decode_base64(object.get("imageData"), ASSET_MAX_BYTES)?;
        let extension = image_extension(&mime_type, &bytes)
            .ok_or_else(|| format!("Figma asset {hash} is not a supported image"))?;
        total += bytes.len();
        if total > ASSETS_MAX_BYTES {
            return Err("The selection images exceed the 64 MB limit".into());
        }
        if !seen.insert(hash.clone()) {
            continue;
        }
        let mut metadata = object.clone();
        metadata.remove("imageData");
        let path = format!("assets/{hash}.{extension}");
        metadata.insert("path".into(), Value::String(path.clone()));
        assets.push(StagedAsset {
            hash,
            mime_type,
            path,
            bytes,
            metadata,
        });
    }
    Ok(assets)
}

fn generation_id() -> String {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{:013}-{}", unix_millis(), &suffix[..8])
}

fn is_generation_id(name: &str) -> bool {
    let Some((millis, suffix)) = name.split_once('-') else {
        return false;
    };
    millis.len() == 13
        && millis.bytes().all(|byte| byte.is_ascii_digit())
        && suffix.len() == 8
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn stage_generation(root: &Path, bundle: &Value) -> Result<FigmaGeneration, String> {
    let bundle = bundle.as_object().ok_or("The Figma bundle is invalid")?;
    if bundle.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err("The Figma bundle version is not supported".into());
    }
    let source = parse_source(bundle.get("source"))?;
    let document = parse_document(bundle.get("document"))?;
    if !bundle.get("root").is_some_and(Value::is_object) {
        return Err("The Figma bundle has no layer tree".into());
    }
    let preview = bundle
        .get("preview")
        .and_then(Value::as_object)
        .ok_or("The Figma bundle has no preview")?;
    if preview.get("mimeType").and_then(Value::as_str) != Some("image/png") {
        return Err("The Figma preview is not a PNG".into());
    }
    let preview_bytes = decode_base64(preview.get("imageData"), BUNDLE_PREVIEW_MAX_BYTES)?;
    if !is_png(&preview_bytes) {
        return Err("The Figma preview is not a PNG".into());
    }
    let assets = parse_assets(bundle.get("assets"))?;
    let diagnostics: Vec<String> = bundle
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| text(Some(entry)))
                .map(|entry| truncate(&entry, MAX_DIAGNOSTIC_CHARS))
                .take(MAX_DIAGNOSTICS)
                .collect()
        })
        .unwrap_or_default();
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let id = generation_id();
    let directory = root.join(&id);
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    if let Err(error) = write_generation(&directory, bundle, &preview_bytes, &assets) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    prune_generations(root, &id);
    Ok(FigmaGeneration {
        id,
        preview_bytes: preview_bytes.len() as u64,
        source,
        document,
        diagnostics,
    })
}

fn write_generation(
    directory: &Path,
    bundle: &Map<String, Value>,
    preview_bytes: &[u8],
    assets: &[StagedAsset],
) -> Result<(), String> {
    let write =
        |path: PathBuf, bytes: &[u8]| fs::write(path, bytes).map_err(|error| error.to_string());
    write(directory.join("preview.png"), preview_bytes)?;
    let assets_dir = directory.join("assets");
    fs::create_dir(&assets_dir).map_err(|error| error.to_string())?;
    for asset in assets {
        write(directory.join(&asset.path), &asset.bytes)?;
    }
    let manifest = json!({
        "assets": assets
            .iter()
            .map(|asset| json!({ "hash": asset.hash, "mimeType": asset.mime_type, "path": asset.path }))
            .collect::<Vec<_>>(),
        "preview": { "path": "preview.png", "mimeType": "image/png", "bytes": preview_bytes.len() },
    });
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    write(assets_dir.join("manifest.json"), &manifest)?;
    let mut sanitized = bundle.clone();
    sanitized.insert(
        "preview".into(),
        json!({ "path": "preview.png", "mimeType": "image/png", "bytes": preview_bytes.len() }),
    );
    sanitized.insert(
        "assets".into(),
        Value::Array(
            assets
                .iter()
                .map(|asset| Value::Object(asset.metadata.clone()))
                .collect(),
        ),
    );
    let sanitized =
        serde_json::to_vec(&Value::Object(sanitized)).map_err(|error| error.to_string())?;
    write(directory.join("source-bundle.json"), &sanitized)
}

fn prune_generations(root: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_generation_id(name))
        .collect();
    names.sort_unstable();
    names.reverse();
    for name in names.iter().skip(GENERATIONS_KEPT) {
        if name != keep {
            let _ = fs::remove_dir_all(root.join(name));
        }
    }
}

fn ensure_real_dir(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a link, so MonoCode will not write the Figma preview through it",
            crate::fs::path_to_js(path)
        )),
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("{} is not a folder", crate::fs::path_to_js(path))),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn copy_tree(source: &Path, target: &Path, depth: usize) -> Result<(), String> {
    if depth > PREVIEW_COPY_DEPTH {
        return Err("The Figma capture is nested too deeply".into());
    }
    fs::create_dir(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination, depth + 1)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn prepare_preview(
    generations: &Path,
    id: &str,
    project: &Path,
) -> Result<FigmaPreviewWorkspace, String> {
    if !is_generation_id(id) {
        return Err("The Figma capture id is invalid".into());
    }
    let source = generations.join(id);
    if !source.join("preview.png").is_file() || !source.join("source-bundle.json").is_file() {
        return Err("The Figma capture is no longer available. Generate it again.".into());
    }
    if !fs::metadata(project).is_ok_and(|meta| meta.is_dir()) {
        return Err("Open a project folder to generate the component in".into());
    }
    let root = project.join(PREVIEW_ROOT);
    ensure_real_dir(&root)?;
    ensure_previews_ignored(&root.join(".gitignore"))?;
    let figma = root.join("figma");
    ensure_real_dir(&figma)?;
    let directory = figma.join(id);
    ensure_real_dir(&directory)?;
    let design = directory.join("design");
    remove_dir_if_exists(&design)?;
    copy_tree(&source, &design, 0)?;
    prune_generations(&figma, id);
    Ok(FigmaPreviewWorkspace {
        directory: crate::fs::path_to_js(&directory),
        relative_directory: format!("{PREVIEW_ROOT}/figma/{id}"),
        preview_path: crate::fs::path_to_js(&design.join("preview.png")),
    })
}

fn ensure_previews_ignored(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(PREVIEW_IGNORE.as_bytes()))
            .map_err(|error| error.to_string()),
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "{} is a link, so MonoCode will not write the Figma preview through it",
            crate::fs::path_to_js(path)
        )),
        Ok(meta) if meta.is_file() => {
            let existing = fs::read_to_string(path).map_err(|error| error.to_string())?;
            if existing
                .lines()
                .any(|line| PREVIEW_IGNORE_RULES.contains(&line.trim()))
            {
                return Ok(());
            }
            let separator = if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            fs::OpenOptions::new()
                .append(true)
                .open(path)
                .and_then(|mut file| {
                    file.write_all(format!("{separator}{PREVIEW_FOLDER_IGNORE}\n").as_bytes())
                })
                .map_err(|error| error.to_string())
        }
        Ok(_) => Err(format!("{} is not a file", crate::fs::path_to_js(path))),
        Err(error) => Err(error.to_string()),
    }
}

fn render_plugin_ui(template: &str, token: &str, port: u16) -> Result<String, String> {
    if !valid_token(token) {
        return Err("The Figma bridge pairing token is invalid".into());
    }
    if !template.contains(TOKEN_PLACEHOLDER) || !template.contains(PORT_PLACEHOLDER) {
        return Err("The Figma plugin template is incomplete".into());
    }
    Ok(template
        .replace(TOKEN_PLACEHOLDER, token)
        .replace(PORT_PLACEHOLDER, &port.to_string()))
}

fn plugin_files(token: &str, port: u16) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        ("manifest.json", PLUGIN_MANIFEST.to_string()),
        ("code.js", PLUGIN_CODE.to_string()),
        ("ui.html", render_plugin_ui(PLUGIN_UI, token, port)?),
    ])
}

fn plugin_installed(directory: &Path, token: &str) -> bool {
    valid_token(token)
        && directory.join("manifest.json").is_file()
        && directory.join("code.js").is_file()
        && fs::read_to_string(directory.join("ui.html")).is_ok_and(|ui| ui.contains(token))
}

fn sibling(directory: &Path, suffix: &str) -> PathBuf {
    let mut name = directory.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    directory.with_file_name(name)
}

fn remove_dir_if_exists(directory: &Path) -> Result<(), String> {
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn verify_plugin(directory: &Path, files: &[(&'static str, String)]) -> Result<(), String> {
    for (name, contents) in files {
        let installed = fs::read(directory.join(name)).map_err(|error| error.to_string())?;
        if installed != contents.as_bytes() {
            return Err(format!("{name} changed while installing the Figma plugin"));
        }
    }
    Ok(())
}

fn complete_plugin(directory: &Path) -> bool {
    ["manifest.json", "code.js", "ui.html"].iter().all(|name| {
        fs::metadata(directory.join(name)).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
    })
}

fn recover_interrupted_install(directory: &Path, backup: &Path) -> Result<(), String> {
    if !backup.exists() {
        return Ok(());
    }
    if !directory.exists() || (!complete_plugin(directory) && complete_plugin(backup)) {
        remove_dir_if_exists(directory)?;
        return fs::rename(backup, directory).map_err(|error| error.to_string());
    }
    remove_dir_if_exists(backup)
}

fn install_plugin(directory: &Path, token: &str, port: u16) -> Result<(), String> {
    let files = plugin_files(token, port)?;
    let staging = sibling(directory, "staging");
    let backup = sibling(directory, "backup");
    if let Some(parent) = directory.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    recover_interrupted_install(directory, &backup)?;
    remove_dir_if_exists(&staging)?;
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let result = (|| {
        for (name, contents) in &files {
            write_secret_file(&staging.join(name), contents)?;
        }
        verify_plugin(&staging, &files)?;
        let had_active = directory.exists();
        if had_active {
            fs::rename(directory, &backup).map_err(|error| error.to_string())?;
        }
        if let Err(error) = fs::rename(&staging, directory) {
            if had_active {
                let _ = fs::rename(&backup, directory);
            }
            return Err(error.to_string());
        }
        if let Err(error) = verify_plugin(directory, &files) {
            let _ = fs::remove_dir_all(directory);
            if had_active {
                let _ = fs::rename(&backup, directory);
            }
            return Err(error);
        }
        remove_dir_if_exists(&backup)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::client::IntoClientRequest;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "monocode-figma-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn encode(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn document() -> Value {
        json!({ "id": "0:0", "name": "Design system", "fileKey": null, "pageId": "0:1", "pageName": "Buttons" })
    }

    fn bundle() -> Value {
        json!({
            "schemaVersion": 1,
            "document": document(),
            "source": { "nodeId": "12:34", "name": "Button", "type": "COMPONENT", "width": 120.0, "height": 40.0 },
            "root": { "id": "12:34", "name": "Button", "children": [] },
            "preview": { "mimeType": "image/png", "bytes": PNG.len(), "imageData": encode(PNG) },
            "assets": [
                { "hash": "ABCDEF0123456789ABCDEF0123456789ABCDEF01", "mimeType": "image/png", "bytes": PNG.len(), "imageData": encode(PNG) }
            ],
            "diagnostics": ["effects: 12:35"],
        })
    }

    fn quiet_bridge(root: &Path) -> FigmaBridge {
        FigmaBridge::new(root.to_path_buf(), true, TOKEN.into(), Arc::new(|_| true))
    }

    fn joined_plugin(port: u16) -> WebSocket<TcpStream> {
        let mut socket = connect(port, TOKEN, "null").unwrap();
        let session_token = read_json(&mut socket)["sessionToken"]
            .as_str()
            .unwrap()
            .to_string();
        socket
            .send(Message::text(
                json!({ "v": 1, "type": "join", "role": "figma-plugin", "channel": "monocode-test", "sessionToken": session_token })
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(read_json(&mut socket)["message"]["result"], true);
        socket
    }

    #[test]
    fn requires_the_pairing_token_for_every_client() {
        assert!(client_allowed(Some("null"), Some(TOKEN), TOKEN));
        assert!(client_allowed(None, Some(TOKEN), TOKEN));
        assert!(client_allowed(
            Some("https://www.figma.com"),
            Some(TOKEN),
            TOKEN
        ));
        assert!(!client_allowed(None, None, TOKEN));
        assert!(!client_allowed(Some("null"), Some(&TOKEN[1..]), TOKEN));
        assert!(!client_allowed(
            Some("https://evil.example"),
            Some(TOKEN),
            TOKEN
        ));
        assert!(!client_allowed(
            Some("http://www.figma.com"),
            Some(TOKEN),
            TOKEN
        ));
        assert!(!client_allowed(
            Some("https://www.figma.com:8443"),
            Some(TOKEN),
            TOKEN
        ));
        assert!(!client_allowed(
            Some("https://figma.com.evil.example"),
            Some(TOKEN),
            TOKEN
        ));
        assert!(!client_allowed(Some("null"), Some(""), ""));
    }

    #[test]
    fn reads_the_token_from_the_query() {
        assert_eq!(query_token(Some("token=abc")), Some("abc"));
        assert_eq!(query_token(Some("a=1&token=abc")), Some("abc"));
        assert_eq!(query_token(Some("a=1")), None);
        assert_eq!(query_token(None), None);
    }

    #[test]
    fn generates_valid_tokens() {
        let token = random_token();
        assert!(valid_token(&token));
        assert_ne!(token, random_token());
        assert!(!valid_token("ABCDEF"));
        assert!(!valid_token(&TOKEN.to_uppercase()));
    }

    #[test]
    fn plugin_manifest_allows_only_the_bridge_port() {
        let manifest: Value = serde_json::from_str(PLUGIN_MANIFEST).unwrap();
        let expected = json!([format!("ws://localhost:{FIGMA_BRIDGE_PORT}")]);
        assert_eq!(manifest["networkAccess"]["allowedDomains"], expected);
        assert_eq!(manifest["networkAccess"]["devAllowedDomains"], expected);
        assert_eq!(manifest["main"], "code.js");
        assert_eq!(manifest["ui"], "ui.html");
    }

    #[test]
    fn renders_the_plugin_ui_with_the_pairing() {
        let ui = render_plugin_ui(PLUGIN_UI, TOKEN, FIGMA_BRIDGE_PORT).unwrap();
        assert!(ui.contains(TOKEN));
        assert!(ui.contains(&format!("const BRIDGE_PORT = {FIGMA_BRIDGE_PORT};")));
        assert!(!ui.contains(TOKEN_PLACEHOLDER));
        assert!(!ui.contains(PORT_PLACEHOLDER));
        assert!(render_plugin_ui("<html></html>", TOKEN, FIGMA_BRIDGE_PORT).is_err());
        assert!(render_plugin_ui(PLUGIN_UI, "invalid", FIGMA_BRIDGE_PORT).is_err());
    }

    #[test]
    fn installs_and_replaces_the_plugin_atomically() {
        let root = temp_root("install");
        let directory = root.join("figma-plugin");
        install_plugin(&directory, TOKEN, FIGMA_BRIDGE_PORT).unwrap();
        assert!(plugin_installed(&directory, TOKEN));
        let rotated = random_token();
        install_plugin(&directory, &rotated, FIGMA_BRIDGE_PORT).unwrap();
        assert!(plugin_installed(&directory, &rotated));
        assert!(!plugin_installed(&directory, TOKEN));
        assert!(!sibling(&directory, "staging").exists());
        assert!(!sibling(&directory, "backup").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovers_a_complete_backup_after_an_interrupted_install() {
        let root = temp_root("recover");
        let directory = root.join("figma-plugin");
        install_plugin(&directory, TOKEN, FIGMA_BRIDGE_PORT).unwrap();
        let backup = sibling(&directory, "backup");
        fs::rename(&directory, &backup).unwrap();
        fs::create_dir_all(&directory).unwrap();
        fs::File::create(directory.join("manifest.json"))
            .unwrap()
            .write_all(b"{")
            .unwrap();
        recover_interrupted_install(&directory, &backup).unwrap();
        assert!(plugin_installed(&directory, TOKEN));
        assert!(!backup.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stages_a_bundle_without_inline_image_data() {
        let root = temp_root("stage");
        let generation = stage_generation(&root, &bundle()).unwrap();
        let directory = root.join(&generation.id);
        assert!(is_generation_id(&generation.id));
        assert_eq!(generation.preview_bytes, PNG.len() as u64);
        assert_eq!(generation.source.name, "Button");
        assert_eq!(generation.document.page_name, "Buttons");
        assert_eq!(generation.diagnostics, vec!["effects: 12:35".to_string()]);
        assert_eq!(fs::read(directory.join("preview.png")).unwrap(), PNG);
        let asset = "assets/abcdef0123456789abcdef0123456789abcdef01.png";
        assert_eq!(fs::read(directory.join(asset)).unwrap(), PNG);
        let staged = fs::read_to_string(directory.join("source-bundle.json")).unwrap();
        assert!(!staged.contains("imageData"));
        let staged: Value = serde_json::from_str(&staged).unwrap();
        assert_eq!(staged["preview"]["path"], "preview.png");
        assert_eq!(staged["assets"][0]["path"], asset);
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(directory.join("assets").join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["assets"][0]["path"], asset);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_bundles_with_unsafe_images() {
        let root = temp_root("reject");
        let mut fake_preview = bundle();
        fake_preview["preview"]["imageData"] = json!(encode(b"<svg></svg>"));
        assert!(stage_generation(&root, &fake_preview).is_err());
        let mut bad_hash = bundle();
        bad_hash["assets"][0]["hash"] = json!("../../escape");
        assert!(stage_generation(&root, &bad_hash).is_err());
        let mut wrong_type = bundle();
        wrong_type["assets"][0]["mimeType"] = json!("image/svg+xml");
        assert!(stage_generation(&root, &wrong_type).is_err());
        let mut no_tree = bundle();
        no_tree["root"] = Value::Null;
        assert!(stage_generation(&root, &no_tree).is_err());
        let staged = fs::read_dir(&root)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(staged, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepares_a_git_ignored_preview_inside_the_project() {
        let root = temp_root("preview");
        let generations = root.join("generations");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let generation = stage_generation(&generations, &bundle()).unwrap();
        let workspace = prepare_preview(&generations, &generation.id, &project).unwrap();
        let directory = project.join(".monocode").join("figma").join(&generation.id);
        assert_eq!(
            workspace,
            FigmaPreviewWorkspace {
                directory: crate::fs::path_to_js(&directory),
                relative_directory: format!(".monocode/figma/{}", generation.id),
                preview_path: crate::fs::path_to_js(&directory.join("design").join("preview.png")),
            }
        );
        assert_eq!(
            fs::read_to_string(project.join(".monocode").join(".gitignore")).unwrap(),
            "*\n"
        );
        assert_eq!(
            fs::read(directory.join("design").join("preview.png")).unwrap(),
            PNG
        );
        assert!(directory
            .join("design")
            .join("source-bundle.json")
            .is_file());
        assert!(directory
            .join("design")
            .join("assets")
            .join("manifest.json")
            .is_file());
        fs::write(directory.join("Button.tsx"), "export {}").unwrap();
        prepare_preview(&generations, &generation.id, &project).unwrap();
        assert!(directory.join("Button.tsx").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adds_the_preview_rule_to_an_existing_ignore_file() {
        let root = temp_root("preview-ignore");
        let generations = root.join("generations");
        let project = root.join("project");
        let ignore = project.join(".monocode").join(".gitignore");
        fs::create_dir_all(project.join(".monocode")).unwrap();
        let generation = stage_generation(&generations, &bundle()).unwrap();
        for (before, after) in [
            ("custom\n", "custom\nfigma/\n"),
            ("custom", "custom\nfigma/\n"),
            ("", "figma/\n"),
            ("custom\nfigma/\n", "custom\nfigma/\n"),
            ("*\n", "*\n"),
        ] {
            fs::write(&ignore, before).unwrap();
            prepare_preview(&generations, &generation.id, &project).unwrap();
            prepare_preview(&generations, &generation.id, &project).unwrap();
            assert_eq!(fs::read_to_string(&ignore).unwrap(), after);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_previews_it_cannot_place_safely() {
        let root = temp_root("preview-refuse");
        let generations = root.join("generations");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        let generation = stage_generation(&generations, &bundle()).unwrap();
        assert!(prepare_preview(&generations, "../escape", &project).is_err());
        assert!(prepare_preview(&generations, "0000000000000-deadbeef", &project).is_err());
        assert!(prepare_preview(&generations, &generation.id, &root.join("missing")).is_err());
        let odd = root.join("odd");
        fs::create_dir_all(odd.join(".monocode").join(".gitignore")).unwrap();
        assert!(prepare_preview(&generations, &generation.id, &odd).is_err());
        fs::write(project.join(".monocode"), "not a folder").unwrap();
        assert!(prepare_preview(&generations, &generation.id, &project).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn will_not_follow_a_linked_preview_folder() {
        let root = temp_root("preview-link");
        let generations = root.join("generations");
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, project.join(".monocode")).unwrap();
        let generation = stage_generation(&generations, &bundle()).unwrap();
        assert!(prepare_preview(&generations, &generation.id, &project).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn will_not_write_through_a_linked_ignore_file() {
        let root = temp_root("preview-ignore-link");
        let generations = root.join("generations");
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(project.join(".monocode")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(
            outside.join("planted"),
            project.join(".monocode").join(".gitignore"),
        )
        .unwrap();
        let generation = stage_generation(&generations, &bundle()).unwrap();
        assert!(prepare_preview(&generations, &generation.id, &project).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_only_recent_generations() {
        let root = temp_root("prune");
        for index in 0..GENERATIONS_KEPT + 3 {
            fs::create_dir(root.join(format!("{index:013}-{index:08x}"))).unwrap();
        }
        fs::create_dir(root.join("unrelated")).unwrap();
        prune_generations(&root, "none");
        let mut names: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        assert_eq!(names.len(), GENERATIONS_KEPT + 1);
        assert!(names.contains(&"unrelated".to_string()));
        assert!(!names.contains(&format!("{:013}-{:08x}", 0, 0)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn captures_the_selection_or_one_layer_by_id() {
        assert_eq!(capture_params(None).unwrap(), json!({}));
        assert_eq!(
            capture_params(Some(" 12:34 ")).unwrap(),
            json!({ "nodeId": "12:34" })
        );
        assert_eq!(
            capture_params(Some("I12:34;56:78")).unwrap(),
            json!({ "nodeId": "I12:34;56:78" })
        );
        assert!(capture_params(Some("")).is_err());
        assert!(capture_params(Some("../12:34")).is_err());
    }

    #[test]
    fn validates_live_selections() {
        let selection = parse_selection(Some(&json!({
            "selectionCount": 1,
            "document": document(),
            "source": { "nodeId": "1:2", "name": "Card", "type": "FRAME", "width": 320, "height": 200 },
        })))
        .unwrap();
        assert_eq!(selection.source.unwrap().node_type, "FRAME");
        let empty = parse_selection(Some(
            &json!({ "selectionCount": 0, "document": document(), "source": null }),
        ))
        .unwrap();
        assert!(empty.source.is_none());
        assert!(parse_selection(Some(&json!({
            "selectionCount": 2,
            "document": document(),
            "source": { "nodeId": "1:2", "name": "Card", "type": "FRAME", "width": 320, "height": 200 },
        })))
        .is_err());
        assert!(parse_selection(Some(&json!({
            "selectionCount": 1,
            "document": document(),
            "source": { "nodeId": "1:2", "name": "Card", "type": "FRAME\" and run \"rm", "width": 320, "height": 200 },
        })))
        .is_err());
        assert!(parse_selection(Some(&json!({
            "selectionCount": 1,
            "document": document(),
            "source": { "nodeId": "1:2", "name": "Card", "type": "FRAME", "width": -1, "height": 200 },
        })))
        .is_err());
        assert!(parse_selection(Some(&json!({ "selectionCount": 0 }))).is_err());
    }

    #[test]
    fn preview_requires_png_bytes() {
        let preview =
            preview_data_url(&json!({ "mimeType": "image/png", "imageData": encode(PNG) }))
                .unwrap();
        assert!(preview.starts_with("data:image/png;base64,"));
        assert!(preview_data_url(
            &json!({ "mimeType": "image/png", "imageData": encode(b"GIF89a") })
        )
        .is_err());
        assert!(preview_data_url(
            &json!({ "mimeType": "image/svg+xml", "imageData": encode(PNG) })
        )
        .is_err());
    }

    fn connect(port: u16, token: &str, origin: &str) -> Result<WebSocket<TcpStream>, String> {
        let stream =
            TcpStream::connect((Ipv4Addr::LOCALHOST, port)).map_err(|error| error.to_string())?;
        let mut request = format!("ws://localhost:{port}/?token={token}")
            .into_client_request()
            .map_err(|error| error.to_string())?;
        request
            .headers_mut()
            .insert("origin", origin.parse().map_err(|_| "origin".to_string())?);
        tungstenite::client(request, stream)
            .map(|(socket, _)| socket)
            .map_err(|error| error.to_string())
    }

    fn read_json(socket: &mut WebSocket<TcpStream>) -> Value {
        loop {
            if let Message::Text(text) = socket.read().unwrap() {
                return serde_json::from_str(text.as_str()).unwrap();
            }
        }
    }

    #[test]
    fn plugin_session_publishes_selection_and_answers_requests() {
        let root = temp_root("session");
        let bridge = quiet_bridge(&root);
        let port = bridge.start(0).unwrap();
        assert!(connect(port, &random_token(), "null").is_err());
        let mut socket = connect(port, TOKEN, "null").unwrap();
        let hello = read_json(&mut socket);
        assert_eq!(hello["type"], "hello");
        let session_token = hello["sessionToken"].as_str().unwrap().to_string();
        socket
            .send(Message::text(
                json!({ "v": 1, "type": "join", "role": "figma-plugin", "channel": "monocode-test", "sessionToken": "wrong" })
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(read_json(&mut socket)["type"], "error");
        socket
            .send(Message::text(
                json!({ "v": 1, "type": "join", "role": "figma-plugin", "channel": "monocode-test", "sessionToken": session_token })
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(read_json(&mut socket)["message"]["result"], true);
        socket
            .send(Message::text(
                json!({
                    "v": 1, "id": "s1", "type": "message", "channel": "monocode-test",
                    "message": { "id": "s1", "command": "selection_changed", "params": { "selection": {
                        "selectionCount": 1,
                        "document": document(),
                        "source": { "nodeId": "1:2", "name": "Card", "type": "FRAME", "width": 320, "height": 200 },
                    } } },
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            read_json(&mut socket)["message"]["result"]["accepted"],
            true
        );
        let connections = bridge.connections();
        assert_eq!(connections.len(), 1);
        let connection_id = connections[0].id.clone();
        assert_eq!(
            connections[0]
                .selection
                .as_ref()
                .unwrap()
                .source
                .as_ref()
                .unwrap()
                .name,
            "Card"
        );
        let requester = bridge.clone();
        let requested = std::thread::spawn(move || {
            requester.request(
                &connection_id,
                "get_selection",
                json!({}),
                Duration::from_secs(5),
            )
        });
        let command = read_json(&mut socket);
        assert_eq!(command["message"]["command"], "get_selection");
        let id = command["message"]["id"].as_str().unwrap().to_string();
        socket
            .send(Message::text(
                json!({ "v": 1, "id": id, "type": "message", "channel": "monocode-test", "message": { "id": id, "result": { "selectionCount": 0 } } })
                    .to_string(),
            ))
            .unwrap();
        assert_eq!(requested.join().unwrap().unwrap()["selectionCount"], 0);
        bridge.stop();
        drop(socket);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_a_generation_no_window_can_take() {
        let root = temp_root("undelivered");
        let bridge = FigmaBridge::new(
            root.to_path_buf(),
            true,
            TOKEN.into(),
            Arc::new(|notice| !matches!(notice, Notice::Generation(_))),
        );
        let port = bridge.start(0).unwrap();
        let mut socket = joined_plugin(port);
        socket
            .send(Message::text(
                json!({
                    "v": 1, "id": "g1", "type": "message", "channel": "monocode-test",
                    "message": { "id": "g1", "command": "generate_code_from_selection", "params": { "bundle": bundle() } },
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            read_json(&mut socket)["message"]["error"],
            "Open a MonoCode window, then generate the component again."
        );
        assert_eq!(
            fs::read_dir(&root)
                .map(|entries| entries.count())
                .unwrap_or(0),
            0
        );
        bridge.stop();
        drop(socket);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_browser_pages_from_other_origins() {
        let root = temp_root("origin");
        let bridge = quiet_bridge(&root);
        let port = bridge.start(0).unwrap();
        assert!(connect(port, TOKEN, "https://evil.example").is_err());
        bridge.stop();
        fs::remove_dir_all(root).unwrap();
    }
}
