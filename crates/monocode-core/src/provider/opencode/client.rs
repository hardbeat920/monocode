//! OpenCode's versioned HTTP API, matching opencodeClient.ts.
use super::*;
use base64::Engine;

#[derive(Clone)]
pub(super) struct Client {
    pub url: String,
    pub directory: String,
    pub v2: bool,
    pub password: Option<String>,
    agent: ureq::Agent,
}
impl Client {
    pub fn new(url: String, directory: String, v2: bool, password: Option<String>) -> Self {
        Self {
            url,
            directory,
            v2,
            password,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .build(),
        }
    }
    pub fn path(&self, path: &str) -> String {
        if self.v2 {
            format!("/api{path}")
        } else {
            path.into()
        }
    }
    fn wire(&self, method: &str, path: &str) -> ureq::Request {
        let mut request = self.agent.request(
            method,
            &format!("{}{}", self.url.trim_end_matches('/'), path),
        );
        if !self.directory.is_empty() {
            request = request
                .query("directory", &self.directory)
                .set("x-opencode-directory", &encode(&self.directory));
        }
        if let Some(password) = &self.password {
            let auth =
                base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
            request = request.set("Authorization", &format!("Basic {auth}"));
        }
        request
    }
    pub fn request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        self.request_timeout(method, path, body, Duration::from_secs(30))
    }
    pub fn request_timeout(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let wire = self.wire(method, path).timeout(timeout);
        let response = if let Some(body) = body {
            wire.set("Content-Type", "application/json")
                .send_string(&body.to_string())
        } else {
            wire.call()
        };
        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                return Err(format!("OpenCode HTTP {code}: {body}"));
            }
            Err(error) => return Err(format!("OpenCode HTTP request failed: {error}")),
        };
        let text = response.into_string().map_err(|e| e.to_string())?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        let mut value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("OpenCode returned invalid JSON: {e}"))?;
        if value.get("data").is_some() {
            value = value["data"].take();
        }
        Ok(value)
    }
    pub fn events(
        &self,
        send: mpsc::Sender<Wire>,
        stop: Arc<AtomicBool>,
        shutdown: Arc<Mutex<Option<std::net::TcpStream>>>,
    ) {
        let client = self.clone();
        thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                // Own a clone of the loopback socket so Stop/Done can close a
                // silent SSE stream without imposing an active read deadline.
                let body = client.event_body(&shutdown, &stop)?;
                send.send(Wire::Connected).map_err(|e| e.to_string())?;
                let mut reader = BufReader::new(body);
                let mut data = String::new();
                loop {
                    let mut line = String::new();
                    let bytes = reader.read_line(&mut line).map_err(|e| e.to_string())?;
                    if stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if bytes == 0 {
                        return Err("OpenCode event stream ended unexpectedly.".into());
                    }
                    let line = line.trim_end_matches(['\r', '\n']);
                    if line.is_empty() {
                        if !data.is_empty() {
                            if let Ok(event) = serde_json::from_str::<Value>(data.trim_end()) {
                                if send.send(Wire::Frame(event)).is_err() {
                                    return Ok(());
                                }
                            }
                            data.clear();
                        }
                    } else if let Some(part) = line.strip_prefix("data:") {
                        data.push_str(part.strip_prefix(' ').unwrap_or(part));
                        data.push('\n');
                    }
                }
            })();
            if let Err(error) = result {
                if !stop.load(Ordering::Relaxed) {
                    let _ = send.send(Wire::Ended(error));
                }
            }
        });
    }
    fn event_body(
        &self,
        shutdown: &Mutex<Option<std::net::TcpStream>>,
        stop: &AtomicBool,
    ) -> Result<HttpBody, String> {
        use std::net::ToSocketAddrs;
        let mut url = url::Url::parse(&format!(
            "{}{}",
            self.url.trim_end_matches('/'),
            self.path("/event")
        ))
        .map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("OpenCode's event service must use HTTP or HTTPS".into());
        }
        if !self.directory.is_empty() {
            url.query_pairs_mut()
                .append_pair("directory", &self.directory);
        }
        let host = url.host_str().ok_or("OpenCode event URL has no host")?;
        let port = url
            .port_or_known_default()
            .ok_or("OpenCode event URL has no port")?;
        let addresses = (host, port).to_socket_addrs().map_err(|e| e.to_string())?;
        let mut socket = addresses
            .filter_map(|address| {
                std::net::TcpStream::connect_timeout(&address, Duration::from_secs(15)).ok()
            })
            .next()
            .ok_or("Could not connect to OpenCode's event service")?;
        socket
            .set_read_timeout(Some(Duration::from_secs(15)))
            .map_err(|e| e.to_string())?;
        socket
            .set_write_timeout(Some(Duration::from_secs(15)))
            .map_err(|e| e.to_string())?;
        *shutdown.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(socket.try_clone().map_err(|e| e.to_string())?);
        if stop.load(Ordering::Relaxed) {
            let _ = socket.shutdown(std::net::Shutdown::Both);
            return Err("Stopped".into());
        }
        let mut socket = if url.scheme() == "https" {
            // Match ureq's default TLS backend and trust roots, keeping normal
            // certificate and hostname verification for the local service.
            let roots = ureq::rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            let config = ureq::rustls::ClientConfig::builder_with_provider(
                ureq::rustls::crypto::ring::default_provider().into(),
            )
            .with_protocol_versions(&[&ureq::rustls::version::TLS12, &ureq::rustls::version::TLS13])
            .map_err(|e| format!("Could not configure OpenCode event TLS: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
            let name = ureq::rustls::pki_types::ServerName::try_from(host.to_owned())
                .map_err(|e| format!("Invalid OpenCode event TLS hostname: {e}"))?;
            let mut connection = ureq::rustls::ClientConnection::new(Arc::new(config), name)
                .map_err(|e| e.to_string())?;
            connection
                .complete_io(&mut socket)
                .map_err(|e| format!("OpenCode event TLS handshake failed: {e}"))?;
            EventSocket::Tls(Box::new(ureq::rustls::StreamOwned::new(connection, socket)))
        } else {
            EventSocket::Plain(socket)
        };
        let target = url
            .query()
            .map(|query| format!("{}?{query}", url.path()))
            .unwrap_or_else(|| url.path().into());
        let mut headers = format!(
            "GET {target} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: text/event-stream\r\nConnection: close\r\n"
        );
        if !self.directory.is_empty() {
            headers.push_str(&format!(
                "x-opencode-directory: {}\r\n",
                encode(&self.directory)
            ));
        }
        if let Some(password) = &self.password {
            let auth =
                base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
            headers.push_str(&format!("Authorization: Basic {auth}\r\n"));
        }
        headers.push_str("\r\n");
        socket
            .write_all(headers.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.split_whitespace().nth(1) != Some("200") {
            return Err(format!("OpenCode event stream failed: {}", line.trim()));
        }
        let mut chunked = false;
        loop {
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("OpenCode event stream closed during startup".into());
            }
            if line.trim().is_empty() {
                break;
            }
            if line.to_lowercase().starts_with("transfer-encoding:")
                && line.to_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
        // Only the handshake is bounded. Quiet active streams remain open.
        reader
            .get_ref()
            .socket()
            .set_read_timeout(None)
            .map_err(|e| e.to_string())?;
        Ok(HttpBody {
            reader,
            chunked,
            remaining: 0,
            separator: false,
            ended: false,
        })
    }
    pub fn session(&self, request: &Request) -> Result<String, String> {
        let permissions = self.permissions(request);
        if let Some(id) = &request.provider_id {
            match self.request("GET", &self.path(&format!("/session/{}", encode(id))), None) {
                Ok(session) => {
                    let directory = session["directory"]
                        .as_str()
                        .or_else(|| session["location"]["directory"].as_str());
                    let mut resolved = session["id"].as_str().unwrap_or(id).to_owned();
                    if directory.is_some_and(|directory| {
                        normalize_directory(directory) != normalize_directory(&self.directory)
                    }) {
                        let fork = match self.request(
                            "POST",
                            &self.path(&format!("/session/{}/fork", encode(id))),
                            Some(json!({})),
                        ) {
                            Err(error)
                                if error.contains("HTTP 404")
                                    || error.contains("empty_session") =>
                            {
                                return self.create_session(permissions);
                            }
                            result => result?,
                        };
                        resolved = fork["id"]
                            .as_str()
                            .ok_or("OpenCode fork returned no session id")?
                            .into();
                        if self.v2 {
                            let moved = self
                                .request(
                                    "POST",
                                    &self.path(&format!("/session/{}/move", encode(&resolved))),
                                    Some(json!({"directory":self.directory,"delivery":"queue"})),
                                )
                                .and_then(|_| {
                                    self.request(
                                        "GET",
                                        &self.path(&format!("/session/{}", encode(&resolved))),
                                        None,
                                    )
                                });
                            if !moved
                                .as_ref()
                                .ok()
                                .and_then(|v| {
                                    v["location"]["directory"]
                                        .as_str()
                                        .or_else(|| v["directory"].as_str())
                                })
                                .is_some_and(|directory| {
                                    normalize_directory(directory)
                                        == normalize_directory(&self.directory)
                                })
                            {
                                let _ = self.request(
                                    "DELETE",
                                    &self.path(&format!("/session/{}", encode(&resolved))),
                                    None,
                                );
                                return Err(
                                    "OpenCode did not move the forked session into this workspace"
                                        .into(),
                                );
                            }
                        }
                    }
                    let _ = self.request(
                        "PATCH",
                        &self.path(&format!("/session/{}", encode(&resolved))),
                        Some(permissions),
                    );
                    return Ok(resolved);
                }
                Err(error) if error.contains("HTTP 404") || error.contains("empty_session") => {}
                Err(error) => return Err(error),
            }
        }
        self.create_session(permissions)
    }
    fn create_session(&self, permissions: Value) -> Result<String, String> {
        let mut body = permissions;
        if self.v2 {
            body["location"] = json!({"directory":self.directory});
        }
        let session = self.request("POST", &self.path("/session"), Some(body))?;
        session["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "OpenCode returned no session id".into())
    }
    pub fn revert(&self, session: &str, message: &str) -> Result<(), String> {
        let base = format!("/session/{}", encode(session));
        if self.v2 {
            self.request(
                "POST",
                &self.path(&format!("{base}/revert/stage")),
                Some(json!({"messageID":message})),
            )?;
            self.request("POST", &self.path(&format!("{base}/revert/commit")), None)?;
        } else {
            self.request(
                "POST",
                &format!("{base}/revert"),
                Some(json!({"messageID":message})),
            )?;
        }
        Ok(())
    }
    pub fn repair_unsupported_file_turn(&self, session: &str) -> Result<(), String> {
        let messages = self.request(
            "GET",
            &self.path(&format!("/session/{}/message", encode(session))),
            None,
        )?;
        let rows: Vec<_> = messages.as_array().into_iter().flatten().collect();
        let media = regex::Regex::new(r#"(?i)file part media type\s+([^\s'\"`]+)"#).unwrap();
        let mut failures = Vec::new();
        for row in &rows {
            let info = row.get("info").unwrap_or(row);
            if info["role"] != "assistant" && info["type"] != "assistant" {
                continue;
            }
            let error = events::error_message(&info["error"]);
            if !error.to_lowercase().contains("functionality not supported") {
                continue;
            }
            let Some(mime) = media
                .captures(&error)
                .map(|capture| capture[1].to_lowercase())
            else {
                continue;
            };
            let Some(parent) = info["parentID"].as_str() else {
                continue;
            };
            let Some(created) = info["time"]["created"].as_f64() else {
                continue;
            };
            let rejected = rows
                .iter()
                .find(|row| row["info"]["id"] == parent || row["id"] == parent)
                .is_some_and(|row| {
                    row["parts"].as_array().into_iter().flatten().any(|part| {
                        part["type"] == "file"
                            && part["mime"]
                                .as_str()
                                .is_some_and(|value| value.to_lowercase() == mime)
                    })
                });
            if !rejected {
                continue;
            }
            let later_success = rows.iter().any(|row| {
                let info = row.get("info").unwrap_or(row);
                (info["role"] == "assistant" || info["type"] == "assistant")
                    && info["error"].is_null()
                    && info["time"]["created"]
                        .as_f64()
                        .is_some_and(|time| time > created)
            });
            if !later_success {
                failures.push((parent.to_owned(), created));
            }
        }
        failures.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((message, _)) = failures.first() {
            self.revert(session, message)?;
        }
        Ok(())
    }
    fn permissions(&self, request: &Request) -> Value {
        let mut rules = if request.settings.access == AccessMode::FullAccess {
            vec![("*", "allow")]
        } else {
            vec![("*", "ask"), ("question", "allow")]
        };
        if matches!(
            request.settings.access,
            AccessMode::AutoAcceptEdits | AccessMode::Auto
        ) {
            rules.push(("edit", "allow"));
        }
        if request.settings.access == AccessMode::Auto {
            rules.push(("read", "allow"));
        }
        let rows: Vec<_> = rules
            .into_iter()
            .map(|(permission, action)| {
                if self.v2 {
                    json!({"action":permission,"resource":"*","effect":action})
                } else {
                    json!({"permission":permission,"pattern":"*","action":action})
                }
            })
            .collect();
        if self.v2 {
            json!({"permissions":rows})
        } else {
            json!({"permission":rows})
        }
    }
    pub fn prompt(
        &self,
        request: &Request,
        session: &str,
        steer: bool,
        compact: bool,
    ) -> Result<Option<String>, String> {
        let model = request.model.as_deref().ok_or("OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.")?;
        let (provider, model) = model.split_once('/').filter(|(p,m)| !p.is_empty() && !m.is_empty())
            .ok_or("OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.")?;
        let base = format!("/session/{}", encode(session));
        let variant = request.settings.model_settings.get("variant");
        if self.v2 {
            let mut selected = json!({"providerID":provider,"id":model});
            if let Some(variant) = variant {
                selected["variant"] = variant.clone().into();
            }
            self.request(
                "POST",
                &self.path(&format!("{base}/model")),
                Some(json!({"model":selected})),
            )?;
        }
        if compact {
            let result = if self.v2 {
                self.request(
                    "POST",
                    &self.path(&format!("{base}/compact")),
                    Some(json!({"delivery":"queue"})),
                )?
            } else {
                self.request_timeout(
                    "POST",
                    &format!("{base}/summarize"),
                    Some(json!({"providerID":provider,"modelID":model})),
                    Duration::from_secs(30 * 60),
                )?
            };
            return Ok(result["id"].as_str().map(str::to_owned));
        }
        let agent = if request.settings.plan || request.settings.access == AccessMode::ReadOnly {
            "plan"
        } else {
            request
                .settings
                .model_settings
                .get("agent")
                .map(String::as_str)
                .filter(|agent| *agent != "plan")
                .unwrap_or("build")
        };
        let parts = parts(request)?;
        let result = if self.v2 {
            self.request(
                "POST",
                &self.path(&format!("{base}/agent")),
                Some(json!({"agent":agent})),
            )?;
            let files: Vec<_> = parts
                .iter()
                .filter(|part| part["type"] == "file")
                .map(|part| json!({"uri":part["url"],"name":part["filename"]}))
                .collect();
            let text = parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            self.request(
                "POST",
                &self.path(&format!("{base}/prompt")),
                Some(
                    json!({"text":text,"files":files,"delivery":if steer {"steer"} else {"queue"}}),
                ),
            )?
        } else {
            let mut body = json!({"model":{"providerID":provider,"modelID":model},"agent":agent,"parts":parts});
            if let Some(variant) = variant {
                body["variant"] = variant.clone().into();
            }
            self.request("POST", &format!("{base}/prompt_async"), Some(body))?
        };
        Ok(result["id"].as_str().map(str::to_owned))
    }
    pub fn abort(&self, session: &str) {
        let base = format!("/session/{}", encode(session));
        if self.v2 {
            let _ = self.request(
                "POST",
                &format!("{}?resume=false", self.path(&format!("{base}/interrupt"))),
                None,
            );
        } else {
            let _ = self.request("POST", &format!("{base}/abort"), Some(json!({})));
        }
    }
    pub fn permission(&self, session: &str, id: &str, allow: bool) -> Result<(), String> {
        let reply = if allow { "once" } else { "reject" };
        let (path, body) = if self.v2 {
            (
                self.path(&format!(
                    "/session/{}/permission/{}/reply",
                    encode(session),
                    encode(id)
                )),
                json!({"decision":reply}),
            )
        } else {
            (
                format!("/permission/{}/reply", encode(id)),
                json!({"reply":reply}),
            )
        };
        self.request("POST", &path, Some(body)).map(|_| ())
    }
}
pub(super) fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
fn normalize_directory(value: &str) -> String {
    value.replace('\\', "/").trim_end_matches('/').into()
}
fn parts(request: &Request) -> Result<Vec<Value>, String> {
    let mut parts = Vec::new();
    if !request.prompt.is_empty() {
        parts.push(json!({"type":"text","text":request.prompt}));
    }
    for path in &request.settings.attachments {
        let mime = crate::composer::image_mime(path)
            .map(str::to_owned)
            .or_else(|| {
                std::fs::read(path)
                    .ok()
                    .filter(|bytes| !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok())
                    .map(|_| "text/plain".into())
            });
        if let Some(mime) = mime {
            let file = url::Url::from_file_path(path)
                .map_err(|_| "OpenCode attachment path must be absolute")?;
            parts.push(json!({"type":"file","mime":mime,"filename":path.file_name().unwrap_or_default().to_string_lossy(),"url":file.as_str()}));
        }
    }
    Ok(parts)
}

enum EventSocket {
    Plain(std::net::TcpStream),
    Tls(Box<ureq::rustls::StreamOwned<ureq::rustls::ClientConnection, std::net::TcpStream>>),
}
impl EventSocket {
    fn socket(&self) -> &std::net::TcpStream {
        match self {
            Self::Plain(socket) => socket,
            Self::Tls(stream) => &stream.sock,
        }
    }
}
impl Read for EventSocket {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.read(buffer),
            Self::Tls(stream) => stream.read(buffer),
        }
    }
}
impl Write for EventSocket {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.write(buffer),
            Self::Tls(stream) => stream.write(buffer),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(socket) => socket.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}
struct HttpBody {
    reader: BufReader<EventSocket>,
    chunked: bool,
    remaining: usize,
    separator: bool,
    ended: bool,
}
impl Read for HttpBody {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() || self.ended {
            return Ok(0);
        }
        if !self.chunked {
            return self.reader.read(buffer);
        }
        if self.remaining == 0 {
            if self.separator {
                let mut crlf = [0; 2];
                self.reader.read_exact(&mut crlf)?;
                self.separator = false;
            }
            let mut size = String::new();
            if self.reader.read_line(&mut size)? == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "OpenCode event stream ended within a chunk",
                ));
            }
            self.remaining = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "OpenCode event stream sent an invalid chunk",
                    )
                })?;
            if self.remaining == 0 {
                self.ended = true;
                return Ok(0);
            }
        }
        let length = buffer.len().min(self.remaining);
        let bytes = self.reader.read(&mut buffer[..length])?;
        if bytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "OpenCode event stream ended within a chunk",
            ));
        }
        self.remaining -= bytes;
        if self.remaining == 0 {
            self.separator = true;
        }
        Ok(bytes)
    }
}
