//! Port of host/listener.ts.

use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::http::{HttpServer, Transport};
use super::tls::{HostIdentity, server_config};

/// `isLoopback(address)` for the address text Node reported.
pub fn is_loopback(address: Option<&str>) -> bool {
    let Some(address) = address else {
        return false;
    };
    address.starts_with("127.")
        || address == "::1"
        || address.to_ascii_lowercase().starts_with("::ffff:127.")
}

pub fn is_loopback_ip(address: IpAddr) -> bool {
    is_loopback(Some(&address.to_string()))
}

// A TLS connection opens with a handshake record; plain HTTP opens with a
// method name.
const TLS_HANDSHAKE: u8 = 0x16;
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_POLL: Duration = Duration::from_millis(25);

pub type LoopbackCheck = Arc<dyn Fn(IpAddr) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct HostListenerOptions {
    pub port: u16,
    /// `127.0.0.1` for loopback only, or an address such as `0.0.0.0`.
    pub bind: String,
    /// Enables TLS. Without it, only loopback clients are served.
    pub identity: Option<HostIdentity>,
    /// Overridable in tests, which cannot open a non-loopback connection.
    pub loopback: Option<LoopbackCheck>,
}

/// A listening socket. Closing it stops new connections; open connections
/// belong to the `HttpServer` and keep running.
pub struct HostListener {
    address: SocketAddr,
    closed: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl HostListener {
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    /// Stops accepting and releases the port before returning.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl Drop for HostListener {
    fn drop(&mut self) {
        self.close();
    }
}

fn handshake(
    config: Arc<rustls::ServerConfig>,
    mut socket: TcpStream,
) -> Option<rustls::StreamOwned<rustls::ServerConnection, TcpStream>> {
    let mut connection = rustls::ServerConnection::new(config).ok()?;
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    while connection.is_handshaking() {
        let left = deadline.checked_duration_since(Instant::now())?;
        socket.set_read_timeout(Some(left)).ok()?;
        socket.set_write_timeout(Some(left)).ok()?;
        match connection.complete_io(&mut socket) {
            Ok((0, 0)) if connection.is_handshaking() => return None,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(rustls::StreamOwned::new(connection, socket))
}

fn connection(
    server: Arc<HttpServer>,
    tls: Option<Arc<rustls::ServerConfig>>,
    loopback: LoopbackCheck,
    socket: TcpStream,
    peer: SocketAddr,
) {
    // macOS hands accepted sockets the listener's non-blocking flag.
    if socket.set_nonblocking(false).is_err()
        || socket.set_read_timeout(Some(FIRST_BYTE_TIMEOUT)).is_err()
    {
        return;
    }
    let local = loopback(peer.ip());
    let mut first = [0u8; 1];
    match socket.peek(&mut first) {
        Ok(1) => {}
        _ => return,
    }
    if let Some(config) = tls.filter(|_| first[0] == TLS_HANDSHAKE) {
        if let Some(stream) = handshake(config, socket) {
            server.serve(Transport::Tls(Box::new(stream)), peer);
        }
    } else if local {
        server.serve(Transport::Plain(socket), peer);
    }
}

/// Serves `server` on one port. Loopback clients (this machine's CLI, or an
/// SSH forward) may use plain HTTP. Every other client must use TLS; desktops
/// pin the certificate fingerprint they received in the pairing link.
pub fn listen_host(
    server: Arc<HttpServer>,
    options: HostListenerOptions,
) -> Result<HostListener, String> {
    let loopback: LoopbackCheck = options
        .loopback
        .clone()
        .unwrap_or_else(|| Arc::new(is_loopback_ip));
    let tls = options.identity.as_ref().map(server_config).transpose()?;
    let listener = TcpListener::bind((options.bind.as_str(), options.port))
        .map_err(|error| format!("listen {}:{}: {error}", options.bind, options.port))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let closed = Arc::new(AtomicBool::new(false));
    let stop = closed.clone();
    let thread = std::thread::Builder::new()
        .name("monocode-host-listener".into())
        .spawn(move || {
            while !stop.load(Ordering::SeqCst) && !server.is_closed() {
                match listener.accept() {
                    Ok((socket, peer)) => {
                        let server = server.clone();
                        let tls = tls.clone();
                        let loopback = loopback.clone();
                        let _ = std::thread::Builder::new()
                            .name("monocode-host-connection".into())
                            .spawn(move || connection(server, tls, loopback, socket, peer));
                    }
                    Err(_) => std::thread::sleep(ACCEPT_POLL),
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(HostListener {
        address,
        closed,
        thread: Mutex::new(Some(thread)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::http::{HttpHandler, Request, Response};
    use crate::host::tls::create_host_certificate;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::io::Read;

    struct Digester;
    impl HttpHandler for Digester {
        fn handle(&self, request: &mut Request<'_>) -> Response {
            let body = request.read_body(64 * 1024 * 1024).unwrap_or_default();
            let hash: String = Sha256::digest(&body)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            Response::new(200).body(
                json!({ "size": body.len(), "hash": hash, "encrypted": request.encrypted })
                    .to_string(),
            )
        }
    }

    fn start(identity: &HostIdentity, loopback: Option<LoopbackCheck>) -> HostListener {
        listen_host(
            HttpServer::new(Arc::new(Digester)),
            HostListenerOptions {
                port: 0,
                bind: "127.0.0.1".into(),
                identity: Some(identity.clone()),
                loopback,
            },
        )
        .unwrap()
    }

    fn send(port: u16, fingerprint: Option<&str>, body: &[u8]) -> Result<Value, String> {
        let agent =
            crate::remote_tls::agent(fingerprint, Duration::from_secs(5), Duration::from_secs(20))?;
        let scheme = if fingerprint.is_some() {
            "https"
        } else {
            "http"
        };
        let response = agent
            .post(&format!("{scheme}://127.0.0.1:{port}/rpc"))
            .send_bytes(body)
            .map_err(|error| error.to_string())?;
        let mut text = String::new();
        response
            .into_reader()
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        serde_json::from_str(&text).map_err(|error| error.to_string())
    }

    #[test]
    fn serves_plain_http_to_loopback_and_tls_to_everyone_on_one_port() {
        let identity =
            create_host_certificate("MonoCode Host", std::time::SystemTime::now()).unwrap();
        let listener = start(&identity, None);
        let port = listener.local_addr().port();
        let body = vec![7u8; 3 * 1024 * 1024];
        let digest: String = Sha256::digest(&body)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            send(port, None, &body).unwrap(),
            json!({ "size": body.len(), "hash": digest, "encrypted": false })
        );
        // The pinned agent fails unless the host presents this certificate.
        assert_eq!(
            send(port, Some(&identity.fingerprint), &body).unwrap(),
            json!({ "size": body.len(), "hash": digest, "encrypted": true })
        );
        assert!(send(port, Some(&"A".repeat(43)), b"{}").is_err());
    }

    #[test]
    fn refuses_plain_http_from_another_computer() {
        let identity =
            create_host_certificate("MonoCode Host", std::time::SystemTime::now()).unwrap();
        let listener = start(&identity, Some(Arc::new(|_| false)));
        let port = listener.local_addr().port();
        assert!(send(port, None, b"{}").is_err());
        assert_eq!(
            send(port, Some(&identity.fingerprint), b"{}").unwrap()["encrypted"],
            true
        );
    }

    #[test]
    fn recognizes_ipv4_ipv6_and_mapped_loopback_addresses() {
        for address in ["127.0.0.1", "127.1.2.3", "::1", "::ffff:127.0.0.1"] {
            assert!(is_loopback(Some(address)), "{address}");
        }
        for address in [
            Some("10.0.0.5"),
            Some("::ffff:10.0.0.5"),
            Some("fe80::1"),
            None,
        ] {
            assert!(!is_loopback(address), "{address:?}");
        }
        assert!(is_loopback_ip("::ffff:127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn closing_releases_the_port_for_a_new_listener() {
        let identity =
            create_host_certificate("MonoCode Host", std::time::SystemTime::now()).unwrap();
        let listener = start(&identity, None);
        let port = listener.local_addr().port();
        listener.close();
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        // Another test's outgoing connection may hold this ephemeral port for
        // a moment, so retry as the host does when it rebinds.
        let mut attempts = 0;
        let again = loop {
            match listen_host(
                HttpServer::new(Arc::new(Digester)),
                HostListenerOptions {
                    port,
                    bind: "127.0.0.1".into(),
                    identity: None,
                    loopback: None,
                },
            ) {
                Ok(again) => break again,
                Err(error) if attempts >= 20 => panic!("{error}"),
                Err(_) => {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        assert_eq!(send(port, None, b"x").unwrap()["size"], 1);
        drop(again);
    }
}
