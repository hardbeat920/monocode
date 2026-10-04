//! The values the quick composer hands out: the launch (`QuickLaunch` in
//! src/features/quick-composer/model/quickComposer.ts), the working copy
//! choice (quickWorkspace.ts), and the git popup messages
//! (quickGitPopup.ts).
//!
//! The engine's automations package owns the same shapes. This crate may
//! not depend on the engine, so these mirror its JSON exactly: the app
//! converts with `serde_json::to_value` and `from_value`, or field by field.

use monocode_core::attachment::persistable_attachment;
use monocode_core::block::ModelSettings;
use monocode_core::paths::path_key;
use monocode_core::session::WorkspaceMode;
use monocode_core::{Attachment, HarnessId, RuntimeMode};
use serde::{Deserialize, Serialize};

/// `intent`: the turn mode a leading composer command picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuickIntent {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "orchestrate")]
    Orchestrate,
}

/// `QuickLaunch`: a session the floating composer hands to a window. The
/// engine's `QuickLaunchRequest` reads the same JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLaunchRequest {
    pub prompt: String,
    /// Create an unsent user draft instead of starting an agent turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<QuickIntent>,
    pub cwd: String,
    pub harness: HarnessId,
    /// Missing means the harness default, resolved by the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<RuntimeMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_mode: Option<WorkspaceMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    /// Bring the new session forward instead of starting it quietly.
    pub reveal: bool,
}

/// `quickLaunchAttachments`: the portable fields of attachments on disk.
pub fn quick_launch_attachments(files: &[Attachment]) -> Result<Vec<Attachment>, String> {
    files
        .iter()
        .map(|file| {
            if file.path.as_deref().is_none_or(str::is_empty) {
                return Err(format!("Could not attach {}.", file.name));
            }
            Ok(persistable_attachment(file))
        })
        .collect()
}

/// `Worktree` from src/features/source-control/model/worktrees.ts, as the
/// engine's projects package serializes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
    pub missing: bool,
    pub dirty: Option<bool>,
    pub unpushed: Option<i64>,
    pub session_ids: Vec<String>,
}

impl Worktree {
    /// A linked worktree with a branch and nothing else set.
    pub fn linked(path: &str, branch: &str, head: &str) -> Self {
        Self {
            path: path.into(),
            branch: Some(branch.into()),
            head: head.into(),
            ..Self::default()
        }
    }
}

/// `GitBranchInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub name: String,
    #[serde(default)]
    pub current: bool,
    #[serde(default)]
    pub remote: Option<String>,
}

impl GitBranchInfo {
    pub fn local(name: &str, current: bool) -> Self {
        Self {
            name: name.into(),
            current,
            remote: None,
        }
    }

    pub fn remote(name: &str, remote: &str) -> Self {
        Self {
            name: name.into(),
            current: false,
            remote: Some(remote.into()),
        }
    }

    /// `branchRef`: `remote/name` for a remote branch.
    pub fn reference(&self) -> String {
        match &self.remote {
            Some(remote) => format!("{remote}/{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// `GitBranches`: what `monocode_git::fs::git_branches` returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranches {
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub branches: Vec<GitBranchInfo>,
}

/// `QuickWorkspace`: the panel's working copy choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickWorkspace {
    #[serde(default)]
    pub cwd: Option<String>,
    pub mode: WorkspaceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<Worktree>,
}

impl QuickWorkspace {
    /// `{ cwd, mode: "current" }`.
    pub fn current(cwd: Option<&str>) -> Self {
        Self {
            cwd: cwd.map(str::to_string),
            mode: WorkspaceMode::Current,
            base: None,
            tree: None,
        }
    }

    /// `choice.tree?.path ?? choice.cwd ?? ""`: where git runs.
    pub fn git_cwd(&self) -> String {
        self.tree
            .as_ref()
            .map(|tree| tree.path.clone())
            .or_else(|| self.cwd.clone())
            .unwrap_or_default()
    }

    /// The working copy is a linked worktree (`!!tree && !tree.isMain`).
    pub fn in_linked_worktree(&self) -> bool {
        self.tree.as_ref().is_some_and(|tree| !tree.is_main)
    }
}

/// `workspaceForProject`: switching projects never carries another
/// repository's working copy or base.
pub fn workspace_for_project(choice: &QuickWorkspace, cwd: Option<&str>) -> QuickWorkspace {
    if choice.cwd.as_deref() == cwd {
        return choice.clone();
    }
    QuickWorkspace::current(cwd)
}

/// The workspace fields of a launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickWorkspaceFields {
    pub workspace_mode: Option<WorkspaceMode>,
    pub worktree_base: Option<String>,
    pub worktree_cwd: Option<String>,
}

impl QuickWorkspaceFields {
    /// Copy the fields onto a launch.
    pub fn apply(self, launch: &mut QuickLaunchRequest) {
        launch.workspace_mode = self.workspace_mode;
        launch.worktree_base = self.worktree_base;
        launch.worktree_cwd = self.worktree_cwd;
    }
}

/// `quickWorkspaceLaunch` once the worktrees are listed: a new worktree is
/// created later, on the first turn; an existing one must still be listed.
/// `listed` is `None` when the choice needs no listing (see
/// [`needs_worktree_check`]).
pub fn quick_workspace_fields(
    choice: &QuickWorkspace,
    listed: Option<&[Worktree]>,
) -> Result<QuickWorkspaceFields, String> {
    if choice.mode == WorkspaceMode::Worktree {
        return Ok(QuickWorkspaceFields {
            workspace_mode: Some(WorkspaceMode::Worktree),
            worktree_base: Some(
                choice
                    .base
                    .clone()
                    .filter(|base| !base.is_empty())
                    .unwrap_or_else(|| "HEAD".into()),
            ),
            worktree_cwd: None,
        });
    }
    let (Some(chosen), Some(cwd), Some(listed)) = (&choice.tree, &choice.cwd, listed) else {
        return Ok(QuickWorkspaceFields::default());
    };
    let tree = listed
        .iter()
        .find(|tree| !tree.missing && path_key(&tree.path) == path_key(&chosen.path))
        .ok_or_else(|| {
            "This worktree is no longer available. Select another working copy.".to_string()
        })?;
    Ok(if path_key(&tree.path) == path_key(cwd) {
        QuickWorkspaceFields::default()
    } else {
        QuickWorkspaceFields {
            worktree_cwd: Some(tree.path.clone()),
            ..QuickWorkspaceFields::default()
        }
    })
}

/// The choice picked an existing worktree, so the launch must list the
/// project's worktrees first.
pub fn needs_worktree_check(choice: &QuickWorkspace) -> bool {
    choice.mode != WorkspaceMode::Worktree && choice.tree.is_some() && choice.cwd.is_some()
}

/// `QuickGitKind`: which picker the git popup shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuickGitKind {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "base")]
    Base,
    #[serde(rename = "branch")]
    Branch,
}

/// The rectangle of the control that opened the popup, relative to the
/// composer panel's top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuickGitAnchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `QuickGitRequest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickGitRequest {
    pub id: String,
    pub kind: QuickGitKind,
    pub choice: QuickWorkspace,
    /// The branches the composer had, so a cold popup shows them at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<GitBranches>,
    pub anchor: QuickGitAnchor,
}

impl QuickGitRequest {
    /// The checks `quick_git_open` made before opening.
    pub fn validate(&self) -> Result<(), String> {
        let finite = [
            self.anchor.x,
            self.anchor.y,
            self.anchor.width,
            self.anchor.height,
        ]
        .iter()
        .all(|n| n.is_finite());
        if self.id.is_empty() || !finite || self.choice.cwd.as_deref().is_none_or(str::is_empty) {
            return Err("Invalid picker request.".into());
        }
        Ok(())
    }
}

/// `QuickGitResult`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickGitResult {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<QuickWorkspace>,
    pub restore_focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_kind: Option<QuickGitKind>,
}

#[cfg(test)]
mod tests {
    use monocode_core::AttachmentKind;
    use serde_json::json;

    use super::*;

    #[test]
    fn launch_json_matches_the_engine_shape() {
        let input = json!({
            "prompt": "fix the test",
            "cwd": "/tmp/project",
            "harness": "claude",
            "model": "claude-opus",
            "modelSettings": { "effort": "high", "fast": "true" },
            "runtimeMode": "auto-accept-edits",
            "reveal": false
        });
        let request: QuickLaunchRequest = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), input);
        let plan = json!({
            "prompt": "ship", "draft": true, "intent": "orchestrate", "cwd": "/repo",
            "harness": "codex", "workspaceMode": "worktree", "worktreeBase": "origin/develop",
            "reveal": true
        });
        let request: QuickLaunchRequest = serde_json::from_value(plan.clone()).unwrap();
        assert_eq!(request.intent, Some(QuickIntent::Orchestrate));
        assert_eq!(serde_json::to_value(&request).unwrap(), plan);
    }

    #[test]
    fn launch_attachments_need_a_path() {
        let on_disk = Attachment {
            id: "shot".into(),
            name: "Screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 4,
            path: Some("/tmp/Screenshot.png".into()),
            data: Some("dGVzdA==".into()),
            preview_url: Some("blob:x".into()),
            ..Attachment::default()
        };
        let portable = quick_launch_attachments(std::slice::from_ref(&on_disk)).unwrap();
        assert_eq!(portable[0].data, None);
        assert_eq!(portable[0].preview_url, None);
        assert_eq!(portable[0].path.as_deref(), Some("/tmp/Screenshot.png"));
        let pasted = Attachment {
            path: None,
            ..on_disk
        };
        assert_eq!(
            quick_launch_attachments(&[pasted]).unwrap_err(),
            "Could not attach Screenshot.png."
        );
    }

    #[test]
    fn switching_projects_resets_the_working_copy() {
        let choice = QuickWorkspace {
            cwd: Some("/a".into()),
            mode: WorkspaceMode::Worktree,
            base: Some("main".into()),
            tree: None,
        };
        assert_eq!(workspace_for_project(&choice, Some("/a")), choice);
        assert_eq!(
            workspace_for_project(&choice, Some("/b")),
            QuickWorkspace::current(Some("/b"))
        );
    }

    #[test]
    fn workspace_fields_follow_the_choice() {
        let worktree = QuickWorkspace {
            cwd: Some("/repo".into()),
            mode: WorkspaceMode::Worktree,
            base: None,
            tree: None,
        };
        let fields = quick_workspace_fields(&worktree, None).unwrap();
        assert_eq!(fields.workspace_mode, Some(WorkspaceMode::Worktree));
        assert_eq!(fields.worktree_base.as_deref(), Some("HEAD"));
        let existing = QuickWorkspace {
            cwd: Some("/repo".into()),
            mode: WorkspaceMode::Current,
            base: None,
            tree: Some(Worktree::linked("/repo-feature", "feature", "abc")),
        };
        assert!(needs_worktree_check(&existing));
        let listed = [Worktree::linked("/repo-feature", "feature", "abc")];
        assert_eq!(
            quick_workspace_fields(&existing, Some(&listed))
                .unwrap()
                .worktree_cwd
                .as_deref(),
            Some("/repo-feature")
        );
        assert_eq!(
            quick_workspace_fields(&existing, Some(&[])).unwrap_err(),
            "This worktree is no longer available. Select another working copy."
        );
        assert_eq!(
            quick_workspace_fields(&QuickWorkspace::current(Some("/repo")), None).unwrap(),
            QuickWorkspaceFields::default()
        );
    }

    #[test]
    fn git_request_preserves_the_composers_branch_snapshot() {
        let value = json!({
            "id": "first-open", "kind": "workspace",
            "choice": { "cwd": "/repo", "mode": "current" },
            "branches": { "current": "main", "detached": false,
                "branches": [{ "name": "main", "current": true, "remote": null }] },
            "anchor": { "x": 10.0, "y": 10.0, "width": 100.0, "height": 24.0 }
        });
        let request: QuickGitRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(request.validate().is_ok());
        assert_eq!(serde_json::to_value(&request).unwrap(), value);
        let mut blank = request.clone();
        blank.choice.cwd = Some(String::new());
        assert!(blank.validate().is_err());
        let mut nan = request;
        nan.anchor.x = f64::NAN;
        assert!(nan.validate().is_err());
    }
}
