//! What the Connect settings page and the remote project dialog need from
//! the app: the native connection commands the React views called through
//! `invoke` (`remote_machines`, `remote_request`, `remote_pair`, and the
//! `remote_ssh_*` jobs).
//!
//! The engine implements [`RemoteHost`] over its `RemoteConnections`
//! entity. Every method except [`RemoteHost::machines`] and
//! [`RemoteHost::request`] has a default that fails or does nothing, so a
//! gallery or a test fills in only what it checks.

use gpui::{App, Task};
use monocode_layout::paths::remote_path;
use monocode_remote::host::protocol::{HostProject, RemoteMachine, SshSetup};
use serde_json::Value;

/// An async host call. Errors carry the message the view shows.
pub type HostTask<T> = Task<Result<T, String>>;

fn unavailable<T: 'static>() -> HostTask<T> {
    Task::ready(Err("Not available in this build".into()))
}

/// `remote_ssh_begin`: set up a machine over SSH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshBegin {
    /// An SSH address (`user@my-mac-mini`) or an alias from the SSH config.
    pub target: String,
    /// The name shown in MonoCode. Empty uses the host's own name.
    pub name: String,
    /// Empty in the form means the SSH config decides.
    pub port: Option<u16>,
    /// Install this desktop's host version even when an older host has
    /// running turns.
    pub upgrade: bool,
}

/// The connection commands. Ports of the Tauri commands in
/// src-tauri/src/remote.rs and src-tauri/src/remote_ssh.rs.
pub trait RemoteHost: 'static {
    /// `remote_machines`: the saved connections.
    fn machines(&self, cx: &mut App) -> HostTask<Vec<RemoteMachine>>;

    /// `remote_request`: one host method on a machine, such as
    /// `environment.describe` or `projects.browse`.
    fn request(
        &self,
        machine_id: &str,
        method: &str,
        params: Value,
        cx: &mut App,
    ) -> HostTask<Value>;

    /// `getVersion`: this desktop's version, which names the host package
    /// the connect command installs.
    fn app_version(&self, _cx: &mut App) -> HostTask<String> {
        unavailable()
    }

    /// `remote_pair`: pairs the machine in a `monocode://pair` link.
    fn pair(&self, _link: &str, _name: &str, _cx: &mut App) -> HostTask<RemoteMachine> {
        unavailable()
    }

    /// `remote_retry`: forget the machine's route and recent failure, so the
    /// next request tries every address again.
    fn retry(&self, _machine_id: &str, _cx: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }

    /// `remote_disconnect`: delete the saved connection and close its
    /// forward. The host keeps running.
    fn disconnect(&self, _machine_id: &str, _cx: &mut App) -> HostTask<()> {
        unavailable()
    }

    /// `remote_ssh_begin`: starts a setup job and returns its id.
    fn ssh_begin(&self, _request: SshBegin, _cx: &mut App) -> HostTask<String> {
        unavailable()
    }

    /// `remote_ssh_reconnect`: runs `connect` again on a machine set up over
    /// SSH, optionally installing this desktop's host version.
    fn ssh_reconnect(&self, _machine_id: &str, _upgrade: bool, _cx: &mut App) -> HostTask<String> {
        unavailable()
    }

    /// `remote_ssh_poll`: the job's progress, prompt, and result.
    fn ssh_poll(&self, _job_id: &str, _cx: &mut App) -> HostTask<SshSetup> {
        unavailable()
    }

    /// `remote_ssh_answer`: a host trust answer (`yes` or `no`) or a
    /// password or passphrase for the job's current prompt. The answer is
    /// used once and never saved.
    fn ssh_answer(
        &self,
        _job_id: &str,
        _prompt_id: &str,
        _answer: String,
        _cx: &mut App,
    ) -> HostTask<()> {
        unavailable()
    }

    /// `remote_ssh_cancel`.
    fn ssh_cancel(&self, _job_id: &str, _cx: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }

    /// `rememberRemoteProject`: saves a folder on a machine as a rail project
    /// and returns its rail key. The default only computes the key.
    fn remember_project(
        &self,
        environment_id: &str,
        project: &HostProject,
        _cx: &mut App,
    ) -> String {
        remote_project_key(environment_id, &project.cwd)
    }

    /// `refreshRemoteMachines`: a view paired, removed, or set up a machine,
    /// so other views should read the machine list again.
    fn machines_changed(&self, _cx: &mut App) {}
}

/// `remoteProjectKey`: a project's rail key, its folder under the machine's
/// `remote://<environment>/` root without a trailing slash.
pub fn remote_project_key(environment_id: &str, cwd: &str) -> String {
    let slashed = cwd.replace('\\', "/");
    remote_path(environment_id, slashed.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::remote_project_key;

    #[test]
    fn project_keys_live_under_the_machine_root() {
        assert_eq!(
            remote_project_key("env", "/home/me/code/app"),
            "remote://env/home/me/code/app"
        );
        assert_eq!(
            remote_project_key("env", "/home/me/code/app/"),
            "remote://env/home/me/code/app"
        );
        assert_eq!(
            remote_project_key("env", "C:\\Users\\me\\app"),
            "remote://env/C:/Users/me/app"
        );
    }
}
