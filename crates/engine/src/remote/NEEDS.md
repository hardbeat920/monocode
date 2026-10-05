# Notes for the lead from the remote package

Each item has a local workaround in place. None blocks the package.

## Shared files I changed

- crates/engine/Cargo.toml: the `remote` feature now turns on
  `dep:monocode-remote` and `dep:base64`, and `monocode-remote` is a new
  optional dependency. Cargo.lock gained that edge.

## Runtime

- `RemoteHooks` has only `session_events`, which serves the headless host.
  The desktop installs `DesktopRemoteHooks`, whose `session_events` does
  nothing: remote tabs get their transcript from host snapshots. The remote
  calls the other packages asked for live on their own peer traits, so the
  app has to wire them (next section).

## App wiring

- `RemoteGlobal::init_native(kv, data_dir, cx)` after `Engine::init`, with
  the same `Kv` the other packages use.
- Submit: `SubmitPeers.remote = Rc::new(remote::peers::SubmitRemote)`.
- Attention's `ApprovalRouter`: `is_remote` →
  `RemoteGlobal::is_remote_session`, `remote_approve` →
  `RemoteGlobal::approve`, `remote_answer` → `RemoteGlobal::answer`. A
  finished remote turn already calls `Notifier::announce_finished_later`
  when the `attention` feature is on.
- History's `HistoryHost`: `remote_session_for` →
  `RemoteGlobal::remote_session_for`; `remember_remote_session` →
  `RemoteGlobal::remember_remote_session` plus focusing the composer;
  `select_remote_session` → `RemoteSessions::tab_for_remote_session`, or a
  new tab bound with `RemoteSessions::bind_tab`.
- Projects' `ProjectsHooks::remote_session_for` →
  `RemoteGlobal::remote_session_for`.
- Workspace's `WorkspaceDelegate`: `remember_remote_session` →
  `RemoteGlobal::forget_tab`, `remote_session_for`,
  `remote_pending_worktree` (`.is_some()`), and `remote_summary` →
  `RemoteGlobal::cached_summary` (title and harness).
- Workspace file backend: `RemoteFs::new(Arc::new(LocalFs), client)`
  (feature `workspace`) answers `remote://` paths on their machine. Other
  file and Git callers use `remote::invoke_workspace(command, args, cx)`
  before running a command locally, and `decode_remote_binary` for
  `read_binary_file`.

## Views

- A remote tab calls `RemoteSessions::open(shell, visible, cx)` each time
  it draws. `RemoteTab::{MissingProject, Connecting, NotConnected}` carry the
  placeholder text.
- `RemoteSession::set_current_branch` needs the projects package's branch
  state for `execution_path()` (`useProjectBranchesState`).

## Other crates

- `temporary_worktree_branch_name` now exists in submit, projects, and
  here. It belongs in `monocode_core`.
- monocode-remote's `Machine` and `JobView` keep their fields private, so
  the transport reads them back through serde as the protocol types.
- `Kv::set_item` cannot fail, so the TypeScript's "Cannot save your request
  locally" outbox error never happens.
