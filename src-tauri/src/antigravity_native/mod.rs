//! Windows desktop Antigravity: Google OAuth, Code Assist and a local tool loop.
//! No provider executable, external runtime or third-party account store is used.
mod auth;
mod protocol;
mod storage;
#[cfg(test)]
mod tests;
mod tools;
mod transport;

use auth::{now_ms, Auth};
use protocol::{Generation, History, HistoryItem, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use storage::Store;
use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use transport::{Cancel, NativeError, Result, Transport};

struct Login {
    owner: String,
    id: String,
    cancel: Cancel,
}
struct Active {
    turn_id: String,
    cancel: Cancel,
}
struct Session {
    owner: String,
    cwd: PathBuf,
    history: AsyncMutex<Option<History>>,
    turn_gate: AsyncMutex<()>,
    retired: AtomicBool,
    closed: AtomicBool,
    active: Mutex<Option<Active>>,
    approvals: Mutex<HashMap<u64, (String, oneshot::Sender<bool>)>>,
}

// A released owner keeps the encrypted history id and workspace for resume.
type Binding = (Option<String>, String, PathBuf);

struct TurnGuard<'a> {
    host: &'a NativeHost,
    thread: &'a str,
    session: &'a Arc<Session>,
    gate: Option<tokio::sync::MutexGuard<'a, ()>>,
}

impl<'a> TurnGuard<'a> {
    async fn lock(host: &'a NativeHost, thread: &'a str, session: &'a Arc<Session>) -> Self {
        Self {
            host,
            thread,
            session,
            gate: Some(session.turn_gate.lock().await),
        }
    }
}

impl Drop for TurnGuard<'_> {
    fn drop(&mut self) {
        // Nested active/history guards have already dropped on every return.
        // Release the turn gate before handing a closed session to a new owner.
        drop(self.gate.take());
        self.host.release_closed(self.thread, self.session);
    }
}

#[derive(Default)]
pub struct NativeHost {
    auth: Auth,
    transport: Transport,
    login: Mutex<Option<Login>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    bindings: Mutex<HashMap<String, Binding>>,
    request_ids: AtomicU64,
    cancelled_turns: Mutex<Vec<(String, String, String)>>,
    catalog_cancel: Mutex<Cancel>,
}

impl NativeHost {
    fn bind_owned(&self, owner: &str, thread: &str, id: &str, cwd: PathBuf) -> Result<()> {
        let id = uuid::Uuid::parse_str(id).map_err(|_| NativeError::new("session", "This conversation uses ACP history. Start a new session to use native Windows Antigravity."))?.to_string();
        let mut bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        // Registry locks always follow bindings -> sessions -> active. Never
        // await a turn while holding them; binding only tries the turn gate.
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(session) = sessions.get(thread).cloned() {
            let closed = session.closed.load(Ordering::SeqCst);
            if (!closed && session.owner != owner)
                || (session.retired.load(Ordering::SeqCst) && !closed)
            {
                return Err(NativeError::new(
                    "session",
                    "Antigravity session belongs to another window or is stopping.",
                ));
            }
            let _turn = session.turn_gate.try_lock().map_err(|_| {
                NativeError::new(
                    "session",
                    "Cannot bind an Antigravity session before its turn has finished.",
                )
            })?;
            session.retired.store(true, Ordering::SeqCst);
            sessions.remove(thread);
            if closed {
                if let Some((bound_owner, _, _)) = bindings.get_mut(thread) {
                    if bound_owner.as_deref() == Some(session.owner.as_str()) {
                        *bound_owner = None;
                    }
                }
            }
        }
        if bindings
            .get(thread)
            .is_some_and(|(old_owner, _, _)| old_owner.as_deref().is_some_and(|old| old != owner))
        {
            return Err(NativeError::new(
                "session",
                "Antigravity history belongs to another window.",
            ));
        }
        bindings.insert(thread.into(), (Some(owner.into()), id, cwd));
        Ok(())
    }
    async fn stop_owned(
        &self,
        owner: &str,
        thread: &str,
        forget: bool,
        store: &Store,
    ) -> Result<()> {
        let session = {
            let bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
            let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if bindings.get(thread).is_some_and(|(old_owner, _, _)| {
                old_owner.as_deref().is_some_and(|old| old != owner)
            }) || sessions
                .get(thread)
                .is_some_and(|session| session.owner != owner)
            {
                return Err(NativeError::new(
                    "session",
                    "Antigravity history belongs to another window.",
                ));
            }
            let session = sessions.get(thread).cloned();
            if let Some(session) = &session {
                // An explicit stop owns cleanup, even if the window closes too.
                session.closed.store(false, Ordering::SeqCst);
                session.retired.store(true, Ordering::SeqCst);
                session.cancel();
            }
            session
        };
        let _turn = match &session {
            Some(session) => Some(session.turn_gate.lock().await),
            None => None,
        };
        let mut bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        // A duplicate stop must not remove a replacement registered after the
        // first stop completed. The Arc identifies the retired generation.
        if sessions.get(thread).is_some_and(|current| {
            session
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, current))
        }) || (session.is_some()
            && !sessions.contains_key(thread)
            && bindings
                .get(thread)
                .is_some_and(|(owner, _, _)| owner.is_some()))
            || bindings.get(thread).is_some_and(|(old_owner, _, _)| {
                old_owner.as_deref().is_some_and(|old| old != owner)
            })
        {
            return Ok(());
        }
        if forget {
            if let Some((_, id, _)) = bindings.get(thread) {
                store.remove(&format!("sessions/{id}.dpapi"))?;
            }
            bindings.remove(thread);
        } else if let Some((bound_owner, _, _)) = bindings.get_mut(thread) {
            *bound_owner = None;
        }
        sessions.remove(thread);
        Ok(())
    }
    pub fn cancel_all(&self) {
        let mut catalog_cancel = self
            .catalog_cancel
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        catalog_cancel.cancel();
        *catalog_cancel = Cancel::default();
        if let Some(login) = self
            .login
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            login.cancel.cancel();
        }
        for session in self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            session.cancel();
        }
    }
    pub fn cancel_window(&self, owner: &str) {
        if let Some(login) = self
            .login
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            if login.owner == owner {
                login.cancel.cancel();
            }
        }
        let mut bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let owned: Vec<_> = sessions
            .iter()
            .filter(|(_, session)| session.owner == owner)
            .map(|(thread, session)| (thread.clone(), session.clone()))
            .collect();
        for (thread, session) in owned {
            if !session.retired.swap(true, Ordering::SeqCst) {
                session.closed.store(true, Ordering::SeqCst);
            }
            session.cancel();
            if session.closed.load(Ordering::SeqCst) && session.turn_gate.try_lock().is_ok() {
                sessions.remove(&thread);
            }
        }
        for (thread, (bound_owner, _, _)) in bindings.iter_mut() {
            if bound_owner.as_deref() == Some(owner) && !sessions.contains_key(thread) {
                *bound_owner = None;
            }
        }
    }
    fn release_closed(&self, thread: &str, session: &Arc<Session>) {
        if !session.closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(_turn) = session.turn_gate.try_lock() else {
            return;
        };
        let mut bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if session.closed.load(Ordering::SeqCst)
            && sessions
                .get(thread)
                .is_some_and(|current| Arc::ptr_eq(current, session))
        {
            sessions.remove(thread);
            if let Some((owner, _, _)) = bindings.get_mut(thread) {
                if owner.as_deref() == Some(session.owner.as_str()) {
                    *owner = None;
                }
            }
        }
    }
    fn session(&self, thread: &str, owner: &str, cwd: &Path) -> Result<Arc<Session>> {
        if thread.is_empty() || thread.len() > 256 {
            return Err(NativeError::new(
                "session",
                "Invalid Antigravity session id.",
            ));
        }
        let mut bindings = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(session) = sessions.get(thread) {
            if session.owner != owner || session.cwd != cwd {
                return Err(NativeError::new(
                    "session",
                    "Antigravity session belongs to a different window or workspace.",
                ));
            }
            return Ok(session.clone());
        }
        if let Some((bound_owner, _, bound_cwd)) = bindings.get_mut(thread) {
            if bound_owner.as_deref().is_some_and(|old| old != owner) || bound_cwd != cwd {
                return Err(NativeError::new(
                    "session",
                    "Antigravity history belongs to another window or workspace.",
                ));
            }
            *bound_owner = Some(owner.into());
        }
        let session = Arc::new(Session {
            owner: owner.into(),
            cwd: cwd.into(),
            history: AsyncMutex::new(None),
            turn_gate: AsyncMutex::new(()),
            retired: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            active: Mutex::new(None),
            approvals: Mutex::new(HashMap::new()),
        });
        sessions.insert(thread.into(), session.clone());
        Ok(session)
    }
    fn owned_session(&self, thread: &str, owner: &str) -> Result<Option<Arc<Session>>> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let session = sessions.get(thread).cloned();
        if session.as_ref().is_some_and(|s| s.owner != owner) {
            return Err(NativeError::new(
                "session",
                "Antigravity session belongs to another window.",
            ));
        }
        Ok(session)
    }
}

impl Session {
    fn begin_turn(&self, host: &NativeHost, input: &TurnInput, cancel: &Cancel) -> Result<bool> {
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if self.retired.load(Ordering::SeqCst) {
            return Err(NativeError::cancelled());
        }
        if host
            .cancelled_turns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|(owner, thread, turn)| {
                owner == &self.owner && thread == &input.session_id && turn == &input.turn_id
            })
        {
            return Ok(false);
        }
        if active.is_some() {
            return Err(NativeError::new(
                "session",
                "An Antigravity turn is already running.",
            ));
        }
        *active = Some(Active {
            turn_id: input.turn_id.clone(),
            cancel: cancel.clone(),
        });
        Ok(true)
    }
    async fn load_history<'a>(
        &'a self,
        host: &NativeHost,
        thread: &str,
        email: &str,
        store: &Store,
        cancel: &Cancel,
    ) -> Result<tokio::sync::MutexGuard<'a, Option<History>>> {
        let mut guard = cancel
            .run(Duration::from_secs(180), async {
                Ok(self.history.lock().await)
            })
            .await?;
        if cancel.is_cancelled() || self.retired.load(Ordering::SeqCst) {
            return Err(NativeError::cancelled());
        }
        if guard.is_none() {
            let binding = host
                .bindings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(thread)
                .cloned();
            let history = if let Some((owner, id, cwd)) = binding {
                if owner.as_deref() != Some(self.owner.as_str()) || cwd != self.cwd {
                    return Err(NativeError::new(
                        "session",
                        "Antigravity resume belongs to another window or workspace.",
                    ));
                }
                store
                    .read::<History>(&format!("sessions/{id}.dpapi"))?
                    .ok_or_else(|| {
                        NativeError::new(
                            "session",
                            "Antigravity native history is unavailable. Start a new session.",
                        )
                    })?
            } else {
                History::new(&self.cwd, email.into())
            };
            *guard = Some(history);
        }
        let history = guard.as_ref().expect("native history");
        if Path::new(&history.cwd) != self.cwd || history.email != email {
            return Err(NativeError::new("session", "Antigravity history belongs to another workspace or Google account. Start a new session."));
        }
        Ok(guard)
    }
    fn cancel(&self) {
        if let Some(active) = self
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            active.cancel.cancel();
        }
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

fn store(window: &WebviewWindow) -> Result<Store> {
    if !cfg!(windows) {
        return Err(NativeError::new(
            "platform",
            "Use the Antigravity ACP transport on this platform.",
        ));
    }
    Ok(Store(
        window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| NativeError::storage())?
            .join("antigravity-native"),
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    backend_available: bool,
    authenticated: bool,
    email: Option<String>,
    login_pending: bool,
    auth_error: Option<String>,
}

#[tauri::command]
pub async fn antigravity_native_status(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
) -> Result<AccountStatus> {
    let summary = if cfg!(windows) {
        match store(&window) {
            Ok(store) => host.auth.summary(&store).await,
            Err(error) => Err(error),
        }
    } else {
        Ok(None)
    };
    let pending = host
        .login
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some();
    Ok(account_status(summary, pending))
}

fn account_status(summary: Result<Option<String>>, pending: bool) -> AccountStatus {
    let (email, auth_error) = match summary {
        Ok(email) => (email, None),
        Err(error) => (None, Some(error.message)),
    };
    AccountStatus {
        backend_available: cfg!(windows),
        authenticated: email.is_some(),
        email,
        login_pending: pending,
        auth_error,
    }
}

#[tauri::command]
pub async fn antigravity_native_login(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
) -> Result<()> {
    let store = store(&window)?;
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = Cancel::default();
    {
        let mut flow = host.login.lock().unwrap_or_else(|p| p.into_inner());
        if flow.is_some() {
            return Err(NativeError::new(
                "oauth",
                "Google sign-in is already in progress.",
            ));
        }
        *flow = Some(Login {
            owner: window.label().into(),
            id: id.clone(),
            cancel: cancel.clone(),
        });
    }
    let result = async {
        let account = auth::login(&host.transport, &cancel, |url| {
            open::that_detached(url)
                .map_err(|_| NativeError::new("oauth", "Could not open the Google sign-in page."))
        })
        .await?;
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        host.auth.save(&store, account, &cancel).await
    }
    .await;
    let mut flow = host.login.lock().unwrap_or_else(|p| p.into_inner());
    if flow.as_ref().is_some_and(|flow| flow.id == id) {
        *flow = None;
    }
    let _ = window
        .app_handle()
        .emit("antigravity-native-account-changed", ());
    result
}

#[tauri::command]
pub fn antigravity_native_cancel_login(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
) -> Result<()> {
    if let Some(flow) = host
        .login
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
    {
        if flow.owner != window.label() {
            return Err(NativeError::new(
                "oauth",
                "Google sign-in belongs to another window.",
            ));
        }
        flow.cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn antigravity_native_logout(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
) -> Result<()> {
    host.cancel_all();
    host.auth.logout(&store(&window)?).await?;
    let _ = window
        .app_handle()
        .emit("antigravity-native-account-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn antigravity_native_catalog(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
) -> Result<Vec<Value>> {
    let cancel = host
        .catalog_cancel
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    host.auth
        .account(&store(&window)?, &host.transport, &cancel)
        .await?;
    Ok(protocol::catalog())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInput {
    session_id: String,
    turn_id: String,
    cwd: String,
    model: String,
    #[serde(default)]
    model_settings: HashMap<String, String>,
    runtime_mode: String,
    intent: Option<String>,
    text: String,
    #[serde(default)]
    attachments: Vec<Attachment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attachment {
    name: String,
    mime_type: String,
    data: Option<String>,
    path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeEvent {
    session_id: String,
    turn_id: String,
    event: Value,
}

fn event_sink(
    window: WebviewWindow,
    session_id: String,
    turn_id: String,
    cancel: Cancel,
) -> impl FnMut(Value) {
    move |event| {
        if !cancel.is_cancelled() {
            let _ = window.emit(
                "antigravity-native-event",
                NativeEvent {
                    session_id: session_id.clone(),
                    turn_id: turn_id.clone(),
                    event,
                },
            );
        }
    }
}

#[tauri::command]
pub async fn antigravity_native_send(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
    input: TurnInput,
) -> Result<()> {
    let store = store(&window)?;
    if !["supervised", "auto", "auto-accept-edits", "full-access"]
        .contains(&input.runtime_mode.as_str())
        || uuid::Uuid::parse_str(&input.turn_id).is_err()
    {
        return Err(NativeError::new(
            "session",
            "Invalid Antigravity turn policy or id.",
        ));
    }
    let cwd = PathBuf::from(&input.cwd)
        .canonicalize()
        .map_err(|_| NativeError::new("session", "Antigravity workspace does not exist."))?;
    if !cwd.is_dir() {
        return Err(NativeError::new(
            "session",
            "Antigravity workspace must be a directory.",
        ));
    }
    let model = protocol::resolve_model(
        &input.model,
        input.model_settings.get("thinking").map(String::as_str),
    )?;
    let session = host.session(&input.session_id, window.label(), &cwd)?;
    let _turn = TurnGuard::lock(&host, &input.session_id, &session).await;
    let cancel = Cancel::default();
    if !session.begin_turn(&host, &input, &cancel)? {
        return Ok(());
    }
    let mut emit = event_sink(
        window.clone(),
        input.session_id.clone(),
        input.turn_id.clone(),
        cancel.clone(),
    );
    let result = async {
        let account = host.auth.account(&store, &host.transport, &cancel).await?;
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        let mut history_guard = session
            .load_history(&host, &input.session_id, &account.email, &store, &cancel)
            .await?;
        let history = history_guard.as_mut().expect("native history");
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        recover_tools(history);
        host.bindings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                input.session_id.clone(),
                (Some(window.label().into()), history.id.clone(), cwd.clone()),
            );
        let parts = user_parts(&cwd, &input, &cancel)?;
        history.contents.push(HistoryItem {
            role: "user".into(),
            parts,
            model: None,
        });
        store.write(&history.filename(), history)?;
        emit(json!({"type":"session.started"}));
        emit(json!({"type":"session.providerBound","providerSessionId":history.id}));
        emit(json!({"type":"turn.started","providerTurnId":input.turn_id}));
        emit(json!({"type":"session.configChanged","model":model.id}));
        emit(json!({"type":"turn.accepted"}));
        agent_loop(
            &host, &session, history, &input, &model, &account, &store, &cancel, &mut emit,
        )
        .await
    }
    .await;
    if let Err(error) = &result {
        if error.code == "quota" {
            let mut event = json!({"type":"usage.limited"});
            if let Some(seconds) = error.retry_after {
                event["resetsAt"] = json!(now_ms().saturating_add(seconds.saturating_mul(1000)));
            }
            emit(event);
        }
    }
    session
        .approvals
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
    *session.active.lock().unwrap_or_else(|p| p.into_inner()) = None;
    if cancel.is_cancelled() {
        return Ok(());
    }
    result
}

fn user_parts(cwd: &Path, input: &TurnInput, cancel: &Cancel) -> Result<Vec<Value>> {
    const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
    if input.text.len() > 2 * 1024 * 1024 || input.attachments.len() > 20 {
        return Err(NativeError::new(
            "input",
            "Antigravity prompt is too large.",
        ));
    }
    let mut parts = vec![json!({"text":input.text})];
    let mut total = input.text.len();
    for attachment in &input.attachments {
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        let data = if let Some(data) = &attachment.data {
            data.clone()
        } else if let Some(path) = &attachment.path {
            // Attachments are explicitly supplied by the user and may live in
            // Downloads or MonoCode's temporary attachment directory. Model
            // file tools keep their separate workspace-only path checks.
            let path = cwd.join(path);
            let file = std::fs::File::open(&path)
                .map_err(|_| NativeError::new("input", "Attachment could not be read."))?;
            let metadata = file
                .metadata()
                .map_err(|_| NativeError::new("input", "Attachment could not be read."))?;
            if !metadata.is_file() {
                return Err(NativeError::new(
                    "input",
                    "Attachment must be a regular file.",
                ));
            }
            if metadata.len() > MAX_ATTACHMENT_BYTES {
                return Err(NativeError::new("input", "Attachment exceeds 10 MiB."));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(MAX_ATTACHMENT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| NativeError::new("input", "Attachment could not be read."))?;
            if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
                return Err(NativeError::new("input", "Attachment exceeds 10 MiB."));
            }
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        } else {
            return Err(NativeError::new("input", "Attachment data is unavailable."));
        };
        total += data.len();
        if total > 20 * 1024 * 1024 {
            return Err(NativeError::new(
                "input",
                "Antigravity attachments are too large.",
            ));
        }
        if attachment.mime_type.starts_with("image/") || attachment.mime_type == "application/pdf" {
            parts.push(json!({"inlineData":{"mimeType":attachment.mime_type,"data":data}}));
        } else {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .map_err(|_| NativeError::new("input", "Invalid attachment data."))?;
            let text = String::from_utf8(bytes).map_err(|_| {
                NativeError::new(
                    "input",
                    "This model supports text, image and PDF attachments.",
                )
            })?;
            parts.push(json!({"text":format!("Attached file: {}\n{text}", attachment.name)}));
        }
    }
    Ok(parts)
}

fn recover_tools(history: &mut History) {
    let answered: std::collections::HashSet<String> = history
        .contents
        .iter()
        .flat_map(|item| &item.parts)
        .filter_map(|part| part["functionResponse"]["id"].as_str().map(str::to_string))
        .collect();
    let mut missing = Vec::new();
    for item in &history.contents {
        for part in &item.parts {
            if let (Some(id), Some(name)) = (
                part["functionCall"]["id"].as_str(),
                part["functionCall"]["name"].as_str(),
            ) {
                if !answered.contains(id) {
                    let response = history.tool_results.get(id).cloned().unwrap_or_else(|| json!({"error":"Tool was interrupted before execution. Do not assume it completed."}));
                    missing.push(HistoryItem {
                        role: "model".into(),
                        parts: vec![
                            json!({"functionResponse":{"id":id,"name":name,"response":response}}),
                        ],
                        model: item.model.clone(),
                    });
                }
            }
        }
    }
    history.contents.extend(missing);
}

#[allow(clippy::too_many_arguments)]
async fn agent_loop(
    host: &NativeHost,
    session: &Session,
    history: &mut History,
    input: &TurnInput,
    model: &protocol::Model,
    initial_account: &auth::Account,
    store: &Store,
    cancel: &Cancel,
    emit: &mut impl FnMut(Value),
) -> Result<()> {
    let plan = input.intent.as_deref() == Some("plan");
    for _ in 0..32 {
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        let account = host.auth.account(store, &host.transport, cancel).await?;
        if account.email != initial_account.email {
            return Err(NativeError::new(
                "session",
                "Antigravity account changed during this turn.",
            ));
        }
        let request = history.request(
            model,
            account.project.as_deref().ok_or_else(NativeError::auth)?,
            tools::definitions(plan),
            plan,
        );
        let mut generation = Generation::default();
        host.transport
            .stream(&account.access_token, &request, cancel, |chunk| {
                generation.chunk(chunk, emit)
            })
            .await?;
        generation.complete()?;
        if cancel.is_cancelled() {
            return Err(NativeError::cancelled());
        }
        if generation.parts.iter().any(|p| p["thought"] == true) {
            emit(json!({"type":"reasoning.completed"}));
        }
        if generation
            .parts
            .iter()
            .any(|p| p["text"].is_string() && p["thought"] != true)
        {
            emit(json!({"type":"message.completed"}));
        }
        if let Some(usage) = generation.usage {
            let mut context = json!({"type":"context","window":model.context});
            let mut metrics = json!({"type":"turn.metrics"});
            if let Some(input) = usage["promptTokenCount"].as_u64() {
                context["used"] = json!(input);
                metrics["inputTokens"] = json!(input);
            }
            if let Some(output) = usage["candidatesTokenCount"].as_u64() {
                metrics["outputTokens"] =
                    json!(output.saturating_add(usage["thoughtsTokenCount"].as_u64().unwrap_or(0)));
            }
            emit(context);
            emit(metrics);
        }
        history.contents.push(HistoryItem {
            role: "model".into(),
            parts: generation.parts,
            model: Some(model.wire.clone()),
        });
        store.write(&history.filename(), history)?;
        if generation.calls.is_empty() {
            history.last_execution = Some(uuid::Uuid::new_v4().to_string());
            store.write(&history.filename(), history)?;
            emit(json!({"type":"turn.ready"}));
            return Ok(());
        }
        for call in generation.calls {
            let result = if let Some(result) = history.tool_results.get(&call.id) {
                result.clone()
            } else {
                let allowed = authorize_tool(host, session, input, &call, cancel, emit).await?;
                if cancel.is_cancelled() {
                    return Err(NativeError::cancelled());
                }
                if !allowed {
                    json!({"error":"Tool denied by the user or session policy."})
                } else {
                    history.tool_results.insert(call.id.clone(), json!({"error":"Tool outcome is unknown after interruption. Do not repeat it automatically."}));
                    store.write(&history.filename(), history)?;
                    let result =
                        tools::execute(session.cwd.clone(), call.clone(), cancel.clone()).await;
                    match result {
                        Ok(value) => value,
                        Err(error) => json!({"error":error.message}),
                    }
                }
            };
            if cancel.is_cancelled() {
                return Err(NativeError::cancelled());
            }
            history.tool_results.insert(call.id.clone(), result.clone());
            history.contents.push(HistoryItem {
                role: "model".into(),
                parts: vec![
                    json!({"functionResponse":{"id":call.id,"name":call.name,"response":result}}),
                ],
                model: Some(model.wire.clone()),
            });
            store.write(&history.filename(), history)?;
            emit(
                json!({"type":"tool.updated","callId":call.id,"status":if result.get("error").is_some() {"failed"} else {"completed"},"detail":result.get("output").or_else(|| result.get("error")).and_then(Value::as_str)}),
            );
            if cancel.is_cancelled() {
                return Err(NativeError::cancelled());
            }
        }
    }
    Err(NativeError::new(
        "limit",
        "Antigravity reached the limit of 32 agent cycles. Send a follow-up to continue.",
    ))
}

fn tool_preview(call: &ToolCall, title: &str) -> Value {
    let bounded = |text: &str| text.chars().take(32 * 1024).collect::<String>();
    let mut preview = json!({"title":bounded(title)});
    match call.name.as_str() {
        "powershell" => {
            preview["kind"] = json!("shell");
            preview["output"] = json!(bounded(call.args["command"].as_str().unwrap_or_default()));
        }
        "edit_file" | "write_file" => {
            preview["kind"] = json!("write");
            preview["path"] = call.args["path"].clone();
            let old = call.args["oldText"].as_str().unwrap_or_default();
            let new = call
                .args
                .get("newText")
                .or_else(|| call.args.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut lines = Vec::new();
            for (kind, text) in [("del", old), ("add", new)] {
                for (index, text) in bounded(text).lines().take(100).enumerate() {
                    lines.push(json!({"kind":kind,"number":index + 1,"text":text.chars().take(1024).collect::<String>()}));
                }
            }
            preview["contentOnly"] = json!(call.name == "write_file");
            preview["deletions"] = json!(old.lines().count());
            preview["additions"] = json!(new.lines().count());
            preview["lines"] = json!(lines);
        }
        "search_files" => {
            preview["kind"] = json!("search");
            preview["query"] = call.args["query"].clone();
        }
        _ => {
            preview["kind"] = json!("read");
        }
    }
    if call.name != "powershell" && call.args["path"].is_string() {
        preview["path"] = call.args["path"].clone();
    }
    preview
}

async fn authorize_tool(
    host: &NativeHost,
    session: &Session,
    input: &TurnInput,
    call: &ToolCall,
    cancel: &Cancel,
    emit: &mut impl FnMut(Value),
) -> Result<bool> {
    let kind = match call.name.as_str() {
        "edit_file" | "write_file" => "edit",
        "powershell" => "execute",
        "search_files" => "search",
        _ => "read",
    };
    let title = format!(
        "{} {}",
        call.name,
        call.args
            .get("path")
            .or_else(|| call.args.get("command"))
            .and_then(Value::as_str)
            .unwrap_or_default()
    );
    let preview = tool_preview(call, &title);
    emit(
        json!({"type":"tool.started","callId":call.id,"title":title,"kind":kind,"status":"running","preview":preview}),
    );
    match tools::permission(
        &call.name,
        &input.runtime_mode,
        input.intent.as_deref() == Some("plan"),
    ) {
        tools::Permission::Allow => Ok(true),
        tools::Permission::Deny => Ok(false),
        tools::Permission::Ask => {
            let request_id = host.request_ids.fetch_add(1, Ordering::Relaxed) + 1;
            let (sender, receiver) = oneshot::channel();
            session
                .approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(request_id, (input.turn_id.clone(), sender));
            emit(
                json!({"type":"approval.requested","requestId":request_id,"title":title,"kind":kind,"callId":call.id,"preview":preview}),
            );
            let response = cancel
                .run(Duration::from_secs(10 * 60), async {
                    receiver.await.map_err(|_| NativeError::cancelled())
                })
                .await;
            session
                .approvals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&request_id);
            emit(
                json!({"type":"approval.resolved","requestId":request_id,"decision":match &response { Ok(true) => "allow", Ok(false) => "deny", Err(_) => "cancelled" }}),
            );
            response
        }
    }
}

#[tauri::command]
pub fn antigravity_native_approve(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
    session_id: String,
    turn_id: String,
    request_id: u64,
    decision: String,
) -> Result<()> {
    if !["allow", "deny"].contains(&decision.as_str()) {
        return Err(NativeError::new(
            "permission",
            "Invalid tool permission response.",
        ));
    }
    if let Some(session) = host.owned_session(&session_id, window.label())? {
        let mut approvals = session.approvals.lock().unwrap_or_else(|p| p.into_inner());
        if approvals
            .get(&request_id)
            .is_some_and(|(turn, _)| turn == &turn_id)
        {
            if let Some((_, sender)) = approvals.remove(&request_id) {
                let _ = sender.send(decision == "allow");
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn antigravity_native_cancel(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
    session_id: String,
    turn_id: Option<String>,
) -> Result<()> {
    let session = host.owned_session(&session_id, window.label())?;
    if let Some(turn) = &turn_id {
        let mut cancelled = host
            .cancelled_turns
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if cancelled.len() >= 128 {
            cancelled.remove(0);
        }
        cancelled.push((window.label().into(), session_id.clone(), turn.clone()));
    }
    if let Some(session) = session {
        let active = session.active.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(active) = active.as_ref() {
            if turn_id.as_deref().is_none_or(|id| id == active.turn_id) {
                active.cancel.cancel();
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn antigravity_native_bind(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
    session_id: String,
    provider_session_id: String,
    cwd: String,
) -> Result<()> {
    let cwd = PathBuf::from(cwd)
        .canonicalize()
        .map_err(|_| NativeError::new("session", "Resume workspace is unavailable."))?;
    host.bind_owned(window.label(), &session_id, &provider_session_id, cwd)
}

#[tauri::command]
pub async fn antigravity_native_stop(
    window: WebviewWindow,
    host: State<'_, NativeHost>,
    session_id: String,
    forget: bool,
) -> Result<()> {
    host.stop_owned(window.label(), &session_id, forget, &store(&window)?)
        .await
}
