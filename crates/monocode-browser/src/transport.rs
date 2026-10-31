//! The loopback transport agents reach the browser through: one JSON line
//! in, `{"token", "namespace", "action", "input", "requestId"}`, one JSON
//! line out, `{"ok": true, "result": <ActionResult>}` or `{"ok": false,
//! "error": …}`. The agent side is [`crate::agent::client`].
//!
//! The Tauri app's control server carries other namespaces too and uses the
//! pieces here for its `browser` requests; an app without such a server
//! runs [`serve`], which answers only `browser`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::protocol::{ActionRequest, ActionResult, NAMESPACE};
use crate::BrowserHost;

/// The error for a token the server does not know (revoked, or from before
/// an app restart). Clients tell it apart from other refusals by this text.
pub const UNAUTHORIZED: &str = "Connection revoked or unauthorized";
/// The error when every slot for a request is taken; nothing was run.
pub const BUSY: &str = "MonoCode is busy with other requests; nothing was run. Retry in a moment.";
/// Longest request line the server reads, the newline included.
pub const MAX_REQUEST_LINE: usize = 256 * 1024;
/// Browser requests handled at the same time; more get [`BUSY`].
pub const MAX_BROWSER_REQUESTS: usize = 64;
/// Readers of request lines in [`serve`]. A request leaves its reader for a
/// thread of its own as soon as it is read, since it may wait for the user
/// and the browser for minutes.
const SERVE_THREADS: usize = 4;
/// Accepted connections waiting for a reader in [`serve`].
const SERVE_QUEUE: usize = 32;

/// One request line.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub token: String,
    pub namespace: String,
    pub action: String,
    pub input: Value,
    pub request_id: String,
}

/// Read and validate one request line.
pub fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut raw = String::new();
    BufReader::new(stream)
        .take(MAX_REQUEST_LINE as u64 + 1)
        .read_line(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > MAX_REQUEST_LINE {
        return Err("Request exceeds 256 KiB".into());
    }
    let request: Request = serde_json::from_str(&raw).map_err(|_| "Invalid control request")?;
    if !request.input.is_object() || request.request_id.is_empty() || request.request_id.len() > 128
    {
        return Err("Invalid input or request ID".into());
    }
    Ok(request)
}

/// Browser actions never go through a window: the host answers
/// synchronously, and policy refusals are ordinary `blocked` results. Only
/// an unknown token is a transport error.
pub fn browser_reply(
    request: Request,
    handle: impl FnOnce(&str, ActionRequest) -> Result<ActionResult, String>,
) -> Result<Value, String> {
    let result = handle(
        &request.token,
        ActionRequest {
            request_id: request.request_id,
            action: request.action,
            input: request.input,
        },
    )?;
    Ok(json!({"ok": true, "result": result}))
}

/// Write the answer line.
pub fn respond(mut stream: TcpStream, result: Result<Value, String>) {
    let response = result.unwrap_or_else(|error| json!({"ok": false, "error": error}));
    let _ = writeln!(stream, "{response}");
}

/// Hand a browser request to a thread of its own, at most
/// [`MAX_BROWSER_REQUESTS`] at a time (`running` counts them); the caller's
/// reader is free again at once.
pub fn spawn_request(
    stream: TcpStream,
    request: Request,
    running: &Arc<AtomicUsize>,
    handle: impl FnOnce(&str, ActionRequest) -> Result<ActionResult, String> + Send + 'static,
) {
    if running
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_BROWSER_REQUESTS).then_some(n + 1)
        })
        .is_err()
    {
        respond(stream, Err(BUSY.into()));
        return;
    }
    let slot = running.clone();
    let spawned = std::thread::Builder::new()
        .name("control-browser".into())
        .spawn(move || {
            let result = browser_reply(request, handle);
            respond(stream, result);
            slot.fetch_sub(1, Ordering::AcqRel);
        });
    if spawned.is_err() {
        // The stream went with the closure; the client sees the close as a
        // lost answer, and nothing ran.
        running.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Serve `browser` requests for `host` on `listener` until it fails. For
/// an app whose loopback carries nothing else; blocks, so run it on a
/// thread of its own.
pub fn serve(listener: TcpListener, host: Arc<BrowserHost>) {
    let (tx, rx) = mpsc::sync_channel::<TcpStream>(SERVE_QUEUE);
    let rx = Arc::new(Mutex::new(rx));
    let running = Arc::new(AtomicUsize::new(0));
    for _ in 0..SERVE_THREADS {
        let rx = rx.clone();
        let host = host.clone();
        let running = running.clone();
        std::thread::spawn(move || loop {
            let stream = match rx.lock() {
                Ok(rx) => rx.recv(),
                Err(_) => return,
            };
            let Ok(mut stream) = stream else { return };
            let request = match read_request(&mut stream) {
                Ok(request) if request.namespace == NAMESPACE => request,
                Ok(_) => {
                    respond(stream, Err("Unknown control namespace".into()));
                    continue;
                }
                Err(error) => {
                    respond(stream, Err(error));
                    continue;
                }
            };
            let host = host.clone();
            spawn_request(stream, request, &running, move |token, request| {
                host.handle_token_request(token, request)
            });
        });
    }
    for stream in listener.incoming().flatten() {
        if let Err(mpsc::TrySendError::Full(stream)) = tx.try_send(stream) {
            // Say so rather than drop the connection: the client then knows
            // nothing ran. A short write timeout keeps a client that does
            // not read from holding up the accept loop.
            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
            respond(stream, Err(BUSY.into()));
        }
    }
}
