//! Native OpenCode HTTP/SSE transport, ported from the Tauri adapter.
//! v1 owns a local `serve` child; v2 connects to OpenCode's managed service.
use super::*;
use std::collections::VecDeque;
use std::sync::{atomic::AtomicUsize, OnceLock};
mod client;
mod events;
use client::Client;

enum Wire {
    Connected,
    Frame(Value),
    Ended(String),
    Admitted(Result<Option<String>, String>),
    Steered(crate::follow_up::Message, Result<Option<String>, String>),
}
struct StreamStop {
    stopped: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<std::net::TcpStream>>>,
}
impl Drop for StreamStop {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(socket) = self.socket.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
}
struct Server {
    child: std::process::Child,
    url: String,
    _registration: process::Registration,
    _account: crate::accounts::Tracked,
}
impl Drop for Server {
    fn drop(&mut self) {
        process::terminate(&mut self.child);
    }
}
struct Idle {
    server: Server,
    session: String,
    account_id: String,
    cwd: PathBuf,
    key: String,
    at: Instant,
    owner: Option<i64>,
}
static IDLE_LIMIT: AtomicUsize = AtomicUsize::new(0);
fn idle() -> &'static Mutex<Vec<Idle>> {
    static IDLE: OnceLock<Mutex<Vec<Idle>>> = OnceLock::new();
    IDLE.get_or_init(Default::default)
}
pub(super) fn configure(limit: usize) {
    IDLE_LIMIT.store(limit.min(5), Ordering::Relaxed);
    thread::spawn(parked::enforce_idle_limit);
}
pub(super) fn idle_entries() -> Vec<(u32, Instant, Option<i64>)> {
    idle()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|entry| (entry.server.child.id(), entry.at, entry.owner))
        .collect()
}
pub(super) fn retire_idle(pids: &std::collections::HashSet<u32>) {
    let retired = {
        let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
        let mut retired = Vec::new();
        let mut index = 0;
        while index < pool.len() {
            if pids.contains(&pool[index].server.child.id()) {
                retired.push(pool.remove(index));
            } else {
                index += 1;
            }
        }
        retired
    };
    drop(retired);
}
pub(super) fn discard_worktree(cwd: &Path) {
    let retired = {
        let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
        let mut retired = Vec::new();
        let mut index = 0;
        while index < pool.len() {
            if crate::files::is_equal_or_inside(
                &pool[index].cwd.to_string_lossy(),
                &cwd.to_string_lossy(),
            ) {
                retired.push(pool.remove(index));
            } else {
                index += 1;
            }
        }
        retired
    };
    drop(retired);
}
pub(super) fn forget_session(provider_id: &str, account_id: &str, cwd: &Path) {
    events::forget_session(provider_id, account_id, cwd);
    let retired = {
        let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
        let mut retired = Vec::new();
        let mut index = 0;
        while index < pool.len() {
            if pool[index].session == provider_id
                && pool[index].account_id == account_id
                && pool[index].cwd == cwd
            {
                retired.push(pool.remove(index));
            } else {
                index += 1;
            }
        }
        retired
    };
    drop(retired);
}
fn park(server: Server, request: &Request, session: String) {
    if IDLE_LIMIT.load(Ordering::Relaxed) == 0 {
        return;
    }
    process::mark_idle(server.child.id(), true);
    {
        let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
        pool.push(Idle {
            server,
            session,
            account_id: request.settings.account.id.clone(),
            cwd: request.cwd.clone(),
            key: parked::key(request),
            at: Instant::now(),
            owner: process::owner(),
        });
    }
    parked::enforce_idle_limit();
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(3 * 60));
        let retired = {
            let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
            let mut retired = Vec::new();
            let mut index = 0;
            while index < pool.len() {
                if pool[index].at.elapsed() >= Duration::from_secs(3 * 60) {
                    retired.push(pool.remove(index));
                } else {
                    index += 1;
                }
            }
            retired
        };
        drop(retired);
    });
}
fn command(request: &Request) -> Result<Command, String> {
    let mut command = Command::new(&request.binary);
    crate::scm::shell::apply_gui_env(&mut command);
    command.envs(&request.settings.environment);
    request.settings.account.apply(request.kind, &mut command)?;
    command
        .current_dir(&request.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::isolate(&mut command);
    Ok(command)
}
fn exec(request: &Request, args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    let mut child = command(request)?
        .args(args)
        .spawn()
        .map_err(|e| e.to_string())?;
    let _registration = process::register(child.id());
    let _account = crate::accounts::track(child.id(), request.kind, &request.settings.account);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let output = Arc::new(Mutex::new(String::new()));
    let mut readers = Vec::new();
    for stream in [Box::new(stdout) as Box<dyn Read + Send>, Box::new(stderr)] {
        let output = output.clone();
        readers.push(thread::spawn(move || {
            // Lossy, as in Tauri: one invalid byte must not discard the whole
            // stream (and with it the version line).
            let mut bytes = Vec::new();
            let _ = stream.take(8 * 1024 * 1024).read_to_end(&mut bytes);
            output
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_str(&String::from_utf8_lossy(&bytes));
        }));
    }
    let started = Instant::now();
    let result = loop {
        if cancel.load(Ordering::Relaxed) {
            break Err("Stopped".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(format!("OpenCode {} failed", args.join(" ")))
                };
            }
            Err(error) => break Err(error.to_string()),
            _ => {}
        }
        if started.elapsed() >= Duration::from_secs(30) {
            break Err(format!("OpenCode {} timed out", args.join(" ")));
        }
        thread::sleep(TICK);
    };
    process::terminate(&mut child);
    for reader in readers {
        let _ = reader.join();
    }
    let text = output.lock().unwrap_or_else(|e| e.into_inner()).clone();
    result.map(|_| text.clone()).map_err(|error: String| {
        if text.trim().is_empty() {
            error
        } else {
            format!("{error}: {}", text.trim())
        }
    })
}
fn generation(request: &Request, cancel: &AtomicBool) -> Result<bool, String> {
    let output = exec(request, &["--version"], cancel)?;
    // No word boundaries: `v1.14.20` and ANSI-colored output must still match,
    // as with the Tauri parser.
    let re = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap();
    let version = re.captures(&output).ok_or_else(|| {
        format!(
            "Unable to determine OpenCode version from {:?}. MonoCode requires v1.14.19 or newer.",
            output.trim().chars().take(200).collect::<String>()
        )
    })?;
    let components: Vec<u64> = (1..=3).map(|ix| version[ix].parse().unwrap_or(0)).collect();
    match components[0] {
        1 if components.as_slice() >= [1, 14, 19].as_slice() => Ok(false),
        2 if components.as_slice() >= [2, 0, 15].as_slice() => Ok(true),
        1 => Err(format!(
            "OpenCode v{} is too old. Upgrade to v1.14.19 or newer.",
            &version[0]
        )),
        2 => Err(format!(
            "OpenCode v{} is too old. Upgrade to v2.0.15 or newer.",
            &version[0]
        )),
        _ => Err(format!(
            "OpenCode v{} is not supported. MonoCode supports OpenCode v1 and v2.",
            &version[0]
        )),
    }
}
fn service_url(output: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)^https?://(?:127\.0\.0\.1|localhost):\d+/?$").unwrap();
    output
        .lines()
        .map(str::trim)
        .find(|line| re.is_match(line))
        .map(|line| line.trim_end_matches('/').into())
}
fn connect(
    request: &Request,
    cancel: &AtomicBool,
    v2: bool,
) -> Result<(Client, Option<Server>), String> {
    let directory = request.cwd.to_string_lossy().into_owned();
    if v2 {
        let mut url = exec(request, &["service", "status"], cancel)
            .ok()
            .and_then(|output| service_url(&output));
        if url.is_none() {
            url = service_url(&exec(request, &["service", "start"], cancel)?);
            if url.is_none() {
                url = service_url(&exec(request, &["service", "status"], cancel)?);
            }
        }
        let url = url.ok_or("OpenCode background service did not report a local URL.")?;
        let password = exec(request, &["service", "get", "password"], cancel)?
            .trim()
            .to_owned();
        if password.is_empty() {
            return Err("OpenCode background service did not report a password.".into());
        }
        return Ok((Client::new(url, directory, true, Some(password)), None));
    }
    if let Some(id) = request.provider_id.as_deref() {
        let found = {
            let mut pool = idle().lock().unwrap_or_else(|e| e.into_inner());
            let key = parked::key(request);
            pool.iter()
                .position(|idle| idle.session == id && idle.key == key)
                .map(|index| pool.remove(index))
        };
        if let Some(mut found) = found {
            if found.at.elapsed() < Duration::from_secs(3 * 60)
                && matches!(found.server.child.try_wait(), Ok(None))
            {
                process::mark_idle(found.server.child.id(), false);
                return Ok((
                    Client::new(found.server.url.clone(), directory, false, None),
                    Some(found.server),
                ));
            }
        }
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    let mut child = command(request)?
        .args(["serve", "--hostname=127.0.0.1", &format!("--port={port}")])
        .spawn()
        .map_err(|e| e.to_string())?;
    let registration = process::register(child.id());
    let account = crate::accounts::track(child.id(), request.kind, &request.settings.account);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (send, receive) = mpsc::channel();
    for stream in [Box::new(stdout) as Box<dyn Read + Send>, Box::new(stderr)] {
        let send = send.clone();
        thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if !sent && line.to_lowercase().contains("listening") {
                    // The advertised URL may contain punctuation after it.
                    let url = line
                        .split_whitespace()
                        .find(|part| part.starts_with("http://") || part.starts_with("https://"));
                    if let Some(url) = url {
                        let _ = send.send(url.trim_end_matches([',', '.', ';']).to_owned());
                        sent = true;
                    }
                }
            }
        });
    }
    drop(send);
    let mut server = Server {
        child,
        url: String::new(),
        _registration: registration,
        _account: account,
    };
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        if !matches!(server.child.try_wait(), Ok(None)) {
            return Err("OpenCode server exited before it was ready".into());
        }
        match receive.recv_timeout(TICK) {
            Ok(url) => {
                server.url = url.clone();
                return Ok((Client::new(url, directory, false, None), Some(server)));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("OpenCode server exited before reporting a URL".into());
            }
            _ => {}
        }
        if started.elapsed() >= Duration::from_secs(30) {
            return Err("OpenCode server startup timed out".into());
        }
    }
}

pub(super) fn run(
    mut request: Request,
    cancel: Arc<AtomicBool>,
    controls: Option<mpsc::Receiver<Control>>,
    probe: bool,
    compact: bool,
    mut emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<Option<Value>, String> {
    let v2 = generation(&request, &cancel)?;
    if probe {
        return catalog(&request, &cancel, v2).map(Some);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    request.prompt = commands::prepare_prompt(
        Kind::OpenCode,
        request
            .settings
            .prepared_prompt
            .as_deref()
            .unwrap_or(&request.prompt),
        &request.cwd.to_string_lossy(),
        &request.settings.attachments,
    )?;
    let (client, mut server) = connect(&request, &cancel, v2)?;
    let session = client.session(&request)?;
    if request.provider_id.is_some() {
        let _ = client.repair_unsupported_file_turn(&session);
    }
    request.provider_id = Some(session.clone());
    emit(vec![Event::SessionId(session.clone())])?;
    let mut state = events::State::new(request.clone(), session.clone(), compact);
    let (send, receive) = mpsc::channel();
    let stop = StreamStop {
        stopped: Arc::new(AtomicBool::new(false)),
        socket: Arc::new(Mutex::new(None)),
    };
    client.events(send.clone(), stop.stopped.clone(), stop.socket.clone());
    let started = Instant::now();
    let mut connected = false;
    let mut steering = false;
    let mut aborted = false;
    let mut queue = VecDeque::new();
    let result = (|| -> Result<(), String> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                state.cancel(&client);
                aborted = true;
                return Err("Stopped".into());
            }
            if let Some(server) = server.as_mut() {
                if !matches!(server.child.try_wait(), Ok(None)) {
                    return Err("OpenCode server exited".into());
                }
            }
            if let Some(controls) = &controls {
                while let Ok(control) = controls.try_recv() {
                    if let Decision::Steer(message) = control.decision {
                        queue.push_back(message);
                    } else {
                        let events = state.respond(&client, control)?;
                        if !events.is_empty() {
                            emit(events)?;
                        }
                    }
                }
            }
            if connected && state.admitted && !steering {
                if let Some(message) = queue.pop_front() {
                    steering = true;
                    let client = client.clone();
                    let session = session.clone();
                    let mut request = request.clone();
                    let send = send.clone();
                    thread::spawn(move || {
                        request.settings.attachments = message.attachments.clone();
                        let result = commands::prepare_prompt(
                            Kind::OpenCode,
                            message.prepared_prompt.as_deref().unwrap_or(&message.text),
                            &request.cwd.to_string_lossy(),
                            &message.attachments,
                        )
                        .and_then(|prompt| {
                            request.prompt = prompt;
                            client.prompt(&request, &session, true, false)
                        });
                        let _ = send.send(Wire::Steered(message, result));
                    });
                }
            }
            match receive.recv_timeout(TICK) {
                Ok(Wire::Connected) => {
                    connected = true;
                    let client = client.clone();
                    let request = request.clone();
                    let session = session.clone();
                    let send = send.clone();
                    thread::spawn(move || {
                        let _ = send.send(Wire::Admitted(
                            client.prompt(&request, &session, false, compact),
                        ));
                    });
                }
                Ok(Wire::Frame(event)) => {
                    let events = state.frame(&client, event, controls.is_some())?;
                    if !events.is_empty() {
                        emit(events)?;
                    }
                }
                Ok(Wire::Admitted(result)) => {
                    state.admit(result?);
                    if compact && !v2 {
                        return emit(vec![Event::Done]);
                    }
                    if !compact {
                        emit(vec![Event::Ready])?;
                    }
                }
                Ok(Wire::Steered(message, result)) => {
                    steering = false;
                    match result {
                        Ok(_) => {
                            state.protocol.text_key = None;
                            emit(vec![Event::FollowUp(message)])?;
                        }
                        Err(error) => emit(vec![Event::FollowUpFailed {
                            id: message.id,
                            error,
                        }])?,
                    }
                }
                Ok(Wire::Ended(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("OpenCode event stream ended unexpectedly.".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(result) = state.terminal.take() {
                result?;
                return emit(vec![Event::Done]);
            }
            if crate::visual_bridge::configured(&request.settings.environment) {
                emit(Vec::new())?;
            }
            if !connected && started.elapsed() >= Duration::from_secs(30) {
                return Err("OpenCode event stream startup timed out".into());
            }
            if compact && started.elapsed() >= Duration::from_secs(30 * 60) {
                return Err("OpenCode compaction timed out".into());
            }
            if request.settings.text_only && started.elapsed() >= Duration::from_secs(45) {
                return Err("Background text request timed out".into());
            }
        }
    })();
    drop(stop);
    if result.is_err() && !aborted {
        state.cancel(&client);
        aborted = true;
    }
    // Stop aborts the prompt while retaining the local HTTP server, just as
    // cancelOpenCodeTurn does. A transport failure still retires its server.
    if result.is_ok() || (aborted && cancel.load(Ordering::Relaxed)) {
        if let Some(server) = server.take() {
            park(server, &request, session);
        }
    }
    result.map(|_| None)
}

fn catalog(request: &Request, cancel: &AtomicBool, v2: bool) -> Result<Value, String> {
    let mut models = Vec::new();
    let mut agents = Vec::new();
    if v2 {
        let (mut client, _) = connect(request, cancel, true)?;
        client.directory.clear();
        let raw = client.request("GET", &client.path("/model"), None)?;
        let providers = client.request("GET", &client.path("/provider"), None)?;
        let raw_agents = client
            .request("GET", &client.path("/agent"), None)
            .unwrap_or_else(|_| json!([]));
        for agent in raw_agents.as_array().into_iter().flatten() {
            let id = agent["id"]
                .as_str()
                .or_else(|| agent["name"].as_str())
                .unwrap_or("");
            let primary = agent["mode"]
                .as_str()
                .is_none_or(|mode| matches!(mode, "primary" | "all"));
            if primary
                && !id.is_empty()
                && agent["hidden"] != true
                && !matches!(id, "compaction" | "summary" | "title")
            {
                agents.push(json!({"value":id,"name":agent["name"].as_str().unwrap_or(id)}));
            }
        }
        for row in raw.as_array().into_iter().flatten() {
            let Some(provider) = row["providerID"].as_str() else {
                continue;
            };
            let Some(raw_id) = row["id"].as_str().or_else(|| row["modelID"].as_str()) else {
                continue;
            };
            if row["enabled"] == false {
                continue;
            }
            let id = raw_id
                .strip_prefix(&format!("{provider}/"))
                .unwrap_or(raw_id);
            let provider_name = providers
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["id"] == provider)
                .and_then(|p| p["name"].as_str())
                .unwrap_or(provider);
            models.push(catalog_row(
                &format!("{provider}/{id}"),
                row,
                Some(provider_name),
                &agents,
            ));
        }
    } else {
        if let Ok(output) = exec(request, &["agent", "list"], cancel) {
            let re = regex::Regex::new(r"^(.+)\s+\((\S+)\)\s*$").unwrap();
            for line in output.lines() {
                if let Some(row) = re.captures(line) {
                    let id = row[1].trim();
                    if matches!(&row[2], "primary" | "all")
                        && !matches!(id, "compaction" | "summary" | "title")
                    {
                        agents.push(json!({"value":id,"name":id}));
                    }
                }
            }
        }
        let output = exec(request, &["models", "--verbose"], cancel)?;
        let slug = regex::Regex::new(r"^(\S+/\S+)\s*$").unwrap();
        let mut current: Option<String> = None;
        let mut json = String::new();
        for line in output.lines() {
            let next = slug.captures(line).map(|row| row[1].to_owned());
            if next.is_some() {
                if let Some(id) = current.take() {
                    if let Ok(row) = serde_json::from_str::<Value>(&json) {
                        models.push(catalog_row(&id, &row, None, &agents));
                    }
                }
                json.clear();
                current = next;
            } else if current.is_some() {
                json.push_str(line);
                json.push('\n');
            }
        }
        if let Some(id) = current {
            if let Ok(row) = serde_json::from_str::<Value>(&json) {
                models.push(catalog_row(&id, &row, None, &agents));
            }
        }
    }
    models.sort_by_key(|row| row["name"].as_str().unwrap_or("").to_lowercase());
    Ok(json!({"availableModels":models}))
}
fn catalog_row(id: &str, row: &Value, provider: Option<&str>, agents: &[Value]) -> Value {
    let mut variants: Vec<Value> = if let Some(rows) = row["variants"].as_array() {
        rows.iter()
            .filter_map(|row| row["id"].as_str().map(|id| json!({"value":id,"name":id})))
            .collect()
    } else {
        row["variants"]
            .as_object()
            .into_iter()
            .flat_map(|rows| rows.keys())
            .map(|id| json!({"value":id,"name":id}))
            .collect()
    };
    let order = [
        "none",
        "minimal",
        "low",
        "medium",
        "high",
        "xhigh",
        "extra-high",
        "max",
        "ultra",
    ];
    variants.sort_by_key(|row| {
        let value = row["value"].as_str().unwrap_or("").to_lowercase();
        (
            order
                .iter()
                .position(|rank| *rank == value)
                .unwrap_or(usize::MAX),
            value,
        )
    });
    let mut config = Vec::new();
    let provider_id = id.split('/').next().unwrap_or("");
    if !variants.is_empty() {
        let preferred = if provider_id == "anthropic" || provider_id.starts_with("google") {
            ["high", ""]
        } else {
            ["medium", "high"]
        };
        let selected = preferred
            .iter()
            .find_map(|preferred| variants.iter().find(|row| row["value"] == *preferred))
            .unwrap_or(&variants[0])["value"]
            .clone();
        config.push(json!({"id":"variant","name":"Variant","type":"select","currentValue":selected,"options":variants}));
    }
    if !agents.is_empty() {
        let selected = agents
            .iter()
            .find(|row| row["value"] == "build")
            .unwrap_or(&agents[0])["value"]
            .clone();
        config.push(json!({"id":"agent","name":"Agent","type":"select","currentValue":selected,"options":agents}));
    }
    let name = row["name"].as_str().unwrap_or(id);
    let name = provider
        .map(|provider| format!("{name} · {provider}"))
        .unwrap_or_else(|| name.into());
    json!({"modelId":id,"name":name,"configOptions":config})
}
pub(super) fn rewind(
    request: &Request,
    _before_turn: Option<&str>,
) -> Result<String, rewind::RewindError> {
    let cancel = AtomicBool::new(false);
    let session = request
        .provider_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("The conversation has no saved provider session to rewind")?;
    // Retire every warm launch configuration for this binding before changing
    // durable history, including a process parked with an older model.
    forget_session(session, &request.settings.account.id, &request.cwd);
    let v2 = generation(request, &cancel)?;
    let (client, server) = connect(request, &cancel, v2)?;
    let messages = client.request(
        "GET",
        &client.path(&format!("/session/{}/message", client::encode(session))),
        None,
    )?;
    let mut candidates: Vec<_> = messages
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let info = row.get("info").unwrap_or(row);
            if info["role"] == "user" || info["type"] == "user" {
                info["id"]
                    .as_str()
                    .map(|id| (id.to_owned(), info["time"]["created"].as_f64()))
            } else {
                None
            }
        })
        .collect();
    if candidates.iter().all(|(_, time)| time.is_some()) {
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    }
    let message = candidates
        .last()
        .map(|(id, _)| id)
        .ok_or("OpenCode did not expose the last user message")?;
    let result = client.revert(session, message);
    result.map_err(|message| rewind::RewindError {
        message,
        provider_may_have_changed: true,
    })?;
    // A reverted history must never be resumed by the old warm generation.
    drop(server);
    Ok(session.into())
}
