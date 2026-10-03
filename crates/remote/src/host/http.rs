//! A small HTTP/1.1 server on std threads, standing in for Node's `http`
//! module in `host/server.ts` and `host/listener.ts`.
//!
//! It keeps the limits the TypeScript server set: 8 KiB of headers, 10
//! seconds to receive them, 20 seconds for the whole request, and Node's
//! 5-second keep-alive. Bodies may use `Content-Length` or chunked encoding.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const MAX_HEADER_BYTES: usize = 8192;
pub const HEADERS_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// A plain or TLS connection.
pub enum Transport {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
}

impl Transport {
    fn socket(&self) -> &TcpStream {
        match self {
            Self::Plain(stream) => stream,
            Self::Tls(stream) => &stream.sock,
        }
    }

    fn encrypted(&self) -> bool {
        matches!(self, Self::Tls(_))
    }
}

impl Read for Transport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Transport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

/// Answers one request at a time on a connection's thread.
pub trait HttpHandler: Send + Sync + 'static {
    fn handle(&self, request: &mut Request<'_>) -> Response;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

#[derive(Debug)]
pub enum BodyError {
    /// The body is larger than the caller's limit.
    TooLarge,
    /// The connection failed or timed out while the body arrived.
    Io(io::Error),
    /// The chunked encoding is malformed.
    Invalid,
}

enum Framing {
    Length(u64),
    Chunked,
}

/// Bytes read from the connection but not yet consumed.
struct Buffered {
    transport: Transport,
    buffer: Vec<u8>,
    start: usize,
}

impl Buffered {
    fn available(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
        if self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        }
    }

    /// Reads more bytes before `deadline`. Returns 0 at end of stream.
    fn fill(&mut self, deadline: Instant) -> io::Result<usize> {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.transport
            .socket()
            .set_read_timeout(Some(deadline - now))?;
        if self.start > 0 && self.start == self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
        }
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match self.transport.read(&mut chunk) {
                Ok(count) => {
                    self.buffer.extend_from_slice(&chunk[..count]);
                    return Ok(count);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub struct Request<'a> {
    pub method: String,
    pub url: String,
    pub peer: SocketAddr,
    pub encrypted: bool,
    headers: Vec<(String, String)>,
    connection: &'a mut Buffered,
    framing: Framing,
    deadline: Instant,
    expect_continue: bool,
    /// Set once the body has been read to its end.
    complete: bool,
    /// Bytes of a chunked body still to read in the current chunk.
    chunk_left: u64,
}

impl Request<'_> {
    /// The first header with this name, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Whether the client closed its side while its request waits for an
    /// answer, as Node's response `close` event reported.
    pub fn client_left(&self) -> bool {
        let socket = self.connection.transport.socket();
        if socket.set_nonblocking(true).is_err() {
            return false;
        }
        let mut byte = [0u8; 1];
        // Any bytes here are a TLS close alert or a request the client sent
        // without waiting, neither of which a desktop does while it waits.
        let left = match socket.peek(&mut byte) {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::WouldBlock,
        };
        let _ = socket.set_nonblocking(false);
        left
    }

    /// Reads the whole body, failing past `limit` bytes.
    pub fn read_body(&mut self, limit: usize) -> Result<Vec<u8>, BodyError> {
        if self.expect_continue {
            self.expect_continue = false;
            self.connection
                .transport
                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                .and_then(|_| self.connection.transport.flush())
                .map_err(BodyError::Io)?;
        }
        let mut body = Vec::new();
        match self.framing {
            Framing::Length(length) => {
                if length > limit as u64 {
                    return Err(BodyError::TooLarge);
                }
                while (body.len() as u64) < length {
                    if self.connection.available().is_empty()
                        && self.connection.fill(self.deadline).map_err(BodyError::Io)? == 0
                    {
                        return Err(BodyError::Io(io::ErrorKind::UnexpectedEof.into()));
                    }
                    let wanted = (length - body.len() as u64) as usize;
                    let available = self.connection.available();
                    let take = wanted.min(available.len());
                    body.extend_from_slice(&available[..take]);
                    self.connection.consume(take);
                }
                self.complete = true;
            }
            Framing::Chunked => loop {
                if self.chunk_left == 0 {
                    let line = self.line()?;
                    let size = line.split(';').next().unwrap_or("").trim();
                    let size = u64::from_str_radix(size, 16).map_err(|_| BodyError::Invalid)?;
                    if size == 0 {
                        // Trailers end with an empty line.
                        while !self.line()?.is_empty() {}
                        self.complete = true;
                        break;
                    }
                    if body.len() as u64 + size > limit as u64 {
                        return Err(BodyError::TooLarge);
                    }
                    self.chunk_left = size;
                }
                while self.chunk_left > 0 {
                    if self.connection.available().is_empty()
                        && self.connection.fill(self.deadline).map_err(BodyError::Io)? == 0
                    {
                        return Err(BodyError::Io(io::ErrorKind::UnexpectedEof.into()));
                    }
                    let available = self.connection.available();
                    let take = (self.chunk_left as usize).min(available.len());
                    body.extend_from_slice(&available[..take]);
                    self.connection.consume(take);
                    self.chunk_left -= take as u64;
                }
                if !self.line()?.is_empty() {
                    return Err(BodyError::Invalid);
                }
            },
        }
        Ok(body)
    }

    fn line(&mut self) -> Result<String, BodyError> {
        loop {
            if let Some(end) = self
                .connection
                .available()
                .windows(2)
                .position(|pair| pair == b"\r\n")
            {
                let line =
                    String::from_utf8_lossy(&self.connection.available()[..end]).into_owned();
                self.connection.consume(end + 2);
                return Ok(line);
            }
            if self.connection.available().len() > MAX_HEADER_BYTES {
                return Err(BodyError::Invalid);
            }
            if self.connection.fill(self.deadline).map_err(BodyError::Io)? == 0 {
                return Err(BodyError::Io(io::ErrorKind::UnexpectedEof.into()));
            }
        }
    }
}

/// Open connections, so stopping the host can end them.
pub struct HttpServer {
    handler: Arc<dyn HttpHandler>,
    connections: Mutex<HashMap<u64, TcpStream>>,
    next: AtomicU64,
    closed: AtomicBool,
}

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "",
    }
}

fn write_response(transport: &mut Transport, response: &Response, close: bool) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    head.push_str(if close {
        "Connection: close\r\n\r\n"
    } else {
        "Connection: keep-alive\r\nKeep-Alive: timeout=5\r\n\r\n"
    });
    transport.write_all(head.as_bytes())?;
    transport.write_all(&response.body)?;
    transport.flush()
}

enum Head {
    Request {
        method: String,
        url: String,
        version_11: bool,
        headers: Vec<(String, String)>,
    },
    /// The connection ended or idled out between requests.
    Closed,
    Reject(u16),
}

fn read_head(connection: &mut Buffered, idle: Duration) -> Head {
    // Wait for the first byte of a request, then for the whole head.
    if connection.available().is_empty() {
        match connection.fill(Instant::now() + idle) {
            Ok(0) | Err(_) => return Head::Closed,
            Ok(_) => {}
        }
    }
    let deadline = Instant::now() + HEADERS_TIMEOUT;
    let end = loop {
        if let Some(end) = connection
            .available()
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            break end;
        }
        if connection.available().len() > MAX_HEADER_BYTES {
            return Head::Reject(431);
        }
        match connection.fill(deadline) {
            Ok(0) => return Head::Closed,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::TimedOut => return Head::Reject(408),
            Err(_) => return Head::Closed,
        }
    };
    if end > MAX_HEADER_BYTES {
        return Head::Reject(431);
    }
    let Ok(text) = std::str::from_utf8(&connection.available()[..end]) else {
        return Head::Reject(400);
    };
    let mut lines = text.split("\r\n");
    let mut parts = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(url), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Head::Reject(400);
    };
    if method.is_empty() || !url.starts_with('/') && url != "*" || !version.starts_with("HTTP/1.") {
        return Head::Reject(400);
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Head::Reject(400);
        };
        if name.is_empty() || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Head::Reject(400);
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    let head = Head::Request {
        method: method.into(),
        url: url.into(),
        version_11: version == "HTTP/1.1",
        headers,
    };
    connection.consume(end + 4);
    head
}

impl HttpServer {
    pub fn new(handler: Arc<dyn HttpHandler>) -> Arc<Self> {
        Arc::new(Self {
            handler,
            connections: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        })
    }

    /// Stops serving new requests. Open connections finish their current
    /// request unless `close_all_connections` ends them.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn close_all_connections(&self) {
        let connections = self
            .connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for socket in connections.values() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    /// Serves requests on one connection until it closes. Runs on the
    /// connection's own thread.
    pub fn serve(&self, transport: Transport, peer: SocketAddr) {
        if self.is_closed() {
            return;
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        if let Ok(socket) = transport.socket().try_clone() {
            self.connections
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, socket);
        }
        let _ = transport.socket().set_write_timeout(Some(WRITE_TIMEOUT));
        let mut connection = Buffered {
            transport,
            buffer: Vec::new(),
            start: 0,
        };
        // The listener already waited for the first bytes.
        let mut idle = HEADERS_TIMEOUT;
        while !self.is_closed() {
            let started = Instant::now();
            let (method, url, version_11, headers) = match read_head(&mut connection, idle) {
                Head::Request {
                    method,
                    url,
                    version_11,
                    headers,
                } => (method, url, version_11, headers),
                Head::Closed => break,
                Head::Reject(status) => {
                    let _ = write_response(&mut connection.transport, &Response::new(status), true);
                    break;
                }
            };
            idle = KEEP_ALIVE_TIMEOUT;
            let find = |name: &str| {
                headers
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.as_str())
            };
            let chunked = find("transfer-encoding")
                .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
            let framing = if chunked {
                Framing::Chunked
            } else {
                match find("content-length").map(|value| value.parse::<u64>()) {
                    None => Framing::Length(0),
                    Some(Ok(length)) => Framing::Length(length),
                    Some(Err(_)) => {
                        let _ =
                            write_response(&mut connection.transport, &Response::new(400), true);
                        break;
                    }
                }
            };
            let wants_close = match find("connection").map(str::to_ascii_lowercase) {
                Some(value) if value.contains("close") => true,
                Some(value) if value.contains("keep-alive") => false,
                _ => !version_11,
            };
            let expect_continue =
                find("expect").is_some_and(|value| value.eq_ignore_ascii_case("100-continue"));
            let encrypted = connection.transport.encrypted();
            let mut request = Request {
                method,
                url,
                peer,
                encrypted,
                headers,
                connection: &mut connection,
                complete: matches!(framing, Framing::Length(0)),
                framing,
                deadline: started + REQUEST_TIMEOUT,
                expect_continue,
                chunk_left: 0,
            };
            let response = self.handler.handle(&mut request);
            // An unread body would be parsed as the next request.
            let close = wants_close || !request.complete || self.is_closed();
            if write_response(&mut connection.transport, &response, close).is_err() || close {
                break;
            }
        }
        if let Transport::Tls(stream) = &mut connection.transport {
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
        let _ = connection.transport.socket().shutdown(Shutdown::Both);
        self.connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    struct Echo;
    impl HttpHandler for Echo {
        fn handle(&self, request: &mut Request<'_>) -> Response {
            match request.read_body(64) {
                Ok(body) => Response::new(200).body(body),
                Err(BodyError::TooLarge) => Response::new(400).body("too large"),
                Err(_) => Response::new(400),
            }
        }
    }

    fn start() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = HttpServer::new(Arc::new(Echo));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = stream.unwrap();
                let server = server.clone();
                std::thread::spawn(move || {
                    let peer = stream.peer_addr().unwrap();
                    server.serve(Transport::Plain(stream), peer)
                });
            }
        });
        address
    }

    fn exchange(address: SocketAddr, raw: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(raw).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn reads_length_and_chunked_bodies_and_keeps_connections_alive() {
        let address = start();
        let reply = exchange(
            address,
            b"POST /rpc HTTP/1.1\r\nContent-Length: 2\r\n\r\nhiPOST /rpc HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
        );
        assert_eq!(reply.matches("HTTP/1.1 200 OK").count(), 2, "{reply}");
        assert!(reply.contains("\r\n\r\nhi"));
        assert!(reply.ends_with("\r\n\r\nabcde"));
    }

    #[test]
    fn rejects_oversized_bodies_and_headers_then_closes() {
        let address = start();
        let reply = exchange(
            address,
            b"POST /rpc HTTP/1.1\r\nContent-Length: 100\r\n\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
        assert!(reply.contains("Connection: close"));
        let mut huge = b"POST /rpc HTTP/1.1\r\nX: ".to_vec();
        huge.extend(std::iter::repeat_n(b'a', 9000));
        huge.extend_from_slice(b"\r\n\r\n");
        assert!(exchange(address, &huge).starts_with("HTTP/1.1 431"));
        assert!(exchange(address, b"nonsense\r\n\r\n").starts_with("HTTP/1.1 400"));
    }
}
