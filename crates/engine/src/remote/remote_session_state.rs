//! Port of src/features/connections/model/remoteSessionState.ts.

use monocode_core::Session;
use monocode_remote::host::protocol::HostSession;
use serde_json::Value;

use super::remote_projects::{RemoteProject, remote_path};

/// `remoteSessionState`: show the host's conversation in the app's ordinary
/// session state while keeping the local tab ID and the project's remote
/// path.
///
/// `{ ...shell, ...host }` overlays the host session's fields on the shell's,
/// so fields the host does not send (composer chips, queued messages) stay.
/// The overlay runs on the session fields without the transcript, which the
/// host's copy replaces whole.
pub fn remote_session_state(
    shell: &Session,
    snapshot: &HostSession,
    project: &RemoteProject,
) -> Session {
    let host = &snapshot.session;
    let worktree_cwd =
        (host.cwd != project.cwd).then(|| remote_path(&project.environment_id, &host.cwd));
    let mut base = Session {
        blocks: Vec::new(),
        ..shell.clone()
    };
    let host_fields = Session {
        blocks: Vec::new(),
        ..host.clone()
    };
    let merged = match (
        serde_json::to_value(&base),
        serde_json::to_value(&host_fields),
    ) {
        (Ok(Value::Object(mut fields)), Ok(Value::Object(host_fields))) => {
            fields.extend(host_fields);
            serde_json::from_value::<Session>(Value::Object(fields)).ok()
        }
        _ => None,
    };
    base = merged.unwrap_or(host_fields);
    Session {
        id: shell.id.clone(),
        cwd: shell.cwd.clone(),
        worktree_cwd,
        blocks: host.blocks.clone(),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::session_store::should_persist_session;
    use monocode_core::{Block, BlockRole, Extra, HarnessId};
    use serde_json::json;

    // remoteSessionState.test.ts
    #[test]
    fn keeps_a_host_transcript_in_normal_session_state_under_its_local_tab_id() {
        let mut shell = Session::blank(
            "tab-1",
            HarnessId::Codex,
            "codex:test",
            "remote://env/home/me/repo",
        );
        shell.composer_seed = Some("draft".into());
        let mut host = shell.clone();
        host.id = "host-session".into();
        host.cwd = "/home/me/repo-worktrees/dev".into();
        host.title = "Fix the build".into();
        host.composer_seed = None;
        host.blocks = vec![
            Block::new("turn", BlockRole::User, "Fix it"),
            Block::new("plan", BlockRole::Plan, "Build the fix"),
        ];
        let snapshot: HostSession = serde_json::from_value(json!({
            "projectId": "project",
            "revision": 2,
            "status": "idle",
            "updatedAt": 1,
            "session": host,
        }))
        .unwrap();
        let session = remote_session_state(
            &shell,
            &snapshot,
            &RemoteProject {
                key: shell.cwd.clone(),
                environment_id: "env".into(),
                project_id: "project".into(),
                cwd: "/home/me/repo".into(),
                extra: Extra::new(),
            },
        );
        assert_eq!(session.id, shell.id);
        assert_eq!(session.cwd, shell.cwd);
        assert_eq!(
            session.worktree_cwd.as_deref(),
            Some("remote://env/home/me/repo-worktrees/dev")
        );
        assert_eq!(session.title, "Fix the build");
        assert_eq!(session.blocks, snapshot.session.blocks);
        assert_eq!(session.composer_seed.as_deref(), Some("draft"));
        assert!(!should_persist_session(&session));
    }
}
