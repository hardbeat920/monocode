//! Archiving and deleting projects from their menu, and opening them in an
//! external editor (useProjectMenu.tsx and App.tsx's onRemoveProject).
use super::*;
use monocode_core::external_editor::{self, ExternalEditor};

impl Shell {
    /// Finds installed editors once, off the main thread; the first lookup
    /// may read the login shell's PATH.
    pub(super) fn scan_external_editors(&mut self, cx: &mut Context<Self>) {
        let work = cx
            .background_executor()
            .spawn(async { external_editor::installed() });
        cx.spawn(async move |this, cx| {
            let editors = work.await;
            let _ = this.update(cx, |this, _| this.external_editors = Some(editors));
        })
        .detach();
    }
    pub(super) fn external_editor_menu(&self, path: &str) -> MenuAction {
        let item = MenuAction::new("external-editor", "Open in editor").icon("app-window");
        let none =
            |label: &str| vec![MenuAction::new("external-editor:none", label).disabled(true)];
        if path.starts_with("remote://") {
            return item
                .disabled(true)
                .submenu(none("Remote folders stay on their host"));
        }
        match &self.external_editors {
            None => item.disabled(true).submenu(none("Looking for editors…")),
            Some(editors) if editors.is_empty() => item.submenu(none("No supported editors found")),
            Some(editors) => item.submenu(
                editors
                    .iter()
                    .map(|ExternalEditor { id, name }| {
                        MenuAction::new(format!("external-editor:{id}"), *name)
                    })
                    .collect(),
            ),
        }
    }
    pub(super) fn open_in_external_editor(
        &mut self,
        editor: &str,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        let (editor, path) = (editor.to_owned(), path.to_owned());
        let work = cx
            .background_executor()
            .spawn(async move { external_editor::open(&editor, &path) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = work.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Closes the project's tabs and drops it from the rail. Its chats stay in
    /// history, and opening the folder again (or a new chat there) restores it.
    pub(super) fn archive_project(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_project(path, window, cx);
        self.workspace.archived_projects.retain(|p| p != path);
        self.workspace.archived_projects.insert(0, path.to_owned());
        self.persist(cx);
        self.sync_settings(cx);
        cx.notify();
    }
    pub(super) fn unarchive_project(&mut self, path: &str, cx: &mut Context<Self>) {
        self.workspace.add_project(path);
        self.persist(cx);
        self.sync_settings(cx);
        cx.notify();
    }
    /// Asks before deleting every conversation in the project. The folder on
    /// disk is left alone; opening it again brings the project back empty.
    pub(super) fn confirm_delete_project(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.project_sessions(&path).len();
        let detail = match count {
            0 => String::new(),
            1 => "Its 1 saved conversation will be deleted. ".into(),
            n => format!("Its {n} saved conversations will be deleted. "),
        } + "The folder on disk stays put, and opening it again brings the project back empty.";
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Delete “{}”?", self.project_label(&path)),
            Some(&detail),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.delete_project(&path, window, cx)
            });
        })
        .detach();
    }
    fn delete_project(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let sessions = self.project_sessions(path);
        self.leave_project(path, window, cx);
        self.workspace.archived_projects.retain(|p| p != path);
        self.workspace.project_rail_assignments.remove(path);
        self.workspace.project_appearance.remove(path);
        self.workspace.project_focus.remove(path);
        self.workspace.collapsed.remove(path);
        self.persist(cx);
        self.sync_settings(cx);
        for id in sessions {
            self.delete_session(id, window, cx);
        }
        cx.notify();
    }
    /// Asks before permanently deleting the conversation in a tab.
    pub(super) fn confirm_delete_session(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.session_metadata(id, cx).display_title();
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Delete “{title}”?"),
            Some("This conversation and its saved settings will be removed permanently."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| this.delete_session(id, window, cx));
        })
        .detach();
    }

    fn project_sessions(&self, path: &str) -> Vec<i64> {
        self.session_summaries
            .values()
            .filter(|summary| summary.cwd == path)
            .map(|summary| summary.id)
            .collect()
    }
    /// Closes the project's panes and moves to another project if it was current.
    fn leave_project(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let sessions: HashSet<i64> = self.project_sessions(path).into_iter().collect();
        let panes: Vec<i64> = self
            .workspace
            .open_sessions()
            .into_iter()
            .filter(|pane| sessions.contains(&self.workspace.session_for_pane(*pane)))
            .collect();
        for pane in panes {
            self.close_pane(pane, window, cx);
        }
        self.workspace.projects.retain(|p| p != path);
        self.workspace.project_rail_pinned.remove(path);
        if self.workspace.project == path {
            let next = self
                .workspace
                .projects
                .first()
                .cloned()
                .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
            if !self.workspace.projects.contains(&next) {
                self.workspace.add_project(&next);
            }
            self.select_project(next, window, cx);
        }
    }
}
