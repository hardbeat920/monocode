//! The project terminal docks of one window: the state behind
//! `projectTerminals` and `lastDockSide` in src/app/App.tsx, and the dock
//! callbacks at App.tsx lines 2467-2765 and 10370-10399 that do not touch
//! tabs. The pure dock model is
//! `monocode_layout::project_terminal` (projectTerminal.ts).
//!
//! The `Workspace` owns one `ProjectTerminals` and passes the current
//! project path in. A dock change notifies observers; the workspace saves
//! the snapshot and stops the PTYs of closed terminal files.

use std::collections::HashSet;

use gpui::Context;
use monocode_layout::project_terminal::{
    DockSide, ProjectTerminalDock, Viewport, add_terminal_to_dock, close_terminal_in_dock,
    create_project_terminal, find_project_terminal, map_project_terminal, next_dock_terminal_title,
    patch_project_terminals, project_terminal_file_ids, reorder_dock_terminals,
    select_dock_terminal, split_project_terminals_for_move, with_dock_open, with_dock_side,
    with_dock_size,
};
use monocode_layout::session_ref::SessionRef;
use monocode_layout::terminal_tab::TerminalMetaPatch;
use monocode_layout::{FilePaneTab, WorkspaceTab, new_terminal_file};

use crate::runtime::util::reorder::order_by_ids;
use crate::workspace::paths::is_local_project;

/// What `toggle_running` did with a terminal id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockToggle {
    /// The dock was open and is now hidden.
    Hidden,
    /// The dock was hidden and now shows the terminal.
    Shown,
    /// No dock holds this terminal.
    NotInDock,
}

/// The docks of one window.
pub struct ProjectTerminals {
    docks: Vec<ProjectTerminalDock>,
    /// `lastDockSide`: the side a brand-new project's dock starts on.
    last_dock_side: Option<DockSide>,
    /// `projectTerminalFocused`.
    focused: bool,
}

impl ProjectTerminals {
    pub fn new(docks: Vec<ProjectTerminalDock>, last_dock_side: Option<DockSide>) -> Self {
        Self {
            docks,
            last_dock_side,
            focused: false,
        }
    }

    pub fn docks(&self) -> &[ProjectTerminalDock] {
        &self.docks
    }

    pub fn last_dock_side(&self) -> Option<DockSide> {
        self.last_dock_side
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// `findProjectTerminal`.
    pub fn dock(&self, project_path: &str) -> Option<&ProjectTerminalDock> {
        find_project_terminal(&self.docks, project_path)
    }

    /// The side and size to draw for a project, or `None` while its dock is
    /// hidden or missing (`dockVisible`, `dockGridStyle`).
    pub fn visible_dock(&self, project_path: &str) -> Option<(DockSide, i64)> {
        self.dock(project_path)
            .filter(|dock| dock.open)
            .map(|dock| (dock.side, dock.size))
    }

    /// `projectTerminalFileIds`.
    pub fn file_ids(&self) -> Vec<String> {
        project_terminal_file_ids(&self.docks)
    }

    /// Every terminal file in every dock.
    pub fn files(&self) -> impl Iterator<Item = &FilePaneTab> {
        self.docks.iter().flat_map(|dock| &dock.pane.files)
    }

    fn set_docks(&mut self, docks: Vec<ProjectTerminalDock>, cx: &mut Context<Self>) {
        if docks != self.docks {
            self.docks = docks;
            cx.notify();
        }
    }

    /// Replace every dock, as a window transfer or a restore does.
    pub fn replace(&mut self, docks: Vec<ProjectTerminalDock>, cx: &mut Context<Self>) {
        self.set_docks(docks, cx);
    }

    /// `setProjectTerminalFocused`.
    pub fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.focused != focused {
            self.focused = focused;
            cx.notify();
        }
    }

    /// `openProjectTerminal`: add a terminal to the project's dock, making
    /// the dock when there is none. `false` for a project that is not on
    /// this machine.
    pub fn open(&mut self, project_path: &str, workdir: &str, cx: &mut Context<Self>) -> bool {
        if !is_local_project(project_path) {
            return false;
        }
        let existing = find_project_terminal(&self.docks, project_path);
        let title = existing.map(|dock| next_dock_terminal_title(dock, workdir));
        let file = new_terminal_file(workdir, title.as_deref(), Some(project_path));
        let docks = if existing.is_none() {
            let mut docks = self.docks.clone();
            docks.push(create_project_terminal(
                project_path,
                file,
                Some(self.last_dock_side.unwrap_or(DockSide::Bottom)),
            ));
            docks
        } else {
            map_project_terminal(&self.docks, project_path, |dock| {
                Some(add_terminal_to_dock(dock, file.clone()))
            })
        };
        self.set_docks(docks, cx);
        self.set_focused(true, cx);
        true
    }

    /// The dock half of `onShowProjectTerminal`: reveal and focus the dock
    /// when it has terminals. `false` when the caller should open one.
    pub fn show(&mut self, project_path: &str, cx: &mut Context<Self>) -> bool {
        let Some(dock) = self.dock(project_path) else {
            return false;
        };
        if dock.pane.files.is_empty() {
            return false;
        }
        if !dock.open {
            let docks = map_project_terminal(&self.docks, project_path, |dock| {
                Some(with_dock_open(dock, true))
            });
            self.set_docks(docks, cx);
        }
        self.set_focused(true, cx);
        true
    }

    /// `onToggleProjectTerminal`. `git_cwd` is where a new dock's first
    /// shell starts.
    pub fn toggle(&mut self, project_path: &str, git_cwd: &str, cx: &mut Context<Self>) {
        if !is_local_project(project_path) {
            return;
        }
        let Some(dock) = self.dock(project_path) else {
            self.open(project_path, git_cwd, cx);
            return;
        };
        let next_open = !dock.open;
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(with_dock_open(dock, next_open))
        });
        self.set_docks(docks, cx);
        self.set_focused(next_open, cx);
    }

    /// `onHideProjectTerminal`.
    pub fn hide(&mut self, project_path: &str, cx: &mut Context<Self>) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(with_dock_open(dock, false))
        });
        self.set_docks(docks, cx);
        self.set_focused(false, cx);
    }

    /// `onProjectTerminalSide`: move the dock and remember the side for new
    /// projects.
    pub fn set_side(
        &mut self,
        project_path: &str,
        side: DockSide,
        viewport: Option<Viewport>,
        cx: &mut Context<Self>,
    ) {
        if self.last_dock_side != Some(side) {
            self.last_dock_side = Some(side);
            cx.notify();
        }
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(with_dock_side(dock, side, viewport))
        });
        self.set_docks(docks, cx);
    }

    /// `onProjectTerminalSize` and `commitDockSize`.
    pub fn set_size(
        &mut self,
        project_path: &str,
        size: f64,
        viewport: Option<Viewport>,
        cx: &mut Context<Self>,
    ) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(with_dock_size(dock, size, viewport))
        });
        self.set_docks(docks, cx);
    }

    /// `onSelectProjectTerminal`.
    pub fn select(&mut self, project_path: &str, file_id: &str, cx: &mut Context<Self>) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(select_dock_terminal(dock, file_id))
        });
        self.set_docks(docks, cx);
        self.set_focused(true, cx);
    }

    /// `onReorderProjectTerminals`.
    pub fn reorder(&mut self, project_path: &str, ids: &[String], cx: &mut Context<Self>) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            Some(reorder_dock_terminals(
                dock,
                order_by_ids(&dock.pane.files, ids),
            ))
        });
        self.set_docks(docks, cx);
    }

    /// The `finishClose` of `onCloseProjectTerminal`, after the caller
    /// confirmed.
    pub fn close(&mut self, project_path: &str, file_id: &str, cx: &mut Context<Self>) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            close_terminal_in_dock(dock, file_id)
        });
        self.set_docks(docks, cx);
    }

    /// The `finishClose` of `onCloseOtherProjectTerminals`.
    pub fn close_others(
        &mut self,
        project_path: &str,
        file_id: &str,
        closing: &HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        let docks = map_project_terminal(&self.docks, project_path, |dock| {
            if !dock.pane.files.iter().any(|file| file.id == file_id) {
                return Some(dock.clone());
            }
            let mut next = dock.clone();
            next.pane.files.retain(|file| !closing.contains(&file.id));
            next.pane.active_file_id = file_id.to_string();
            Some(next)
        });
        self.set_docks(docks, cx);
    }

    /// The files `onCloseOtherProjectTerminals` would close, or `None` when
    /// it does nothing.
    pub fn others_than(&self, project_path: &str, file_id: &str) -> Option<Vec<FilePaneTab>> {
        let dock = self.dock(project_path)?;
        if !dock.pane.files.iter().any(|file| file.id == file_id) {
            return None;
        }
        let closing: Vec<FilePaneTab> = dock
            .pane
            .files
            .iter()
            .filter(|file| file.id != file_id)
            .cloned()
            .collect();
        (!closing.is_empty()).then_some(closing)
    }

    /// The dock half of `onTerminalMetaChange`.
    pub fn patch(&mut self, file_id: &str, patch: &TerminalMetaPatch, cx: &mut Context<Self>) {
        let docks = patch_project_terminals(&self.docks, file_id, patch);
        self.set_docks(docks, cx);
    }

    /// The dock half of `onToggleRunningTerminal`.
    pub fn toggle_running(&mut self, file_id: &str, cx: &mut Context<Self>) -> DockToggle {
        let Some(dock) = self
            .docks
            .iter()
            .find(|dock| dock.pane.files.iter().any(|file| file.id == file_id))
            .cloned()
        else {
            return DockToggle::NotInDock;
        };
        if dock.open {
            let docks = map_project_terminal(&self.docks, &dock.project_path, |entry| {
                Some(with_dock_open(entry, false))
            });
            self.set_docks(docks, cx);
            self.set_focused(false, cx);
            return DockToggle::Hidden;
        }
        let docks = map_project_terminal(&self.docks, &dock.project_path, |entry| {
            Some(with_dock_open(&select_dock_terminal(entry, file_id), true))
        });
        self.set_docks(docks, cx);
        self.set_focused(true, cx);
        DockToggle::Shown
    }

    /// The dock half of `applyProjectLocationChange`.
    pub fn rebase_project(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        let docks = self
            .docks
            .iter()
            .map(|dock| {
                if monocode_layout::paths::same_project_path(&dock.project_path, from) {
                    ProjectTerminalDock {
                        project_path: to.to_string(),
                        ..dock.clone()
                    }
                } else {
                    dock.clone()
                }
            })
            .collect();
        self.set_docks(docks, cx);
    }

    /// `splitProjectTerminalsForMove`: take the docks that follow the moving
    /// tabs into a new window, and keep the rest.
    pub fn take_for_move<S: SessionRef>(
        &mut self,
        moving_tabs: &[WorkspaceTab],
        remaining_tabs: &[WorkspaceTab],
        sessions: &[S],
        cx: &mut Context<Self>,
    ) -> Vec<ProjectTerminalDock> {
        let (moving, remaining) =
            split_project_terminals_for_move(&self.docks, moving_tabs, remaining_tabs, sessions);
        self.set_docks(remaining, cx);
        moving
    }
}

#[cfg(test)]
mod tests {
    //! The cases of src/features/projects/model/projectTerminal.test.ts,
    //! against the layout functions and this entity.

    use super::*;
    use gpui::{AppContext, Entity, TestAppContext};
    use monocode_core::{HarnessId, Session};
    use monocode_layout::project_terminal::{clamp_dock_size, dock_grid_style};
    use monocode_layout::{leaf, new_tab};

    fn chat(id: &str, cwd: &str) -> Session {
        Session::blank(id, HarnessId::Cursor, "", cwd)
    }

    #[test]
    fn opens_a_bottom_dock_with_the_first_terminal_focused() {
        let file = new_terminal_file("/tmp/a", None, None);
        let dock = create_project_terminal("/tmp/a/", file.clone(), None);
        assert_eq!(dock.project_path, "/tmp/a");
        assert_eq!(dock.side, DockSide::Bottom);
        assert!(dock.open);
        assert_eq!(dock.pane.files, vec![file.clone()]);
        assert_eq!(dock.pane.active_file_id, file.id);
    }

    #[test]
    fn appends_a_tab_focuses_it_and_reveals_a_hidden_dock() {
        let first = new_terminal_file("/tmp/a", Some("a"), None);
        let second = new_terminal_file("/tmp/a", Some("a 2"), None);
        let dock = with_dock_open(
            &create_project_terminal("/tmp/a", first.clone(), None),
            false,
        );
        let next = add_terminal_to_dock(&dock, second.clone());
        assert!(next.open);
        let ids: Vec<&str> = next
            .pane
            .files
            .iter()
            .map(|file| file.id.as_str())
            .collect();
        assert_eq!(ids, vec![first.id.as_str(), second.id.as_str()]);
        assert_eq!(next.pane.active_file_id, second.id);
        assert_eq!(next_dock_terminal_title(&next, "/tmp/a"), "a 3");
    }

    #[test]
    fn drops_the_dock_when_the_last_terminal_closes() {
        let file = new_terminal_file("/tmp/a", None, None);
        let dock = create_project_terminal("/tmp/a", file.clone(), None);
        assert_eq!(close_terminal_in_dock(&dock, &file.id), None);
    }

    #[test]
    fn focuses_a_neighbor_after_closing_one_of_several_terminals() {
        let first = new_terminal_file("/tmp/a", Some("one"), None);
        let second = new_terminal_file("/tmp/a", Some("two"), None);
        let dock = add_terminal_to_dock(
            &create_project_terminal("/tmp/a", first.clone(), None),
            second.clone(),
        );
        let next = close_terminal_in_dock(&dock, &second.id).unwrap();
        assert_eq!(next.pane.files, vec![first.clone()]);
        assert_eq!(next.pane.active_file_id, first.id);
    }

    #[test]
    fn updates_only_the_matching_project_and_can_remove_it() {
        let alpha =
            create_project_terminal("/tmp/a", new_terminal_file("/tmp/a", None, None), None);
        let beta = create_project_terminal("/tmp/b", new_terminal_file("/tmp/b", None, None), None);
        let docks = vec![alpha.clone(), beta];
        assert_eq!(
            find_project_terminal(&docks, "/tmp/a/").map(|dock| &dock.pane.id),
            Some(&alpha.pane.id)
        );
        let hidden =
            map_project_terminal(&docks, "/tmp/a", |dock| Some(with_dock_open(dock, false)));
        assert!(!hidden[0].open);
        assert!(hidden[1].open);
        let removed = map_project_terminal(&docks, "/tmp/a", |_| None);
        let paths: Vec<&str> = removed
            .iter()
            .map(|dock| dock.project_path.as_str())
            .collect();
        assert_eq!(paths, vec!["/tmp/b"]);
    }

    #[test]
    fn renames_a_terminal_by_id_without_touching_other_docks() {
        let file = new_terminal_file("/tmp/a", Some("zsh"), None);
        let other = new_terminal_file("/tmp/b", Some("other"), None);
        let docks = vec![
            create_project_terminal("/tmp/a", file.clone(), None),
            create_project_terminal("/tmp/b", other.clone(), None),
        ];
        let next = patch_project_terminals(
            &docks,
            &file.id,
            &TerminalMetaPatch {
                title: Some("npm".into()),
                cwd: Some("/tmp/a/app".into()),
                foreground: None,
            },
        );
        assert_eq!(next[0].pane.files[0].path, "npm");
        assert_eq!(next[0].pane.files[0].cwd, "/tmp/a/app");
        assert_eq!(next[1].pane.files[0].path, "other");
        assert_eq!(select_dock_terminal(&next[0], &other.id), next[0]);
    }

    #[test]
    fn records_a_foreground_process_without_renaming_other_docks() {
        let file = new_terminal_file("/tmp/a", Some("zsh"), None);
        let docks = vec![create_project_terminal("/tmp/a", file.clone(), None)];
        let next = patch_project_terminals(
            &docks,
            &file.id,
            &TerminalMetaPatch {
                title: Some("vite".into()),
                cwd: None,
                foreground: Some(Some("vite".into())),
            },
        );
        assert_eq!(next[0].pane.files[0].path, "vite");
        assert_eq!(next[0].pane.files[0].foreground.as_deref(), Some("vite"));
        let cleared = patch_project_terminals(
            &next,
            &file.id,
            &TerminalMetaPatch {
                foreground: Some(None),
                ..Default::default()
            },
        );
        assert_eq!(cleared[0].pane.files[0].foreground, None);
    }

    #[test]
    fn clamps_to_the_axis_min_and_70_percent_of_the_viewport() {
        let viewport = Some(Viewport {
            width: 1000.0,
            height: 400.0,
        });
        assert_eq!(clamp_dock_size(DockSide::Bottom, 10.0, viewport), 88);
        assert_eq!(clamp_dock_size(DockSide::Left, 900.0, viewport), 700);
    }

    #[test]
    fn keeps_size_when_the_axis_stays_the_same() {
        let dock = ProjectTerminalDock {
            size: 200,
            ..create_project_terminal("/tmp/a", new_terminal_file("/tmp/a", None, None), None)
        };
        assert_eq!(with_dock_side(&dock, DockSide::Top, None).size, 200);
        assert_eq!(
            with_dock_side(&dock, DockSide::Top, None).side,
            DockSide::Top
        );
    }

    #[test]
    fn collapses_to_a_single_main_area_when_hidden() {
        assert_eq!(dock_grid_style(None, 220.0).grid_template_areas, "\"main\"");
    }

    #[test]
    fn places_the_dock_on_the_requested_edge() {
        assert_eq!(
            dock_grid_style(Some(DockSide::Bottom), 220.0).grid_template_areas,
            "\"main\" \"dock\""
        );
        assert_eq!(
            dock_grid_style(Some(DockSide::Top), 220.0).grid_template_areas,
            "\"dock\" \"main\""
        );
        assert_eq!(
            dock_grid_style(Some(DockSide::Left), 360.0).grid_template_areas,
            "\"dock main\""
        );
        assert_eq!(
            dock_grid_style(Some(DockSide::Right), 360.0).grid_template_areas,
            "\"main dock\""
        );
        assert_eq!(
            dock_grid_style(Some(DockSide::Left), 300.0).grid_template_columns,
            "300px minmax(0, 1fr)"
        );
    }

    #[test]
    fn lists_every_terminal_in_every_dock() {
        let a = new_terminal_file("/tmp/a", None, None);
        let b = new_terminal_file("/tmp/b", None, None);
        assert_eq!(
            project_terminal_file_ids(&[
                create_project_terminal("/tmp/a", a.clone(), None),
                create_project_terminal("/tmp/b", b.clone(), None),
            ]),
            vec![a.id, b.id]
        );
    }

    #[test]
    fn moves_a_dock_only_when_every_tab_of_that_project_is_leaving() {
        let a1 = new_tab("s1");
        let a2 = new_tab("s2");
        let b1 = WorkspaceTab {
            layout: leaf("s3"),
            ..new_tab("s3")
        };
        let sessions = [
            chat("s1", "/tmp/a"),
            chat("s2", "/tmp/a"),
            chat("s3", "/tmp/b"),
        ];
        let docks = vec![
            create_project_terminal("/tmp/a", new_terminal_file("/tmp/a", None, None), None),
            create_project_terminal("/tmp/b", new_terminal_file("/tmp/b", None, None), None),
        ];
        let (moving, remaining) = split_project_terminals_for_move(
            &docks,
            std::slice::from_ref(&a1),
            &[a2.clone(), b1.clone()],
            &sessions,
        );
        assert!(moving.is_empty());
        let paths: Vec<&str> = remaining
            .iter()
            .map(|dock| dock.project_path.as_str())
            .collect();
        assert_eq!(paths, vec!["/tmp/a", "/tmp/b"]);

        let (moving, remaining) =
            split_project_terminals_for_move(&docks, &[a1, a2], &[b1], &sessions);
        let moving: Vec<&str> = moving
            .iter()
            .map(|dock| dock.project_path.as_str())
            .collect();
        let remaining: Vec<&str> = remaining
            .iter()
            .map(|dock| dock.project_path.as_str())
            .collect();
        assert_eq!(moving, vec!["/tmp/a"]);
        assert_eq!(remaining, vec!["/tmp/b"]);
    }

    fn entity(cx: &mut TestAppContext) -> Entity<ProjectTerminals> {
        cx.new(|_| ProjectTerminals::new(Vec::new(), None))
    }

    #[gpui::test]
    fn opens_toggles_and_closes_through_the_entity(cx: &mut TestAppContext) {
        let terminals = entity(cx);
        let project = "/Users/me/repo";
        assert!(terminals.update(cx, |t, cx| t.open(project, project, cx)));
        assert!(terminals.update(cx, |t, cx| t.open(project, project, cx)));
        terminals.read_with(cx, |t, _| {
            let dock = t.dock(project).unwrap();
            assert_eq!(dock.pane.files.len(), 2);
            assert_eq!(dock.pane.files[1].path, "repo 2");
            assert!(t.is_focused());
            assert_eq!(t.visible_dock(project), Some((DockSide::Bottom, 220)));
        });

        terminals.update(cx, |t, cx| t.toggle(project, project, cx));
        terminals.read_with(cx, |t, _| {
            assert_eq!(t.visible_dock(project), None);
            assert!(!t.is_focused());
        });
        assert!(terminals.update(cx, |t, cx| t.show(project, cx)));
        terminals.update(cx, |t, cx| {
            t.set_side(project, DockSide::Left, None, cx);
        });
        terminals.read_with(cx, |t, _| {
            assert_eq!(t.last_dock_side(), Some(DockSide::Left));
            assert_eq!(t.visible_dock(project), Some((DockSide::Left, 220)));
        });

        let ids = terminals.read_with(cx, |t, _| t.file_ids());
        let closing = terminals
            .read_with(cx, |t, _| t.others_than(project, &ids[0]))
            .unwrap();
        let closing: HashSet<String> = closing.into_iter().map(|file| file.id).collect();
        terminals.update(cx, |t, cx| t.close_others(project, &ids[0], &closing, cx));
        assert_eq!(
            terminals.read_with(cx, |t, _| t.file_ids()),
            vec![ids[0].clone()]
        );
        terminals.update(cx, |t, cx| t.close(project, &ids[0], cx));
        assert!(terminals.read_with(cx, |t, _| t.dock(project).is_none()));

        // A new project's dock starts on the remembered side.
        terminals.update(cx, |t, cx| t.open("/Users/me/other", "/Users/me/other", cx));
        assert_eq!(
            terminals.read_with(cx, |t, _| t.visible_dock("/Users/me/other")),
            Some((DockSide::Left, 360))
        );
        // Remote and home folders get no dock.
        assert!(!terminals.update(cx, |t, cx| t.open("remote://box/repo", "/repo", cx)));
        assert!(!terminals.update(cx, |t, cx| t.open("~", "~", cx)));
    }

    #[gpui::test]
    fn toggles_a_running_terminal_from_the_chip(cx: &mut TestAppContext) {
        let terminals = entity(cx);
        let project = "/Users/me/repo";
        terminals.update(cx, |t, cx| t.open(project, project, cx));
        terminals.update(cx, |t, cx| t.open(project, project, cx));
        let ids = terminals.read_with(cx, |t, _| t.file_ids());
        assert_eq!(
            terminals.update(cx, |t, cx| t.toggle_running(&ids[0], cx)),
            DockToggle::Hidden
        );
        assert_eq!(
            terminals.update(cx, |t, cx| t.toggle_running(&ids[0], cx)),
            DockToggle::Shown
        );
        assert_eq!(
            terminals.read_with(cx, |t, _| t
                .dock(project)
                .unwrap()
                .pane
                .active_file_id
                .clone()),
            ids[0]
        );
        assert_eq!(
            terminals.update(cx, |t, cx| t.toggle_running("nope", cx)),
            DockToggle::NotInDock
        );
        terminals.update(cx, |t, cx| {
            t.reorder(project, &[ids[1].clone(), ids[0].clone()], cx)
        });
        assert_eq!(
            terminals.read_with(cx, |t, _| t.file_ids()),
            vec![ids[1].clone(), ids[0].clone()]
        );
        terminals.update(cx, |t, cx| t.rebase_project(project, "/Users/me/moved", cx));
        assert!(terminals.read_with(cx, |t, _| t.dock("/Users/me/moved").is_some()));
    }
}
