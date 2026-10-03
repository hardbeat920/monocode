//! What the remote package fills in for the runtime and the other packages,
//! and the calls it makes into them.
//!
//! - `DesktopRemoteHooks`: the runtime's `RemoteHooks`.
//! - `SubmitRemote` (with `submit`): submit's `SubmitRemoteHooks`, so the
//!   composer's send, draft, compact, stop, and plan build reach the host.
//! - `RemoteFs` (with `workspace`): the workspace file backend, with
//!   `remote://` paths answered by their machine.
//! - `RemotePeers`: the calls this package makes out. The default announces
//!   a finished host turn through attention's notifier when that package is
//!   built in.
//!
//! Attention's `ApprovalRouter` and the history, projects, and workspace
//! hooks mix several packages, so the app composes them; it calls
//! `RemoteGlobal::{is_remote_session, approve, answer, remote_session_for,
//! remember_remote_session, remote_pending_worktree, cached_summary}`.

use gpui::App;
use monocode_core::HarnessEvent;

use crate::runtime::hooks::RemoteHooks;

/// The runtime's remote hooks on the desktop.
pub struct DesktopRemoteHooks;

impl RemoteHooks for DesktopRemoteHooks {
    /// Remote tabs never receive local harness events: their transcript
    /// comes from host snapshots. Forwarding events to desktops is the
    /// headless host's job (`monocode-host`).
    fn session_events(&self, _session_id: &str, _events: &[HarnessEvent], _cx: &mut App) {}
}

/// Calls the remote package makes into other packages.
pub trait RemotePeers {
    /// App.tsx `announceSessionFinished` on the next tick, for a host turn
    /// that ended. Attention's notifier decides whether the tab is visible.
    fn announce_finished_later(&self, session_id: &str, cx: &mut App) {
        announce_finished_later(session_id, cx);
    }
}

/// The default peers: attention's notifier when it is built in.
pub struct DefaultRemotePeers;

impl RemotePeers for DefaultRemotePeers {}

/// `Notifier::announce_finished_later` (attention NEEDS item 5).
pub fn announce_finished_later(session_id: &str, cx: &mut App) {
    #[cfg(feature = "attention")]
    if let Some(attention) = crate::attention::Attention::try_global(cx) {
        let notifier = attention.notifier.clone();
        notifier.update(cx, |notifier, cx| {
            notifier.announce_finished_later(session_id, cx)
        });
    }
    #[cfg(not(feature = "attention"))]
    let _ = (session_id, cx);
}

#[cfg(feature = "submit")]
mod submit_hooks {
    use gpui::App;
    use monocode_core::Attachment;
    use monocode_core::block::PlanBuildTarget;

    use super::super::{RemoteGlobal, RemoteTurnOptions};
    use crate::submit::SubmitOptions;
    use crate::submit::hooks::SubmitRemoteHooks;

    /// Submit's remote hooks: `remoteProjectFor` and the open remote tab's
    /// actions (`remoteSessionActions`, `buildRemotePlan`).
    pub struct SubmitRemote;

    impl SubmitRemoteHooks for SubmitRemote {
        fn is_remote(&self, cwd: &str, cx: &App) -> bool {
            RemoteGlobal::is_remote_project(cwd, cx)
        }

        fn submit(
            &self,
            session_id: &str,
            text: &str,
            attachments: &[Attachment],
            options: &SubmitOptions,
            cx: &mut App,
        ) -> bool {
            let options = RemoteTurnOptions {
                intent: options.intent,
                draft_block_id: options.draft_block_id.clone(),
            };
            RemoteGlobal::submit(session_id, text, attachments, &options, cx)
        }

        fn save_draft(
            &self,
            session_id: &str,
            text: &str,
            attachments: &[Attachment],
            cx: &mut App,
        ) -> bool {
            RemoteGlobal::save_draft(session_id, text, attachments, cx)
        }

        fn compact(&self, session_id: &str, cx: &mut App) -> bool {
            RemoteGlobal::compact(session_id, cx)
        }

        fn stop(&self, session_id: &str, cx: &mut App) {
            RemoteGlobal::stop(session_id, cx);
        }

        fn build_plan(
            &self,
            session_id: &str,
            block_id: &str,
            target: Option<&PlanBuildTarget>,
            cx: &mut App,
        ) {
            RemoteGlobal::build_plan(session_id, block_id, target, cx);
        }
    }
}

#[cfg(feature = "submit")]
pub use submit_hooks::SubmitRemote;

#[cfg(feature = "workspace")]
mod workspace_fs {
    use std::sync::Arc;

    use serde::de::DeserializeOwned;
    use serde_json::{Map, Value, json};

    use super::super::client::RemoteClient;
    use super::super::remote_commands::is_remote_path;
    use crate::workspace::files::backend::{FileMtime, FsBackend, FsEntry, FsFuture, ProjectFile};

    /// The workspace file backend with `remote://` paths answered by the
    /// machine that owns them, as `invokeWorkspace` routed `listProjectFiles`,
    /// `statFiles`, and `listDir`.
    pub struct RemoteFs {
        local: Arc<dyn FsBackend>,
        client: RemoteClient,
    }

    impl RemoteFs {
        pub fn new(local: Arc<dyn FsBackend>, client: RemoteClient) -> Self {
            Self { local, client }
        }

        fn run<T: DeserializeOwned + Send + 'static>(
            &self,
            command: &str,
            args: Value,
        ) -> FsFuture<T> {
            let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
            let run = self.client.run_remote_command(command, args);
            Box::pin(async move {
                serde_json::from_value(run.await?).map_err(|error| error.to_string())
            })
        }
    }

    impl FsBackend for RemoteFs {
        fn list_project_files(&self, cwd: String) -> FsFuture<Vec<ProjectFile>> {
            if is_remote_path(&json!(cwd)) {
                return self.run("list_project_files", json!({ "cwd": cwd }));
            }
            self.local.list_project_files(cwd)
        }

        fn stat_files(&self, paths: Vec<String>) -> FsFuture<Vec<FileMtime>> {
            let local = self.local.clone();
            let stat = self.client.stat_files(paths, move |paths| {
                let stat = local.stat_files(paths);
                Box::pin(async move {
                    serde_json::to_value(stat.await?).map_err(|error| error.to_string())
                })
            });
            Box::pin(async move {
                stat.await?
                    .into_iter()
                    .map(|entry| serde_json::from_value(entry).map_err(|error| error.to_string()))
                    .collect()
            })
        }

        fn list_dir(&self, path: String) -> FsFuture<Vec<FsEntry>> {
            if is_remote_path(&json!(path)) {
                return self.run("list_dir", json!({ "path": path }));
            }
            self.local.list_dir(path)
        }
    }
}

#[cfg(feature = "workspace")]
pub use workspace_fs::RemoteFs;
