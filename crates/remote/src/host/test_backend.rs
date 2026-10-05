//! A stand-in [`HostBackend`] for tests. It creates sessions and records
//! turns instead of running providers, lets a test deliver output and finish
//! turns, and answers workspace methods by echoing what it received, so
//! server tests run end to end without the host engine.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use monocode_core::{AgentModel, Attachment, Block, BlockRole, RuntimeMode, Session};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::attachments::{parse_remote_attachments, resolve_attachments};
use super::backend::HostBackend;
use super::js;
use super::protocol::{
    CommandReceipt, HostDirectory, HostDirectoryEntry, HostProject, HostSession, HostSessionStatus,
    HostSessionSummary, RemoteProvider, parse_provider, provider_name,
};
use super::store::{HostStore, SessionPatch, now_ms};

/// A turn the test backend's provider was asked to run.
#[derive(Debug, Clone, PartialEq)]
pub struct TestTurn {
    pub session_id: String,
    pub run_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

pub struct TestBackend {
    store: Arc<HostStore>,
    providers: Vec<RemoteProvider>,
    turns: Mutex<Vec<TestTurn>>,
    turn_ready: Condvar,
    /// Provider CLIs `resolve_binary` reports.
    pub binaries: Mutex<HashMap<RemoteProvider, PathBuf>>,
    /// Answers for the next model probes, in order. Empty answers no models.
    pub models: Mutex<VecDeque<Result<Vec<AgentModel>, String>>>,
    pub probes: AtomicUsize,
    /// Workspace calls, as `(method, arguments)`.
    pub calls: Mutex<Vec<(String, Value)>>,
    switching: Mutex<HashSet<String>>,
    pub closed: AtomicBool,
}

fn running_sessions_message(sessions: &[HostSessionSummary]) -> String {
    let names = sessions
        .iter()
        .take(3)
        .map(|session| format!("\"{}\"", session.title))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if sessions.len() > 3 {
        format!(" and {} more", sessions.len() - 3)
    } else {
        String::new()
    };
    if sessions.len() == 1 {
        format!(
            "{names} is running on the host. Switching branches changes the files it is working on."
        )
    } else {
        format!(
            "{} sessions are running on the host: {names}{more}. Switching branches changes the files they are working on.",
            sessions.len()
        )
    }
}

fn text<'a>(params: &'a Map<String, Value>, key: &str, label: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096 && !value.contains('\0'))
        .ok_or_else(|| format!("Invalid {label}"))
}

impl TestBackend {
    pub fn new(store: Arc<HostStore>, providers: Vec<RemoteProvider>) -> Self {
        Self {
            store,
            providers,
            turns: Mutex::new(Vec::new()),
            turn_ready: Condvar::new(),
            binaries: Mutex::new(HashMap::new()),
            models: Mutex::new(VecDeque::new()),
            probes: AtomicUsize::new(0),
            calls: Mutex::new(Vec::new()),
            switching: Mutex::new(HashSet::new()),
            closed: AtomicBool::new(false),
        }
    }

    pub fn store_arc(&self) -> Arc<HostStore> {
        self.store.clone()
    }

    pub fn turns(&self) -> Vec<TestTurn> {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits until the provider has received `count` turns.
    pub fn wait_for_turns(&self, count: usize, timeout: Duration) -> Vec<TestTurn> {
        let deadline = Instant::now() + timeout;
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        while turns.len() < count {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            turns = self
                .turn_ready
                .wait_timeout(turns, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        turns.clone()
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut HostSession)) -> Result<(), String> {
        self.store.transaction(|| {
            let mut next = (*self.store.session(id)?).clone();
            change(&mut next);
            next.revision += 1;
            next.updated_at = now_ms();
            self.store.save(next, &json!({ "type": "test" }))?;
            Ok(())
        })
    }

    /// Streams assistant text into the running turn, as `message.delta` did.
    pub fn deliver(&self, session_id: &str, text: &str) -> Result<(), String> {
        self.update(session_id, |value| {
            let id = format!("{}-reply", value.run_id.clone().unwrap_or_default());
            match value.session.blocks.iter_mut().find(|block| block.id == id) {
                Some(block) => block.text.push_str(text),
                None => value
                    .session
                    .blocks
                    .push(Block::new(id, BlockRole::Assistant, text)),
            }
        })
    }

    /// Ends the running turn.
    pub fn finish(&self, session_id: &str) -> Result<(), String> {
        self.update(session_id, |value| {
            value.status = HostSessionStatus::Idle;
            value.session.busy = Some(false);
        })
    }

    fn record(&self, method: &str, arguments: Value) -> Value {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((method.into(), arguments.clone()));
        json!({ "method": method, "arguments": arguments })
    }

    fn create(&self, command_id: &str, params: &Map<String, Value>) -> Result<HostSession, String> {
        let project = self
            .store
            .project(text(params, "projectId", "project ID")?)?;
        if self
            .switching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&project.id)
        {
            return Err("Wait for the branch switch to finish".into());
        }
        let harness = params
            .get("harness")
            .and_then(Value::as_str)
            .and_then(parse_provider)
            .ok_or("Invalid provider or permission mode")?;
        if !self.providers.contains(&harness) {
            return Err(format!(
                "{} is not available on this host",
                provider_name(harness)
            ));
        }
        let runtime_mode: RuntimeMode =
            serde_json::from_value(params.get("runtimeMode").cloned().unwrap_or(Value::Null))
                .map_err(|_| "Invalid provider or permission mode")?;
        let cwd = self.resolve_worktree(&project.cwd, params.get("worktreeCwd"))?;
        let mut session = Session::blank(
            uuid::Uuid::new_v4().to_string(),
            harness,
            text(params, "model", "model")?,
            cwd,
        );
        session.runtime_mode = runtime_mode;
        session.title = "New remote session".into();
        let _ = command_id;
        let now = now_ms();
        Ok(HostSession {
            session,
            project_id: project.id,
            revision: 0,
            run_id: None,
            status: HostSessionStatus::Idle,
            created_at: Some(now),
            updated_at: now,
            archived: None,
            pinned: None,
            auto_worktree_branch: None,
            block_revisions: None,
            extra: Default::default(),
        })
    }
}

impl HostBackend for TestBackend {
    fn store(&self) -> &HostStore {
        &self.store
    }

    fn open_project(&self, path: &str) -> Result<HostProject, String> {
        if !Path::new(path).is_absolute() || path.contains('\0') {
            return Err("Choose an absolute directory path on the host".into());
        }
        let cwd = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        if !cwd.is_dir() {
            return Err("Project path is not a directory".into());
        }
        let name = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.store.add_project(&cwd.to_string_lossy(), &name)
    }

    fn update_session(&self, id: &str, patch: SessionPatch) -> Result<HostSessionSummary, String> {
        self.store.update_session(id, &patch)
    }

    fn command(&self, params: &Map<String, Value>) -> Result<CommandReceipt, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("Host is stopping".into());
        }
        let command_id = text(params, "commandId", "command ID")?.to_string();
        let signature: String = Sha256::digest(Value::Object(params.clone()).to_string())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if let Some(previous) = self.store.receipt(&command_id, &signature)? {
            return Ok(previous);
        }
        let kind = params.get("type").and_then(Value::as_str).unwrap_or("");
        let (receipt, turn) = self.store.transaction(|| {
            let (value, turn) = match kind {
                "create" => (self.create(&command_id, params)?, None),
                "send" => {
                    let mut value =
                        (*self
                            .store
                            .session(text(params, "sessionId", "session ID")?)?)
                        .clone();
                    if value.status == HostSessionStatus::Running {
                        return Err("Wait for the current turn to finish".into());
                    }
                    let references = parse_remote_attachments(params.get("attachments"))?;
                    let attachments = resolve_attachments(&self.store, &references)?;
                    let prompt = js::string(params.get("text"));
                    let mut block = Block::new(command_id.clone(), BlockRole::User, prompt.clone());
                    if !attachments.is_empty() {
                        block.attachments = Some(attachments.clone());
                    }
                    let run_id = uuid::Uuid::new_v4().to_string();
                    value.session.blocks.push(block);
                    value.session.busy = Some(true);
                    value.status = HostSessionStatus::Running;
                    value.run_id = Some(run_id.clone());
                    let turn = TestTurn {
                        session_id: value.session.id.clone(),
                        run_id,
                        text: prompt,
                        attachments,
                    };
                    (value, Some(turn))
                }
                _ => return Err("Unsupported command".into()),
            };
            let mut value = value;
            value.revision += 1;
            value.updated_at = now_ms();
            let saved = self
                .store
                .save(value, &json!({ "type": "command", "command": params }))?;
            let receipt = CommandReceipt {
                command_id: command_id.clone(),
                session_id: saved.session.id.clone(),
                revision: saved.revision,
            };
            self.store.record_receipt(&signature, &receipt)?;
            Ok((receipt, turn))
        })?;
        if let Some(turn) = turn {
            self.turns
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(turn);
            self.turn_ready.notify_all();
        }
        Ok(receipt)
    }

    fn with_idle_project(
        &self,
        project_id: &str,
        force: bool,
        action: &mut dyn FnMut() -> Result<Value, String>,
    ) -> Result<Value, String> {
        if self
            .switching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(project_id)
        {
            return Err("A branch switch is already in progress".into());
        }
        let running: Vec<_> = self
            .store
            .summaries(project_id)?
            .into_iter()
            .filter(|session| session.status == HostSessionStatus::Running)
            .collect();
        if !running.is_empty() && !force {
            return Err(running_sessions_message(&running));
        }
        self.switching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(project_id.into());
        let result = action();
        self.switching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(project_id);
        result
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    fn resolve_binary(&self, provider: RemoteProvider) -> Result<PathBuf, String> {
        if let Some(path) = self
            .binaries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&provider)
        {
            return Ok(path.clone());
        }
        if self.providers.contains(&provider) {
            Ok(std::env::current_exe().map_err(|error| error.to_string())?)
        } else {
            Err(format!("{} is not installed", provider_name(provider)))
        }
    }

    fn discover_models(
        &self,
        _provider: RemoteProvider,
        _cwd: &str,
    ) -> Result<Vec<AgentModel>, String> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        self.models
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Ok(Vec::new()))
    }

    /// `browseHostDirectories`, from host/browse.ts.
    fn browse_directories(&self, path: Option<&Value>) -> Result<HostDirectory, String> {
        let requested = match path {
            None => None,
            Some(Value::String(text)) if text.len() <= 4096 && !text.contains('\0') => {
                Some(text.as_str()).filter(|text| !text.trim().is_empty())
            }
            Some(_) => return Err("Invalid directory path".into()),
        };
        let home = super::server::home_dir();
        let requested = requested.map(PathBuf::from).unwrap_or(home);
        if !requested.is_absolute() {
            return Err("Choose an absolute directory path".into());
        }
        let metadata = std::fs::metadata(&requested).map_err(|error| error.to_string())?;
        if !metadata.is_dir() {
            return Err("Path is not a directory".into());
        }
        let mut entries: Vec<HostDirectoryEntry> = std::fs::read_dir(&requested)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| HostDirectoryEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries.truncate(500);
        Ok(HostDirectory {
            path: requested.to_string_lossy().into_owned(),
            parent: requested
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned()),
            entries,
        })
    }

    fn resolve_worktree(&self, project_cwd: &str, cwd: Option<&Value>) -> Result<String, String> {
        match cwd {
            None | Some(Value::Null) => Ok(project_cwd.into()),
            Some(Value::String(path)) if path == project_cwd => Ok(path.clone()),
            Some(Value::String(_)) => Err("Choose an available worktree of this project".into()),
            Some(_) => Err("Invalid working copy".into()),
        }
    }

    fn branches(&self, cwd: &str) -> Result<Value, String> {
        Ok(self.record("branches", json!({ "cwd": cwd })))
    }

    fn switch_branch(
        &self,
        cwd: &str,
        branch: Option<&Value>,
        remote: Option<&Value>,
    ) -> Result<Value, String> {
        Ok(self.record(
            "switch_branch",
            json!({ "cwd": cwd, "branch": branch, "remote": remote }),
        ))
    }

    fn create_branch(&self, cwd: &str, branch: Option<&Value>) -> Result<Value, String> {
        Ok(self.record("create_branch", json!({ "cwd": cwd, "branch": branch })))
    }

    fn worktrees(&self, project_cwd: &str) -> Result<Value, String> {
        Ok(self.record("worktrees", json!({ "projectCwd": project_cwd })))
    }

    fn create_worktree(
        &self,
        project_cwd: &str,
        branch: Option<&Value>,
        base: Option<&Value>,
        existing: Option<&Value>,
        cwd: &str,
    ) -> Result<Value, String> {
        Ok(self.record(
            "create_worktree",
            json!({ "projectCwd": project_cwd, "branch": branch, "base": base, "existing": existing, "cwd": cwd }),
        ))
    }

    fn read_file(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String> {
        let path = path.and_then(Value::as_str).ok_or("Invalid file path")?;
        let root = std::fs::canonicalize(cwd).map_err(|error| error.to_string())?;
        let file = std::fs::canonicalize(root.join(path)).map_err(|error| error.to_string())?;
        if !file.starts_with(&root) || file == root {
            return Err("Path is outside the project".into());
        }
        Ok(Value::String(
            std::fs::read_to_string(file).map_err(|error| error.to_string())?,
        ))
    }

    fn list_files(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String> {
        Ok(self.record("list_files", json!({ "cwd": cwd, "path": path })))
    }

    fn index_files(&self, cwd: &str) -> Result<Value, String> {
        Ok(self.record("index_files", json!({ "cwd": cwd })))
    }

    fn workspace_run(
        &self,
        command: Option<&Value>,
        args: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        match command.and_then(Value::as_str) {
            Some("nothing") => Ok(None),
            Some(_) => Ok(Some(
                self.record("workspace_run", json!({ "command": command, "args": args })),
            )),
            None => Err("Unsupported workspace command".into()),
        }
    }

    fn invalidate_workspace_roots(&self) {
        self.record("invalidate_workspace_roots", Value::Null);
    }

    fn search_files(&self, cwd: &str, query: Option<&Value>) -> Result<Value, String> {
        Ok(self.record("search_files", json!({ "cwd": cwd, "query": query })))
    }

    fn search_content(&self, cwd: &str, params: &Map<String, Value>) -> Result<Value, String> {
        Ok(self.record(
            "search_content",
            json!({ "cwd": cwd, "query": params.get("query") }),
        ))
    }

    fn create_path(
        &self,
        cwd: &str,
        parent: Option<&Value>,
        name: Option<&Value>,
        is_dir: Option<&Value>,
    ) -> Result<Value, String> {
        Ok(self.record(
            "create_path",
            json!({ "cwd": cwd, "parent": parent, "name": name, "isDir": is_dir }),
        ))
    }

    fn write_file(
        &self,
        cwd: &str,
        path: Option<&Value>,
        expected: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        self.record(
            "write_file",
            json!({ "cwd": cwd, "path": path, "expected": expected, "content": content }),
        );
        Ok(None)
    }

    fn git_index(&self, cwd: &str) -> Result<Value, String> {
        Ok(self.record("git_index", json!({ "cwd": cwd })))
    }

    fn file_diff(&self, cwd: &str, path: Option<&Value>, staged: bool) -> Result<Value, String> {
        Ok(self.record(
            "file_diff",
            json!({ "cwd": cwd, "path": path, "staged": staged }),
        ))
    }

    fn git_action(
        &self,
        cwd: &str,
        action: Option<&Value>,
        path: Option<&Value>,
        message: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        self.record(
            "git_action",
            json!({ "cwd": cwd, "action": action, "path": path, "message": message, "content": content }),
        );
        Ok(None)
    }
}
