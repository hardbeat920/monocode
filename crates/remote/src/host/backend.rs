//! The boundary between the host server and the host's engine.
//!
//! `host/server.ts` called into `HostEngine` (sessions, turns, commands), the
//! workspace modules (files, Git, worktrees, project browsing), and the
//! provider catalogs. Those are ported separately; they plug into the server
//! by implementing [`HostBackend`].
//!
//! Every method blocks. The server calls them on its connection threads,
//! several at once, so implementations must be thread safe. Errors are the
//! message the desktop shows, as `error.message` was in TypeScript.
//!
//! Parameters typed `Option<&Value>` are request fields the TypeScript passed
//! on unchecked as `unknown`, with `None` for `undefined`. The backend
//! validates them, as the TypeScript functions did.

use std::path::PathBuf;

use monocode_core::AgentModel;
use serde_json::{Map, Value};

use super::protocol::{
    CommandReceipt, HostDirectory, HostProject, HostSessionSummary, RemoteProvider,
};
use super::store::{HostStore, SessionPatch};

pub trait HostBackend: Send + Sync + 'static {
    /// `HostEngine.store`: the store the engine writes sessions to.
    fn store(&self) -> &HostStore;

    /// `engine.openProject(cwd)`: registers an absolute directory.
    fn open_project(&self, cwd: &str) -> Result<HostProject, String>;

    /// `engine.updateSession(id, patch)`: flushes batched output, then
    /// applies the metadata change through the store.
    fn update_session(&self, id: &str, patch: SessionPatch) -> Result<HostSessionSummary, String>;

    /// `engine.command(params)`: validates and applies one `HostCommand`.
    fn command(&self, params: &Map<String, Value>) -> Result<CommandReceipt, String>;

    /// `engine.withIdleProject(projectId, action, force)`: runs `action`
    /// while no session in the project is running, unless `force`.
    fn with_idle_project(
        &self,
        project_id: &str,
        force: bool,
        action: &mut dyn FnMut() -> Result<Value, String>,
    ) -> Result<Value, String>;

    /// `engine.close()` and the child backend's `close()`: stops providers
    /// before the host exits.
    fn close(&self);

    /// The provider CLI the host would launch, as `resolve*Binary()`
    /// found it. An error means the provider is not installed.
    fn resolve_binary(&self, provider: RemoteProvider) -> Result<PathBuf, String>;

    /// `discover*Models(cwd)`. Implementations also update the catalog the
    /// engine resolves models from (`setHarnessModels`).
    fn discover_models(
        &self,
        provider: RemoteProvider,
        cwd: &str,
    ) -> Result<Vec<AgentModel>, String>;

    /// `browseHostDirectories(path)`.
    fn browse_directories(&self, path: Option<&Value>) -> Result<HostDirectory, String>;

    /// `resolveHostWorktreeAsync(projectCwd, cwd)`: the project checkout, or
    /// one of its registered worktrees.
    fn resolve_worktree(&self, project_cwd: &str, cwd: Option<&Value>) -> Result<String, String>;

    /// `hostBranches(cwd)`.
    fn branches(&self, cwd: &str) -> Result<Value, String>;

    /// `switchHostBranch(cwd, branch, remote)`.
    fn switch_branch(
        &self,
        cwd: &str,
        branch: Option<&Value>,
        remote: Option<&Value>,
    ) -> Result<Value, String>;

    /// `createHostBranch(cwd, branch)`.
    fn create_branch(&self, cwd: &str, branch: Option<&Value>) -> Result<Value, String>;

    /// `hostWorktrees(projectCwd)`.
    fn worktrees(&self, project_cwd: &str) -> Result<Value, String>;

    /// `createHostWorktree(projectCwd, branch, base, existing, cwd)`.
    fn create_worktree(
        &self,
        project_cwd: &str,
        branch: Option<&Value>,
        base: Option<&Value>,
        existing: Option<&Value>,
        cwd: &str,
    ) -> Result<Value, String>;

    /// `readHostFile(cwd, path)`.
    fn read_file(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String>;

    /// `listHostFiles(cwd, path)`.
    fn list_files(&self, cwd: &str, path: Option<&Value>) -> Result<Value, String>;

    /// `indexHostFiles(cwd)`.
    fn index_files(&self, cwd: &str) -> Result<Value, String>;

    /// `WorkspaceCommands.run(command, args)`: this app's file and Git
    /// commands, limited to host projects. `None` answers `null`.
    fn workspace_run(
        &self,
        command: Option<&Value>,
        args: Option<&Value>,
    ) -> Result<Option<Value>, String>;

    /// `WorkspaceCommands.invalidateRoots()`, after a worktree is created.
    fn invalidate_workspace_roots(&self);

    /// `searchHostFiles(cwd, query)`.
    fn search_files(&self, cwd: &str, query: Option<&Value>) -> Result<Value, String>;

    /// `searchHostContent(cwd, params)`.
    fn search_content(&self, cwd: &str, params: &Map<String, Value>) -> Result<Value, String>;

    /// `createHostPath(cwd, parent, name, isDir)`.
    fn create_path(
        &self,
        cwd: &str,
        parent: Option<&Value>,
        name: Option<&Value>,
        is_dir: Option<&Value>,
    ) -> Result<Value, String>;

    /// `writeHostFile(cwd, path, expected, content)`. `None` answers `null`.
    fn write_file(
        &self,
        cwd: &str,
        path: Option<&Value>,
        expected: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String>;

    /// `hostGitIndex(cwd)`.
    fn git_index(&self, cwd: &str) -> Result<Value, String>;

    /// `hostFileDiff(cwd, path, staged)`.
    fn file_diff(&self, cwd: &str, path: Option<&Value>, staged: bool) -> Result<Value, String>;

    /// `hostGitAction(cwd, action, path, message, content)`. `None` answers
    /// `null`.
    fn git_action(
        &self,
        cwd: &str,
        action: Option<&Value>,
        path: Option<&Value>,
        message: Option<&Value>,
        content: Option<&Value>,
    ) -> Result<Option<Value>, String>;
}
