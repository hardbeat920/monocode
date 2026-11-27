use crate::{tls, Address, Gate, Identity, LinkResult, REQUEST_LIMIT, RESPONSE_LIMIT};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, Notify, Semaphore},
    task::JoinSet,
    time::timeout,
};

pub fn server_endpoint(
    identity: &Identity,
    gate: Gate,
    address: SocketAddr,
) -> LinkResult<quinn::Endpoint> {
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .map_err(|e| e.to_string())?;
    // Windows makes IPv6 sockets IPv6-only by default. The desktop listens
    // on [::], so enable IPv4-mapped packets before binding on every platform.
    if address.is_ipv6() {
        socket.set_only_v6(false).map_err(|e| e.to_string())?;
    }
    socket.bind(&address.into()).map_err(|e| e.to_string())?;
    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
    quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(tls::server(identity, gate)?),
        socket.into(),
        Arc::new(quinn::TokioRuntime),
    )
    .map_err(|e| e.to_string())
}
pub async fn read_frame(stream: &mut quinn::RecvStream, limit: usize) -> LinkResult<Value> {
    let length = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    if length == 0 || length > limit {
        return Err("Message exceeds the connection limit".into());
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid connection message".into())
}
pub async fn write_frame(
    stream: &mut quinn::SendStream,
    value: &Value,
    limit: usize,
) -> LinkResult<()> {
    write_message(stream, value, limit).await?;
    stream.finish().map_err(|e| e.to_string())
}
/// One frame on a stream that stays open for more.
async fn write_message(
    stream: &mut quinn::SendStream,
    value: &Value,
    limit: usize,
) -> LinkResult<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Message exceeds the connection limit".into());
    }
    stream
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(&bytes).await.map_err(|e| e.to_string())
}

/// Where a watch handler sends its updates. The stream ends when it is dropped.
pub type WatchSink = mpsc::Sender<Value>;
/// How often an idle watch proves the desktop is still there.
pub const WATCH_HEARTBEAT: Duration = Duration::from_secs(10);
/// Answers each request on its own stream. A request marked `"watch": true`
/// goes to `watcher` instead, and its stream stays open: every value the
/// watcher sends becomes a frame, with heartbeats between, until the watcher
/// drops its sink or the phone goes away.
pub async fn serve<F, Fut, W, WFut>(endpoint: quinn::Endpoint, gate: Gate, handler: F, watcher: W)
where
    F: Fn(String, Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = LinkResult<Value>> + Send + 'static,
    W: Fn(String, Value, WatchSink) -> WFut + Send + Sync + 'static,
    WFut: Future<Output = LinkResult<()>> + Send + 'static,
{
    let handler = Arc::new(handler);
    let watcher = Arc::new(watcher);
    let slots = Arc::new(Semaphore::new(32));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let handler = handler.clone();
        let watcher = watcher.clone();
        let gate = gate.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(Ok(connection)) = timeout(Duration::from_secs(10), incoming).await else {
                return;
            };
            let Some(certs) = connection.peer_identity().and_then(|v| {
                v.downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
                    .ok()
            }) else {
                return;
            };
            let Some(cert) = certs.first() else {
                return;
            };
            let key = crate::fingerprint(cert.as_ref());
            while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                if !gate.admits(&key) {
                    connection.close(1_u32.into(), b"Device revoked");
                    break;
                }
                // Long enough for a message's attachments over a slow network.
                let request = timeout(REQUEST_UPLOAD, read_frame(&mut recv, REQUEST_LIMIT))
                    .await
                    .unwrap_or_else(|_| Err("Desktop request timed out".into()))
                    .and_then(|request| {
                        if gate.admits(&key) {
                            Ok(request)
                        } else {
                            Err("Device revoked".into())
                        }
                    });
                if let Some(request) = request
                    .as_ref()
                    .ok()
                    .filter(|request| request["watch"] == true)
                {
                    // A watch runs beside the connection's requests, not in front of them.
                    let (sink, updates) = mpsc::channel(64);
                    let watching = watcher(key.clone(), request.clone(), sink);
                    tokio::spawn(stream_watch(
                        send,
                        recv,
                        updates,
                        watching,
                        gate.clone(),
                        key.clone(),
                    ));
                    continue;
                }
                // Sequential per connection bounds work and preserves command ordering.
                let response = match request {
                    Ok(request) => timeout(Duration::from_secs(30), handler(key.clone(), request))
                        .await
                        .unwrap_or_else(|_| Err("Desktop request timed out".into())),
                    Err(error) => Err(error),
                };
                let response = match response {
                    Ok(result) => serde_json::json!({"ok":true,"result":result}),
                    Err(error) => serde_json::json!({"ok":false,"error":error}),
                };
                let _ = timeout(
                    Duration::from_secs(10),
                    write_frame(&mut send, &response, RESPONSE_LIMIT),
                )
                .await;
            }
        });
    }
}
async fn stream_watch(
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    mut updates: mpsc::Receiver<Value>,
    watching: impl Future<Output = LinkResult<()>>,
    gate: Gate,
    key: String,
) {
    let frame = |result: LinkResult<Value>| match result {
        Ok(value) => serde_json::json!({"ok":true,"result":value}),
        Err(error) => serde_json::json!({"ok":false,"error":error}),
    };
    // The watcher answers first, as a request would: refused, or watching.
    if let Err(error) = timeout(Duration::from_secs(30), watching)
        .await
        .unwrap_or_else(|_| Err("Desktop request timed out".into()))
    {
        let _ = timeout(
            Duration::from_secs(10),
            write_frame(&mut send, &frame(Err(error)), RESPONSE_LIMIT),
        )
        .await;
        return;
    }
    if write_message(
        &mut send,
        &serde_json::json!({"ok":true,"watching":true}),
        RESPONSE_LIMIT,
    )
    .await
    .is_err()
    {
        return;
    }
    // The phone closes its side to stop watching.
    let mut closed =
        Box::pin(
            async move { while recv.read_chunk(1024, true).await.is_ok_and(|c| c.is_some()) {} },
        );
    let mut heartbeat = tokio::time::interval(WATCH_HEARTBEAT);
    heartbeat.tick().await;
    loop {
        let value = tokio::select! {
            update = updates.recv() => match update {
                Some(value) => frame(Ok(value)),
                None => break,
            },
            _ = heartbeat.tick() => serde_json::json!({"ok":true,"heartbeat":true}),
            _ = &mut closed => return,
        };
        if !gate.admits(&key) {
            let _ = write_frame(
                &mut send,
                &frame(Err("Device revoked".into())),
                RESPONSE_LIMIT,
            )
            .await;
            return;
        }
        let written = timeout(
            Duration::from_secs(10),
            write_message(&mut send, &value, RESPONSE_LIMIT),
        )
        .await;
        if !matches!(written, Ok(Ok(()))) {
            return;
        }
    }
    let _ = send.finish();
}

async fn connect(
    identity: &Identity,
    pin: &str,
    addresses: &[Address],
) -> LinkResult<(quinn::Endpoint, quinn::Connection)> {
    if addresses.is_empty() || addresses.len() > 16 {
        return Err("Desktop has no usable addresses".into());
    }
    let config = tls::client(identity, pin)?;
    let mut attempts = JoinSet::new();
    for address in addresses.iter().cloned() {
        let config = config.clone();
        attempts.spawn(async move {
            let resolved = tokio::net::lookup_host((address.host.as_str(), address.port))
                .await
                .map_err(|e| e.to_string())?;
            for remote in resolved.take(4) {
                let bind = if remote.is_ipv4() {
                    "0.0.0.0:0"
                } else {
                    "[::]:0"
                };
                let mut endpoint =
                    quinn::Endpoint::client(bind.parse().unwrap()).map_err(|e| e.to_string())?;
                endpoint.set_default_client_config(config.clone());
                let connecting = endpoint
                    .connect(remote, "monocode.local")
                    .map_err(|e| e.to_string())?;
                if let Ok(Ok(connection)) = timeout(Duration::from_secs(4), connecting).await {
                    return Ok::<_, String>((endpoint, connection));
                }
            }
            Err("Address did not answer".into())
        });
    }
    let winner = timeout(Duration::from_secs(8), async {
        while let Some(result) = attempts.join_next().await {
            if let Ok(Ok(winner)) = result {
                return Some(winner);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    attempts.abort_all();
    winner.ok_or_else(|| "Cannot reach your desktop. Check that MonoCode is running and its remote address is reachable.".into())
}

/// A connection to the desktop held open between requests, so the next one
/// skips the QUIC and TLS handshake.
struct Pooled {
    endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    since: Instant,
}
/// Idle connections by desktop, identity and addresses, newest last. The
/// desktop answers one request at a time per connection, so a request takes
/// one for itself and gives it back after.
fn pool() -> &'static Mutex<HashMap<String, Vec<Pooled>>> {
    static POOL: OnceLock<Mutex<HashMap<String, Vec<Pooled>>>> = OnceLock::new();
    POOL.get_or_init(Default::default)
}
/// Idle connections kept per desktop: enough for a burst of reads in parallel.
const POOL_SIZE: usize = 4;
/// An idle connection older than this is let go rather than trusted, say
/// after the phone slept or changed networks.
const POOL_IDLE: Duration = Duration::from_secs(20);

fn pool_key(identity: &Identity, pin: &str, addresses: &[Address]) -> String {
    let addresses: Vec<String> = addresses
        .iter()
        .map(|address| format!("{}:{}", address.host, address.port))
        .collect();
    format!(
        "{}|{}|{}",
        pin.to_lowercase(),
        crate::fingerprint(identity.certificate.as_bytes()),
        addresses.join(",")
    )
}
fn take(key: &str) -> Option<(quinn::Endpoint, quinn::Connection)> {
    let mut pool = pool().lock().unwrap();
    let idle = pool.get_mut(key)?;
    while let Some(pooled) = idle.pop() {
        if pooled.connection.close_reason().is_none() && pooled.since.elapsed() < POOL_IDLE {
            return Some((pooled.endpoint, pooled.connection));
        }
        pooled.connection.close(0_u32.into(), b"Idle");
    }
    None
}
fn give(key: &str, endpoint: quinn::Endpoint, connection: quinn::Connection) {
    if connection.close_reason().is_some() {
        return;
    }
    let mut pool = pool().lock().unwrap();
    let idle = pool.entry(key.to_string()).or_default();
    idle.push(Pooled {
        endpoint,
        connection,
        since: Instant::now(),
    });
    if idle.len() > POOL_SIZE {
        idle.remove(0).connection.close(0_u32.into(), b"Idle");
    }
}

/// Why a request on a connection failed: the link itself, which a fresh
/// connection may not hit, or the desktop taking too long, which it would.
enum Failure {
    Link(String),
    Timeout,
}
impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Link(error) => error,
            Failure::Timeout => "Desktop request timed out".into(),
        }
    }
}

/// Runs `run` on a pooled connection, or on a new one when none is idle or
/// the pooled one turns out to be gone; its request is sent again under the
/// same ID, which the desktop deduplicates. A connection that served its
/// request goes back to the pool.
async fn on_connection<T, F, Fut>(
    identity: &Identity,
    pin: &str,
    addresses: &[Address],
    run: F,
) -> LinkResult<T>
where
    F: Fn(quinn::Endpoint, quinn::Connection) -> Fut,
    Fut: Future<Output = Result<(T, bool), Failure>>,
{
    let key = pool_key(identity, pin, addresses);
    if let Some((endpoint, connection)) = take(&key) {
        match run(endpoint.clone(), connection.clone()).await {
            Ok((value, reusable)) => {
                if reusable {
                    give(&key, endpoint, connection);
                }
                return Ok(value);
            }
            Err(Failure::Link(_)) => connection.close(0_u32.into(), b"Stale"),
            Err(failure) => {
                connection.close(0_u32.into(), b"Timed out");
                return Err(failure.into());
            }
        }
    }
    let (endpoint, connection) = connect(identity, pin, addresses).await?;
    match run(endpoint.clone(), connection.clone()).await {
        Ok((value, reusable)) => {
            if reusable {
                give(&key, endpoint, connection);
            }
            Ok(value)
        }
        Err(failure) => {
            connection.close(0_u32.into(), b"Failed");
            Err(failure.into())
        }
    }
}

fn answer(response: Value) -> LinkResult<Value> {
    if response["ok"] == true {
        Ok(response["result"].clone())
    } else {
        Err(response["error"]
            .as_str()
            .unwrap_or("Desktop request failed")
            .into())
    }
}

/// The server's wait for a request to arrive in full.
const REQUEST_UPLOAD: Duration = Duration::from_secs(120);

/// The desktop's 30 seconds to answer, and the time to upload the request:
/// most are tiny, but one carrying attachments can be megabytes.
fn exchange_timeout(request: &Value) -> Duration {
    let size = serde_json::to_vec(request).map_or(0, |bytes| bytes.len()) as u64;
    Duration::from_secs(35) + Duration::from_secs(size / (256 * 1024)).min(REQUEST_UPLOAD)
}

pub async fn exchange(
    identity: &Identity,
    pin: &str,
    addresses: &[Address],
    request: Value,
) -> LinkResult<Value> {
    let response = on_connection(identity, pin, addresses, |_, connection| {
        let request = &request;
        async move {
            let response = timeout(exchange_timeout(request), async {
                let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
                write_frame(&mut send, request, REQUEST_LIMIT).await?;
                read_frame(&mut recv, RESPONSE_LIMIT).await
            })
            .await
            .map_err(|_| Failure::Timeout)?
            .map_err(Failure::Link)?;
            Ok((response, true))
        }
    })
    .await?;
    answer(response)
}

/// A watch the phone holds open: updates arrive as the desktop sends them.
pub struct Watch {
    key: String,
    endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    updates: tokio::sync::Mutex<mpsc::UnboundedReceiver<LinkResult<Value>>>,
    ready: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
    /// Set once the stream failed or the desktop ended it.
    ended: Arc<AtomicBool>,
}
impl Watch {
    /// Wait without consuming/projecting data, so the layout worker remains
    /// responsive to commands and drains a whole burst on its own thread.
    pub async fn ready(&self) {
        let notified = self.ready.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let updates = self.updates.lock().await;
        if !updates.is_empty() || updates.is_closed() {
            return;
        }
        drop(updates);
        notified.await;
    }
    /// The updates that arrived, waiting up to `wait` for the first. Empty when
    /// none came in time; an error once the watch has ended.
    pub async fn next(&self, wait: Duration) -> LinkResult<Vec<Value>> {
        let mut updates = self.updates.lock().await;
        let first = match timeout(wait, updates.recv()).await {
            Err(_) => return Ok(Vec::new()),
            Ok(None) => return Err("Desktop stopped watching".into()),
            Ok(Some(update)) => update?,
        };
        let mut values = vec![first];
        // A burst arrives as one batch, so the phone renders once for it.
        while let Ok(update) = updates.try_recv() {
            match update {
                Ok(value) => values.push(value),
                Err(error) if values.is_empty() => return Err(error),
                Err(_) => break,
            }
        }
        Ok(values)
    }
    /// Stops watching. Ending the stream tells the desktop; the connection
    /// itself goes back to the pool for the next request, unless the watch
    /// ended on a failure.
    pub async fn close(&self) {
        // Dropping the task's streams finishes ours and stops the desktop's.
        self.task.abort();
        self.ready.notify_waiters();
        if self.ended.load(Ordering::Relaxed) || self.connection.close_reason().is_some() {
            self.connection.close(0_u32.into(), b"Stopped watching");
            let _ = timeout(Duration::from_secs(2), self.endpoint.wait_idle()).await;
        } else {
            give(&self.key, self.endpoint.clone(), self.connection.clone());
        }
    }
}

/// Opens a watch: one request whose stream the desktop keeps writing to.
pub async fn watch(
    identity: &Identity,
    pin: &str,
    addresses: &[Address],
    mut request: Value,
) -> LinkResult<Watch> {
    request["watch"] = Value::Bool(true);
    let key = pool_key(identity, pin, addresses);
    // A refused watch leaves its connection as good as it was.
    let opened = on_connection(identity, pin, addresses, |endpoint, connection| {
        let request = &request;
        async move {
            let opened = timeout(Duration::from_secs(35), async {
                let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
                write_message(&mut send, request, REQUEST_LIMIT).await?;
                let answer = read_frame(&mut recv, RESPONSE_LIMIT).await?;
                Ok::<_, String>((send, recv, answer))
            })
            .await
            .map_err(|_| Failure::Timeout)?
            .map_err(Failure::Link)?;
            let (send, recv, answer) = opened;
            if answer["ok"] != true {
                let error = answer["error"]
                    .as_str()
                    .unwrap_or("Desktop request failed")
                    .to_string();
                return Ok((Err(error), true));
            }
            // Held out of the pool while it watches.
            Ok((Ok((endpoint, connection, send, recv)), false))
        }
    })
    .await?;
    let (endpoint, connection, send, mut recv) = opened?;
    let (sender, updates) = mpsc::unbounded_channel();
    let ready = Arc::new(Notify::new());
    let incoming = ready.clone();
    let ended = Arc::new(AtomicBool::new(false));
    let ending = ended.clone();
    let task = tokio::spawn(async move {
        // Finishing our side tells the desktop to stop, so hold it open.
        let _send = send;
        loop {
            let frame = timeout(WATCH_HEARTBEAT * 3, read_frame(&mut recv, RESPONSE_LIMIT))
                .await
                .unwrap_or_else(|_| Err("Desktop stopped answering".into()));
            let update = match frame {
                Ok(frame) if frame["heartbeat"] == true => continue,
                Ok(frame) if frame["ok"] == true => Ok(frame["result"].clone()),
                Ok(frame) => Err(frame["error"]
                    .as_str()
                    .unwrap_or("Desktop request failed")
                    .to_string()),
                Err(error) => Err(error),
            };
            if update.is_err() {
                ending.store(true, Ordering::Relaxed);
                let _ = sender.send(update);
                incoming.notify_waiters();
                break;
            }
            if sender.send(update).is_err() {
                break;
            }
            incoming.notify_waiters();
        }
    });
    Ok(Watch {
        key,
        endpoint,
        connection,
        updates: tokio::sync::Mutex::new(updates),
        ready,
        task,
        ended,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn no_watch(_: String, _: Value, _: WatchSink) -> LinkResult<()> {
        Err("Unsupported mobile operation".into())
    }

    #[tokio::test]
    async fn waiting_for_a_watch_sleeps_until_data_and_does_not_consume_it() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(phone.fingerprint().unwrap(), None);
        let endpoint =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let (sinks, mut opened) = mpsc::channel(1);
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            |_, request| async move { Ok(request) },
            move |_, _, sink| {
                let sinks = sinks.clone();
                async move {
                    sinks.send(sink).await.unwrap();
                    Ok(())
                }
            },
        ));
        let watching = watch(
            &phone,
            &desktop.fingerprint().unwrap(),
            &[Address {
                host: "127.0.0.1".into(),
                port,
            }],
            json!({"method":"conversation.watch"}),
        )
        .await
        .unwrap();
        let sink = opened.recv().await.unwrap();
        let ready = watching.ready();
        tokio::pin!(ready);
        assert!(timeout(Duration::from_millis(10), &mut ready)
            .await
            .is_err());
        sink.send(json!({"text":"arrived"})).await.unwrap();
        timeout(Duration::from_secs(5), &mut ready).await.unwrap();
        timeout(Duration::from_secs(5), watching.ready())
            .await
            .unwrap();
        assert_eq!(
            watching.next(Duration::ZERO).await.unwrap(),
            vec![json!({"text":"arrived"})]
        );
        let ready = watching.ready();
        tokio::pin!(ready);
        assert!(timeout(Duration::from_millis(10), &mut ready)
            .await
            .is_err());
        watching.close().await;
        timeout(Duration::from_secs(5), &mut ready).await.unwrap();
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_watch_streams_updates_until_the_desktop_ends_it() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(phone.fingerprint().unwrap(), None);
        let endpoint =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            |_, request| async move { Ok(request) },
            |_, request: Value, sink: WatchSink| async move {
                if request["method"] != "conversation.watch" {
                    return Err("Unsupported mobile operation".into());
                }
                tokio::spawn(async move {
                    for n in 0..3 {
                        sink.send(json!({ "n": n })).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                });
                Ok(())
            },
        ));
        let address = [Address {
            host: "127.0.0.1".into(),
            port,
        }];
        let pin = desktop.fingerprint().unwrap();
        let refused = watch(&phone, &pin, &address, json!({"method":"other"})).await;
        assert_eq!(refused.err().unwrap(), "Unsupported mobile operation");

        let watching = watch(
            &phone,
            &pin,
            &address,
            json!({"method":"conversation.watch"}),
        )
        .await
        .unwrap();
        let mut seen = Vec::new();
        while seen.len() < 3 {
            seen.extend(watching.next(Duration::from_secs(5)).await.unwrap());
        }
        assert_eq!(seen, vec![json!({"n":0}), json!({"n":1}), json!({"n":2})]);
        // The watcher dropped its sink: the stream ends and says so.
        assert!(watching.next(Duration::from_secs(5)).await.is_err());
        watching.close().await;
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn closing_a_watch_on_the_phone_releases_the_desktop_sink() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(phone.fingerprint().unwrap(), None);
        let endpoint =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let (released, mut was_released) = mpsc::channel::<()>(1);
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            |_, request| async move { Ok(request) },
            move |_, _, sink: WatchSink| {
                let released = released.clone();
                async move {
                    tokio::spawn(async move {
                        sink.closed().await;
                        released.send(()).await.unwrap();
                    });
                    Ok(())
                }
            },
        ));
        let watching = watch(
            &phone,
            &desktop.fingerprint().unwrap(),
            &[Address {
                host: "127.0.0.1".into(),
                port: endpoint.local_addr().unwrap().port(),
            }],
            json!({}),
        )
        .await
        .unwrap();
        watching.close().await;
        timeout(Duration::from_secs(5), was_released.recv())
            .await
            .expect("desktop sink released");
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn direct_quic_round_trip_with_pinned_mutual_tls_over_ipv4_and_ipv6() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(phone.fingerprint().unwrap(), None);
        let endpoint = server_endpoint(&desktop, gate.clone(), "[::]:0".parse().unwrap()).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            |_, request| async move { Ok(request) },
            no_watch,
        ));
        for host in ["127.0.0.1", "::1"] {
            let result = exchange(
                &phone,
                &desktop.fingerprint().unwrap(),
                &[Address {
                    host: host.into(),
                    port,
                }],
                json!({"method":"workspace.list"}),
            )
            .await
            .unwrap_or_else(|error| panic!("QUIC connection to {host} failed: {error}"));
            assert_eq!(result, json!({"method":"workspace.list"}));
        }
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn requests_and_a_closed_watch_share_one_pooled_connection() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(phone.fingerprint().unwrap(), None);
        let endpoint =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            |_, request| async move { Ok(request) },
            |_, _, sink: WatchSink| async move {
                tokio::spawn(async move { sink.closed().await });
                Ok(())
            },
        ));
        let address = [Address {
            host: "127.0.0.1".into(),
            port: endpoint.local_addr().unwrap().port(),
        }];
        let pin = desktop.fingerprint().unwrap();
        for n in 0..3 {
            let result = exchange(&phone, &pin, &address, json!({ "n": n }))
                .await
                .unwrap();
            assert_eq!(result, json!({ "n": n }));
        }
        let watching = watch(&phone, &pin, &address, json!({})).await.unwrap();
        watching.close().await;
        assert_eq!(
            exchange(&phone, &pin, &address, json!("after"))
                .await
                .unwrap(),
            json!("after")
        );
        assert_eq!(endpoint.open_connections(), 1);
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn unpaired_and_wrongly_pinned_clients_never_reach_the_handler() {
        let desktop = Identity::generate().unwrap();
        let paired = Identity::generate().unwrap();
        let unpaired = Identity::generate().unwrap();
        let gate = Gate::default();
        gate.allow(paired.fingerprint().unwrap(), None);
        let endpoint =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let task = tokio::spawn(serve(
            endpoint.clone(),
            gate,
            move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                async { Ok(Value::Null) }
            },
            no_watch,
        ));
        let address = [Address {
            host: "127.0.0.1".into(),
            port: endpoint.local_addr().unwrap().port(),
        }];
        assert!(exchange(
            &unpaired,
            &desktop.fingerprint().unwrap(),
            &address,
            json!({})
        )
        .await
        .is_err());
        assert!(exchange(
            &paired,
            &unpaired.fingerprint().unwrap(),
            &address,
            json!({})
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        endpoint.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn revocation_applies_to_an_existing_connection_and_large_frames_are_rejected() {
        let desktop = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let gate = Gate::default();
        let key = phone.fingerprint().unwrap();
        gate.allow(key.clone(), None);
        let server =
            server_endpoint(&desktop, gate.clone(), "127.0.0.1:0".parse().unwrap()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let task = tokio::spawn(serve(
            server.clone(),
            gate.clone(),
            move |_, _| {
                count.fetch_add(1, Ordering::SeqCst);
                async { Ok(json!("accepted")) }
            },
            no_watch,
        ));
        let mut client = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client.set_default_client_config(
            tls::client(&phone, &desktop.fingerprint().unwrap()).unwrap(),
        );
        let connection = client
            .connect(server.local_addr().unwrap(), "monocode.local")
            .unwrap()
            .await
            .unwrap();
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        write_frame(&mut send, &json!({}), REQUEST_LIMIT)
            .await
            .unwrap();
        assert_eq!(
            read_frame(&mut recv, RESPONSE_LIMIT).await.unwrap()["ok"],
            true
        );
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        send.write_u32((REQUEST_LIMIT + 1) as u32).await.unwrap();
        send.finish().unwrap();
        assert_eq!(
            read_frame(&mut recv, RESPONSE_LIMIT).await.unwrap()["ok"],
            false
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        gate.remove(&key);
        let denied = timeout(Duration::from_secs(2), async {
            let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
            send.write_all(&[0, 0, 0, 2, b'{', b'}'])
                .await
                .map_err(|e| e.to_string())?;
            send.finish().map_err(|e| e.to_string())?;
            recv.read_to_end(1024).await.map_err(|e| e.to_string())
        })
        .await;
        assert!(!matches!(denied, Ok(Ok(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        client.close(0_u32.into(), b"Test complete");
        server.close(0_u32.into(), b"Test complete");
        task.await.unwrap();
    }
}
