//! A paired phone controls the workspace through the app that owns it. The
//! host pairs devices, serves the link and checks every request; the app
//! answers them from its own state. No arbitrary app commands are exposed.
use super::catalog::{recent_sessions, session_page};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use monocode_mobile_link::{now, Address, Gate, Identity, WatchSink, PORT, VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::sync::oneshot;

/// Methods the app answers. Pairing, leaving and the catalog are the host's.
const APP_METHODS: &[&str] = &[
    "conversation.read",
    "conversation.older",
    "models.list",
    "accounts.usage",
    "session.configure",
    "project.changes",
    "project.worktrees",
    "project.files",
    "project.file",
    "project.search",
    "project.fileHighlight",
    "project.git",
    "project.gitAction",
    "session.start",
    "conversation.send",
    "session.cancel",
    "session.rename",
    "session.delete",
    "session.archive",
];
/// Under the phone's own wait, so it hears why rather than timing out.
const REPLY_WAIT: Duration = Duration::from_secs(25);
const MAX_PENDING: usize = 16;
const MAX_WATCHES_PER_DEVICE: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub fingerprint: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    enabled: bool,
    #[serde(default)]
    open_router: bool,
    #[serde(default)]
    remote_address: String,
    #[serde(default)]
    devices: Vec<Device>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            open_router: true,
            remote_address: String::new(),
            devices: Vec::new(),
        }
    }
}

/// What Settings shows about mobile access.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub listening: bool,
    pub name: String,
    pub fingerprint: Option<String>,
    pub addresses: Vec<Address>,
    pub devices: Vec<Device>,
    pub open_router: bool,
    pub remote_address: String,
    pub router_status: String,
    /// Why the service could not start, when it could not.
    pub error: Option<String>,
}

/// A pairing code: `url` goes in the QR code and expires at `expires` (seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct Invite {
    pub url: String,
    pub expires: u64,
}

/// A phone request for the app to answer with [`Request::reply`].
pub struct Request {
    /// Scoped to the device, stable when the phone retries the same command.
    pub command_id: String,
    pub method: String,
    pub params: Value,
    reply: oneshot::Sender<Result<Value, String>>,
}
impl Request {
    pub fn reply(self, result: Result<Value, String>) {
        let _ = self.reply.send(result);
    }
}

/// A phone starts following a conversation. The app accepts or refuses it
/// with [`WatchStart::accept`], then sends frames with [`MobileHost::push`]
/// until it receives [`Event::WatchStop`] or a push reports [`Push::Gone`].
pub struct WatchStart {
    pub id: String,
    pub conversation_id: String,
    /// How many of the latest turns to open on.
    pub tail: Option<u64>,
    /// The first turn the phone already has, to carry on from instead.
    pub from: Option<String>,
    /// The phone takes field edits and text suffixes.
    pub incremental: bool,
    accept: oneshot::Sender<Result<(), String>>,
}
impl WatchStart {
    pub fn accept(self, result: Result<(), String>) {
        let _ = self.accept.send(result);
    }
}

pub enum Event {
    Request(Request),
    Watch(WatchStart),
    WatchStop(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Push {
    /// The phone has not taken earlier frames; keep the baseline and retry.
    Busy,
    /// The watch has ended.
    Gone,
}

struct Invitation {
    expires: u64,
}
struct Watcher {
    peer: String,
    sink: WatchSink,
}
struct Inner {
    config: Config,
    identity: Option<Identity>,
    endpoint: Option<quinn::Endpoint>,
    invitations: HashMap<String, Invitation>,
    catalog: Value,
    watches: HashMap<String, Watcher>,
    router_address: Option<Address>,
    router_status: String,
    router_started: bool,
    error: Option<String>,
}

#[derive(Clone)]
pub struct MobileHost {
    inner: Arc<Mutex<Inner>>,
    gate: Gate,
    path: PathBuf,
    events: async_channel::Sender<Event>,
    runtime: Arc<OnceLock<tokio::runtime::Runtime>>,
    /// Serializes keyring access without holding `inner`, which the UI reads.
    identity_load: Arc<Mutex<()>>,
    pending: Arc<AtomicUsize>,
    shut_down: Arc<AtomicBool>,
}

fn unavailable<T>(_: T) -> String {
    "Mobile service unavailable".into()
}

impl MobileHost {
    /// Loads `path` (the desktop's `mobile-access.json`) and starts serving
    /// when mobile access is on. The app reads phone requests from the receiver.
    pub fn open(path: PathBuf) -> Result<(Self, async_channel::Receiver<Event>), String> {
        let config: Config = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Mobile settings are invalid: {e}"))?
        } else {
            Config::default()
        };
        let gate = Gate::default();
        for device in &config.devices {
            gate.allow(device.fingerprint.clone(), None);
        }
        let enabled = config.enabled;
        let (events, receiver) = async_channel::bounded(64);
        let host = Self {
            inner: Arc::new(Mutex::new(Inner {
                config,
                identity: None,
                endpoint: None,
                invitations: HashMap::new(),
                catalog: json!({"projects":[],"monos":[],"sessions":[]}),
                watches: HashMap::new(),
                router_address: None,
                router_status: "Off".into(),
                router_started: false,
                error: None,
            })),
            gate,
            path,
            events,
            runtime: Arc::default(),
            identity_load: Arc::default(),
            pending: Arc::default(),
            shut_down: Arc::default(),
        };
        // The keyring read can wait on securityd or an access prompt, and the
        // app opens the host while launching.
        if enabled {
            let starting = host.clone();
            std::thread::Builder::new()
                .name("monocode-mobile-start".into())
                .spawn(move || {
                    if let Err(error) = starting.start() {
                        if let Ok(mut inner) = starting.inner.lock() {
                            inner.error = Some(error);
                        }
                    }
                })
                .map_err(|e| format!("Mobile service could not start: {e}"))?;
        }
        Ok((host, receiver))
    }

    pub fn status(&self) -> Status {
        let Ok(inner) = self.inner.lock() else {
            return Status::default();
        };
        Status {
            enabled: inner.config.enabled,
            listening: inner.endpoint.is_some(),
            name: name(),
            fingerprint: inner.identity.as_ref().and_then(|v| v.fingerprint().ok()),
            addresses: addresses(&inner),
            devices: inner.config.devices.clone(),
            open_router: inner.config.open_router,
            remote_address: inner.config.remote_address.clone(),
            router_status: inner.router_status.clone(),
            error: inner.error.clone(),
        }
    }

    /// Whether the app should keep its catalog published.
    pub fn enabled(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.config.enabled)
    }

    pub fn configure(
        &self,
        enabled: bool,
        open_router: bool,
        remote_address: &str,
    ) -> Result<(), String> {
        let remote_address = remote_address.trim();
        if !remote_address.is_empty() {
            Address::parse(remote_address)?;
        }
        {
            let mut inner = self.inner.lock().map_err(unavailable)?;
            let config = Config {
                enabled,
                open_router,
                remote_address: remote_address.into(),
                devices: inner.config.devices.clone(),
            };
            self.save(&config)?;
            inner.config = config;
            inner.error = None;
            if !enabled {
                if let Some(endpoint) = inner.endpoint.take() {
                    endpoint.close(1_u32.into(), b"Mobile access disabled");
                }
                for key in inner.invitations.keys() {
                    self.gate.remove(key);
                }
                inner.invitations.clear();
                inner.watches.clear();
            }
        }
        if enabled {
            if let Err(error) = self.start() {
                self.inner.lock().map_err(unavailable)?.error = Some(error.clone());
                return Err(error);
            }
        }
        super::router::restart(self);
        Ok(())
    }

    /// One visible invitation at a time; a new one revokes the last.
    pub fn invite(&self) -> Result<Invite, String> {
        let identity = self.identity()?;
        let mut inner = self.inner.lock().map_err(unavailable)?;
        if !inner.config.enabled || inner.endpoint.is_none() {
            return Err("Enable mobile access first".into());
        }
        for key in inner.invitations.keys() {
            self.gate.remove(key);
        }
        inner.invitations.clear();
        let join = Identity::generate()?;
        let key = join.fingerprint()?;
        let expires = now() + 15 * 60;
        self.gate.allow(key.clone(), Some(expires));
        inner.invitations.insert(key, Invitation { expires });
        let invitation = json!({"version":VERSION,"name":name(),"fingerprint":identity.fingerprint()?,"addresses":addresses(&inner),"identity":join,"expires":expires});
        let code =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invitation).map_err(|e| e.to_string())?);
        Ok(Invite {
            url: format!("monocode://pair?i={code}"),
            expires,
        })
    }

    pub fn cancel_invite(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            for key in inner.invitations.keys() {
                self.gate.remove(key);
            }
            inner.invitations.clear();
        }
    }

    pub fn revoke(&self, device_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(unavailable)?;
        let mut config = inner.config.clone();
        if let Some(device) = config.devices.iter().find(|v| v.id == device_id) {
            self.gate.remove(&device.fingerprint);
            let peer = device.fingerprint.clone();
            inner.watches.retain(|_, watch| watch.peer != peer);
        }
        config.devices.retain(|device| device.id != device_id);
        self.save(&config)?;
        inner.config = config;
        Ok(())
    }

    /// The app's workspace: `projects`, `monos` and `sessions` (newest first).
    pub fn publish(&self, catalog: Value) -> Result<(), String> {
        if !catalog["projects"].is_array()
            || !catalog["monos"].is_array()
            || !catalog["sessions"].is_array()
        {
            return Err("Invalid mobile catalog".into());
        }
        self.inner.lock().map_err(unavailable)?.catalog = catalog;
        Ok(())
    }

    /// The watch's next frame.
    pub fn push(&self, id: &str, update: Value) -> Result<(), Push> {
        let mut inner = self.inner.lock().map_err(|_| Push::Gone)?;
        let watch = inner.watches.get(id).ok_or(Push::Gone)?;
        match watch.sink.try_send(update) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Err(Push::Busy),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                inner.watches.remove(id);
                Err(Push::Gone)
            }
        }
    }

    /// Stops serving and releases the router mapping; for app shutdown.
    pub fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(endpoint) = inner.endpoint.take() {
                endpoint.close(0_u32.into(), b"Desktop closed");
            }
            inner.watches.clear();
        }
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, String> {
        if let Some(runtime) = self.runtime.get() {
            return Ok(runtime);
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("monocode-mobile")
            .enable_all()
            .build()
            .map_err(|e| format!("Mobile service could not start: {e}"))?;
        Ok(self.runtime.get_or_init(|| runtime))
    }

    fn start(&self) -> Result<(), String> {
        let identity = self.identity()?;
        let runtime = self.runtime()?;
        let mut inner = self.inner.lock().map_err(unavailable)?;
        if !inner.config.enabled
            || self.shut_down.load(Ordering::SeqCst)
            || inner.endpoint.is_some()
        {
            return Ok(());
        }
        let endpoint = {
            let _context = runtime.enter();
            monocode_mobile_link::server_endpoint(
                &identity,
                self.gate.clone(),
                format!("[::]:{PORT}").parse().unwrap(),
            )
            .map_err(|error| {
                // Another copy of the app (or the earlier desktop) holds the
                // port; phones would reach it instead of this one.
                if error.contains("in use") || error.contains("os error 48") {
                    format!("Another MonoCode window or app is already accepting phone connections on port {PORT}. Quit it, then turn Accept connections off and on.")
                } else {
                    error
                }
            })?
        };
        inner.endpoint = Some(endpoint.clone());
        drop(inner);
        let service = self.clone();
        let watching = self.clone();
        runtime.spawn(monocode_mobile_link::serve(
            endpoint,
            self.gate.clone(),
            move |peer, request| {
                let host = service.clone();
                async move { host.handle(&peer, request).await }
            },
            move |peer, request, sink| {
                let host = watching.clone();
                async move { host.watch(&peer, request, sink).await }
            },
        ));
        super::router::restart(self);
        Ok(())
    }

    fn save(&self, config: &Config) -> Result<(), String> {
        let parent = self.path.parent().ok_or("Invalid mobile settings path")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!(".mobile-access-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &temporary,
            serde_json::to_vec(config).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &self.path).map_err(|e| e.to_string())
    }

    /// The desktop's long-lived identity, kept in the OS keyring under the same
    /// entry the earlier desktop app used, so paired phones stay paired.
    fn identity(&self) -> Result<Identity, String> {
        let _loading = self.identity_load.lock().map_err(unavailable)?;
        let paired = {
            let inner = self.inner.lock().map_err(unavailable)?;
            if let Some(identity) = &inner.identity {
                return Ok(identity.clone());
            }
            !inner.config.devices.is_empty()
        };
        let entry = keyring::Entry::new("com.monocode.mobile", &keyring_account(&self.path))
            .map_err(|e| e.to_string())?;
        let identity = match entry.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map_err(|_| "Saved desktop mobile identity is invalid")?,
            Err(keyring::Error::NoEntry) => {
                if paired {
                    return Err("Desktop mobile identity is missing. Remove paired devices before creating a new identity.".into());
                }
                let identity = Identity::generate()?;
                entry
                    .set_password(&serde_json::to_string(&identity).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                identity
            }
            Err(error) => return Err(format!("Cannot read desktop mobile identity: {error}")),
        };
        self.inner.lock().map_err(unavailable)?.identity = Some(identity.clone());
        Ok(identity)
    }

    fn enroll(&self, inner: &mut Inner, peer: &str, params: &Value) -> Result<Value, String> {
        let invitation = inner
            .invitations
            .get(peer)
            .ok_or("Invitation already used or revoked")?;
        if invitation.expires <= now() {
            return Err("Invitation expired".into());
        }
        let certificate = params["certificate"]
            .as_str()
            .ok_or("Missing device identity")?;
        let proof = params["proof"].as_str().ok_or("Missing device proof")?;
        let fingerprint = monocode_mobile_link::verify_proof(certificate, peer, proof)?;
        let name = params["name"]
            .as_str()
            .unwrap_or("Phone")
            .trim()
            .chars()
            .take(80)
            .collect::<String>();
        let id = uuid::Uuid::new_v4().to_string();
        let mut config = inner.config.clone();
        config
            .devices
            .retain(|device| device.fingerprint != fingerprint);
        config.devices.push(Device {
            id: id.clone(),
            name,
            fingerprint: fingerprint.clone(),
        });
        self.save(&config)?;
        inner.config = config;
        inner.invitations.remove(peer);
        self.gate.remove(peer);
        self.gate.allow(fingerprint, None);
        let desktop = inner
            .identity
            .as_ref()
            .ok_or("Desktop mobile identity is unavailable")?
            .fingerprint()?;
        Ok(
            json!({"deviceId":id,"name":self::name(),"fingerprint":desktop,"addresses":addresses(inner)}),
        )
    }

    async fn handle(&self, peer: &str, request: Value) -> Result<Value, String> {
        if request["version"] != VERSION {
            return Err("Update MonoCode on both devices".into());
        }
        let method = request["method"].as_str().ok_or("Invalid mobile method")?;
        let params = request["params"].clone();
        if !params.is_object() {
            return Err("Invalid mobile parameters".into());
        }
        let device = {
            let mut inner = self.inner.lock().map_err(unavailable)?;
            if !inner.config.enabled {
                return Err("Mobile access disabled".into());
            }
            if method == "device.enroll" {
                return self.enroll(&mut inner, peer, &params);
            }
            let device = inner
                .config
                .devices
                .iter()
                .find(|device| device.fingerprint == peer)
                .ok_or("Device revoked or not paired")?
                .clone();
            match method {
                "device.leave" => {
                    let mut config = inner.config.clone();
                    config.devices.retain(|v| v.id != device.id);
                    self.save(&config)?;
                    inner.config = config;
                    self.gate.remove(peer);
                    inner.watches.retain(|_, watch| watch.peer != peer);
                    return Ok(Value::Null);
                }
                "workspace.list" => {
                    let mut catalog = inner.catalog.clone();
                    catalog["sessions"] = json!(recent_sessions(&catalog["sessions"]));
                    catalog["addresses"] = json!(addresses(&inner));
                    return Ok(catalog);
                }
                "sessions.list" => return Ok(session_page(&inner.catalog["sessions"], &params)),
                _ => {}
            }
            if !APP_METHODS.contains(&method) {
                return Err("Unsupported mobile operation".into());
            }
            device
        };
        let command_id = request["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or("Missing command ID")?;
        if self.pending.fetch_add(1, Ordering::SeqCst) >= MAX_PENDING {
            self.pending.fetch_sub(1, Ordering::SeqCst);
            return Err("Desktop is busy; try again".into());
        }
        let (reply, receive) = oneshot::channel();
        let sent = self
            .events
            .try_send(Event::Request(Request {
                command_id: format!("{}:{command_id}", device.id),
                method: method.into(),
                params,
                reply,
            }))
            .is_ok();
        let response = if sent {
            tokio::time::timeout(REPLY_WAIT, receive)
                .await
                .map_err(|_| "Desktop did not answer".to_string())
                .and_then(|reply| reply.map_err(|_| "Desktop is unavailable".to_string()))
        } else {
            Err("Desktop is unavailable".to_string())
        };
        self.pending.fetch_sub(1, Ordering::SeqCst);
        let response = response?;
        if !self.gate.admits(peer) {
            return Err("Device revoked".into());
        }
        response
    }

    /// A phone starts watching a conversation. The app sends its state, then
    /// every change, until the phone goes or the watch is stopped.
    async fn watch(&self, peer: &str, request: Value, sink: WatchSink) -> Result<(), String> {
        if request["version"] != VERSION {
            return Err("Update MonoCode on both devices".into());
        }
        if request["method"] != "conversation.watch" {
            return Err("Unsupported mobile operation".into());
        }
        let params = &request["params"];
        let conversation = params["conversationId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or("Missing workspace item")?
            .to_string();
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut inner = self.inner.lock().map_err(unavailable)?;
            if !inner.config.enabled {
                return Err("Mobile access disabled".into());
            }
            if !inner
                .config
                .devices
                .iter()
                .any(|device| device.fingerprint == peer)
            {
                return Err("Device revoked or not paired".into());
            }
            inner.watches.retain(|_, watch| !watch.sink.is_closed());
            if inner
                .watches
                .values()
                .filter(|watch| watch.peer == peer)
                .count()
                >= MAX_WATCHES_PER_DEVICE
            {
                return Err("Desktop is busy; try again".into());
            }
            inner.watches.insert(
                id.clone(),
                Watcher {
                    peer: peer.into(),
                    sink: sink.clone(),
                },
            );
        }
        let (accept, accepted) = oneshot::channel();
        let start = Event::Watch(WatchStart {
            id: id.clone(),
            conversation_id: conversation,
            tail: params["tail"].as_u64(),
            from: params["from"]
                .as_str()
                .filter(|from| from.len() <= 256)
                .map(Into::into),
            incremental: params["incremental"] == true,
            accept,
        });
        let accepted = if self.events.try_send(start).is_ok() {
            tokio::time::timeout(REPLY_WAIT, accepted)
                .await
                .map_err(|_| "Desktop did not answer".to_string())
                .and_then(|result| result.map_err(|_| "Desktop is unavailable".to_string()))
                .and_then(|result| result)
        } else {
            Err("Desktop is unavailable".into())
        };
        if let Err(error) = accepted {
            if let Ok(mut inner) = self.inner.lock() {
                inner.watches.remove(&id);
            }
            return Err(error);
        }
        // End the watch with the phone or the host, whichever goes first.
        let host = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sink.closed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {
                        if !host.inner.lock().is_ok_and(|inner| inner.watches.contains_key(&id)) {
                            break;
                        }
                    }
                }
            }
            if let Ok(mut inner) = host.inner.lock() {
                inner.watches.remove(&id);
            }
            let _ = host.events.send(Event::WatchStop(id)).await;
        });
        Ok(())
    }

    pub(super) fn router_config(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| {
            inner.config.enabled && inner.config.open_router && inner.endpoint.is_some()
        })
    }
    pub(super) fn router_result(&self, address: Option<Address>, status: String) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.router_address = address;
            inner.router_status = status;
        }
    }
    pub(super) fn router_claim(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|mut inner| !std::mem::replace(&mut inner.router_started, true))
    }
    pub(super) fn is_shut_down(&self) -> bool {
        self.shut_down.load(Ordering::SeqCst)
    }
}

fn keyring_account(path: &Path) -> String {
    format!("desktop:{}", path.display())
}

fn addresses(inner: &Inner) -> Vec<Address> {
    let mut result: Vec<Address> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| {
            let ip = interface.ip();
            if ip.is_loopback()
                || ip.is_unspecified()
                || matches!(ip, std::net::IpAddr::V6(v) if v.is_unicast_link_local())
            {
                None
            } else {
                Some(Address {
                    host: ip.to_string(),
                    port: PORT,
                })
            }
        })
        .collect();
    if let Some(address) = &inner.router_address {
        result.insert(0, address.clone());
    }
    if let Ok(address) = Address::parse(&inner.config.remote_address) {
        result.insert(0, address);
    }
    result.dedup();
    result.truncate(16);
    result
}

fn name() -> String {
    hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (
        MobileHost,
        async_channel::Receiver<Event>,
        Identity,
        Identity,
    ) {
        let join = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let key = join.fingerprint().unwrap();
        let dir =
            std::env::temp_dir().join(format!("monocode-mobile-test-{}", uuid::Uuid::new_v4()));
        let (host, events) = MobileHost::open(dir.join("mobile-access.json")).unwrap();
        host.gate.allow(key.clone(), Some(now() + 900));
        {
            let mut inner = host.inner.lock().unwrap();
            inner.config.enabled = true;
            inner.identity = Some(Identity::generate().unwrap());
            inner.invitations.insert(
                key,
                Invitation {
                    expires: now() + 900,
                },
            );
        }
        (host, events, join, phone)
    }

    fn check_pending_start_stays_stopped(shutdown: bool) {
        let (host, _events, _join, _phone) = fixture();
        host.inner.lock().unwrap().config.open_router = false;
        let loading = host.identity_load.lock().unwrap();
        let starting = host.clone();
        let (entered, waiting) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            entered.send(()).unwrap();
            starting.start()
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        if shutdown {
            host.shutdown();
        } else {
            host.configure(false, false, "").unwrap();
        }
        drop(loading);
        let result = worker.join().unwrap();
        let listening = host.status().listening;
        host.shutdown();
        if host.path.exists() {
            std::fs::remove_dir_all(host.path.parent().unwrap()).unwrap();
        }
        result.unwrap();
        assert!(!listening, "listener started after shutdown={shutdown}");
    }

    #[test]
    fn pending_start_does_not_listen_after_disable() {
        check_pending_start_stays_stopped(false);
    }

    #[test]
    fn pending_start_does_not_listen_after_shutdown() {
        check_pending_start_stays_stopped(true);
    }

    #[test]
    fn invitation_is_consumed_once_and_only_public_device_metadata_is_persisted() {
        let (host, _events, join, phone) = fixture();
        let key = join.fingerprint().unwrap();
        let params = json!({"certificate":phone.certificate,"proof":phone.proof(&key).unwrap(),"name":"iPhone"});
        let mut inner = host.inner.lock().unwrap();
        let result = host.enroll(&mut inner, &key, &params).unwrap();
        assert!(result["deviceId"].is_string());
        assert!(!host.gate.admits(&key));
        assert!(host.gate.admits(&phone.fingerprint().unwrap()));
        assert!(host.enroll(&mut inner, &key, &params).is_err());
        let saved = std::fs::read_to_string(&host.path).unwrap();
        assert!(!saved.contains("privateKey") && !saved.contains(&phone.private_key));
        assert_eq!(inner.config.devices.len(), 1);
        std::fs::remove_dir_all(host.path.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_proof_or_expiration_does_not_consume_an_invitation_or_enroll_a_device() {
        let (host, _events, join, phone) = fixture();
        let key = join.fingerprint().unwrap();
        let mut inner = host.inner.lock().unwrap();
        let params = json!({"certificate":phone.certificate,"proof":phone.proof("different-invitation").unwrap()});
        assert!(host.enroll(&mut inner, &key, &params).is_err());
        assert!(inner.config.devices.is_empty() && inner.invitations.contains_key(&key));
        inner.invitations.get_mut(&key).unwrap().expires = now() - 1;
        let params = json!({"certificate":phone.certificate,"proof":phone.proof(&key).unwrap()});
        assert!(host.enroll(&mut inner, &key, &params).is_err());
        assert!(inner.config.devices.is_empty() && !host.path.exists());
    }

    fn paired() -> (MobileHost, async_channel::Receiver<Event>, String) {
        let (host, events, join, phone) = fixture();
        let key = join.fingerprint().unwrap();
        let params = json!({"certificate":phone.certificate,"proof":phone.proof(&key).unwrap()});
        host.enroll(&mut host.inner.lock().unwrap(), &key, &params)
            .unwrap();
        (host, events, phone.fingerprint().unwrap())
    }

    #[tokio::test]
    async fn requests_reach_the_app_scoped_to_the_device_and_return_its_answer() {
        let (host, events, peer) = paired();
        let answering = tokio::spawn(async move {
            let Ok(Event::Request(request)) = events.recv().await else {
                panic!("expected a request");
            };
            assert!(request.command_id.ends_with(":c1"));
            assert_eq!(request.method, "session.cancel");
            request.reply(Ok(json!({"sessionId":"s"})));
        });
        let result = host
            .handle(&peer, json!({"version":VERSION,"id":"c1","method":"session.cancel","params":{"conversationId":"s"}}))
            .await
            .unwrap();
        assert_eq!(result["sessionId"], "s");
        answering.await.unwrap();
        assert!(host
            .handle(
                &peer,
                json!({"version":VERSION,"id":"c2","method":"shell.exec","params":{}})
            )
            .await
            .is_err());
        assert!(host
            .handle(
                "stranger",
                json!({"version":VERSION,"id":"c3","method":"workspace.list","params":{}})
            )
            .await
            .is_err());
        std::fs::remove_dir_all(host.path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn the_catalog_answers_workspace_polls_without_the_app() {
        let (host, _events, peer) = paired();
        host.publish(
            json!({"projects":[{"id":"p"}],"monos":[],"sessions":[{"id":"s","projectId":"p"}]}),
        )
        .unwrap();
        let catalog = host
            .handle(
                &peer,
                json!({"version":VERSION,"id":"c","method":"workspace.list","params":{}}),
            )
            .await
            .unwrap();
        assert_eq!(catalog["sessions"][0]["id"], "s");
        assert!(catalog["addresses"].is_array());
        assert!(host.publish(json!({"projects":[]})).is_err());
        std::fs::remove_dir_all(host.path.parent().unwrap()).unwrap();
    }
}
