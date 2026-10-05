//! Port of src/features/workspace/model/layout.ts.
//!
//! Split tree for a tab. Same-direction splits share a group so new panes
//! divide space equally until the user drags a sash. Panes can be dragged
//! onto another pane's edge: same-axis siblings reorder, a perpendicular
//! edge nests a new split, and a drop in another group relocates the leaf
//! there. Session cards from the sidebar use the same edges to open or move
//! a chat into that pane. cmd-d splits right, shift-cmd-d splits down,
//! cmd-opt-arrows move focus to the adjacent pane.
//!
//! The TypeScript returned the same object when nothing changed, and some
//! callers compared by identity. These functions take references and return
//! new values; an unchanged result is an equal copy, so compare with `==`.

use monocode_core::{Extra, HarnessId};
use serde::{Deserialize, Serialize};

use crate::ids::random_uuid;
use crate::js;
use crate::terminal_tab::{TerminalMetaPatch, apply_terminal_meta, default_terminal_title};
use monocode_core::paths::path_key;

/// `SplitDir`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SplitDir {
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "down")]
    Down,
}

/// `FocusDir`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FocusDir {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
}

impl FocusDir {
    pub const fn as_str(self) -> &'static str {
        match self {
            FocusDir::Left => "left",
            FocusDir::Right => "right",
            FocusDir::Up => "up",
            FocusDir::Down => "down",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            FocusDir::Left,
            FocusDir::Right,
            FocusDir::Up,
            FocusDir::Down,
        ]
        .into_iter()
        .find(|dir| dir.as_str() == value)
    }
}

/// A pane in the split tree. Its id is a session id or a surface pane id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeafNode {
    pub id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A group of panes laid out along one axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitNode {
    pub id: String,
    pub dir: SplitDir,
    pub children: Vec<LayoutNode>,
    /// Fractions of the group, one per child, summing to 1.
    pub sizes: Vec<f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `LayoutNode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LayoutNode {
    #[serde(rename = "leaf")]
    Leaf(LeafNode),
    #[serde(rename = "split")]
    Split(SplitNode),
}

impl LayoutNode {
    /// The node's own id.
    pub fn id(&self) -> &str {
        match self {
            LayoutNode::Leaf(leaf) => &leaf.id,
            LayoutNode::Split(split) => &split.id,
        }
    }

    /// The leaf id when this node is a leaf.
    pub fn leaf_id(&self) -> Option<&str> {
        match self {
            LayoutNode::Leaf(leaf) => Some(&leaf.id),
            LayoutNode::Split(_) => None,
        }
    }

    /// `child.type === "leaf" && child.id === id`.
    pub fn is_leaf(&self, id: &str) -> bool {
        self.leaf_id() == Some(id)
    }

    /// A split node with no extra fields.
    pub fn split(
        id: impl Into<String>,
        dir: SplitDir,
        children: Vec<LayoutNode>,
        sizes: Vec<f64>,
    ) -> Self {
        LayoutNode::Split(SplitNode {
            id: id.into(),
            dir,
            children,
            sizes,
            extra: Extra::new(),
        })
    }
}

/// `PlanTabSource`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTabSource {
    pub session_id: String,
    pub block_id: String,
    pub title: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `ReleaseNotesTabSource` from src/app/model/releaseNotes.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotesTabSource {
    pub version: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ReleaseNotesTabSource {
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            extra: Extra::new(),
        }
    }
}

/// `CommitTabSource`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitTabSource {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl CommitTabSource {
    pub fn new(
        sha: impl Into<String>,
        short_sha: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            sha: sha.into(),
            short_sha: short_sha.into(),
            subject: subject.into(),
            extra: Extra::new(),
        }
    }
}

/// `AgentTabSource`: one orchestration worker, opened for inspection beside
/// its lead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabSource {
    pub session_id: String,
    pub lead_id: String,
    pub harness: HarnessId,
    #[serde(flatten)]
    pub extra: Extra,
}

impl AgentTabSource {
    pub fn new(
        session_id: impl Into<String>,
        lead_id: impl Into<String>,
        harness: HarnessId,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            lead_id: lead_id.into(),
            harness,
            extra: Extra::new(),
        }
    }
}

/// `SessionChangesSource`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChangesSource {
    pub session_id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl SessionChangesSource {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            extra: Extra::new(),
        }
    }
}

/// `GitFileDiffKind` from src/platform/tauri/fs.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitFileDiffKind {
    #[serde(rename = "staged")]
    Staged,
    #[serde(rename = "unstaged")]
    Unstaged,
}

/// `FilePaneTab`: one tab in an editor or terminal pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePaneTab {
    pub id: String,
    pub path: String,
    pub cwd: String,
    /// Owning project when cwd points at one of its linked worktrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanTabSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<ReleaseNotesTabSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<bool>,
    /// Single working-tree review of every changed file (unified diff).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<bool>,
    /// Which side of a staged or unstaged path was selected in source control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_kind: Option<GitFileDiffKind>,
    /// Read-only diff built from one session's captured before and after snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_changes: Option<SessionChangesSource>,
    /// Historical commit review (unified diff, read-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitTabSource>,
    /// Read-only transcript of an orchestration worker. Live only, not persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentTabSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<bool>,
    /// Foreground command when it isn't the shell. Live only, not persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    /// Temporary tab: the next preview open in its pane replaces it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl FilePaneTab {
    /// A tab with only the required fields set.
    pub fn new(id: impl Into<String>, path: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            cwd: cwd.into(),
            project_cwd: None,
            plan: None,
            release_notes: None,
            review: None,
            changes: None,
            change_kind: None,
            session_changes: None,
            commit: None,
            agent: None,
            terminal: None,
            foreground: None,
            preview: None,
            extra: Extra::new(),
        }
    }
}

/// `true` for `Some(true)`, the truthiness of an optional boolean.
fn flag(value: Option<bool>) -> bool {
    value == Some(true)
}

/// `EditorPane`: a tab strip of files or terminals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorPane {
    pub id: String,
    pub files: Vec<FilePaneTab>,
    pub active_file_id: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl EditorPane {
    pub fn new(
        id: impl Into<String>,
        files: Vec<FilePaneTab>,
        active_file_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            files,
            active_file_id: active_file_id.into(),
            extra: Extra::new(),
        }
    }
}

/// `SurfaceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceKind {
    #[serde(rename = "editor")]
    Editor,
    #[serde(rename = "terminal")]
    Terminal,
}

/// `WorkspaceTab["kind"]`, which is always `"session"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum WorkspaceTabKind {
    #[default]
    #[serde(rename = "session")]
    Session,
}

/// `WorkspaceTab`: one title-bar tab and its split tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    pub kind: WorkspaceTabKind,
    pub id: String,
    pub layout: LayoutNode,
    pub focused_id: String,
    #[serde(default)]
    pub editor_panes: Vec<EditorPane>,
    #[serde(default)]
    pub terminal_panes: Vec<EditorPane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_open: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_focused: Option<bool>,
    /// Explicit tab group; absent means ungrouped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl WorkspaceTab {
    /// A tab with no surface panes, flags, or group.
    pub fn new(id: impl Into<String>, layout: LayoutNode, focused_id: impl Into<String>) -> Self {
        Self {
            kind: WorkspaceTabKind::Session,
            id: id.into(),
            layout,
            focused_id: focused_id.into(),
            editor_panes: Vec::new(),
            terminal_panes: Vec::new(),
            diff_open: None,
            diff_focused: None,
            group_id: None,
            extra: Extra::new(),
        }
    }
}

/// `MIN_SIZE`: the smallest fraction a pane keeps when a sash moves.
pub const MIN_SIZE: f64 = 0.08;

/// `leaf`.
pub fn leaf(session_id: impl Into<String>) -> LayoutNode {
    LayoutNode::Leaf(LeafNode {
        id: session_id.into(),
        extra: Extra::new(),
    })
}

/// `newTab`.
pub fn new_tab(session_id: &str) -> WorkspaceTab {
    WorkspaceTab::new(random_uuid(), leaf(session_id), session_id)
}

/// `resetTabToSession`: keep the tab's identity and group; replace its
/// contents with one session leaf.
pub fn reset_tab_to_session(tab: &WorkspaceTab, session_id: &str) -> WorkspaceTab {
    WorkspaceTab {
        layout: leaf(session_id),
        focused_id: session_id.to_string(),
        editor_panes: Vec::new(),
        terminal_panes: Vec::new(),
        diff_open: Some(false),
        diff_focused: Some(false),
        ..tab.clone()
    }
}

/// `projectCwd && projectCwd !== cwd ? { projectCwd } : {}`.
fn distinct_project_cwd(project_cwd: Option<&str>, cwd: &str) -> Option<String> {
    project_cwd
        .filter(|project| !project.is_empty() && *project != cwd)
        .map(str::to_string)
}

/// `newFileTab`.
pub fn new_file_tab(
    path: &str,
    cwd: &str,
    review: bool,
    change_kind: Option<GitFileDiffKind>,
    project_cwd: Option<&str>,
) -> FilePaneTab {
    FilePaneTab {
        project_cwd: distinct_project_cwd(project_cwd, cwd),
        review: review.then_some(true),
        change_kind,
        ..FilePaneTab::new(random_uuid(), path, cwd)
    }
}

/// `focusPath || cwd`.
fn path_or_cwd(focus_path: Option<&str>, cwd: &str) -> String {
    focus_path
        .filter(|path| !path.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

/// `newChangesTab`.
pub fn new_changes_tab(
    cwd: &str,
    focus_path: Option<&str>,
    focus_kind: Option<GitFileDiffKind>,
    project_cwd: Option<&str>,
) -> FilePaneTab {
    FilePaneTab {
        project_cwd: distinct_project_cwd(project_cwd, cwd),
        review: Some(true),
        changes: Some(true),
        change_kind: focus_kind,
        ..FilePaneTab::new(random_uuid(), path_or_cwd(focus_path, cwd), cwd)
    }
}

/// `newSessionChangesTab`.
pub fn new_session_changes_tab(
    cwd: &str,
    session_id: &str,
    focus_path: Option<&str>,
    project_cwd: Option<&str>,
) -> FilePaneTab {
    FilePaneTab {
        project_cwd: distinct_project_cwd(project_cwd, cwd),
        review: Some(true),
        session_changes: Some(SessionChangesSource::new(session_id)),
        ..FilePaneTab::new(random_uuid(), path_or_cwd(focus_path, cwd), cwd)
    }
}

/// `newCommitTab`.
pub fn new_commit_tab(
    cwd: &str,
    commit: CommitTabSource,
    project_cwd: Option<&str>,
) -> FilePaneTab {
    let path = format!("commit:{}", commit.sha);
    FilePaneTab {
        project_cwd: distinct_project_cwd(project_cwd, cwd),
        commit: Some(commit),
        ..FilePaneTab::new(random_uuid(), path, cwd)
    }
}

/// `newPlanTab`.
pub fn new_plan_tab(session_id: &str, block_id: &str, title: &str, cwd: &str) -> FilePaneTab {
    FilePaneTab {
        plan: Some(PlanTabSource {
            session_id: session_id.to_string(),
            block_id: block_id.to_string(),
            title: title.to_string(),
            extra: Extra::new(),
        }),
        ..FilePaneTab::new(random_uuid(), format!("plan:{block_id}"), cwd)
    }
}

/// `newReleaseNotesWorkspaceTab`.
pub fn new_release_notes_workspace_tab(release_notes: ReleaseNotesTabSource) -> WorkspaceTab {
    let file = FilePaneTab {
        release_notes: Some(release_notes.clone()),
        ..FilePaneTab::new(
            random_uuid(),
            format!("release-notes:{}", release_notes.version),
            "~",
        )
    };
    let pane = new_editor_pane(file);
    let mut tab = WorkspaceTab::new(random_uuid(), leaf(pane.id.clone()), pane.id.clone());
    tab.editor_panes = vec![pane];
    tab
}

/// `newEditorWorkspaceTab`: a top-level workspace tab whose first and only
/// pane is this file.
pub fn new_editor_workspace_tab(file: FilePaneTab) -> WorkspaceTab {
    let pane = new_editor_pane(file);
    let mut tab = WorkspaceTab::new(random_uuid(), leaf(pane.id.clone()), pane.id.clone());
    tab.editor_panes = vec![pane];
    tab
}

/// `previewWorkspaceFile`: the file of a top-level tab that holds nothing
/// but one preview file.
pub fn preview_workspace_file(tab: &WorkspaceTab) -> Option<&FilePaneTab> {
    let pane = tab.editor_panes.first();
    if !pane.is_some_and(|pane| tab.layout.is_leaf(&pane.id))
        || tab.editor_panes.len() != 1
        || !tab.terminal_panes.is_empty()
        || pane.is_some_and(|pane| pane.files.len() != 1)
    {
        return None;
    }
    let file = &pane?.files[0];
    flag(file.preview).then_some(file)
}

/// What `openWorkspaceFile` decided.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenWorkspaceFile {
    pub tabs: Vec<WorkspaceTab>,
    pub tab_id: String,
    pub pane_id: Option<String>,
}

/// `openWorkspaceFile`. Workspace file-tab mode: focus the tab already
/// showing `file`, else reuse the project's preview tab, else insert
/// `created`. Pure over `tabs` so it runs inside a state updater and sees
/// opens that have not rendered yet.
pub fn open_workspace_file(
    tabs: &[WorkspaceTab],
    file: &FilePaneTab,
    created: WorkspaceTab,
    insert: impl FnOnce(&[WorkspaceTab], WorkspaceTab) -> Vec<WorkspaceTab>,
    pin: bool,
) -> OpenWorkspaceFile {
    let key = editor_tab_key(file);
    let project = path_key(file.project_cwd.as_deref().unwrap_or(&file.cwd));
    let existing = tabs.iter().enumerate().find_map(|(index, tab)| {
        tab.editor_panes
            .iter()
            .find(|pane| pane.files.iter().any(|open| editor_tab_key(open) == key))
            .map(|pane| (index, pane.id.clone()))
    });
    let hit = existing.as_ref().map(|(index, _)| *index).or_else(|| {
        if pin {
            return None;
        }
        tabs.iter().position(|tab| {
            preview_workspace_file(tab).is_some_and(|open| {
                path_key(open.project_cwd.as_deref().unwrap_or(&open.cwd)) == project
            })
        })
    });
    let Some(hit) = hit else {
        let tab_id = created.id.clone();
        return OpenWorkspaceFile {
            tabs: insert(tabs, created),
            tab_id,
            pane_id: None,
        };
    };
    let pane_id = existing
        .map(|(_, pane_id)| pane_id)
        .or_else(|| tabs[hit].editor_panes.first().map(|pane| pane.id.clone()));
    OpenWorkspaceFile {
        tabs: tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                if index == hit {
                    open_editor_tab(
                        tab,
                        file,
                        &OpenEditorTabOptions {
                            pin,
                            ..Default::default()
                        },
                    )
                } else {
                    tab.clone()
                }
            })
            .collect(),
        tab_id: tabs[hit].id.clone(),
        pane_id,
    }
}

/// `newAgentTab`. `path` carries the label: an agent tab has no file behind it.
pub fn new_agent_tab(title: &str, cwd: &str, agent: AgentTabSource) -> FilePaneTab {
    FilePaneTab {
        agent: Some(agent),
        ..FilePaneTab::new(random_uuid(), title, cwd)
    }
}

/// `newTerminalFile`.
pub fn new_terminal_file(cwd: &str, title: Option<&str>, project_cwd: Option<&str>) -> FilePaneTab {
    let path = title
        .map(str::to_string)
        .unwrap_or_else(|| default_terminal_title(cwd));
    FilePaneTab {
        project_cwd: distinct_project_cwd(project_cwd, cwd),
        terminal: Some(true),
        ..FilePaneTab::new(random_uuid(), path, cwd)
    }
}

/// `newTerminalWorkspaceTab`.
pub fn new_terminal_workspace_tab(file: FilePaneTab) -> WorkspaceTab {
    let pane = new_editor_pane(file);
    let mut tab = WorkspaceTab::new(random_uuid(), leaf(pane.id.clone()), pane.id.clone());
    tab.terminal_panes = vec![pane];
    tab
}

/// `nextTerminalTitleFromFiles`: the directory name, then `name 2`, `name 3`.
pub fn next_terminal_title_from_files<'a>(
    files: impl IntoIterator<Item = &'a FilePaneTab>,
    cwd: &str,
) -> String {
    let base = default_terminal_title(cwd);
    let taken: std::collections::HashSet<&str> = files
        .into_iter()
        .filter(|file| is_terminal_tab(file))
        .map(|file| file.path.as_str())
        .collect();
    if !taken.contains(base.as_str()) {
        return base;
    }
    let mut index = 2;
    while taken.contains(format!("{base} {index}").as_str()) {
        index += 1;
    }
    format!("{base} {index}")
}

/// `nextTerminalTitle`.
pub fn next_terminal_title(tab: &WorkspaceTab, cwd: &str) -> String {
    next_terminal_title_from_files(tab.terminal_panes.iter().flat_map(|pane| &pane.files), cwd)
}

/// `updateTerminalTab`: apply a PTY patch to the matching terminal.
pub fn update_terminal_tab(
    tab: &WorkspaceTab,
    file_id: &str,
    patch: &TerminalMetaPatch,
) -> WorkspaceTab {
    let mut changed = false;
    let terminal_panes: Vec<EditorPane> = tab
        .terminal_panes
        .iter()
        .map(|pane| EditorPane {
            files: pane
                .files
                .iter()
                .map(|file| {
                    if file.terminal != Some(true) || file.id != file_id {
                        return file.clone();
                    }
                    let next = apply_terminal_meta(file, patch);
                    if next != *file {
                        changed = true;
                    }
                    next
                })
                .collect(),
            ..pane.clone()
        })
        .collect();
    if !changed {
        return tab.clone();
    }
    with_surface_panes(tab, SurfaceKind::Terminal, terminal_panes)
}

/// `surfacePanes`.
pub fn surface_panes(tab: &WorkspaceTab, kind: SurfaceKind) -> &[EditorPane] {
    match kind {
        SurfaceKind::Editor => &tab.editor_panes,
        SurfaceKind::Terminal => &tab.terminal_panes,
    }
}

/// `withSurfacePanes`.
pub fn with_surface_panes(
    tab: &WorkspaceTab,
    kind: SurfaceKind,
    panes: Vec<EditorPane>,
) -> WorkspaceTab {
    let mut next = tab.clone();
    match kind {
        SurfaceKind::Editor => next.editor_panes = panes,
        SurfaceKind::Terminal => next.terminal_panes = panes,
    }
    next
}

/// `findSurfacePane`.
pub fn find_surface_pane<'a>(
    tab: &'a WorkspaceTab,
    pane_id: &str,
) -> Option<(SurfaceKind, &'a EditorPane)> {
    if let Some(editor) = tab.editor_panes.iter().find(|pane| pane.id == pane_id) {
        return Some((SurfaceKind::Editor, editor));
    }
    tab.terminal_panes
        .iter()
        .find(|pane| pane.id == pane_id)
        .map(|pane| (SurfaceKind::Terminal, pane))
}

/// `isPlanTab`.
pub fn is_plan_tab(file: &FilePaneTab) -> bool {
    file.plan.is_some()
}

/// `isReleaseNotesTab`.
pub fn is_release_notes_tab(file: &FilePaneTab) -> bool {
    file.release_notes.is_some()
}

/// `isCommitTab`.
pub fn is_commit_tab(file: &FilePaneTab) -> bool {
    file.commit.is_some()
}

/// `isTerminalTab`.
pub fn is_terminal_tab(file: &FilePaneTab) -> bool {
    flag(file.terminal)
}

/// `isAgentTab`.
pub fn is_agent_tab(file: &FilePaneTab) -> bool {
    file.agent.is_some()
}

/// `isVirtualDocumentTab`.
pub fn is_virtual_document_tab(file: &FilePaneTab) -> bool {
    is_plan_tab(file) || is_release_notes_tab(file) || is_commit_tab(file) || is_agent_tab(file)
}

/// `isFilesystemTab`.
pub fn is_filesystem_tab(file: &FilePaneTab) -> bool {
    !is_terminal_tab(file) && !is_virtual_document_tab(file) && file.session_changes.is_none()
}

/// `focusedFileTab`.
pub fn focused_file_tab(tab: &WorkspaceTab) -> Option<&FilePaneTab> {
    let pane = tab
        .editor_panes
        .iter()
        .find(|entry| entry.id == tab.focused_id)
        .or_else(|| {
            tab.terminal_panes
                .iter()
                .find(|entry| entry.id == tab.focused_id)
        })?;
    pane.files
        .iter()
        .find(|file| file.id == pane.active_file_id)
}

/// `isolateTerminalPanes`: move terminal tabs out of file panes so the two
/// never share a tab strip.
pub fn isolate_terminal_panes(tab: &WorkspaceTab) -> WorkspaceTab {
    let mixed = tab
        .editor_panes
        .iter()
        .any(|pane| pane.files.iter().any(is_terminal_tab));
    if !mixed {
        return tab.clone();
    }

    let mut layout = tab.layout.clone();
    let mut focused_id = tab.focused_id.clone();
    let mut editor_panes = Vec::new();
    let mut terminal_panes = tab.terminal_panes.clone();

    for pane in &tab.editor_panes {
        let files: Vec<FilePaneTab> = pane
            .files
            .iter()
            .filter(|file| !is_terminal_tab(file))
            .cloned()
            .collect();
        let terminals: Vec<FilePaneTab> = pane
            .files
            .iter()
            .filter(|file| is_terminal_tab(file))
            .cloned()
            .collect();
        if !files.is_empty() {
            let active_file_id = if files.iter().any(|file| file.id == pane.active_file_id) {
                pane.active_file_id.clone()
            } else {
                files[0].id.clone()
            };
            editor_panes.push(EditorPane {
                files: files.clone(),
                active_file_id,
                ..pane.clone()
            });
        }
        if terminals.is_empty() {
            continue;
        }
        if files.is_empty() {
            terminal_panes.push(EditorPane {
                files: terminals,
                ..pane.clone()
            });
            continue;
        }
        let active_file_id = if terminals.iter().any(|file| file.id == pane.active_file_id) {
            pane.active_file_id.clone()
        } else {
            terminals[0].id.clone()
        };
        let split = EditorPane::new(random_uuid(), terminals, active_file_id);
        layout = split_pane(&layout, &pane.id, SplitDir::Down, &split.id);
        if pane.id == tab.focused_id && split.active_file_id == pane.active_file_id {
            focused_id = split.id.clone();
        }
        terminal_panes.push(split);
    }

    WorkspaceTab {
        layout,
        focused_id,
        editor_panes,
        terminal_panes,
        ..tab.clone()
    }
}

/// `isReviewTab`.
pub fn is_review_tab(file: &FilePaneTab) -> bool {
    flag(file.review) && !is_virtual_document_tab(file)
}

/// `isChangesTab`.
pub fn is_changes_tab(file: &FilePaneTab) -> bool {
    flag(file.changes) && is_review_tab(file)
}

/// `isSessionChangesTab`.
pub fn is_session_changes_tab(file: &FilePaneTab) -> bool {
    file.session_changes.is_some() && is_review_tab(file)
}

/// `editorTabKey`: tabs with the same key are the same document.
pub fn editor_tab_key(file: &FilePaneTab) -> String {
    if flag(file.terminal) {
        return format!("terminal:{}", file.id);
    }
    if let Some(agent) = &file.agent {
        return format!("agent:{}", agent.session_id);
    }
    if let Some(plan) = &file.plan {
        return format!("plan:{}", plan.block_id);
    }
    if let Some(release_notes) = &file.release_notes {
        return format!("release-notes:{}", release_notes.version);
    }
    if let Some(commit) = &file.commit {
        return format!("commit:{}:{}", file.cwd, commit.sha);
    }
    if let Some(session_changes) = &file.session_changes {
        return format!(
            "session-changes:{}:{}",
            file.cwd, session_changes.session_id
        );
    }
    if flag(file.changes) {
        return format!("changes:{}", file.cwd);
    }
    if flag(file.review) {
        format!("review:{}", file.path)
    } else {
        format!("file:{}", file.path)
    }
}

/// `newEditorPane`.
pub fn new_editor_pane(file: FilePaneTab) -> EditorPane {
    let active_file_id = file.id.clone();
    EditorPane::new(random_uuid(), vec![file], active_file_id)
}

/// Which side of the focused non-editor pane receives a new editor pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EditorSplitSide {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
}

/// `OpenEditorTabOptions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenEditorTabOptions {
    /// Which side of the focused non-editor pane receives a new editor pane.
    pub split: Option<EditorSplitSide>,
    /// Open (or promote an existing tab) as permanent instead of preview.
    pub pin: bool,
}

/// `isPreviewableTab`: tabs opened by browsing lists (files, per-file diffs,
/// commits, session diffs).
pub fn is_previewable_tab(file: &FilePaneTab) -> bool {
    !flag(file.terminal)
        && file.agent.is_none()
        && file.plan.is_none()
        && file.release_notes.is_none()
        && !flag(file.changes)
}

/// `withoutPreview`.
fn without_preview(file: &FilePaneTab) -> FilePaneTab {
    let mut next = file.clone();
    if flag(file.preview) {
        next.preview = None;
    }
    next
}

/// `pinEditorFile`: make a preview tab permanent. Returns an equal copy of
/// `tab` when nothing matched.
pub fn pin_editor_file(tab: &WorkspaceTab, file_id: &str) -> WorkspaceTab {
    let mut next = tab.clone();
    for pane in &mut next.editor_panes {
        if let Some(file) = pane.files.iter_mut().find(|entry| entry.id == file_id)
            && flag(file.preview)
        {
            *file = without_preview(file);
        }
    }
    next
}

/// `openEditorTab`: focus an existing editor tab, or open it in the focused
/// editor pane or a new split.
pub fn open_editor_tab(
    tab: &WorkspaceTab,
    file: &FilePaneTab,
    options: &OpenEditorTabOptions,
) -> WorkspaceTab {
    if flag(file.terminal) {
        return open_terminal_tab(tab, file, None);
    }
    let tab = isolate_terminal_panes(tab);

    let key = editor_tab_key(file);
    let existing = tab
        .editor_panes
        .iter()
        .enumerate()
        .find_map(|(pane_index, pane)| {
            pane.files
                .iter()
                .position(|entry| editor_tab_key(entry) == key)
                .map(|file_index| (pane_index, file_index))
        });
    if let Some((pane_index, file_index)) = existing {
        let mut next = tab.clone();
        next.focused_id = tab.editor_panes[pane_index].id.clone();
        next.diff_focused = Some(false);
        let pane = &mut next.editor_panes[pane_index];
        let entry = &mut pane.files[file_index];
        if options.pin {
            *entry = without_preview(entry);
            if flag(file.review) {
                entry.change_kind = file.change_kind;
            }
        } else if flag(file.review) {
            entry.change_kind = file.change_kind;
        }
        pane.active_file_id = entry.id.clone();
        return next;
    }

    let file = if is_previewable_tab(file) && !options.pin {
        FilePaneTab {
            preview: Some(true),
            ..file.clone()
        }
    } else {
        without_preview(file)
    };
    let target = tab
        .editor_panes
        .iter()
        .position(|pane| pane.id == tab.focused_id)
        .or_else(|| (!tab.editor_panes.is_empty()).then_some(0));
    if let Some(target) = target {
        let mut next = tab.clone();
        next.focused_id = tab.editor_panes[target].id.clone();
        next.diff_focused = Some(false);
        let pane = &mut next.editor_panes[target];
        let preview_index = if flag(file.preview) {
            pane.files.iter().position(|entry| flag(entry.preview))
        } else {
            None
        };
        pane.active_file_id = file.id.clone();
        match preview_index {
            Some(index) => pane.files[index] = file,
            None => pane.files.push(file),
        }
        return next;
    }

    let editor_pane = new_editor_pane(file);
    WorkspaceTab {
        layout: split_pane_relative(
            &tab.layout,
            &tab.focused_id,
            SplitDir::Right,
            &editor_pane.id,
            options.split == Some(EditorSplitSide::Left),
        ),
        focused_id: editor_pane.id.clone(),
        diff_focused: Some(false),
        editor_panes: vec![editor_pane],
        ..tab
    }
}

/// `openChangesTab`: focus this working copy's Changes tab, creating it if needed.
pub fn open_changes_tab(
    tab: &WorkspaceTab,
    cwd: &str,
    focus_path: Option<&str>,
    focus_kind: Option<GitFileDiffKind>,
    project_cwd: Option<&str>,
) -> WorkspaceTab {
    let tab = isolate_terminal_panes(tab);
    let next_file = new_changes_tab(cwd, focus_path, focus_kind, project_cwd);
    let next_key = editor_tab_key(&next_file);
    let matches = |file: &FilePaneTab| editor_tab_key(file) == next_key;
    let existing = tab
        .editor_panes
        .iter()
        .enumerate()
        .find_map(|(pane_index, pane)| {
            pane.files
                .iter()
                .find(|file| matches(file))
                .map(|file| (pane_index, file.clone()))
        });

    if let Some((pane_index, existing_file)) = existing {
        // The reused tab takes the section it was opened from; no kind
        // shows every change.
        let mut updated = FilePaneTab {
            change_kind: focus_kind,
            ..existing_file
        };
        if let Some(path) = focus_path.filter(|path| !path.is_empty()) {
            updated.path = path.to_string();
        }
        let mut next = tab.clone();
        next.focused_id = tab.editor_panes[pane_index].id.clone();
        next.diff_focused = Some(false);
        let pane = &mut next.editor_panes[pane_index];
        pane.files = drop_per_file_review_tabs(&pane.files, cwd)
            .into_iter()
            .map(|file| {
                if file.id == updated.id {
                    updated.clone()
                } else {
                    file
                }
            })
            .collect();
        pane.active_file_id = updated.id.clone();
        return next;
    }

    let mut opened = open_editor_tab(&tab, &next_file, &OpenEditorTabOptions::default());
    for pane in &mut opened.editor_panes {
        if !pane.files.iter().any(&matches) {
            continue;
        }
        let active = pane
            .files
            .iter()
            .find(|file| matches(file))
            .map(|file| file.id.clone())
            .unwrap_or_else(|| pane.active_file_id.clone());
        pane.files = drop_per_file_review_tabs(&pane.files, cwd);
        pane.active_file_id = active;
    }
    opened
}

/// `openSessionChangesTab`: focus one session's exact captured diff,
/// creating a tab if needed.
pub fn open_session_changes_tab(
    tab: &WorkspaceTab,
    cwd: &str,
    session_id: &str,
    focus_path: Option<&str>,
    project_cwd: Option<&str>,
    pin: bool,
) -> WorkspaceTab {
    let next_file = new_session_changes_tab(cwd, session_id, focus_path, project_cwd);
    let key = editor_tab_key(&next_file);
    let existing = tab
        .editor_panes
        .iter()
        .enumerate()
        .find_map(|(pane_index, pane)| {
            pane.files
                .iter()
                .find(|file| editor_tab_key(file) == key)
                .map(|file| (pane_index, file.clone()))
        });
    let Some((pane_index, existing_file)) = existing else {
        return open_editor_tab(
            tab,
            &next_file,
            &OpenEditorTabOptions {
                pin,
                ..Default::default()
            },
        );
    };

    let focused = match focus_path.filter(|path| !path.is_empty()) {
        Some(path) => FilePaneTab {
            path: path.to_string(),
            ..existing_file
        },
        None => existing_file,
    };
    let updated = if pin {
        without_preview(&focused)
    } else {
        focused
    };
    let mut next = tab.clone();
    next.focused_id = tab.editor_panes[pane_index].id.clone();
    next.diff_focused = Some(false);
    let pane = &mut next.editor_panes[pane_index];
    for file in &mut pane.files {
        if file.id == updated.id {
            *file = updated.clone();
        }
    }
    pane.active_file_id = updated.id;
    next
}

/// `openCommitTab`: focus a historical commit's unified diff, creating the
/// tab if needed.
pub fn open_commit_tab(
    tab: &WorkspaceTab,
    cwd: &str,
    commit: CommitTabSource,
    project_cwd: Option<&str>,
    pin: bool,
) -> WorkspaceTab {
    open_editor_tab(
        tab,
        &new_commit_tab(cwd, commit, project_cwd),
        &OpenEditorTabOptions {
            pin,
            ..Default::default()
        },
    )
}

/// `dropPerFileReviewTabs`.
fn drop_per_file_review_tabs(files: &[FilePaneTab], cwd: &str) -> Vec<FilePaneTab> {
    files
        .iter()
        .filter(|file| {
            file.cwd != cwd
                || !is_review_tab(file)
                || is_changes_tab(file)
                || is_session_changes_tab(file)
        })
        .cloned()
        .collect()
}

/// `openTerminalTab`: open a terminal in its own pane. Files never share
/// this tab strip.
pub fn open_terminal_tab(
    tab: &WorkspaceTab,
    file: &FilePaneTab,
    occupy_pane_id: Option<&str>,
) -> WorkspaceTab {
    let tab = isolate_terminal_panes(tab);
    let occupy = occupy_pane_id.filter(|id| {
        !id.is_empty()
            && !tab.editor_panes.iter().any(|pane| pane.id == *id)
            && !tab.terminal_panes.iter().any(|pane| pane.id == *id)
    });
    if let Some(occupy) = occupy {
        let pane = new_editor_pane(file.clone());
        let mut next = tab.clone();
        next.layout = replace_leaf_id(&tab.layout, occupy, &pane.id);
        next.focused_id = pane.id.clone();
        next.diff_focused = Some(false);
        next.terminal_panes.push(pane);
        return next;
    }

    let target = tab
        .terminal_panes
        .iter()
        .position(|pane| pane.id == tab.focused_id)
        .or_else(|| (!tab.terminal_panes.is_empty()).then_some(0));
    if let Some(target) = target {
        let mut next = tab.clone();
        next.focused_id = tab.terminal_panes[target].id.clone();
        next.diff_focused = Some(false);
        let pane = &mut next.terminal_panes[target];
        pane.files.push(file.clone());
        pane.active_file_id = file.id.clone();
        return next;
    }

    let pane = new_editor_pane(file.clone());
    WorkspaceTab {
        layout: split_pane(&tab.layout, &tab.focused_id, SplitDir::Down, &pane.id),
        focused_id: pane.id.clone(),
        diff_focused: Some(false),
        terminal_panes: vec![pane],
        ..tab
    }
}

/// `equalSizes`.
fn equal_sizes(n: usize) -> Vec<f64> {
    vec![1.0 / n as f64; n]
}

/// `normalize`.
fn normalize(sizes: &[f64]) -> Vec<f64> {
    let total: f64 = sizes.iter().fold(0.0, |sum, n| sum + n);
    if total <= 0.0 {
        return equal_sizes(sizes.len());
    }
    sizes.iter().map(|n| n / total).collect()
}

/// `splitPane`: split the leaf `focused_id` and put `new_session_id` after it.
pub fn split_pane(
    node: &LayoutNode,
    focused_id: &str,
    dir: SplitDir,
    new_session_id: &str,
) -> LayoutNode {
    split_pane_relative(node, focused_id, dir, new_session_id, false)
}

/// `splitPaneRelative`.
fn split_pane_relative(
    node: &LayoutNode,
    focused_id: &str,
    dir: SplitDir,
    new_session_id: &str,
    before: bool,
) -> LayoutNode {
    let wrap = |child: &LayoutNode| {
        let children = if before {
            vec![leaf(new_session_id), child.clone()]
        } else {
            vec![child.clone(), leaf(new_session_id)]
        };
        LayoutNode::split(random_uuid(), dir, children, vec![0.5, 0.5])
    };
    let split = match node {
        LayoutNode::Leaf(leaf_node) => {
            if leaf_node.id != focused_id {
                return node.clone();
            }
            return wrap(node);
        }
        LayoutNode::Split(split) => split,
    };

    let direct = split
        .children
        .iter()
        .position(|child| child.is_leaf(focused_id));
    if let Some(direct) = direct {
        if split.dir == dir {
            let insert_at = if before { direct } else { direct + 1 };
            let mut children = split.children.clone();
            children.insert(insert_at, leaf(new_session_id));
            let sizes = equal_sizes(children.len());
            return LayoutNode::Split(SplitNode {
                children,
                sizes,
                ..split.clone()
            });
        }
        return LayoutNode::Split(SplitNode {
            children: split
                .children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    if i == direct {
                        wrap(child)
                    } else {
                        child.clone()
                    }
                })
                .collect(),
            ..split.clone()
        });
    }

    LayoutNode::Split(SplitNode {
        children: split
            .children
            .iter()
            .map(|child| split_pane_relative(child, focused_id, dir, new_session_id, before))
            .collect(),
        ..split.clone()
    })
}

/// `replaceLeafId`: swap one leaf id for another, keeping the split tree intact.
pub fn replace_leaf_id(node: &LayoutNode, from_id: &str, to_id: &str) -> LayoutNode {
    if from_id == to_id {
        return node.clone();
    }
    match node {
        LayoutNode::Leaf(leaf_node) => {
            if leaf_node.id == from_id {
                leaf(to_id)
            } else {
                node.clone()
            }
        }
        LayoutNode::Split(split) => LayoutNode::Split(SplitNode {
            children: split
                .children
                .iter()
                .map(|child| replace_leaf_id(child, from_id, to_id))
                .collect(),
            ..split.clone()
        }),
    }
}

/// `removePane`: drop a leaf. Parent splits collapse to the remaining child.
pub fn remove_pane(node: &LayoutNode, session_id: &str) -> Option<LayoutNode> {
    let split = match node {
        LayoutNode::Leaf(leaf_node) => {
            return (leaf_node.id != session_id).then(|| node.clone());
        }
        LayoutNode::Split(split) => split,
    };
    let mut kept: Vec<(LayoutNode, f64)> = Vec::new();
    for (i, child) in split.children.iter().enumerate() {
        if let Some(child) = remove_pane(child, session_id) {
            kept.push((child, split.sizes.get(i).copied().unwrap_or(0.0)));
        }
    }
    match kept.len() {
        0 => None,
        1 => kept.pop().map(|(child, _)| child),
        _ => {
            let sizes: Vec<f64> = kept.iter().map(|(_, size)| *size).collect();
            Some(LayoutNode::Split(SplitNode {
                children: kept.into_iter().map(|(child, _)| child).collect(),
                sizes: normalize(&sizes),
                ..split.clone()
            }))
        }
    }
}

/// `closeLeaf`: close one pane in a tab. Remaining chats, files, and
/// terminals stay; returns `None` only when this was the last leaf.
pub fn close_leaf(tab: &WorkspaceTab, leaf_id: &str) -> Option<WorkspaceTab> {
    let next_layout = remove_pane(&tab.layout, leaf_id)?;
    let next_focus = if tab.focused_id == leaf_id {
        sibling_leaf_id(&tab.layout, leaf_id)
            .unwrap_or_else(|| first_leaf_id(&next_layout).to_string())
    } else {
        tab.focused_id.clone()
    };
    Some(WorkspaceTab {
        layout: next_layout,
        focused_id: next_focus,
        ..tab.clone()
    })
}

/// `closeSurfacePanes`: close every pane of one surface kind. Returns `None`
/// only when nothing remains.
pub fn close_surface_panes(tab: &WorkspaceTab, kind: SurfaceKind) -> Option<WorkspaceTab> {
    let mut remaining = Some(tab.clone());
    for pane in surface_panes(tab, kind) {
        remaining = remaining.and_then(|acc| close_leaf(&acc, &pane.id));
    }
    remaining.map(|remaining| with_surface_panes(&remaining, kind, Vec::new()))
}

/// `setSplitRatio`: move the sash between `index` and `index + 1` to
/// `boundary` (0 to 1 of the group).
pub fn set_split_ratio(
    node: &LayoutNode,
    split_id: &str,
    index: usize,
    boundary: f64,
) -> LayoutNode {
    let split = match node {
        LayoutNode::Leaf(_) => return node.clone(),
        LayoutNode::Split(split) => split,
    };
    if split.id != split_id {
        return LayoutNode::Split(SplitNode {
            children: split
                .children
                .iter()
                .map(|child| set_split_ratio(child, split_id, index, boundary))
                .collect(),
            ..split.clone()
        });
    }
    if split.sizes.is_empty() || index >= split.sizes.len() - 1 {
        return node.clone();
    }
    LayoutNode::Split(SplitNode {
        sizes: split_sizes_at_boundary(&split.sizes, index, boundary),
        ..split.clone()
    })
}

/// `splitSizesAtBoundary`: move only the two panes beside the sash, keeping
/// their total and the `MIN_SIZE` floor.
pub fn split_sizes_at_boundary(current: &[f64], index: usize, boundary: f64) -> Vec<f64> {
    if current.is_empty() || index >= current.len() - 1 {
        return current.to_vec();
    }
    let mut sizes = current.to_vec();
    let before: f64 = sizes[..index].iter().fold(0.0, |sum, n| sum + n);
    let pair = sizes[index] + sizes[index + 1];
    let min = js::min(MIN_SIZE, pair / 2.0);
    let first = js::min(pair - min, js::max(min, boundary - before));
    sizes[index] = first;
    sizes[index + 1] = pair - first;
    sizes
}

/// `leafIds`.
pub fn leaf_ids(node: &LayoutNode) -> Vec<String> {
    let mut out = Vec::new();
    collect_leaf_ids(node, &mut out);
    out
}

fn collect_leaf_ids(node: &LayoutNode, out: &mut Vec<String>) {
    match node {
        LayoutNode::Leaf(leaf_node) => out.push(leaf_node.id.clone()),
        LayoutNode::Split(split) => {
            for child in &split.children {
                collect_leaf_ids(child, out);
            }
        }
    }
}

/// `leafIds(node).includes(id)`.
pub fn has_leaf(node: &LayoutNode, id: &str) -> bool {
    match node {
        LayoutNode::Leaf(leaf_node) => leaf_node.id == id,
        LayoutNode::Split(split) => split.children.iter().any(|child| has_leaf(child, id)),
    }
}

/// `firstLeafId`. A split with no children has no leaf; it returns its own
/// id, where the TypeScript would throw.
pub fn first_leaf_id(node: &LayoutNode) -> &str {
    match node {
        LayoutNode::Leaf(leaf_node) => &leaf_node.id,
        LayoutNode::Split(split) => split
            .children
            .first()
            .map(first_leaf_id)
            .unwrap_or(&split.id),
    }
}

/// `LayoutRect`: fractions of the tab.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl LayoutRect {
    /// `{ x: 0, y: 0, w: 1, h: 1 }`.
    pub const FULL: LayoutRect = LayoutRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
}

/// `LayoutLeaf["axis"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis {
    #[serde(rename = "x")]
    X,
    #[serde(rename = "y")]
    Y,
}

/// `LayoutLeaf`.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLeaf {
    pub id: String,
    pub rect: LayoutRect,
    pub axis: Axis,
}

/// `LayoutSash`.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSash {
    pub split_id: String,
    pub index: usize,
    pub dir: SplitDir,
    pub group: LayoutRect,
    pub sizes: Vec<f64>,
}

/// The rect of child `i` in a split laid out over `rect`.
fn child_rect(row: bool, rect: LayoutRect, offset: f64, size: f64) -> LayoutRect {
    if row {
        LayoutRect {
            x: rect.x + offset * rect.w,
            y: rect.y,
            w: size * rect.w,
            h: rect.h,
        }
    } else {
        LayoutRect {
            x: rect.x,
            y: rect.y + offset * rect.h,
            w: rect.w,
            h: size * rect.h,
        }
    }
}

/// `layoutLeaves` over the whole tab.
pub fn layout_leaves(node: &LayoutNode) -> Vec<LayoutLeaf> {
    layout_leaves_in(node, LayoutRect::FULL, None)
}

/// `layoutLeaves(node, rect, parentDir)`.
pub fn layout_leaves_in(
    node: &LayoutNode,
    rect: LayoutRect,
    parent_dir: Option<SplitDir>,
) -> Vec<LayoutLeaf> {
    let axis = if parent_dir == Some(SplitDir::Down) {
        Axis::Y
    } else {
        Axis::X
    };
    let split = match node {
        LayoutNode::Leaf(leaf_node) => {
            return vec![LayoutLeaf {
                id: leaf_node.id.clone(),
                rect,
                axis,
            }];
        }
        LayoutNode::Split(split) => split,
    };
    let row = split.dir == SplitDir::Right;
    let mut offset = 0.0;
    let mut out = Vec::new();
    for (i, child) in split.children.iter().enumerate() {
        let size = split.sizes.get(i).copied().unwrap_or(0.0);
        let child_rect = child_rect(row, rect, offset, size);
        offset += size;
        out.extend(layout_leaves_in(child, child_rect, Some(split.dir)));
    }
    out
}

/// `layoutSashes` over the whole tab.
pub fn layout_sashes(node: &LayoutNode) -> Vec<LayoutSash> {
    layout_sashes_in(node, LayoutRect::FULL)
}

/// `layoutSashes(node, rect)`.
pub fn layout_sashes_in(node: &LayoutNode, rect: LayoutRect) -> Vec<LayoutSash> {
    let LayoutNode::Split(split) = node else {
        return Vec::new();
    };
    let row = split.dir == SplitDir::Right;
    let mut offset = 0.0;
    let mut out = Vec::new();
    for (i, child) in split.children.iter().enumerate() {
        let size = split.sizes.get(i).copied().unwrap_or(0.0);
        if i > 0 {
            out.push(LayoutSash {
                split_id: split.id.clone(),
                index: i - 1,
                dir: split.dir,
                group: rect,
                sizes: split.sizes.clone(),
            });
        }
        let child_rect = child_rect(row, rect, offset, size);
        offset += size;
        out.extend(layout_sashes_in(child, child_rect));
    }
    out
}

/// `rangeOverlap`.
fn range_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    js::max(0.0, js::min(a1, b1) - js::max(a0, b0))
}

/// `neighborLeafId`: the adjacent leaf in `dir`, preferring panes that
/// share an edge.
pub fn neighbor_leaf_id(node: &LayoutNode, focused_id: &str, dir: FocusDir) -> Option<String> {
    let panes = layout_leaves(node);
    let current = panes.iter().find(|pane| pane.id == focused_id)?;
    let c = current.rect;

    struct Best<'a> {
        id: &'a str,
        hit: u8,
        gap: f64,
        overlap: f64,
    }
    let mut best: Option<Best> = None;
    for pane in &panes {
        if pane.id == focused_id {
            continue;
        }
        let r = pane.rect;
        let (gap, perp) = match dir {
            FocusDir::Left if r.x + r.w <= c.x + 1e-6 => (
                c.x - (r.x + r.w),
                range_overlap(c.y, c.y + c.h, r.y, r.y + r.h),
            ),
            FocusDir::Right if r.x >= c.x + c.w - 1e-6 => (
                r.x - (c.x + c.w),
                range_overlap(c.y, c.y + c.h, r.y, r.y + r.h),
            ),
            FocusDir::Up if r.y + r.h <= c.y + 1e-6 => (
                c.y - (r.y + r.h),
                range_overlap(c.x, c.x + c.w, r.x, r.x + r.w),
            ),
            FocusDir::Down if r.y >= c.y + c.h - 1e-6 => (
                r.y - (c.y + c.h),
                range_overlap(c.x, c.x + c.w, r.x, r.x + r.w),
            ),
            _ => continue,
        };
        let hit = if perp > 0.0 { 0 } else { 1 };
        let better = match &best {
            None => true,
            Some(best) => {
                hit < best.hit
                    || (hit == best.hit && gap < best.gap)
                    || (hit == best.hit && gap == best.gap && perp > best.overlap)
            }
        };
        if better {
            best = Some(Best {
                id: &pane.id,
                hit,
                gap,
                overlap: perp,
            });
        }
    }
    best.map(|best| best.id.to_string())
}

/// `siblingLeafId`: the session to focus after closing `session_id`, a
/// neighbor's first leaf.
pub fn sibling_leaf_id(node: &LayoutNode, session_id: &str) -> Option<String> {
    let LayoutNode::Split(split) = node else {
        return None;
    };
    if let Some(index) = split
        .children
        .iter()
        .position(|child| child.is_leaf(session_id))
    {
        let neighbor = index
            .checked_sub(1)
            .and_then(|before| split.children.get(before))
            .or_else(|| split.children.get(index + 1));
        return neighbor.map(|neighbor| first_leaf_id(neighbor).to_string());
    }
    split
        .children
        .iter()
        .find_map(|child| sibling_leaf_id(child, session_id))
}

/// `PanePlace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanePlace {
    #[serde(rename = "before")]
    Before,
    #[serde(rename = "after")]
    After,
}

/// `PaneEdge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaneEdge {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "top")]
    Top,
    #[serde(rename = "bottom")]
    Bottom,
}

/// A pane's bounds on screen, as `getBoundingClientRect` reported them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// `paneEdgeFromPoint`: the pane edge nearest to a pointer.
pub fn pane_edge_from_point(x: f64, y: f64, rect: PaneRect) -> PaneEdge {
    let nx = if rect.width <= 0.0 {
        0.0
    } else {
        (x - rect.left) / rect.width - 0.5
    };
    let ny = if rect.height <= 0.0 {
        0.0
    } else {
        (y - rect.top) / rect.height - 0.5
    };
    if nx.abs() > ny.abs() {
        return if nx < 0.0 {
            PaneEdge::Left
        } else {
            PaneEdge::Right
        };
    }
    if ny < 0.0 {
        PaneEdge::Top
    } else {
        PaneEdge::Bottom
    }
}

/// `edgeSplit`.
fn edge_split(edge: PaneEdge) -> (SplitDir, PanePlace) {
    match edge {
        PaneEdge::Left => (SplitDir::Right, PanePlace::Before),
        PaneEdge::Right => (SplitDir::Right, PanePlace::After),
        PaneEdge::Top => (SplitDir::Down, PanePlace::Before),
        PaneEdge::Bottom => (SplitDir::Down, PanePlace::After),
    }
}

/// `leafParent`.
struct LeafParent {
    parent_id: String,
    index: usize,
    dir: SplitDir,
}

fn leaf_parent(node: &LayoutNode, leaf_id: &str) -> Option<LeafParent> {
    let LayoutNode::Split(split) = node else {
        return None;
    };
    for (i, child) in split.children.iter().enumerate() {
        if child.is_leaf(leaf_id) {
            return Some(LeafParent {
                parent_id: split.id.clone(),
                index: i,
                dir: split.dir,
            });
        }
        if let Some(found) = leaf_parent(child, leaf_id) {
            return Some(found);
        }
    }
    None
}

/// `reorderChild`.
fn reorder_child(
    split: &SplitNode,
    from_index: usize,
    to_index: usize,
    place: PanePlace,
) -> SplitNode {
    let n = split.children.len();
    if from_index >= n || to_index >= n {
        return split.clone();
    }
    let mut insert_at = if place == PanePlace::After {
        to_index + 1
    } else {
        to_index
    };
    insert_at = insert_at.min(n);
    if from_index < insert_at {
        insert_at -= 1;
    }
    if from_index == insert_at {
        return split.clone();
    }
    let mut children = split.children.clone();
    let mut sizes = split.sizes.clone();
    let child = children.remove(from_index);
    children.insert(insert_at, child);
    if from_index < sizes.len() {
        let size = sizes.remove(from_index);
        sizes.insert(insert_at.min(sizes.len()), size);
    }
    SplitNode {
        children,
        sizes,
        ..split.clone()
    }
}

/// `reorderInSplit`.
fn reorder_in_split(
    node: &LayoutNode,
    split_id: &str,
    from_index: usize,
    to_index: usize,
    place: PanePlace,
) -> LayoutNode {
    let LayoutNode::Split(split) = node else {
        return node.clone();
    };
    if split.id == split_id {
        return LayoutNode::Split(reorder_child(split, from_index, to_index, place));
    }
    LayoutNode::Split(SplitNode {
        children: split
            .children
            .iter()
            .map(|child| reorder_in_split(child, split_id, from_index, to_index, place))
            .collect(),
        ..split.clone()
    })
}

/// `extractLeaf`: the tree without the leaf, and the leaf itself.
fn extract_leaf(node: &LayoutNode, leaf_id: &str) -> Option<(Option<LayoutNode>, LayoutNode)> {
    let split = match node {
        LayoutNode::Leaf(leaf_node) => {
            return (leaf_node.id == leaf_id).then(|| (None, node.clone()));
        }
        LayoutNode::Split(split) => split,
    };

    let mut children = Vec::new();
    let mut sizes = Vec::new();
    let mut found = None;
    for (i, child) in split.children.iter().enumerate() {
        let size = split.sizes.get(i).copied().unwrap_or(0.0);
        match extract_leaf(child, leaf_id) {
            None => {
                children.push(child.clone());
                sizes.push(size);
            }
            Some((tree, extracted)) => {
                found = Some(extracted);
                if let Some(tree) = tree {
                    children.push(tree);
                    sizes.push(size);
                }
            }
        }
    }

    let extracted = found?;
    match children.len() {
        0 => Some((None, extracted)),
        1 => Some((children.pop(), extracted)),
        _ => Some((
            Some(LayoutNode::Split(SplitNode {
                children,
                sizes: normalize(&sizes),
                ..split.clone()
            })),
            extracted,
        )),
    }
}

/// `insertBeside`: add `incoming` next to the leaf `target_id` in its own
/// split, halving the target's share.
fn insert_beside(
    node: &LayoutNode,
    target_id: &str,
    incoming: &LayoutNode,
    place: PanePlace,
) -> LayoutNode {
    let LayoutNode::Split(split) = node else {
        return node.clone();
    };
    if let Some(index) = split
        .children
        .iter()
        .position(|child| child.is_leaf(target_id))
    {
        let insert_at = if place == PanePlace::Before {
            index
        } else {
            index + 1
        };
        let mut children = split.children.clone();
        let mut sizes = split.sizes.clone();
        let share = sizes.get(index).copied().unwrap_or(0.0) / 2.0;
        // A malformed tree can have fewer sizes than children; pad it so the
        // indexes below line up.
        if sizes.len() <= index {
            sizes.resize(index + 1, 0.0);
        }
        sizes[index] = share;
        children.insert(insert_at, incoming.clone());
        sizes.insert(insert_at, share);
        return LayoutNode::Split(SplitNode {
            children,
            sizes,
            ..split.clone()
        });
    }
    LayoutNode::Split(SplitNode {
        children: split
            .children
            .iter()
            .map(|child| insert_beside(child, target_id, incoming, place))
            .collect(),
        ..split.clone()
    })
}

/// `wrapBeside`: replace the leaf `target_id` with a new split holding it
/// and `incoming`.
fn wrap_beside(
    node: &LayoutNode,
    target_id: &str,
    incoming: &LayoutNode,
    dir: SplitDir,
    place: PanePlace,
) -> LayoutNode {
    match node {
        LayoutNode::Leaf(leaf_node) => {
            if leaf_node.id != target_id {
                return node.clone();
            }
            let children = if place == PanePlace::Before {
                vec![incoming.clone(), node.clone()]
            } else {
                vec![node.clone(), incoming.clone()]
            };
            LayoutNode::split(random_uuid(), dir, children, vec![0.5, 0.5])
        }
        LayoutNode::Split(split) => LayoutNode::Split(SplitNode {
            children: split
                .children
                .iter()
                .map(|child| wrap_beside(child, target_id, incoming, dir, place))
                .collect(),
            ..split.clone()
        }),
    }
}

/// `movePane`: drag a leaf onto another pane's edge. Same-axis siblings
/// keep their sizes and only swap order; a perpendicular edge nests a new
/// split around the target; dropping onto a pane in another group of the
/// same axis relocates the leaf there.
pub fn move_pane(node: &LayoutNode, from_id: &str, to_id: &str, edge: PaneEdge) -> LayoutNode {
    if from_id == to_id {
        return node.clone();
    }
    let (Some(from_at), Some(to_at)) = (leaf_parent(node, from_id), leaf_parent(node, to_id))
    else {
        return node.clone();
    };

    let (dir, place) = edge_split(edge);
    if to_at.dir == dir && from_at.parent_id == to_at.parent_id {
        return reorder_in_split(node, &from_at.parent_id, from_at.index, to_at.index, place);
    }

    let Some((Some(tree), extracted)) = extract_leaf(node, from_id) else {
        return node.clone();
    };
    if !has_leaf(&tree, to_id) {
        return node.clone();
    }
    if leaf_parent(&tree, to_id).is_some_and(|target| target.dir == dir) {
        return insert_beside(&tree, to_id, &extracted, place);
    }
    wrap_beside(&tree, to_id, &extracted, dir, place)
}

/// `placePane`: open `session_id` on `to_id`'s edge, or move it there when
/// it is already a leaf in this tree.
pub fn place_pane(node: &LayoutNode, session_id: &str, to_id: &str, edge: PaneEdge) -> LayoutNode {
    if session_id == to_id {
        return node.clone();
    }
    if !has_leaf(node, to_id) {
        return node.clone();
    }
    if has_leaf(node, session_id) {
        return move_pane(node, session_id, to_id, edge);
    }
    place_layout(node, &leaf(session_id), to_id, edge)
}

/// `placeLayout`: place an intact layout tree beside one pane in another layout.
pub fn place_layout(
    node: &LayoutNode,
    incoming: &LayoutNode,
    to_id: &str,
    edge: PaneEdge,
) -> LayoutNode {
    if !has_leaf(node, to_id) {
        return node.clone();
    }
    let (dir, place) = edge_split(edge);
    if leaf_parent(node, to_id).is_some_and(|target| target.dir == dir) {
        return insert_beside(node, to_id, incoming, place);
    }
    wrap_beside(node, to_id, incoming, dir, place)
}

/// `replacePaneWithLayout`: replace one pane with an intact layout tree.
pub fn replace_pane_with_layout(
    node: &LayoutNode,
    target_id: &str,
    incoming: &LayoutNode,
) -> LayoutNode {
    match node {
        LayoutNode::Leaf(leaf_node) => {
            if leaf_node.id == target_id {
                incoming.clone()
            } else {
                node.clone()
            }
        }
        LayoutNode::Split(split) => LayoutNode::Split(SplitNode {
            children: split
                .children
                .iter()
                .map(|child| replace_pane_with_layout(child, target_id, incoming))
                .collect(),
            ..split.clone()
        }),
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
