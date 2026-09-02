use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State as AxumState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse, InvokeResponseBody};
use tauri::webview::InvokeRequest;
use tauri::{AppHandle, Manager, State, Wry};
use tokio::sync::{broadcast, mpsc, oneshot};

pub const DEFAULT_PORT: u16 = 7847;

const BLOCKED_COMMANDS: &[&str] = &[
    "confirm_quit",
    "hide_window",
    "destroy_window",
    "open_new_window",
    "set_traffic_lights_visible",
    "set_window_background_blur",
    "set_dock_badge",
    "enable_window_glass",
    "stage_window_transfer",
    "take_window_transfer",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    pub urls: Vec<String>,
}

#[derive(Clone)]
struct RemoteRuntime {
    app: AppHandle<Wry>,
    token: Arc<Mutex<String>>,
    port: u16,
    running: Arc<AtomicBool>,
    event_tx: broadcast::Sender<RemoteEvent>,
    shutdown_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

#[derive(Clone, Serialize)]
struct RemoteEvent {
    name: String,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct WsAuthQuery {
    token: Option<String>,
}

#[derive(Deserialize)]
struct ClientMessage {
    #[serde(rename = "type")]
    kind: String,
    id: Option<u64>,
    command: Option<String>,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Serialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
}

pub struct RemoteHost {
    inner: Mutex<Option<RemoteRuntime>>,
}

impl RemoteHost {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    fn runtime(&self) -> Option<RemoteRuntime> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_runtime(&self, runtime: Option<RemoteRuntime>) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = runtime;
    }
}

impl Default for RemoteHost {
    fn default() -> Self {
        Self::new()
    }
}

pub fn fanout_event(app: &AppHandle<Wry>, event: &str, payload: impl Serialize) {
    let Ok(payload) = serde_json::to_value(payload) else {
        return;
    };
    if let Some(host) = app.try_state::<RemoteHost>() {
        if let Some(runtime) = host.runtime() {
            let _ = runtime.event_tx.send(RemoteEvent {
                name: event.to_string(),
                payload,
            });
        }
    }
}

#[tauri::command]
pub fn remote_status(host: State<'_, RemoteHost>) -> RemoteStatus {
    host.runtime()
        .map(|runtime| runtime.status())
        .unwrap_or_else(|| RemoteStatus {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
            urls: vec![],
        })
}

#[tauri::command]
pub async fn remote_start(
    app: AppHandle<Wry>,
    host: State<'_, RemoteHost>,
    port: Option<u16>,
    token: Option<String>,
) -> Result<RemoteStatus, String> {
    if host.runtime().is_some() {
        return Ok(host.runtime().unwrap().status());
    }

    let port = port.unwrap_or(DEFAULT_PORT);
    let token = Arc::new(Mutex::new(
        token
            .map(|value| normalize_token(&value))
            .transpose()?
            .unwrap_or_else(new_token),
    ));
    let running = Arc::new(AtomicBool::new(true));
    let (event_tx, _) = broadcast::channel(512);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let runtime = RemoteRuntime {
        app: app.clone(),
        token: Arc::clone(&token),
        port,
        running: Arc::clone(&running),
        event_tx,
        shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
    };

    let serve_runtime = runtime.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_server(serve_runtime, shutdown_rx).await {
            eprintln!("MonoCode remote server stopped: {error}");
        }
    });

    host.set_runtime(Some(runtime.clone()));
    Ok(runtime.status())
}

#[tauri::command]
pub async fn remote_stop(host: State<'_, RemoteHost>) -> Result<RemoteStatus, String> {
    if let Some(runtime) = host.runtime() {
        runtime.stop();
    }
    host.set_runtime(None);
    Ok(RemoteStatus {
        enabled: false,
        port: DEFAULT_PORT,
        token: String::new(),
        urls: vec![],
    })
}

#[tauri::command]
pub fn remote_regenerate_token(host: State<'_, RemoteHost>) -> Result<RemoteStatus, String> {
    let runtime = host
        .runtime()
        .ok_or_else(|| "Remote access is not enabled".to_string())?;
    *runtime.token.lock().unwrap_or_else(|e| e.into_inner()) = new_token();
    Ok(runtime.status())
}

#[tauri::command]
pub fn remote_set_token(
    host: State<'_, RemoteHost>,
    token: String,
) -> Result<RemoteStatus, String> {
    let runtime = host
        .runtime()
        .ok_or_else(|| "Remote access is not enabled".to_string())?;
    let next = normalize_token(&token)?;
    *runtime.token.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(runtime.status())
}

impl RemoteRuntime {
    fn status(&self) -> RemoteStatus {
        RemoteStatus {
            enabled: self.running.load(Ordering::SeqCst),
            port: self.port,
            token: self.token.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            urls: access_urls(self.port),
        }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(tx) = self
            .shutdown_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = tx.try_send(());
        }
    }

    fn authorized(&self, token: Option<&str>) -> bool {
        let expected = self.token.lock().unwrap_or_else(|e| e.into_inner());
        token.is_some_and(|value| value == expected.as_str())
    }
}

async fn run_server(
    runtime: RemoteRuntime,
    mut shutdown_rx: mpsc::Receiver<()>,
) -> Result<(), String> {
    let addr = SocketAddr::from(([0, 0, 0, 0], runtime.port));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|error| {
        format!(
            "Failed to bind remote server on port {}: {error}",
            runtime.port
        )
    })?;

    let app = Router::new()
        .route("/api/remote/status", get(health))
        .route("/api/remote/ws", get(ws_handler))
        .fallback(any(serve_asset))
        .with_state(runtime.clone());

    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.recv().await;
    });

    server
        .await
        .map_err(|error| format!("Remote server error: {error}"))
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsAuthQuery>,
    AxumState(runtime): AxumState<RemoteRuntime>,
) -> impl IntoResponse {
    if !runtime.authorized(query.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, runtime))
}

async fn handle_socket(socket: WebSocket, runtime: RemoteRuntime) {
    let (mut sender, mut receiver) = socket.split();
    let mut event_rx = runtime.event_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let response = match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(message) => handle_client_message(&runtime, message).await,
                            Err(error) => ServerMessage {
                                kind: "error".into(),
                                id: None,
                                ok: Some(false),
                                result: None,
                                error: Some(format!("Invalid message: {error}")),
                                name: None,
                                payload: None,
                            },
                        };
                        if let Ok(payload) = serde_json::to_string(&response) {
                            if sender.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            event = event_rx.recv() => {
                match event {
                    Ok(event) => {
                        let message = ServerMessage {
                            kind: "event".into(),
                            id: None,
                            ok: None,
                            result: None,
                            error: None,
                            name: Some(event.name),
                            payload: Some(event.payload),
                        };
                        if let Ok(payload) = serde_json::to_string(&message) {
                            if sender.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn handle_client_message(runtime: &RemoteRuntime, message: ClientMessage) -> ServerMessage {
    match message.kind.as_str() {
        "ping" => ServerMessage {
            kind: "pong".into(),
            id: message.id,
            ok: None,
            result: None,
            error: None,
            name: None,
            payload: None,
        },
        "invoke" => {
            let command = match message.command {
                Some(command) => command,
                None => {
                    return ServerMessage {
                        kind: "invoke".into(),
                        id: message.id,
                        ok: Some(false),
                        result: None,
                        error: Some("Missing command".into()),
                        name: None,
                        payload: None,
                    };
                }
            };

            if BLOCKED_COMMANDS.contains(&command.as_str()) {
                return ServerMessage {
                    kind: "invoke".into(),
                    id: message.id,
                    ok: Some(false),
                    result: None,
                    error: Some(format!(
                        "Command `{command}` is not available over remote access"
                    )),
                    name: None,
                    payload: None,
                };
            }

            match dispatch_invoke(&runtime.app, &command, message.args).await {
                Ok(result) => ServerMessage {
                    kind: "invoke".into(),
                    id: message.id,
                    ok: Some(true),
                    result: Some(result),
                    error: None,
                    name: None,
                    payload: None,
                },
                Err(error) => ServerMessage {
                    kind: "invoke".into(),
                    id: message.id,
                    ok: Some(false),
                    result: None,
                    error: Some(error),
                    name: None,
                    payload: None,
                },
            }
        }
        _ => ServerMessage {
            kind: "error".into(),
            id: message.id,
            ok: Some(false),
            result: None,
            error: Some(format!("Unknown message type `{}`", message.kind)),
            name: None,
            payload: None,
        },
    }
}

async fn dispatch_invoke(
    app: &AppHandle<Wry>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let app = app.clone();
    let command = command.to_string();
    let (tx, rx) = oneshot::channel();
    let app_dispatch = app.clone();

    app.run_on_main_thread(move || {
        let result = invoke_on_main(app_dispatch, &command, args);
        let _ = tx.send(result);
    })
    .map_err(|error| format!("Failed to dispatch invoke: {error}"))?;

    rx.await
        .map_err(|_| "Remote invoke was cancelled".to_string())?
}

fn invoke_on_main(
    app: AppHandle<Wry>,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window is not available".to_string())?;
    let url = window
        .url()
        .map_err(|error| format!("Failed to resolve main window URL: {error}"))?;
    let invoke_key = app.invoke_key().to_string();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);

    let request = InvokeRequest {
        cmd: command.to_string(),
        callback: CallbackFn(0),
        error: CallbackFn(0),
        url,
        body: InvokeBody::Json(args),
        headers: HeaderMap::new(),
        invoke_key,
    };

    window.on_message(
        request,
        Box::new(move |_webview, _cmd, response, _callback, _error| {
            let _ = tx.send(response_to_json(response));
        }),
    );

    rx.recv()
        .map_err(|_| "Remote invoke did not respond".to_string())?
}

fn response_to_json(response: InvokeResponse) -> Result<serde_json::Value, String> {
    match response {
        InvokeResponse::Ok(body) => match body {
            InvokeResponseBody::Json(raw) => serde_json::from_str(&raw)
                .map_err(|error| format!("Invalid JSON response: {error}")),
            InvokeResponseBody::Raw(bytes) => Ok(serde_json::Value::String(
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
            )),
        },
        InvokeResponse::Err(error) => Err(match error.0 {
            serde_json::Value::String(message) => message,
            other => other.to_string(),
        }),
    }
}

async fn serve_asset(
    AxumState(runtime): AxumState<RemoteRuntime>,
    request: axum::http::Request<Body>,
) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(asset) = runtime.app.asset_resolver().get(path.to_string()) {
        return asset_response(asset.bytes, asset.mime_type);
    }

    if !path.contains('.') {
        if let Some(asset) = runtime.app.asset_resolver().get("index.html".to_string()) {
            return asset_response(asset.bytes, asset.mime_type);
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

fn asset_response(bytes: Vec<u8>, mime_type: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn normalize_token(token: &str) -> Result<String, String> {
    let trimmed = token.trim();
    if trimmed.len() < 8 {
        return Err("Access token must be at least 8 characters".to_string());
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err("Access token cannot contain spaces".to_string());
    }
    Ok(trimmed.to_string())
}

fn new_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn access_urls(port: u16) -> Vec<String> {
    let mut urls = Vec::new();
    urls.push(format!("http://127.0.0.1:{port}"));
    if let Ok(name) = hostname::get() {
        if let Some(host) = name.to_str().filter(|value| !value.is_empty()) {
            urls.push(format!("http://{host}:{port}"));
        }
    }
    if let Some(ip) = guess_local_ip() {
        let url = format!("http://{ip}:{port}");
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

fn guess_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() {
        return None;
    }
    Some(ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_commands_include_window_controls() {
        assert!(BLOCKED_COMMANDS.contains(&"open_new_window"));
        assert!(BLOCKED_COMMANDS.contains(&"confirm_quit"));
    }

    #[test]
    fn access_urls_include_loopback() {
        let urls = access_urls(7847);
        assert!(urls.iter().any(|url| url.contains("127.0.0.1")));
    }

    #[test]
    fn normalize_token_rejects_short_values() {
        assert!(normalize_token("short").is_err());
        assert!(normalize_token("12345678").is_ok());
    }

    #[test]
    fn new_token_is_uuid_v4() {
        let token = new_token();
        assert!(uuid::Uuid::parse_str(&token).is_ok());
    }
}
