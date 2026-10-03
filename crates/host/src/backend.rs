//! The host's harness setup, and [`HostEngine`] as the server's
//! [`HostBackend`]. Port of the wiring in host/cli.ts `serve` and of the
//! engine and workspace calls host/server.ts made.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use monocode_core::AgentModel;
use monocode_harness::BridgeLease;
use monocode_harness::core::registry::{HarnessRegistry, RegistryOptions};
use monocode_harness::core::task::SharedSpawner;
use monocode_harness::core::{Children, HarnessContext, SharedCatalog, register_builtin_harnesses};
use monocode_remote::host::HostStore;
use monocode_remote::host::backend::HostBackend;
use monocode_remote::host::protocol::{
    CommandReceipt, HostDirectory, HostProject, HostSessionSummary, RemoteProvider, provider_name,
};
use monocode_remote::host::store::SessionPatch;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::browse::browse_host_directories;
use crate::child_backend::{HeadlessChildBackend, host_children};
use crate::engine::HostEngine;
use crate::git_branches::{create_host_branch, host_branches, switch_host_branch};
use crate::git_worktrees::{create_host_worktree, host_worktrees, resolve_host_worktree_listed};
use crate::providers::{HostProviders, discover_models, host_providers};
use crate::runtime::HostRuntime;
use crate::workspace::{
    create_host_path, host_file_diff, host_git_action, host_git_index, index_host_files,
    list_host_files, read_host_file, search_host_content, search_host_files, write_host_file,
};

/// Settings for [`HostEngine::start`].
#[derive(Debug, Clone)]
pub struct HostEngineOptions {
    /// Fixed provider binaries, used instead of searching the PATH.
    pub binaries: HashMap<RemoteProvider, PathBuf>,
    /// Threads for provider I/O and turns.
    pub threads: usize,
}

impl Default for HostEngineOptions {
    fn default() -> Self {
        Self {
            binaries: HashMap::new(),
            threads: 4,
        }
    }
}

/// The real providers: adapters registered over this host's process
/// supervisor, on the host's own executor.
pub struct HostHarness {
    runtime: HostRuntime,
    registry: HarnessRegistry,
    children: Children,
    backend: Arc<HeadlessChildBackend>,
    catalog: SharedCatalog,
    lease: Mutex<Option<BridgeLease>>,
    closed: AtomicBool,
}

impl HostHarness {
    /// Registers every built-in provider. Provider account profiles would
    /// live in the store's data directory; the host refuses named accounts.
    pub fn start(store: &HostStore, options: HostEngineOptions) -> Self {
        let runtime = HostRuntime::new(options.threads);
        let spawner = runtime.spawner();
        let data_dir = store
            .attachment_dir
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let (children, backend) = host_children(data_dir, options.binaries, spawner.clone());
        // The router clears its state when the last lease drops, so the host
        // holds one for its lifetime.
        let lease = children.start_harness_bridge();
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let catalog = SharedCatalog::new();
        register_builtin_harnesses(&HarnessContext::new(
            registry.clone(),
            children.clone(),
            catalog.clone(),
        ));
        Self {
            runtime,
            registry,
            children,
            backend,
            catalog,
            lease: Mutex::new(Some(lease)),
            closed: AtomicBool::new(false),
        }
    }

    pub fn providers(&self) -> HostProviders {
        host_providers(&self.registry)
    }

    pub fn catalog(&self) -> &SharedCatalog {
        &self.catalog
    }

    pub fn spawner(&self) -> SharedSpawner {
        self.runtime.spawner()
    }

    pub fn children(&self) -> &Children {
        &self.children
    }

    /// The provider CLI this host launches.
    pub fn resolve(&self, provider: RemoteProvider) -> Result<PathBuf, String> {
        self.backend.resolve(provider)
    }

    /// `discoverModels[provider](cwd)`, then `setHarnessModels` when the
    /// list is not empty.
    pub fn discover_models(
        &self,
        provider: RemoteProvider,
        cwd: &str,
    ) -> Result<Vec<AgentModel>, String> {
        let spawner = self.spawner();
        let models = smol::block_on(discover_models(
            provider,
            cwd,
            &self.children,
            &spawner,
            &self.catalog,
        ))?;
        if !models.is_empty() {
            self.catalog.set_harness_models(provider, models.clone());
        }
        Ok(models)
    }

    /// Kills every provider process and stops the executor.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.backend.close();
        self.lease.lock().take();
        self.runtime.shutdown();
    }
}

fn to_json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

impl HostBackend for HostEngine {
    fn store(&self) -> &HostStore {
        HostEngine::store(self)
    }

    fn open_project(&self, cwd: &str) -> Result<HostProject, String> {
        HostEngine::open_project(self, cwd)
    }

    fn update_session(&self, id: &str, patch: SessionPatch) -> Result<HostSessionSummary, String> {
        HostEngine::update_session(self, id, &patch)
    }

    fn command(&self, params: &Map<String, Value>) -> Result<CommandReceipt, String> {
        HostEngine::command(self, &Value::Object(params.clone()))
    }

    fn with_idle_project(
        &self,
        project_id: &str,
        force: bool,
        action: &mut dyn FnMut() -> Result<Value, String>,
    ) -> Result<Value, String> {
        HostEngine::with_idle_project(self, project_id, force, action)
    }

    fn close(&self) {
        HostEngine::close(self);
    }

    fn resolve_binary(&self, provider: RemoteProvider) -> Result<PathBuf, String> {
        match self.harness() {
            Some(harness) => harness.resolve(provider),
            None => Err(format!(
                "{} is not available on this host",
                provider_name(provider)
            )),
        }
    }

    fn discover_models(
        &self,
        provider: RemoteProvider,
        cwd: &str,
    ) -> Result<Vec<AgentModel>, String> {
        match self.harness() {
            Some(harness) => harness.discover_models(provider, cwd),
            None => Err(format!(
                "{} is not available on this host",
                provider_name(provider)
            )),
        }
    }

    fn browse_directories(&self, path: Option<&Value>) -> Result<HostDirectory, String> {
        browse_host_directories(path)
    }

    fn resolve_worktree(&self, project_cwd: &str, cwd: Option<&Value>) -> Result<String, String> {
        resolve_host_worktree_listed(project_cwd, cwd)
    }

    fn branches(&self, cwd: &str) -> Result<Value, String> {
        to_json(host_branches(cwd)?)
    }

    fn switch_branch(
        &self,
        cwd: &str,
        branch: Option<&Value>,
        remote: Option<&Value>,
    ) -> Result<Value, String> {
        to_json(switch_host_branch(cwd, branch, remote)?)
    }

    fn create_branch(&self, cwd: &str, branch: Option<&Value>) -> Result<Value, String> {
        to_json(create_host_branch(cwd, branch)?)
    }

    fn worktrees(&self, project_cwd: &str) -> Result<Value, String> {
        to_json(host_worktrees(project_cwd)?)
    }

    fn create_worktree(
        &self,
        project_cwd: &str,
        branch: Option<&Value>,
        base: Option<&Value>,
        existing: Option<&Value>,
        cwd: &str,
    ) -> Result<Value, String> {
        to_json(create_host_worktree(
            project_cwd,
            branch,
            base,
            existing,
            Some(cwd),
        )?)
    }

    fn read_file(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String> {
        to_json(read_host_file(Path::new(cwd), path)?)
    }

    fn list_files(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String> {
        to_json(list_host_files(Path::new(cwd), path)?)
    }

    fn index_files(&self, cwd: &str) -> Result<Value, String> {
        to_json(index_host_files(Path::new(cwd))?)
    }

    fn workspace_run(
        &self,
        command: Option<&Value>,
        args: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let idle =
            |project_id: &str, force: bool, action: &mut dyn FnMut() -> Result<Value, String>| {
                HostEngine::with_idle_project(self, project_id, force, action)
            };
        self.workspace().run(command, args, &idle)
    }

    fn invalidate_workspace_roots(&self) {
        self.workspace().invalidate_roots();
    }

    fn search_files(&self, cwd: &str, query: Option<&Value>) -> Result<Value, String> {
        to_json(search_host_files(Path::new(cwd), query)?)
    }

    fn search_content(&self, cwd: &str, params: &Map<String, Value>) -> Result<Value, String> {
        search_host_content(Path::new(cwd), params)
    }

    fn create_path(
        &self,
        cwd: &str,
        parent: Option<&Value>,
        name: Option<&Value>,
        is_dir: Option<&Value>,
    ) -> Result<Value, String> {
        create_host_path(Path::new(cwd), parent, name, is_dir)
            .map(Value::String)
            .map_err(|error| error.to_string())
    }

    fn write_file(
        &self,
        cwd: &str,
        path: Option<&Value>,
        expected: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        write_host_file(Path::new(cwd), path, expected, content)?;
        Ok(None)
    }

    fn git_index(&self, cwd: &str) -> Result<Value, String> {
        to_json(host_git_index(Path::new(cwd))?)
    }

    fn file_diff(&self, cwd: &str, path: Option<&Value>, staged: bool) -> Result<Value, String> {
        to_json(host_file_diff(Path::new(cwd), path, staged)?)
    }

    fn git_action(
        &self,
        cwd: &str,
        action: Option<&Value>,
        path: Option<&Value>,
        message: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        host_git_action(Path::new(cwd), action, path, message, content)
    }
}
