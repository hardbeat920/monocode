//! Port of the pure parts of src/features/projects/model/projectTerminal.ts:
//! the per-project terminal dock the workspace snapshot saves.
//!
//! `dockGridStyle` returned React CSS properties; here it returns the three
//! grid template strings. `applyDockGridStyle` wrote them to a DOM node and
//! has no port.

use std::collections::HashSet;

use monocode_core::{Extra, js as core_js};
use serde::{Deserialize, Serialize};

use crate::layout::{
    EditorPane, FilePaneTab, WorkspaceTab, new_editor_pane, next_terminal_title_from_files,
};
use crate::paths::{normalize_project_path, same_project_path};
use crate::session_ref::SessionRef;
use crate::terminal_tab::{TerminalMetaPatch, apply_terminal_meta};
use crate::workspace_tab_groups::workspace_tab_cwd;

/// `DockSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockSide {
    #[serde(rename = "top")]
    Top,
    #[serde(rename = "bottom")]
    Bottom,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
}

impl DockSide {
    pub const fn as_str(self) -> &'static str {
        match self {
            DockSide::Top => "top",
            DockSide::Bottom => "bottom",
            DockSide::Left => "left",
            DockSide::Right => "right",
        }
    }

    /// `isDockSide` for a stored string.
    pub fn parse(value: &str) -> Option<Self> {
        [
            DockSide::Top,
            DockSide::Bottom,
            DockSide::Left,
            DockSide::Right,
        ]
        .into_iter()
        .find(|side| side.as_str() == value)
    }
}

/// `isDockSide` for a JSON value.
pub fn is_dock_side(value: Option<&serde_json::Value>) -> Option<DockSide> {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(DockSide::parse)
}

/// `ProjectTerminalDock`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTerminalDock {
    pub project_path: String,
    pub pane: EditorPane,
    pub side: DockSide,
    /// Pixels across the dock, always a whole number after `clampDockSize`.
    pub size: i64,
    pub open: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A viewport in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
}

/// The viewport `clampDockSize` assumed when the caller passed none.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280.0,
    height: 800.0,
};

const VERTICAL_MIN: f64 = 88.0;
const HORIZONTAL_MIN: f64 = 180.0;

/// `isVerticalDock`.
pub fn is_vertical_dock(side: DockSide) -> bool {
    matches!(side, DockSide::Top | DockSide::Bottom)
}

/// `defaultDockSize`, from `DOCK_SIZE_DEFAULT`.
pub fn default_dock_size(side: DockSide) -> i64 {
    match side {
        DockSide::Top | DockSide::Bottom => 220,
        DockSide::Left | DockSide::Right => 360,
    }
}

/// `clampDockSize`: at least the side's minimum, at most 70% of the viewport.
pub fn clamp_dock_size(side: DockSide, value: f64, viewport: Option<Viewport>) -> i64 {
    let viewport = viewport.unwrap_or(DEFAULT_VIEWPORT);
    let vertical = is_vertical_dock(side);
    let min = if vertical {
        VERTICAL_MIN
    } else {
        HORIZONTAL_MIN
    };
    let span = if vertical {
        viewport.height
    } else {
        viewport.width
    };
    let max = crate::js::max(min, (span * 0.7).floor());
    if !value.is_finite() {
        return default_dock_size(side);
    }
    crate::js::min(max, crate::js::max(min, core_js::round(value))) as i64
}

/// `findProjectTerminal`.
pub fn find_project_terminal<'a>(
    docks: &'a [ProjectTerminalDock],
    project_path: &str,
) -> Option<&'a ProjectTerminalDock> {
    docks
        .iter()
        .find(|dock| same_project_path(&dock.project_path, project_path))
}

/// `createProjectTerminal`. `side` defaults to the bottom.
pub fn create_project_terminal(
    project_path: &str,
    file: FilePaneTab,
    side: Option<DockSide>,
) -> ProjectTerminalDock {
    let side = side.unwrap_or(DockSide::Bottom);
    ProjectTerminalDock {
        project_path: normalize_project_path(project_path),
        pane: new_editor_pane(file),
        side,
        size: default_dock_size(side),
        open: true,
        extra: Extra::new(),
    }
}

/// `addTerminalToDock`.
pub fn add_terminal_to_dock(dock: &ProjectTerminalDock, file: FilePaneTab) -> ProjectTerminalDock {
    let mut next = dock.clone();
    next.open = true;
    next.pane.active_file_id = file.id.clone();
    next.pane.files.push(file);
    next
}

/// `nextDockTerminalTitle`.
pub fn next_dock_terminal_title(dock: &ProjectTerminalDock, cwd: &str) -> String {
    next_terminal_title_from_files(&dock.pane.files, cwd)
}

/// `closeTerminalInDock`: `None` when the last terminal closes.
pub fn close_terminal_in_dock(
    dock: &ProjectTerminalDock,
    file_id: &str,
) -> Option<ProjectTerminalDock> {
    let Some(index) = dock.pane.files.iter().position(|file| file.id == file_id) else {
        return Some(dock.clone());
    };
    let files: Vec<FilePaneTab> = dock
        .pane
        .files
        .iter()
        .filter(|file| file.id != file_id)
        .cloned()
        .collect();
    if files.is_empty() {
        return None;
    }
    let active_file_id = if dock.pane.active_file_id == file_id {
        files[index.min(files.len() - 1)].id.clone()
    } else {
        dock.pane.active_file_id.clone()
    };
    let mut next = dock.clone();
    next.pane.files = files;
    next.pane.active_file_id = active_file_id;
    Some(next)
}

/// `selectDockTerminal`.
pub fn select_dock_terminal(dock: &ProjectTerminalDock, file_id: &str) -> ProjectTerminalDock {
    if !dock.pane.files.iter().any(|file| file.id == file_id) || dock.pane.active_file_id == file_id
    {
        return dock.clone();
    }
    let mut next = dock.clone();
    next.pane.active_file_id = file_id.to_string();
    next
}

/// `reorderDockTerminals`.
pub fn reorder_dock_terminals(
    dock: &ProjectTerminalDock,
    files: Vec<FilePaneTab>,
) -> ProjectTerminalDock {
    let mut next = dock.clone();
    next.pane.files = files;
    next
}

/// `patchDockTerminal`.
pub fn patch_dock_terminal(
    dock: &ProjectTerminalDock,
    file_id: &str,
    patch: &TerminalMetaPatch,
) -> ProjectTerminalDock {
    let mut next = dock.clone();
    for file in &mut next.pane.files {
        if file.terminal == Some(true) && file.id == file_id {
            *file = apply_terminal_meta(file, patch);
        }
    }
    next
}

/// `patchProjectTerminals`.
pub fn patch_project_terminals(
    docks: &[ProjectTerminalDock],
    file_id: &str,
    patch: &TerminalMetaPatch,
) -> Vec<ProjectTerminalDock> {
    docks
        .iter()
        .map(|dock| patch_dock_terminal(dock, file_id, patch))
        .collect()
}

/// `mapProjectTerminal`: update or remove the dock of one project.
pub fn map_project_terminal(
    docks: &[ProjectTerminalDock],
    project_path: &str,
    mut update: impl FnMut(&ProjectTerminalDock) -> Option<ProjectTerminalDock>,
) -> Vec<ProjectTerminalDock> {
    let mut next = Vec::new();
    for dock in docks {
        if !same_project_path(&dock.project_path, project_path) {
            next.push(dock.clone());
            continue;
        }
        if let Some(updated) = update(dock) {
            next.push(updated);
        }
    }
    next
}

/// `withDockOpen`.
pub fn with_dock_open(dock: &ProjectTerminalDock, open: bool) -> ProjectTerminalDock {
    ProjectTerminalDock {
        open,
        ..dock.clone()
    }
}

/// `withDockSide`.
pub fn with_dock_side(
    dock: &ProjectTerminalDock,
    side: DockSide,
    viewport: Option<Viewport>,
) -> ProjectTerminalDock {
    if dock.side == side {
        return dock.clone();
    }
    ProjectTerminalDock {
        side,
        size: clamp_dock_size(side, dock.size as f64, viewport),
        ..dock.clone()
    }
}

/// `withDockSize`.
pub fn with_dock_size(
    dock: &ProjectTerminalDock,
    size: f64,
    viewport: Option<Viewport>,
) -> ProjectTerminalDock {
    ProjectTerminalDock {
        size: clamp_dock_size(dock.side, size, viewport),
        ..dock.clone()
    }
}

/// `projectTerminalFileIds`.
pub fn project_terminal_file_ids(docks: &[ProjectTerminalDock]) -> Vec<String> {
    docks
        .iter()
        .flat_map(|dock| &dock.pane.files)
        .filter(|file| file.terminal == Some(true))
        .map(|file| file.id.clone())
        .collect()
}

/// The CSS grid `dockGridStyle` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockGridStyle {
    pub grid_template_rows: String,
    pub grid_template_columns: String,
    pub grid_template_areas: String,
}

/// `dockGridStyle`: the main area and the dock on `side`, `size` pixels across.
pub fn dock_grid_style(side: Option<DockSide>, size: f64) -> DockGridStyle {
    let style = |rows: &str, columns: &str, areas: &str| DockGridStyle {
        grid_template_rows: rows.into(),
        grid_template_columns: columns.into(),
        grid_template_areas: areas.into(),
    };
    let Some(side) = side else {
        return style("minmax(0, 1fr)", "minmax(0, 1fr)", "\"main\"");
    };
    let px = format!(
        "{}px",
        core_js::number_to_string(crate::js::max(1.0, core_js::round(size)))
    );
    match side {
        DockSide::Top => style(
            &format!("{px} minmax(0, 1fr)"),
            "minmax(0, 1fr)",
            "\"dock\" \"main\"",
        ),
        DockSide::Bottom => style(
            &format!("minmax(0, 1fr) {px}"),
            "minmax(0, 1fr)",
            "\"main\" \"dock\"",
        ),
        DockSide::Left => style(
            "minmax(0, 1fr)",
            &format!("{px} minmax(0, 1fr)"),
            "\"dock main\"",
        ),
        DockSide::Right => style(
            "minmax(0, 1fr)",
            &format!("minmax(0, 1fr) {px}"),
            "\"main dock\"",
        ),
    }
}

/// `splitProjectTerminalsForMove`: a dock follows the tabs of its project
/// into a new window only when every remaining tab of that project is
/// leaving too. Otherwise the original window keeps the running terminals.
pub fn split_project_terminals_for_move<S: SessionRef>(
    docks: &[ProjectTerminalDock],
    moving_tabs: &[WorkspaceTab],
    remaining_tabs: &[WorkspaceTab],
    sessions: &[S],
) -> (Vec<ProjectTerminalDock>, Vec<ProjectTerminalDock>) {
    let remaining_projects = project_paths_of(remaining_tabs, sessions);
    let moving_projects = project_paths_of(moving_tabs, sessions);
    let mut moving = Vec::new();
    let mut remaining = Vec::new();
    for dock in docks {
        let path = normalize_project_path(&dock.project_path);
        let stays = remaining_projects.contains(&path);
        let follows = moving_projects.contains(&path) && !stays;
        if follows {
            moving.push(dock.clone());
        } else {
            remaining.push(dock.clone());
        }
    }
    (moving, remaining)
}

/// `projectPathsOf`.
fn project_paths_of<S: SessionRef>(tabs: &[WorkspaceTab], sessions: &[S]) -> HashSet<String> {
    tabs.iter()
        .filter_map(|tab| workspace_tab_cwd(tab, sessions))
        .map(|cwd| normalize_project_path(&cwd))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{new_tab, new_terminal_file};
    use monocode_core::{HarnessId, Session};

    #[test]
    fn clamps_dock_sizes_to_the_viewport() {
        assert_eq!(clamp_dock_size(DockSide::Bottom, 10.0, None), 88);
        assert_eq!(clamp_dock_size(DockSide::Bottom, 900.0, None), 560);
        assert_eq!(clamp_dock_size(DockSide::Left, 300.4, None), 300);
        assert_eq!(clamp_dock_size(DockSide::Right, 300.5, None), 301);
        assert_eq!(clamp_dock_size(DockSide::Left, f64::NAN, None), 360);
        assert_eq!(
            clamp_dock_size(
                DockSide::Top,
                500.0,
                Some(Viewport {
                    width: 100.0,
                    height: 100.0
                })
            ),
            88
        );
    }

    #[test]
    fn creates_and_edits_a_dock() {
        let first = new_terminal_file("/tmp/a", Some("zsh"), None);
        let dock = create_project_terminal("/tmp/a/", first.clone(), None);
        assert_eq!(dock.project_path, "/tmp/a");
        assert_eq!(dock.side, DockSide::Bottom);
        assert_eq!(dock.size, 220);
        assert!(dock.open);

        let second = new_terminal_file(
            "/tmp/a",
            Some(&next_dock_terminal_title(&dock, "/tmp/a")),
            None,
        );
        assert_eq!(second.path, "a");
        let dock = add_terminal_to_dock(&with_dock_open(&dock, false), second.clone());
        assert!(dock.open);
        assert_eq!(dock.pane.active_file_id, second.id);
        assert_eq!(
            project_terminal_file_ids(std::slice::from_ref(&dock)),
            vec![first.id.clone(), second.id.clone()]
        );

        let closed = close_terminal_in_dock(&dock, &second.id).unwrap();
        assert_eq!(closed.pane.active_file_id, first.id);
        assert_eq!(close_terminal_in_dock(&closed, &first.id), None);
        assert_eq!(
            close_terminal_in_dock(&closed, "missing"),
            Some(closed.clone())
        );
        assert_eq!(
            select_dock_terminal(&dock, &first.id).pane.active_file_id,
            first.id
        );

        let moved = with_dock_side(&dock, DockSide::Left, None);
        assert_eq!(moved.size, 220);
        assert_eq!(with_dock_size(&moved, 2000.0, None).size, 896);
        assert!(find_project_terminal(std::slice::from_ref(&dock), "/tmp/a/").is_some());
        assert_eq!(
            map_project_terminal(std::slice::from_ref(&dock), "/tmp/a", |_| None),
            vec![]
        );
    }

    #[test]
    fn patches_the_matching_terminal() {
        let file = new_terminal_file("/tmp/a", None, None);
        let dock = create_project_terminal("/tmp/a", file.clone(), None);
        let patch = TerminalMetaPatch {
            foreground: Some(Some("vite".into())),
            ..Default::default()
        };
        let patched = patch_project_terminals(std::slice::from_ref(&dock), &file.id, &patch);
        assert_eq!(patched[0].pane.files[0].foreground.as_deref(), Some("vite"));
        assert_eq!(patch_project_terminals(&patched, &file.id, &patch), patched);
    }

    #[test]
    fn builds_the_dock_grid() {
        assert_eq!(dock_grid_style(None, 0.0).grid_template_areas, "\"main\"");
        let bottom = dock_grid_style(Some(DockSide::Bottom), 220.4);
        assert_eq!(bottom.grid_template_rows, "minmax(0, 1fr) 220px");
        assert_eq!(bottom.grid_template_areas, "\"main\" \"dock\"");
        let left = dock_grid_style(Some(DockSide::Left), 0.0);
        assert_eq!(left.grid_template_columns, "1px minmax(0, 1fr)");
    }

    #[test]
    fn a_dock_follows_only_when_its_project_leaves_entirely() {
        let sessions = [
            Session::blank("a1", HarnessId::Cursor, "", "/alpha"),
            Session::blank("a2", HarnessId::Cursor, "", "/alpha"),
            Session::blank("b1", HarnessId::Cursor, "", "/beta"),
        ];
        let alpha =
            create_project_terminal("/alpha", new_terminal_file("/alpha", None, None), None);
        let beta = create_project_terminal("/beta", new_terminal_file("/beta", None, None), None);
        let (moving, remaining) = split_project_terminals_for_move(
            &[alpha.clone(), beta.clone()],
            &[new_tab("a1"), new_tab("b1")],
            &[new_tab("a2")],
            &sessions,
        );
        assert_eq!(moving, vec![beta]);
        assert_eq!(remaining, vec![alpha]);
    }
}
