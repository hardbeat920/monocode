// Companion WebSocket server (host side).
//
// MERGE NOTE: new file; calls into existing modules without changing them.
// Serves the protocol documented in src/lib/transport/protocol.ts:
//
//   ws://<host>:<port>/v1/connect?token=<pairing-token>
//   client -> host  { id, type: "invoke", command, args? }
//   host   -> client { id, type: "result", ok, payload?/error? }
//   host   -> client { type: "event", event, payload }
//
// Auth is a bearer pairing token compared in constant time. Transport
// security comes from the link, not this server:
// - direct LAN: token-gated ws:// on a trusted network;
// - Tailscale: `tailscale serve --bg --https=443 http://localhost:<port>`
//   terminates outer TLS (auto cert) and proxies the WebSocket upgrade;
//   the companion then dials wss://<machine>.<tailnet>.ts.net:443.
// WireGuard already encrypts tailnet traffic, so the token is the only
// credential in both modes (same as Tailscale's own serve model).

use std::collections::HashMap;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use http::{Request, Response};
use tauri::{AppHandle, Listener, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

const HEALTH_BODY: &str = "MONOCODE-COMPANION";
const NOT_FOUND_RESPONSE: &str =
    "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found";

fn health_response() -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{HEALTH_BODY}",
        HEALTH_BODY.len()
    )
    .into_bytes()
}

/// Backend events rebroadcast to companions. Window/menu chrome is host-local
/// and is deliberately not forwarded.
const FORWARDED_EVENTS: &[&str] = &[
    "harness-stdout",
    "harness-stderr",
    "harness-exit",
    "harness-sse",
    "harness-sse-end",
    "pty-data",
    "pty-exit",
    "session-store-changed",
];

/// Connected companions. Shared between the accept loop (registers peers)
/// and the global Tauri event forwarders (broadcasts to peers).
pub struct ServerShared {
    peers: Mutex<HashMap<u64, UnboundedSender<String>>>,
    next_id: AtomicU64,
}

impl ServerShared {
    pub fn new() -> Self {
        Self {
            peers: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn add(&self, sender: UnboundedSender<String>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, sender);
        id
    }

    fn remove(&self, id: u64) {
        self.peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    pub fn peer_count(&self) -> usize {
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn broadcast(&self, frame: String) {
        let peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        for sender in peers.values() {
            // A full peer eventually drops its receiver; dead senders are
            // reaped on disconnect, so delivery here is best-effort.
            let _ = sender.send(frame.clone());
        }
    }
}

/// Register global forwarders once per process. They broadcast to whatever
/// peers are connected (none when the link is disabled: a no-op).
pub fn register_event_forwarding(app: &AppHandle, shared: &Arc<ServerShared>) {
    for name in FORWARDED_EVENTS {
        let event = (*name).to_string();
        let peers = Arc::clone(shared);
        app.listen(event.clone(), move |tauri_event| {
            // `payload()` is the raw JSON string ("" when the emitter sent none).
            let raw = tauri_event.payload();
            let payload: serde_json::Value =
                serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
            let frame = serde_json::json!({
                "type": "event",
                "event": event,
                "payload": payload,
            });
            peers.broadcast(frame.to_string());
        });
    }
}

pub async fn run(app: AppHandle, listener: TcpListener, token: String, shared: Arc<ServerShared>) {
    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        eprintln!("companion: connection from {addr}");
        let app = app.clone();
        let token = token.clone();
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            route_connection(app, stream, token, shared, addr).await;
        });
    }
}

/// Bytes already read from `inner` (the HTTP request head) plus the rest
/// of the socket, so a WebSocket handshake can consume the same request.
struct PrefixedStream<S> {
    prefix: Cursor<Vec<u8>>,
    inner: S,
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let pos = self.prefix.position() as usize;
        let len = self.prefix.get_ref().len();
        if pos < len {
            let rest = &self.prefix.get_ref()[pos..];
            let n = rest.len().min(buf.remaining());
            buf.put_slice(&rest[..n]);
            self.prefix.set_position((pos + n) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn read_http_head<S: AsyncRead + Unpin>(stream: &mut S) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16_384 {
            break;
        }
    }
    Ok(buf)
}

fn is_websocket_upgrade(head: &[u8]) -> bool {
    std::str::from_utf8(head)
        .unwrap_or("")
        .to_ascii_lowercase()
        .lines()
        .any(|line| line.starts_with("upgrade:") && line.contains("websocket"))
}

#[derive(Debug, PartialEq, Eq)]
enum HeadKind {
    Health,
    Other,
}

fn classify_http_head(head: &[u8]) -> HeadKind {
    let text = std::str::from_utf8(head).unwrap_or("");
    let line = text.split(['\r', '\n']).next().unwrap_or("");
    let path = line.split_whitespace().nth(1).unwrap_or("");
    if line.starts_with("GET ") && (path == "/" || path == "/health") {
        HeadKind::Health
    } else {
        HeadKind::Other
    }
}

async fn route_connection<S>(
    app: AppHandle,
    mut stream: S,
    token: String,
    shared: Arc<ServerShared>,
    addr: impl std::fmt::Display,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let head = match tokio::time::timeout(Duration::from_secs(3), read_http_head(&mut stream)).await
    {
        Ok(Ok(head)) if !head.is_empty() => head,
        Ok(Ok(head)) => {
            eprintln!(
                "companion: empty request from {addr} ({} bytes)",
                head.len()
            );
            return;
        }
        Ok(Err(error)) => {
            eprintln!("companion: read failed from {addr}: {error}");
            return;
        }
        Err(_) => {
            eprintln!("companion: timed out waiting for request from {addr}");
            return;
        }
    };
    let first = std::str::from_utf8(&head)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("");
    eprintln!("companion: request from {addr}: {first}");
    if is_websocket_upgrade(&head) {
        let replay = PrefixedStream {
            prefix: Cursor::new(head),
            inner: stream,
        };
        eprintln!("companion: websocket upgrade from {addr}");
        serve_connection(app, replay, token, shared).await;
        eprintln!("companion: websocket closed from {addr}");
        return;
    }
    if classify_http_head(&head) == HeadKind::Health {
        let _ = stream.write_all(&health_response()).await;
        let _ = stream.shutdown().await;
        return;
    }
    eprintln!("companion: rejected non-websocket from {addr}");
    let _ = stream.write_all(NOT_FOUND_RESPONSE.as_bytes()).await;
    let _ = stream.shutdown().await;
}

type ErrorResponse = Response<Option<String>>;

fn reject(status: u16, message: &str) -> ErrorResponse {
    Response::builder()
        .status(status)
        .body(Some(message.to_string()))
        .unwrap_or_else(|_| Response::new(None))
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some(key) {
            return parts.next().map(str::to_string);
        }
    }
    None
}

fn tokens_match(presented: &str, expected: &str) -> bool {
    if presented.len() != expected.len() || presented.is_empty() {
        return false;
    }
    presented
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// Pure upgrade gate so the handshake policy is unit-testable without a
/// socket: right path + (bearer pairing token OR pair mode). Pair mode
/// (`?pair=1`, no token) accepts the socket but leaves it unpaired: it may
/// only invoke `pair_claim` until the 6-digit code checks out.
#[derive(Debug, PartialEq, Eq)]
enum Upgrade {
    Paired,
    Pairing,
}

fn check_upgrade(
    path: &str,
    query: Option<&str>,
    expected_token: &str,
) -> Result<Upgrade, (u16, &'static str)> {
    if path != super::remote::COMPANION_WS_PATH {
        return Err((404, "not found"));
    }
    let ok = query_param(query, "token")
        .map(|presented| tokens_match(&presented, expected_token))
        .unwrap_or(false);
    if ok {
        return Ok(Upgrade::Paired);
    }
    if query_param(query, "pair").as_deref() == Some("1") {
        return Ok(Upgrade::Pairing);
    }
    Err((401, "bad pairing token"))
}

// The handshake callback's Err type is fixed by tungstenite (an HTTP
// response); boxing it would only complicate the accept path.
#[allow(clippy::result_large_err)]
pub async fn serve_connection<S>(
    app: AppHandle,
    stream: S,
    token: String,
    shared: Arc<ServerShared>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // The handshake callback cannot return extra state (tungstenite fixes
    // its signature), so the pair/token decision travels out via shared
    // state the FnOnce closure moves in.
    let decided = Arc::new(std::sync::Mutex::new(None));
    let decide_in = Arc::clone(&decided);
    let ws = match tokio_tungstenite::accept_hdr_async(
        stream,
        move |request: &Request<()>, response: Response<()>| {
            let uri = request.uri();
            match check_upgrade(uri.path(), uri.query(), &token) {
                Ok(mode) => {
                    *decide_in.lock().unwrap_or_else(|e| e.into_inner()) = Some(mode);
                    Ok(response)
                }
                Err((status, message)) => Err(reject(status, message)),
            }
        },
    )
    .await
    {
        Ok(ws) => ws,
        Err(error) => {
            eprintln!("companion: handshake failed: {error}");
            return;
        }
    };
    let pairing = matches!(
        *decided.lock().unwrap_or_else(|e| e.into_inner()),
        Some(Upgrade::Pairing)
    );
    let paired = Arc::new(std::sync::atomic::AtomicBool::new(!pairing));

    let (mut sink, mut incoming) = ws.split();
    let (outgoing, mut mailbox) = unbounded_channel::<String>();
    let peer_id = shared.add(outgoing.clone());

    let writer = tokio::spawn(async move {
        while let Some(frame) = mailbox.recv().await {
            if sink.send(Message::Text(frame.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let mut claim_failures = 0u8;
    while let Some(message) = incoming.next().await {
        match message {
            Ok(Message::Text(text)) => {
                if handle_request(&app, &text, &outgoing, &paired).await {
                    claim_failures = 0;
                } else {
                    // Rejected on an unpaired socket: only pair_claim gets
                    // answers, and only a few wrong guesses before we hang up.
                    claim_failures += 1;
                    if claim_failures >= 6 {
                        break;
                    }
                }
            }
            Ok(Message::Binary(_)) => {
                // Protocol is JSON text only; ignore binary frames.
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    shared.remove(peer_id);
    // `outgoing` (our clone) drops here, closing the mailbox so the writer
    // task exits even if the sink close above raced it.
    drop(outgoing);
    writer.abort();
}

/// Handle one frame. Returns true when the frame was answered (or intentionally
/// absorbed on a paired socket); false when an unpaired socket sent anything
/// but a claim — the caller counts those toward hanging up.
async fn handle_request(
    app: &AppHandle,
    text: &str,
    outgoing: &UnboundedSender<String>,
    paired: &std::sync::atomic::AtomicBool,
) -> bool {
    let request: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return true,
    };
    let id = request.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let command = request
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // Malformed frames are ignored; the client's per-request timeout fires.
    if id == 0 || command.is_empty() {
        return true;
    }
    let args = request
        .get("args")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if command == "pair_claim" && !paired.load(std::sync::atomic::Ordering::SeqCst) {
        return handle_claim(app, id, &args, outgoing, paired).await;
    }
    if !paired.load(std::sync::atomic::Ordering::SeqCst) {
        // Unpaired sockets get exactly one command. Anything else is a probe.
        return false;
    }
    let frame = match super::remote_dispatch::dispatch_command(app, command, args).await {
        Ok(payload) => serde_json::json!({
            "id": id,
            "type": "result",
            "ok": true,
            "payload": payload,
        }),
        Err(error) => serde_json::json!({
            "id": id,
            "type": "result",
            "ok": false,
            "error": error,
        }),
    };
    let _ = outgoing.send(frame.to_string());
    true
}

/// Claim a pairing code on an unpaired socket. Success upgrades the socket
/// to fully paired and hands over the real token; the iPad then reconnects
/// (or continues) as a normal token-authenticated client.
async fn handle_claim(
    app: &AppHandle,
    id: u64,
    args: &serde_json::Value,
    outgoing: &UnboundedSender<String>,
    paired: &std::sync::atomic::AtomicBool,
) -> bool {
    let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let state: tauri::State<'_, crate::remote::RemoteState> = app.state();
    let frame = match crate::remote::verify_pair_code(&state, code) {
        Ok(()) => match state.pairing_token(app) {
            Ok(token) => {
                paired.store(true, std::sync::atomic::Ordering::SeqCst);
                let hosts = state.advertised_hosts();
                let mut payload = serde_json::json!({ "token": token });
                if let Some(obj) = payload.as_object_mut() {
                    if let Some(lan_ip) = hosts.lan_ip {
                        obj.insert("lanIp".into(), serde_json::Value::String(lan_ip));
                    }
                    if let Some(tailnet_host) = hosts.tailnet_host {
                        obj.insert(
                            "tailnetHost".into(),
                            serde_json::Value::String(tailnet_host),
                        );
                    }
                }
                serde_json::json!({
                    "id": id,
                    "type": "result",
                    "ok": true,
                    "payload": payload,
                })
            }
            Err(error) => serde_json::json!({
                "id": id,
                "type": "result",
                "ok": false,
                "error": error,
            }),
        },
        Err(error) => serde_json::json!({
            "id": id,
            "type": "result",
            "ok": false,
            "error": error,
        }),
    };
    let claimed = frame.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let _ = outgoing.send(frame.to_string());
    claimed
}

/// Re-exported for tests that only need frame helpers.
#[allow(dead_code)]
pub fn parse_query_token(query: Option<&str>) -> Option<String> {
    query_param(query, "token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare_is_exact() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc123", "abc12"));
        assert!(!tokens_match("", ""));
    }

    #[test]
    fn query_token_parses() {
        assert_eq!(
            parse_query_token(Some("token=abc&v=1")),
            Some("abc".to_string())
        );
        assert_eq!(parse_query_token(Some("v=1")), None);
        assert_eq!(parse_query_token(None), None);
    }

    #[test]
    fn upgrade_gate() {
        let token = "0123456789abcdef0123456789abcdef0123456789abc";
        assert!(check_upgrade(
            "/v1/connect",
            Some("token=0123456789abcdef0123456789abcdef0123456789abc&v=1"),
            token
        )
        .is_ok());
        // Pair mode upgrades without a token but starts unpaired.
        assert_eq!(
            check_upgrade("/v1/connect", Some("pair=1"), token),
            Ok(Upgrade::Pairing)
        );
        // Wrong token, missing token, empty token.
        assert_eq!(
            check_upgrade("/v1/connect", Some("token=wrong"), token),
            Err((401, "bad pairing token"))
        );
        assert_eq!(
            check_upgrade("/v1/connect", Some("v=1"), token),
            Err((401, "bad pairing token"))
        );
        assert_eq!(
            check_upgrade("/v1/connect", None, token),
            Err((401, "bad pairing token"))
        );
        // Wrong path (e.g. a tailscale-serve health probe on /).
        assert_eq!(
            check_upgrade(
                "/",
                Some("token=0123456789abcdef0123456789abcdef0123456789abc"),
                token
            ),
            Err((404, "not found"))
        );
    }

    #[test]
    fn safari_get_slash_is_health() {
        let head = b"GET / HTTP/1.1\r\nHost: 192.168.4.191:17233\r\n\r\n";
        assert_eq!(classify_http_head(head), HeadKind::Health);
        assert!(!is_websocket_upgrade(head));
        let health = b"GET /health HTTP/1.1\r\nHost: 192.168.4.191:17233\r\n\r\n";
        assert_eq!(classify_http_head(health), HeadKind::Health);
        let ws = b"GET /v1/connect HTTP/1.1\r\nHost: 192.168.4.191:17233\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n";
        assert!(is_websocket_upgrade(ws));
        assert_eq!(classify_http_head(ws), HeadKind::Other);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tungstenite_client_handshakes_through_prefixed_head() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let head = read_http_head(&mut stream).await.unwrap();
            assert!(
                is_websocket_upgrade(&head),
                "head={}",
                String::from_utf8_lossy(&head)
            );
            let replay = PrefixedStream {
                prefix: Cursor::new(head),
                inner: stream,
            };
            tokio_tungstenite::accept_async(replay).await.unwrap();
        });
        let url = format!("ws://{addr}/v1/connect?pair=1");
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request = url.into_client_request().unwrap();
        let (_ws, _response) = tokio_tungstenite::client_async(request, tcp).await.unwrap();
        server.await.unwrap();
    }

    #[test]
    fn forwarded_events_cover_the_streaming_bridges() {
        // The TS bridges in harness/child.ts and pty.ts subscribe to these;
        // dropping one here silently breaks companion realtime sync.
        for name in [
            "harness-stdout",
            "harness-stderr",
            "harness-exit",
            "harness-sse",
            "harness-sse-end",
            "pty-data",
            "pty-exit",
            "session-store-changed",
        ] {
            assert!(
                FORWARDED_EVENTS.contains(&name),
                "missing forwarded event: {name}"
            );
        }
    }
}
