//! Port of host/server.ts.
//!
//! One JSON-RPC endpoint, `POST /rpc`. Requests without a device credential
//! can only redeem a pairing code. Engine and workspace operations go to the
//! [`HostBackend`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use monocode_core::session::LinkedWorkItem;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::attachments::{read_attachment_chunk, write_attachment_chunk};
use super::backend::HostBackend;
use super::changes::MAX_WAIT_MS;
use super::exec::{ExecOptions, exec};
use super::http::{BodyError, HttpHandler, HttpServer, Request, Response};
use super::js;
use super::listener::is_loopback_ip;
use super::protocol::{
    HOST_PROTOCOL_VERSION, HostModelCatalog, HostProject, RemoteProvider, provider_name,
};
use super::store::{SessionPatch, now_ms};
use super::sync_transfer::{SyncLimits, SyncTransfers};

// Providers also add models server-side, without a CLI update.
pub const CATALOG_MAX_AGE_MS: i64 = 5 * 60_000;
// A 1 MiB text file can expand to 6 MiB when JSON escapes control characters.
// Existing files.write sends both the original and replacement contents.
const MAX_BODY: usize = 16 * 1024 * 1024;
// Requests without a device credential can only redeem a pairing code.
const MAX_PAIRING_BODY: usize = 4 * 1024;
// Pairing codes carry 256 bits, so this limit is not what protects them. It
// keeps an unauthenticated caller from spending the host's time and disk.
const MAX_PAIRING_FAILURES_PER_MINUTE: usize = 30;

/// What `environment.describe` advertises.
pub const CAPABILITIES: [&str; 27] = [
    "changes.wait",
    "sessions",
    "projects.browse",
    "models.list",
    "approvals",
    "questions",
    "diff",
    "git.branches",
    "git.switch",
    "git.createBranch",
    "git.worktrees",
    "git.worktreeCreate",
    "files.read",
    "files.list",
    "files.index",
    "workspace.run",
    "files.search",
    "files.searchContent",
    "files.create",
    "files.write",
    "git.index",
    "git.fileDiff",
    "git.action",
    "attachments.upload",
    "attachments.read",
    "sessions.draft",
    "sessions.plan",
];

/// This host's version, reported as `hostVersion`.
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Answers `/lifecycle`, which only loopback clients reach.
pub type Lifecycle = Arc<dyn Fn(&mut Request<'_>) -> Response + Send + Sync>;

#[derive(Clone)]
pub struct HostServerOptions {
    /// Network addresses advertised to paired desktops.
    pub endpoints: Option<Arc<dyn Fn() -> Vec<String> + Send + Sync>>,
    pub lifecycle: Option<Lifecycle>,
    /// `hostVersion` in `environment.describe`.
    pub version: String,
    /// Milliseconds since the epoch. Tests move it forward.
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    pub sync_limits: SyncLimits,
}

impl Default for HostServerOptions {
    fn default() -> Self {
        Self {
            endpoints: None,
            lifecycle: None,
            version: HOST_VERSION.into(),
            clock: Arc::new(now_ms),
            sync_limits: SyncLimits::default(),
        }
    }
}

struct Catalog {
    binaries: String,
    probed: i64,
    catalog: Arc<OnceLock<HostModelCatalog>>,
}

/// The request handler. Serve it with [`create_host_server`] and
/// [`super::listener::listen_host`].
pub struct HostServer {
    backend: Arc<dyn HostBackend>,
    providers: Vec<RemoteProvider>,
    options: HostServerOptions,
    pairing_failures: Mutex<Vec<i64>>,
    catalogs: Mutex<HashMap<String, Catalog>>,
    transfers: SyncTransfers,
}

/// `createHostServer(engine, providers, lifecycle, options)`.
pub fn create_host_server(
    backend: Arc<dyn HostBackend>,
    providers: Vec<RemoteProvider>,
    options: HostServerOptions,
) -> Arc<HttpServer> {
    HttpServer::new(Arc::new(HostServer::new(backend, providers, options)))
}

/// `os.hostname()`.
pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// `os.homedir()`.
#[allow(deprecated)]
pub fn home_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// `process.platform`.
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn json_response(status: u16, body: Vec<u8>) -> Response {
    Response::new(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
}

fn error_response(status: u16, message: &str) -> Response {
    json_response(status, json!({ "error": message }).to_string().into_bytes())
}

fn to_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

/// The request body as a JSON object.
fn body(request: &mut Request<'_>, limit: usize) -> Result<Map<String, Value>, String> {
    let bytes = request.read_body(limit).map_err(|error| match error {
        BodyError::TooLarge => "Request is too large".to_string(),
        BodyError::Io(_) => "aborted".to_string(),
        BodyError::Invalid => "Invalid chunked request body".to_string(),
    })?;
    match serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())? {
        Value::Object(fields) => Ok(fields),
        _ => Err("Invalid request".into()),
    }
}

/// `parseGithubWorkItemUrl`, from src/features/sessions/model/sessionWorkItem.ts.
pub fn parse_github_work_item_url(message: &str) -> Option<(String, String, i64, String)> {
    static GITHUB_URL: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = GITHUB_URL.get_or_init(|| {
        regex::Regex::new(
            r"(?i)https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/(pull|issues)/(\d+)(?-u:\b)",
        )
        .expect("valid pattern")
    });
    let found = pattern.captures(message)?;
    let number = js::number_from_str(&found[4]);
    if !js::is_safe_integer_f64(number) || number <= 0.0 {
        return None;
    }
    let number = number as i64;
    let repo = format!("{}/{}", &found[1], &found[2]);
    let kind = if found[3].eq_ignore_ascii_case("pull") {
        "pr"
    } else {
        "issue"
    };
    let url = format!(
        "https://github.com/{repo}/{}/{number}",
        if kind == "pr" { "pull" } else { "issues" }
    );
    Some((kind.into(), repo, number, url))
}

impl HostServer {
    pub fn new(
        backend: Arc<dyn HostBackend>,
        providers: Vec<RemoteProvider>,
        options: HostServerOptions,
    ) -> Self {
        let transfers = SyncTransfers::new(options.sync_limits);
        Self {
            backend,
            providers,
            options,
            pairing_failures: Mutex::new(Vec::new()),
            catalogs: Mutex::new(HashMap::new()),
            transfers,
        }
    }

    fn now(&self) -> i64 {
        (self.options.clock)()
    }

    fn pair(&self, request: &mut Request<'_>) -> Result<Response, String> {
        let now = self.now();
        {
            let mut failures = self
                .pairing_failures
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            failures.retain(|time| now - time < 60_000);
            if failures.len() >= MAX_PAIRING_FAILURES_PER_MINUTE {
                return Ok(error_response(
                    429,
                    "Too many pairing attempts. Wait a minute and try again.",
                ));
            }
        }
        let input = body(request, MAX_PAIRING_BODY)?;
        let empty = Map::new();
        let params = input
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .map(monocode_core::js::trim)
            .filter(|name| !name.is_empty())
            .map(|name| monocode_core::js::slice_prefix(name, 100).to_string())
            .unwrap_or_else(|| "Desktop".into());
        let code = params.get("code").and_then(Value::as_str).filter(|code| {
            code.len() == 43
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        });
        let store = self.backend.store();
        let device = match code {
            Some(code)
                if js::is_number(input.get("version"), HOST_PROTOCOL_VERSION as f64)
                    && input.get("method").and_then(Value::as_str) == Some("pair.exchange") =>
            {
                store.redeem_pairing(code, &name, now)?
            }
            _ => None,
        };
        let Some(device) = device else {
            self.pairing_failures
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(now);
            return Ok(error_response(
                401,
                "This pairing link is invalid, expired, or already used. Run connect on the machine again for a new link.",
            ));
        };
        Ok(json_response(
            200,
            to_json(&json!({
                "result": {
                    "deviceId": device.id,
                    "token": device.token,
                    "environmentId": store.environment_id,
                    "name": hostname(),
                }
            }))?,
        ))
    }

    /// Identifies each installed provider CLI. An update changes its real
    /// path or modification time, which invalidates the catalog the old
    /// version reported.
    fn provider_binaries(&self) -> String {
        self.providers
            .iter()
            .map(|provider| {
                let identify = || -> Option<String> {
                    let file =
                        std::fs::canonicalize(self.backend.resolve_binary(*provider).ok()?).ok()?;
                    let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
                    let millis = modified.duration_since(UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
                    Some(format!("{}:{millis}", file.display()))
                };
                identify().unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn models(&self, project_id: Option<&Value>) -> Result<HostModelCatalog, String> {
        let cwd = match project_id {
            Some(Value::String(id)) => self.backend.store().project(id)?.cwd,
            _ => home_dir().to_string_lossy().into_owned(),
        };
        let binaries = self.provider_binaries();
        let cell = {
            let mut catalogs = self.catalogs.lock().unwrap_or_else(PoisonError::into_inner);
            let now = self.now();
            match catalogs.get(&cwd) {
                Some(cached)
                    if cached.binaries == binaries && now - cached.probed < CATALOG_MAX_AGE_MS =>
                {
                    cached.catalog.clone()
                }
                _ => {
                    let cell = Arc::new(OnceLock::new());
                    catalogs.insert(
                        cwd.clone(),
                        Catalog {
                            binaries,
                            probed: now,
                            catalog: cell.clone(),
                        },
                    );
                    cell
                }
            }
        };
        let catalog = cell
            .get_or_init(|| {
                let found: Vec<(RemoteProvider, Result<_, String>)> = std::thread::scope(|scope| {
                    let probes: Vec<_> = self
                        .providers
                        .iter()
                        .map(|provider| {
                            let cwd = cwd.as_str();
                            (
                                *provider,
                                scope.spawn(move || self.backend.discover_models(*provider, cwd)),
                            )
                        })
                        .collect();
                    probes
                        .into_iter()
                        .map(|(provider, probe)| {
                            (
                                provider,
                                probe
                                    .join()
                                    .unwrap_or_else(|_| Err("Model discovery failed".into())),
                            )
                        })
                        .collect()
                });
                let mut catalog = HostModelCatalog::default();
                for (provider, result) in found {
                    match result {
                        Ok(models) => {
                            catalog.models.insert(provider, models);
                        }
                        Err(error) => {
                            catalog.errors.insert(provider, error);
                        }
                    }
                }
                catalog
            })
            .clone();
        if !catalog.errors.is_empty() {
            let mut catalogs = self.catalogs.lock().unwrap_or_else(PoisonError::into_inner);
            if catalogs
                .get(&cwd)
                .is_some_and(|cached| Arc::ptr_eq(&cached.catalog, &cell))
            {
                catalogs.remove(&cwd);
            }
        }
        Ok(catalog)
    }

    fn project(&self, params: &Map<String, Value>) -> Result<HostProject, String> {
        self.backend
            .store()
            .project(&js::string(params.get("projectId")))
    }

    /// `resolveHostWorktreeAsync(project.cwd, params.cwd)`.
    fn worktree(
        &self,
        project: &HostProject,
        params: &Map<String, Value>,
    ) -> Result<String, String> {
        self.backend
            .resolve_worktree(&project.cwd, params.get("cwd"))
    }

    fn describe(&self, params: &Map<String, Value>) -> Value {
        let supported = params.get("supportedProviders").and_then(Value::as_array);
        let providers: Vec<&str> = self
            .providers
            .iter()
            .map(|provider| provider_name(*provider))
            .filter(|name| match supported {
                Some(list) => list.iter().any(|entry| entry.as_str() == Some(name)),
                // Older clients validate this list against Codex and Claude only.
                None => *name == "codex" || *name == "claude",
            })
            .collect();
        json!({
            "protocolVersion": HOST_PROTOCOL_VERSION,
            "environmentId": self.backend.store().environment_id,
            "name": hostname(),
            "platform": node_platform(),
            "providers": providers,
            "hostVersion": self.options.version,
            "endpoints": self.options.endpoints.as_ref().map(|endpoints| endpoints()).unwrap_or_default(),
            "capabilities": CAPABILITIES,
        })
    }

    fn sessions_list(&self, params: &Map<String, Value>) -> Result<Vec<u8>, String> {
        let project = self.project(params)?;
        let summaries = self.backend.store().summaries(&project.id)?;
        let mut paths: Vec<String> = Vec::new();
        for summary in &summaries {
            let cwd = summary.cwd.clone().unwrap_or_else(|| project.cwd.clone());
            if !paths.contains(&cwd) {
                paths.push(cwd);
            }
        }
        let branches: HashMap<String, String> = std::thread::scope(|scope| {
            let lookups: Vec<_> = paths
                .iter()
                .map(|cwd| {
                    scope.spawn(move || {
                        let branch = exec(
                            "git",
                            &["symbolic-ref", "--quiet", "--short", "HEAD"],
                            ExecOptions {
                                cwd: Some(cwd.into()),
                                timeout: Duration::from_secs(2),
                                ..Default::default()
                            },
                        )
                        .map(|output| output.stdout.trim().to_string())
                        .unwrap_or_default();
                        (cwd.clone(), branch)
                    })
                })
                .collect();
            lookups
                .into_iter()
                .filter_map(|lookup| lookup.join().ok())
                .collect()
        });
        let listed: Vec<_> = summaries
            .into_iter()
            .map(|mut session| {
                let cwd = session.cwd.clone().unwrap_or_else(|| project.cwd.clone());
                session.repo = Some(project.name.clone());
                session.branch = branches
                    .get(&cwd)
                    .filter(|branch| !branch.is_empty())
                    .cloned();
                session.worktree_cwd = session
                    .cwd
                    .clone()
                    .filter(|cwd| !cwd.is_empty() && *cwd != project.cwd);
                session
            })
            .collect();
        to_json(&listed)
    }

    fn sessions_update(&self, params: &Map<String, Value>) -> Result<Vec<u8>, String> {
        let session_id = js::string(params.get("sessionId"));
        let current = self.backend.store().session(&session_id)?;
        if params.get("projectId").and_then(Value::as_str) != Some(current.project_id.as_str()) {
            return Err("Session does not belong to this project".into());
        }
        let mut patch = SessionPatch::default();
        if let Some(title) = params.get("title") {
            patch.title = Some(title.as_str().ok_or("Invalid session title")?.to_string());
        }
        if let Some(archived) = params.get("archived") {
            patch.archived = Some(archived.as_bool().ok_or("Invalid archive value")?);
        }
        if let Some(pinned) = params.get("pinned") {
            patch.pinned = Some(pinned.as_bool().ok_or("Invalid pin value")?);
        }
        if let Some(item) = params.get("linkedWorkItem") {
            if item.is_null() {
                patch.linked_work_item = Some(None);
            } else {
                let fields = item.as_object().ok_or("Invalid linked work item")?;
                let parsed = parse_github_work_item_url(&js::string(fields.get("url")));
                let matches = parsed.is_some_and(|(kind, repo, number, url)| {
                    fields.get("kind").and_then(Value::as_str) == Some(kind.as_str())
                        && fields.get("repo").and_then(Value::as_str) == Some(repo.as_str())
                        && js::is_number(fields.get("number"), number as f64)
                        && fields.get("url").and_then(Value::as_str) == Some(url.as_str())
                });
                if !matches {
                    return Err("Invalid linked work item".into());
                }
                let item: LinkedWorkItem = serde_json::from_value(item.clone())
                    .map_err(|_| "Invalid linked work item".to_string())?;
                patch.linked_work_item = Some(Some(item));
            }
        }
        if patch.is_empty() {
            return Err("No session changes supplied".into());
        }
        to_json(&self.backend.update_session(&session_id, patch)?)
    }

    fn git_diff(&self, params: &Map<String, Value>) -> Result<Vec<u8>, String> {
        let project = self.project(params)?;
        let cwd = self.worktree(&project, params)?;
        let diff = exec(
            "git",
            &[
                "-c",
                "core.pager=cat",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "HEAD",
                "--",
            ],
            ExecOptions {
                cwd: Some(cwd.into()),
                timeout: Duration::from_secs(10),
                max_buffer: 2 * 1024 * 1024,
                ..Default::default()
            },
        )?;
        to_json(&diff.stdout)
    }

    /// Runs one authenticated method and returns its serialized result.
    fn dispatch(
        &self,
        method: Option<&str>,
        params: &Map<String, Value>,
        token: &str,
        request: &Request<'_>,
    ) -> Result<Vec<u8>, String> {
        let store = self.backend.store();
        let backend = &self.backend;
        let get = |key: &str| params.get(key);
        let optional = |value: Option<Value>| value.unwrap_or(Value::Null);
        match method.unwrap_or("") {
            "environment.describe" => to_json(&self.describe(params)),
            "projects.list" => to_json(&store.projects()?),
            "projects.browse" => to_json(&backend.browse_directories(get("path"))?),
            "projects.open" => to_json(&backend.open_project(&js::string(get("cwd")))?),
            "models.list" => to_json(&self.models(get("projectId"))?),
            "sessions.list" => self.sessions_list(params),
            "sessions.update" => self.sessions_update(params),
            "sessions.delete" => {
                let session_id = js::string(get("sessionId"));
                let current = store.session(&session_id)?;
                if get("projectId").and_then(Value::as_str) != Some(current.project_id.as_str()) {
                    return Err("Session does not belong to this project".into());
                }
                store.delete_session(&session_id)?;
                to_json(&json!({ "deleted": true }))
            }
            "sessions.sync" => {
                let session_id = js::string(get("sessionId"));
                let sync = store.sync(&session_id, js::safe_integer(get("revision")))?;
                to_json(&self.transfers.respond(&session_id, &sync)?)
            }
            "sessions.syncChunk" => to_json(&self.transfers.chunk(
                &js::string(get("sessionId")),
                &js::string(get("transfer")),
                js::number(get("offset")),
            )?),
            "sessions.get" => {
                let value = store.session(&js::string(get("sessionId")))?;
                if js::is_number(get("revision"), value.revision as f64) {
                    to_json(&Value::Null)
                } else {
                    to_json(&*value)
                }
            }
            "changes.wait" => {
                // A desktop that leaves stops waiting at once.
                let timeout = js::safe_integer(get("timeoutMs")).unwrap_or(MAX_WAIT_MS);
                to_json(
                    &store.changes.wait(get("boot"), get("after"), timeout, &|| {
                        request.client_left()
                    }),
                )
            }
            "events.read" => {
                let after = js::safe_integer(get("after"))
                    .filter(|after| *after >= 0)
                    .ok_or("Invalid event cursor")?;
                to_json(&store.events(&js::string(get("sessionId")), after)?)
            }
            "commands.dispatch" => to_json(&backend.command(params)?),
            "attachments.upload" => to_json(&write_attachment_chunk(store, params)?),
            "attachments.read" => to_json(&read_attachment_chunk(store, params)?),
            // Only the caller's own credential. Sessions and other devices
            // are unaffected; the host keeps running.
            "devices.revokeSelf" => to_json(&json!({ "revoked": store.revoke_token(token)? })),
            "git.diff" => self.git_diff(params),
            "git.branches" => {
                let project = self.project(params)?;
                to_json(&backend.branches(&self.worktree(&project, params)?)?)
            }
            "git.switch" => {
                let project = self.project(params)?;
                let cwd = self.worktree(&project, params)?;
                to_json(&backend.with_idle_project(
                    &project.id,
                    get("force") == Some(&Value::Bool(true)),
                    &mut || backend.switch_branch(&cwd, get("branch"), get("remote")),
                )?)
            }
            "git.createBranch" => {
                let project = self.project(params)?;
                let cwd = self.worktree(&project, params)?;
                to_json(&backend.with_idle_project(
                    &project.id,
                    get("force") == Some(&Value::Bool(true)),
                    &mut || backend.create_branch(&cwd, get("branch")),
                )?)
            }
            "git.worktrees" => {
                let project = self.project(params)?;
                to_json(&backend.worktrees(&project.cwd)?)
            }
            "git.worktreeCreate" => {
                let project = self.project(params)?;
                let cwd = self.worktree(&project, params)?;
                let created = backend.with_idle_project(&project.id, false, &mut || {
                    backend.create_worktree(
                        &project.cwd,
                        get("branch"),
                        get("base"),
                        get("existing"),
                        &cwd,
                    )
                })?;
                backend.invalidate_workspace_roots();
                to_json(&created)
            }
            "files.read" => {
                let project = self.project(params)?;
                to_json(&backend.read_file(&self.worktree(&project, params)?, get("path"))?)
            }
            "files.list" => {
                let project = self.project(params)?;
                to_json(&backend.list_files(&self.worktree(&project, params)?, get("path"))?)
            }
            "files.index" => {
                let project = self.project(params)?;
                to_json(&backend.index_files(&self.worktree(&project, params)?)?)
            }
            "workspace.run" => to_json(&optional(
                backend.workspace_run(get("command"), get("args"))?,
            )),
            "files.search" => {
                let project = self.project(params)?;
                to_json(&backend.search_files(&self.worktree(&project, params)?, get("query"))?)
            }
            "files.searchContent" => {
                let project = self.project(params)?;
                to_json(&backend.search_content(&self.worktree(&project, params)?, params)?)
            }
            "files.create" => {
                let project = self.project(params)?;
                to_json(&backend.create_path(
                    &self.worktree(&project, params)?,
                    get("parent"),
                    get("name"),
                    get("isDir"),
                )?)
            }
            "files.write" => {
                let project = self.project(params)?;
                to_json(&optional(backend.write_file(
                    &self.worktree(&project, params)?,
                    get("path"),
                    get("expected"),
                    get("content"),
                )?))
            }
            "git.index" => {
                let project = self.project(params)?;
                to_json(&backend.git_index(&self.worktree(&project, params)?)?)
            }
            "git.fileDiff" => {
                let project = self.project(params)?;
                to_json(&backend.file_diff(
                    &self.worktree(&project, params)?,
                    get("path"),
                    get("staged") == Some(&Value::Bool(true)),
                )?)
            }
            "git.action" => {
                let project = self.project(params)?;
                let cwd = self.worktree(&project, params)?;
                to_json(&backend.with_idle_project(&project.id, false, &mut || {
                    backend
                        .git_action(
                            &cwd,
                            get("action"),
                            get("path"),
                            get("message"),
                            get("content"),
                        )
                        .map(optional)
                })?)
            }
            _ => Err("Unsupported host method".into()),
        }
    }

    fn rpc(&self, request: &mut Request<'_>) -> Result<Response, String> {
        // Desktop native HTTP supplies credentials. This endpoint intentionally
        // accepts no browser origin and provides no permissive CORS escape hatch.
        if request
            .header("origin")
            .is_some_and(|origin| !origin.is_empty())
            || request.method != "POST"
            || request.url != "/rpc"
        {
            return Ok(error_response(403, "Unsupported request origin or route"));
        }
        let authorization = request.header("authorization").unwrap_or("").to_string();
        if authorization.is_empty() {
            return self.pair(request);
        }
        let store = self.backend.store();
        let token = authorization
            .strip_prefix("Bearer ")
            .filter(|token| {
                token.len() == 43
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            })
            .map(str::to_string);
        let Some(token) = token.filter(|token| store.authenticated(token).unwrap_or(false)) else {
            return Ok(error_response(
                401,
                "Device credential is invalid or revoked",
            ));
        };
        let input = body(request, MAX_BODY)?;
        // Reading a request body yields: a device may have been revoked since
        // the headers arrived. Reject it before dispatching any operation.
        if !store.authenticated(&token)? {
            return Ok(error_response(
                401,
                "Device credential is invalid or revoked",
            ));
        }
        if !js::is_number(input.get("version"), HOST_PROTOCOL_VERSION as f64) {
            return Err("Incompatible protocol version".into());
        }
        let method = input.get("method").and_then(Value::as_str);
        if method != Some("environment.describe")
            && input.get("environmentId").and_then(Value::as_str)
                != Some(store.environment_id.as_str())
        {
            return Err("Host identity changed; reconnect this machine explicitly".into());
        }
        let empty = Map::new();
        let params = input
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let result = self.dispatch(method, params, &token, request)?;
        let mut body = Vec::with_capacity(result.len() + 11);
        body.extend_from_slice(b"{\"result\":");
        body.extend_from_slice(&result);
        body.push(b'}');
        Ok(json_response(200, body))
    }
}

impl HttpHandler for HostServer {
    fn handle(&self, request: &mut Request<'_>) -> Response {
        if request.url == "/lifecycle"
            && let Some(lifecycle) = &self.options.lifecycle
        {
            return if is_loopback_ip(request.peer.ip()) {
                lifecycle(request)
            } else {
                Response::new(403)
            };
        }
        match self.rpc(request) {
            Ok(response) => response,
            Err(message) => error_response(400, &message),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
