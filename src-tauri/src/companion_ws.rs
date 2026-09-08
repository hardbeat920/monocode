// Native WebSocket client for the companion (iPad / thin client).
//
// WKWebView loads the UI over https://tauri.localhost, so a JS `WebSocket`
// to ws://<lan-ip> is mixed content and is dropped with no useful error.
// Pairing then sits on "Connecting…" forever even though the Mac listener
// is fine. Dialing from Rust uses a real TCP socket and avoids that gate.
//
// The Tauri command itself must not await the socket: on iOS a spawned
// reader that re-enters AppHandle state can stall the command, so JS never
// sees onopen and reconnects forever. Handshake + read/write live in one
// spawned task; JS is notified with `companion-ws` events.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const EVENT: &str = "companion-ws";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Slightly longer than the Go tsnet Dial timeout (8s) so the CGO error
/// surfaces instead of a leaked blocking task.
#[cfg(target_os = "ios")]
const TSNET_DIAL_TIMEOUT: Duration = Duration::from_secs(9);

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ClientEvent {
    Open {
        id: String,
    },
    Message {
        id: String,
        data: String,
    },
    Close {
        id: String,
        code: u16,
        reason: String,
    },
    Error {
        id: String,
        message: String,
    },
}

enum Outgoing {
    Text(String),
    Close,
}

struct Conn {
    tx: UnboundedSender<Outgoing>,
}

type Table = HashMap<String, Conn>;

pub struct CompanionWs {
    inner: Arc<Mutex<Table>>,
}

impl CompanionWs {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn table(&self) -> Arc<Mutex<Table>> {
        Arc::clone(&self.inner)
    }
}

fn lock(table: &Mutex<Table>) -> std::sync::MutexGuard<'_, Table> {
    table.lock().unwrap_or_else(|e| e.into_inner())
}

fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Tailnet CGNAT (100.64/10), Tailscale IPv6 (fd7a:115c:a1e0::/48), or MagicDNS.
#[cfg_attr(not(any(test, target_os = "ios")), allow(dead_code))]
pub fn is_tailnet_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']);
    if host.ends_with(".ts.net") || host.eq_ignore_ascii_case("ts.net") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let o = ip.octets();
            o[0] == 100 && (64..=127).contains(&o[1])
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let s = ip.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
        Err(_) => false,
    }
}

/// Host/port for a `ws://` URL. `wss://` is rejected: LAN and the embedded
/// tailnet listener are both cleartext TCP (WireGuard covers the tailnet).
pub fn parse_ws_target(url: &str) -> Result<(String, u16), String> {
    let uri: http::Uri = url
        .parse()
        .map_err(|error| format!("bad companion url: {error}"))?;
    match uri.scheme_str() {
        Some("ws") => {}
        Some("wss") => {
            return Err(
                "wss:// is not used for LAN or tailnet TCP pairing — scan the LAN code or use the 6-digit code."
                    .into(),
            );
        }
        _ => return Err("companion url must be ws://".into()),
    }
    let host = uri
        .host()
        .ok_or_else(|| "companion url is missing a host".to_string())?
        .to_string();
    let port = uri.port_u16().unwrap_or(80);
    Ok((host, port))
}

fn take_conn(table: &Mutex<Table>, id: &str) -> Option<Conn> {
    lock(table).remove(id)
}

fn emit(app: &AppHandle, event: ClientEvent) {
    let _ = app.emit(EVENT, event);
}

/// Starts a connection. Resolves as soon as the task is spawned; `open` /
/// `error` / `close` arrive on `companion-ws`.
#[tauri::command]
pub fn companion_ws_open(
    app: AppHandle,
    state: State<'_, CompanionWs>,
    id: String,
    url: String,
) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("bad companion socket id".into());
    }
    let _ = parse_ws_target(&url)?;
    let table = state.table();
    if let Some(previous) = take_conn(&table, &id) {
        let _ = previous.tx.send(Outgoing::Close);
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_socket(app.clone(), table, id.clone(), url).await {
            emit(
                &app,
                ClientEvent::Error {
                    id: id.clone(),
                    message: error.clone(),
                },
            );
            emit(
                &app,
                ClientEvent::Close {
                    id,
                    code: 1006,
                    reason: error,
                },
            );
        }
    });
    Ok(())
}

enum CompanionIo {
    Tcp(TcpStream),
    #[cfg(target_os = "ios")]
    Tsnet(tokio::net::UnixStream),
}

impl AsyncRead for CompanionIo {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            CompanionIo::Tcp(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            #[cfg(target_os = "ios")]
            CompanionIo::Tsnet(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for CompanionIo {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            CompanionIo::Tcp(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            #[cfg(target_os = "ios")]
            CompanionIo::Tsnet(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            CompanionIo::Tcp(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            #[cfg(target_os = "ios")]
            CompanionIo::Tsnet(stream) => std::pin::Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            CompanionIo::Tcp(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            #[cfg(target_os = "ios")]
            CompanionIo::Tsnet(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
        }
    }
}

async fn connect_companion(host: &str, port: u16) -> Result<CompanionIo, String> {
    #[cfg(target_os = "ios")]
    if is_tailnet_host(host) {
        let addr = format!("{host}:{port}");
        eprintln!("companion: tailnet dial {addr}");
        match tokio::time::timeout(
            TSNET_DIAL_TIMEOUT,
            tauri::async_runtime::spawn_blocking(move || crate::tsnet_mobile::dial_blocking(&addr)),
        )
        .await
        {
            Ok(Ok(Ok(stream))) => {
                eprintln!("companion: tailnet dial ok {host}:{port}");
                return Ok(CompanionIo::Tsnet(stream));
            }
            Ok(Ok(Err(tsnet_err))) => {
                eprintln!("companion: tailnet dial failed {host}:{port}: {tsnet_err}");
                // Short OS TCP try: a system Tailscale VPN still works.
                // Do not wait the full handshake timeout here — 100.x is
                // unroutable on iOS without that VPN, and pairing then
                // sits on "Connecting…" instead of falling back to LAN.
                match tokio::time::timeout(Duration::from_secs(2), TcpStream::connect((host, port)))
                    .await
                {
                    Ok(Ok(stream)) => {
                        let _ = stream.set_nodelay(true);
                        return Ok(CompanionIo::Tcp(stream));
                    }
                    _ => return Err(tsnet_err),
                }
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) => {
                eprintln!("companion: tailnet dial timed out {host}:{port}");
                return Err(format!(
                    "timed out connecting over tailnet to {host}:{port}"
                ));
            }
        }
    }

    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| format!("timed out connecting to {host}:{port}"))?
        .map_err(|error| format!("could not reach {host}:{port}: {error}"))?;
    let _ = stream.set_nodelay(true);
    Ok(CompanionIo::Tcp(stream))
}

async fn run_socket(
    app: AppHandle,
    table: Arc<Mutex<Table>>,
    id: String,
    url: String,
) -> Result<(), String> {
    let (host, port) = parse_ws_target(&url)?;
    let stream = connect_companion(&host, port).await?;
    let request = url
        .into_client_request()
        .map_err(|error| format!("bad companion url: {error}"))?;
    let (ws, _response) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::client_async(request, stream),
    )
    .await
    .map_err(|_| format!("timed out completing websocket handshake with {host}:{port}"))?
    .map_err(|error| format!("websocket handshake failed: {error}"))?;

    let (mut sink, mut incoming) = ws.split();
    let (tx, mut mailbox) = unbounded_channel::<Outgoing>();
    lock(&table).insert(id.clone(), Conn { tx });
    emit(&app, ClientEvent::Open { id: id.clone() });

    loop {
        tokio::select! {
            frame = mailbox.recv() => {
                match frame {
                    Some(Outgoing::Text(text)) => {
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Outgoing::Close) | None => {
                        let _ = sink.close().await;
                        break;
                    }
                }
            }
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        emit(
                            &app,
                            ClientEvent::Message {
                                id: id.clone(),
                                data: text.to_string(),
                            },
                        );
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let (code, reason) = match frame {
                            Some(frame) => (u16::from(frame.code), frame.reason.to_string()),
                            None => (1000, String::new()),
                        };
                        emit(
                            &app,
                            ClientEvent::Close {
                                id: id.clone(),
                                code,
                                reason,
                            },
                        );
                        break;
                    }
                    Some(Err(error)) => {
                        let message = error.to_string();
                        emit(
                            &app,
                            ClientEvent::Error {
                                id: id.clone(),
                                message: message.clone(),
                            },
                        );
                        emit(
                            &app,
                            ClientEvent::Close {
                                id: id.clone(),
                                code: 1006,
                                reason: message,
                            },
                        );
                        break;
                    }
                    Some(Ok(_)) => {}
                    None => break,
                }
            }
        }
    }
    let _ = take_conn(&table, &id);
    Ok(())
}

#[tauri::command]
pub fn companion_ws_send(
    state: State<'_, CompanionWs>,
    id: String,
    data: String,
) -> Result<(), String> {
    let table = state.table();
    let live = lock(&table);
    let conn = live
        .get(&id)
        .ok_or_else(|| "companion socket is not open".to_string())?;
    conn.tx
        .send(Outgoing::Text(data))
        .map_err(|_| "companion socket is not open".to_string())
}

#[tauri::command]
pub fn companion_ws_close(state: State<'_, CompanionWs>, id: String) {
    if let Some(conn) = take_conn(&state.table(), &id) {
        let _ = conn.tx.send(Outgoing::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ws_target_reads_host_and_port() {
        let (host, port) =
            parse_ws_target("ws://192.168.4.191:17233/v1/connect?token=abc&v=1").unwrap();
        assert_eq!(host, "192.168.4.191");
        assert_eq!(port, 17233);
    }

    #[test]
    fn parse_ws_target_rejects_http_and_wss() {
        assert!(parse_ws_target("http://192.168.4.191:17233/").is_err());
        assert!(parse_ws_target("wss://mac.tail.ts.net:443/v1/connect").is_err());
        assert!(parse_ws_target("file:///tmp").is_err());
    }

    #[test]
    fn tailnet_hosts() {
        assert!(is_tailnet_host("100.119.157.61"));
        assert!(is_tailnet_host("100.64.0.1"));
        assert!(is_tailnet_host("mac.tail9a5.ts.net"));
        assert!(is_tailnet_host("[fd7a:115c:a1e0::1]"));
        assert!(!is_tailnet_host("192.168.4.191"));
        assert!(!is_tailnet_host("10.0.0.1"));
        assert!(!is_tailnet_host("127.0.0.1"));
        assert!(!is_tailnet_host("example.com"));
    }

    #[test]
    fn socket_ids_are_bounded() {
        assert!(valid_id("abcd-efgh-ijkl"));
        assert!(!valid_id("short"));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id("has space!!"));
    }
}
