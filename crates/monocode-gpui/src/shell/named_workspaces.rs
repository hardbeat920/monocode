//! Named workspaces keep several projects together. Membership and each
//! workspace's tabs are UI state only: sessions keep their own execution
//! directories, and removing a project never closes its sessions or files.
use super::*;
use crate::input::{self, Input};
use gpui::EventEmitter;

/// The workspace menu's name field.
pub(super) struct WorkspaceName {
    input: Entity<Input>,
    _events: Vec<Subscription>,
}
pub(super) enum WorkspaceNameEvent {
    Changed(String),
    Close,
}
impl EventEmitter<WorkspaceNameEvent> for WorkspaceName {}
impl WorkspaceName {
    fn new(name: &str, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut field = Input::field("Workspace name", "Workspace name", content(0.35), cx);
            field.set_text(name.to_owned(), cx);
            field.select_range(0..field.content.len(), cx);
            field
        });
        let events = vec![
            cx.subscribe(&input, |this, _, _: &input::Changed, cx| {
                let name = this.input.read(cx).content.to_string();
                cx.emit(WorkspaceNameEvent::Changed(name));
            }),
            cx.subscribe(&input, |_, _, _: &input::Submitted, cx| {
                cx.emit(WorkspaceNameEvent::Close)
            }),
        ];
        Self {
            input,
            _events: events,
        }
    }
    fn focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}
impl Render for WorkspaceName {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().pb_1().child(
            div()
                .id("workspace-name")
                .debug_selector(|| "workspace-name".into())
                .h(px(32.))
                .flex()
                .items_center()
                .px_2p5()
                .rounded_lg()
                .border_1()
                .border_color(content(0.1))
                .bg(content(0.05))
                .child(self.input.clone()),
        )
    }
}

impl Shell {
    pub(super) fn named_workspace_mode(&self) -> bool {
        self.workspace.named_workspace().is_some()
    }

    /// A project shown in the sidebar: the workspace's members, or the one
    /// selected project.
    pub(super) fn listed_project(&self, path: &str) -> bool {
        if self.named_workspace_mode() {
            self.workspace.is_workspace_member(path)
        } else {
            path == self.workspace.project
        }
    }

    /// A tab that may be shown here: one the picked workspace owns, or for a
    /// single project, one no workspace owns.
    pub(super) fn tab_in_view(&self, tab: i64) -> bool {
        self.workspace.tab_in_named_workspace(tab)
    }

    pub(super) fn pane_project(&self, id: i64, cx: &App) -> Option<String> {
        let session = self.workspace.session_for_pane(id);
        self.session_summaries
            .contains_key(&session)
            .then(|| self.session_metadata(session, cx).summary.cwd.clone())
    }

    fn named_workspace_layout_changed(&mut self, cx: &mut Context<Self>) {
        self.sidebar_list.invalidate_structure();
        self.projects_frame.update(cx, |_, cx| cx.notify());
        self.persist(cx);
        cx.notify();
    }

    /// Show a workspace: its tabs, split layout, focused pane and project
    /// come back. Nothing closes, so drafts, agents and terminals keep running
    /// wherever they were left.
    pub(super) fn enter_named_workspace(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.active_named_workspace.as_deref() == Some(id)
            || self.workspace.named_workspaces.iter().all(|w| w.id != id)
        {
            return;
        }
        self.workspace.sync_pane_owners();
        self.workspace.active_named_workspace = Some(id.into());
        self.restore_named_view(window, cx);
    }

    /// Back to one project. Its own tabs return; workspace tabs stay saved.
    pub(super) fn leave_named_workspace(
        &mut self,
        project: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.sync_pane_owners();
        self.workspace.active_named_workspace = None;
        self.workspace.active = None;
        self.sidebar_list.invalidate_structure();
        self.projects_frame.update(cx, |_, cx| cx.notify());
        self.select_project(project, window, cx);
    }

    /// Pick a project from a picker: in a workspace that leaves it.
    pub(super) fn pick_project(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.named_workspace_mode() {
            self.leave_named_workspace(path, window, cx);
        } else if path != self.workspace.project {
            self.select_project(path, window, cx);
        }
    }

    fn restore_named_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The pane left behind must not join the incoming workspace.
        self.workspace.active = None;
        self.navigation_revision += 1;
        self.selected_sessions.clear();
        self.selection_anchor = None;
        self.sidebar_list.invalidate_structure();
        self.projects_frame.update(cx, |_, cx| cx.notify());
        if self.restore_scope_tab(window, cx) {
            return;
        }
        if let Some(tab) = self
            .visible_tabs(cx)
            .into_iter()
            .rev()
            .find_map(|tab| tab.session())
        {
            let focus = self.workspace.tab_focus(tab);
            self.open(focus, window, cx);
            return;
        }
        let first = self
            .workspace
            .named_workspace()
            .and_then(|w| w.projects.first().cloned());
        if let Some(project) = first {
            self.select_project(project, window, cx);
        }
    }

    /// A new workspace starts with the focused project. From a single
    /// project, its current tab comes along; from a workspace, it starts fresh.
    pub(super) fn new_named_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.sync_pane_owners();
        let from_workspace = self.named_workspace_mode();
        let project = self.workspace.project.clone();
        let name = format!("Workspace {}", self.workspace.named_workspaces.len() + 1);
        self.workspace
            .create_named_workspace(&name, vec![project.clone()]);
        if from_workspace || self.workspace.active.is_none() {
            self.workspace.active = None;
            self.sidebar_list.invalidate_structure();
            self.select_project(project, window, cx);
        } else {
            self.named_workspace_layout_changed(cx);
        }
    }

    /// Forget the current workspace. Its tabs stay open for their projects.
    fn delete_named_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.workspace.active_named_workspace.clone() else {
            return;
        };
        let project = self.workspace.project.clone();
        self.workspace.delete_named_workspace(&id);
        self.workspace.active = None;
        self.sidebar_list.invalidate_structure();
        self.projects_frame.update(cx, |_, cx| cx.notify());
        self.select_project(project, window, cx);
    }

    /// Blank sessions in a workspace of several projects may start elsewhere.
    fn project_choice(&self, id: i64, cx: &App) -> bool {
        self.workspace.named_workspace().is_some_and(|w| {
            w.projects.len() > 1 && {
                let session = self.session_metadata(id, cx);
                !session.summary.has_messages
                    && !session.busy
                    && w.projects.contains(&session.summary.cwd)
            }
        })
    }

    pub(super) fn sync_project_choices(&self, cx: &mut Context<Self>) {
        for (id, view) in &self.sessions {
            let choice = self.project_choice(*id, cx);
            if view.read(cx).project_choice() != choice {
                view.update(cx, |view, cx| view.set_project_choice(choice, cx));
            }
        }
    }

    pub(super) fn open_pending_project_choice(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some((id, anchor)) = self.pending_project_choice.take()
            && self.project_choice(id, cx)
        {
            self.open_menu(
                MenuTarget::SessionProject(id),
                Anchor::Slot(anchor),
                window,
                cx,
            );
        }
        None
    }

    pub(super) fn session_project_menu_items(&self, id: i64, cx: &App) -> Vec<MenuItem> {
        let Some(workspace) = self.workspace.named_workspace() else {
            return Vec::new();
        };
        let cwd = self.session_metadata(id, cx).summary.cwd.clone();
        workspace
            .projects
            .iter()
            .enumerate()
            .map(|(ix, path)| {
                MenuAction::new(format!("project:{ix}"), self.project_label(path))
                    .description(crate::pickers::pretty_parent(path))
                    .checked(*path == cwd)
                    .into()
            })
            .collect()
    }

    /// Start the blank pane's session in another member project. The blank
    /// session keeps its own directory; a new one takes its pane and draft.
    pub(super) fn start_blank_in_project(&mut self, id: i64, action: &str, cx: &mut Context<Self>) {
        let Some(path) = action
            .strip_prefix("project:")
            .and_then(|ix| ix.parse::<usize>().ok())
            .and_then(|ix| self.workspace.named_workspace()?.projects.get(ix).cloned())
        else {
            return;
        };
        if !self.project_choice(id, cx) || self.session_metadata(id, cx).summary.cwd == path {
            return;
        }
        let kind = Kind::from_id(&self.session_metadata(id, cx).summary.provider);
        self.create_session_target(kind, false, None, Some((path, Some(id))), cx);
    }

    /// Put `next` in `pane`'s place and hand it the unsent draft.
    pub(super) fn replace_blank_with(&mut self, pane: i64, next: i64, cx: &mut Context<Self>) {
        if self.workspace.tab_of(pane).is_none() || self.workspace.tab_of(next).is_some() {
            return;
        }
        let tab = self.workspace.tab_of(pane);
        self.workspace.swap_session(pane, next);
        if let Some((from, to)) = tab.zip(self.workspace.tab_of(next)) {
            self.replace_title_tab_key(from, to);
        }
        let draft = self
            .sessions
            .get(&pane)
            .map(|view| view.read(cx).input.read(cx).content.to_string());
        if let Some(draft) = draft.filter(|draft| !draft.is_empty())
            && let Some(view) = self.sessions.get(&next).cloned()
        {
            view.update(cx, |view, cx| {
                view.input.update(cx, |input, cx| input.set_text(draft, cx))
            });
            if let Some(old) = self.sessions.get(&pane).cloned() {
                old.update(cx, |view, cx| {
                    view.input
                        .update(cx, |input, cx| input.set_text(String::new(), cx))
                });
            }
        }
    }

    /// The workspace rows the project pickers offer.
    pub(super) fn picker_workspaces(&self) -> Vec<crate::pickers::PickerWorkspace> {
        self.workspace
            .named_workspaces
            .iter()
            .map(|w| crate::pickers::PickerWorkspace {
                id: w.id.clone(),
                name: w.name.clone(),
                projects: w.projects.len(),
            })
            .collect()
    }

    /// Take a project out of the current workspace. Its panes leave this
    /// workspace's layout, but its sessions, running agents, terminals,
    /// worktrees and files stay as they are.
    pub(super) fn remove_named_workspace_member(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.workspace.named_workspace().map(|w| w.id.clone()) else {
            return;
        };
        if !self.workspace.remove_workspace_member(path) {
            return;
        }
        let panes: Vec<_> = self
            .workspace
            .tabs
            .iter()
            .filter(|tab| self.workspace.tab_owner(**tab) == Some(current.as_str()))
            .flat_map(|tab| self.workspace.layout(*tab).leaf_ids())
            .filter(|id| self.pane_project(*id, cx).as_deref() == Some(path))
            .collect();
        for id in panes {
            if !self.workspace.close_pane(id) {
                self.workspace.close(id);
            }
        }
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        self.navigation_revision += 1;
        let focus_left = self.workspace.project == path
            || self
                .workspace
                .active_tab()
                .is_none_or(|tab| !self.workspace.tab_in_named_workspace(tab));
        if focus_left {
            self.workspace.active = None;
            if let Some(tab) = self
                .visible_tabs(cx)
                .into_iter()
                .rev()
                .find_map(|tab| tab.session())
            {
                let focus = self.workspace.tab_focus(tab);
                self.open(focus, window, cx);
            } else if let Some(project) = self
                .workspace
                .named_workspace()
                .and_then(|w| w.projects.first().cloned())
            {
                self.select_project(project, window, cx);
            }
        }
        self.named_workspace_layout_changed(cx);
    }

    fn toggle_named_workspace_member(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_workspace_member(path) {
            self.remove_named_workspace_member(path, window, cx);
        } else {
            self.workspace.add_workspace_member(path);
            self.named_workspace_layout_changed(cx);
        }
    }

    pub(super) fn toggle_named_workspace_group(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.named_workspace_mut()
            && !workspace.collapsed.remove(path)
        {
            workspace.collapsed.insert(path.into());
        }
        self.named_workspace_layout_changed(cx);
    }

    pub(super) fn named_workspace_menu_items(
        &self,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<MenuItem> {
        let Some(current) = self.workspace.named_workspace().cloned() else {
            return Vec::new();
        };
        let name = cx.new(|cx| WorkspaceName::new(&current.name, cx));
        let id = current.id.clone();
        events.push(cx.subscribe_in(
            &name,
            window,
            move |this, _, event: &WorkspaceNameEvent, window, cx| match event {
                WorkspaceNameEvent::Changed(name) => {
                    this.workspace.rename_named_workspace(&id, name);
                    this.named_workspace_layout_changed(cx);
                }
                WorkspaceNameEvent::Close => {
                    this.close_menu(cx);
                    this.focus_active(window, cx);
                }
            },
        ));
        let focus = name.read(cx).focus(cx);
        let mut items = vec![MenuItem::Custom(name.into(), focus)];
        items.push(MenuItem::Header(
            format!("Projects in {}", current.name).into(),
        ));
        let only = current.projects.len() < 2;
        let mut projects = current.projects.clone();
        projects.extend(
            self.workspace
                .projects
                .iter()
                .filter(|p| !current.projects.contains(p))
                .cloned(),
        );
        for (ix, path) in projects.iter().enumerate() {
            let member = current.projects.contains(path);
            items.push(
                MenuAction::new(format!("member:{ix}"), self.project_label(path))
                    .description(path.clone())
                    .toggle(member)
                    .disabled(member && only)
                    .keep_open()
                    .into(),
            );
        }
        items.push(
            MenuAction::new("open-project", "Open folder…")
                .icon("folder-add")
                .into(),
        );
        if only {
            items.push(MenuItem::Note(
                "A workspace needs at least one project.".into(),
            ));
        }
        items.extend([
            MenuItem::Separator,
            MenuAction::new("delete-workspace", "Delete workspace")
                .icon("trash")
                .description("Its projects, sessions and files stay")
                .danger()
                .into(),
        ]);
        items
    }

    /// The member at `ix` of the menu's project list.
    fn named_workspace_menu_project(&self, ix: usize) -> Option<String> {
        let current = self.workspace.named_workspace()?;
        current
            .projects
            .iter()
            .chain(
                self.workspace
                    .projects
                    .iter()
                    .filter(|p| !current.projects.contains(p)),
            )
            .nth(ix)
            .cloned()
    }

    /// A member toggle keeps the menu open and redraws it in place.
    pub(super) fn named_workspace_menu_toggle(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(path) = action
            .strip_prefix("member:")
            .and_then(|ix| ix.parse().ok())
            .and_then(|ix| self.named_workspace_menu_project(ix))
        else {
            return false;
        };
        self.toggle_named_workspace_member(&path, window, cx);
        true
    }

    pub(super) fn run_named_workspace_menu(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "delete-workspace" {
            self.delete_named_workspace(window, cx);
        } else if action == "open-project" {
            self.open_project(cx);
        } else {
            self.named_workspace_menu_toggle(action, window, cx);
        }
    }

    /// The sidebar's workspace switcher: name, member count and menu.
    pub(super) fn named_workspace_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(current) = self.workspace.named_workspace() else {
            return div().into_any();
        };
        let open = self.menu_open(&MenuTarget::Header);
        div()
            .id("named-workspace")
            .debug_selector(|| "named-workspace-button".into())
            .relative()
            .role(Role::Button)
            .aria_label(format!("Switch workspace, {}", current.name))
            .aria_expanded(open)
            .cursor_pointer()
            .h(px(28.))
            .px_1p5()
            .ml(px(-4.))
            .rounded_md()
            .min_w_0()
            .max_w_full()
            .hover(|s| s.bg(content(0.08)))
            .when(open, |d| d.bg(content(0.08)))
            .flex()
            .items_center()
            .gap_1p5()
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(icon("dashboard", 14.).text_color(content(0.6)))
            .child(div().min_w_0().truncate().child(current.name.clone()))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(content(0.45))
                    .child(projects_label(current.projects.len())),
            )
            .child(icon("chevron-down", 14.))
            .child(self.named_workspace_anchor.probe())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                if this.menu_open(&MenuTarget::Header) {
                    this.close_menu(cx);
                } else {
                    let anchor = Anchor::Slot(this.named_workspace_anchor.clone());
                    this.open_menu(MenuTarget::Header, anchor, window, cx);
                }
            }))
            .into_any()
    }

    /// A workspace in the original project rail. The picked one opens its
    /// menu; another one is entered.
    pub(super) fn rail_workspace_row(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(workspace) = self.workspace.named_workspaces.iter().find(|w| w.id == id) else {
            return div().into_any();
        };
        let current = self.workspace.active_named_workspace.as_deref() == Some(id);
        let key = id.to_owned();
        let selector = format!("rail-workspace-{id}");
        div()
            .id(SharedString::from(format!("rail-workspace-{id}")))
            .debug_selector(move || selector.clone())
            .role(Role::Button)
            .aria_label(format!(
                "{}, workspace, {}",
                workspace.name,
                projects_label(workspace.projects.len())
            ))
            .aria_selected(current)
            .cursor_pointer()
            .mx(px(6.))
            .h(px(30.))
            .px_1p5()
            .rounded_md()
            .flex()
            .items_center()
            .gap_1p5()
            .text_size(px(13.))
            .text_color(content(if current { 1. } else { 0.7 }))
            .when(current, |d| d.bg(content(0.08)))
            .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
            .child(icon("dashboard", 14.).text_color(content(0.6)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(workspace.name.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .child(workspace.projects.len().to_string()),
            )
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if this.workspace.active_named_workspace.as_deref() == Some(key.as_str()) {
                        let anchor = Anchor::Point(event.position());
                        this.open_menu(MenuTarget::NamedWorkspace, anchor, window, cx);
                    } else {
                        this.enter_named_workspace(&key, window, cx);
                    }
                }),
            )
            .into_any()
    }

    /// A project's heading in the workspace's session list.
    pub(super) fn named_workspace_group(
        &self,
        path: &str,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = !self.query.is_empty()
            || self
                .workspace
                .named_workspace()
                .is_none_or(|w| !w.collapsed.contains(path));
        let key = path.to_owned();
        let name = project_name(path);
        let selector = format!("workspace-project-{name}");
        let new_selector = format!("workspace-project-new-{name}");
        div()
            .id(SharedString::from(format!("workspace-project-{path}")))
            .group("workspace-project")
            .relative()
            .debug_selector(move || selector.clone())
            .role(Role::Button)
            .aria_label(format!("{}, {count} sessions", self.project_label(path)))
            .aria_expanded(expanded)
            .aria_description(path.to_owned())
            .cursor_pointer()
            .h(px(30.))
            .mt_1()
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_1p5()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(content(if path == self.workspace.project {
                0.9
            } else {
                0.6
            }))
            .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
            .child(
                icon(
                    if expanded {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    12.,
                )
                .text_color(content(0.4)),
            )
            .child(self.project_mark(path, 12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.project_label(path)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(content(0.4))
                    .group_hover("workspace-project", |s| s.invisible())
                    .child(count.to_string()),
            )
            .child(
                div()
                    .absolute()
                    .right(px(3.))
                    .top(px(3.))
                    .invisible()
                    .group_hover("workspace-project", |s| s.visible())
                    .child(
                        icon_button("workspace-project-new", "New session", "plus")
                            .aria_label(format!("New session in {}", self.project_label(path)))
                            .debug_selector(move || new_selector.clone())
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click({
                                let path = path.to_owned();
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    let kind = this.default_kind();
                                    this.create_session_target(
                                        kind,
                                        false,
                                        None,
                                        Some((path.clone(), None)),
                                        cx,
                                    );
                                })
                            }),
                    ),
            )
            .on_mouse_down(MouseButton::Right, {
                let path = path.to_owned();
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let target = MenuTarget::Project {
                        path: path.clone(),
                        rail: false,
                    };
                    this.open_menu(target, Anchor::Point(event.position), window, cx);
                })
            })
            .on_click(
                cx.listener(move |this, _, _, cx| this.toggle_named_workspace_group(&key, cx)),
            )
            .into_any()
    }

    /// A pane's project, while the workspace holds several projects.
    pub(super) fn badged_pane_project(&self, id: i64, cx: &App) -> Option<String> {
        self.workspace
            .named_workspace()
            .is_some_and(|w| w.projects.len() > 1)
            .then(|| self.pane_project(id, cx))
            .flatten()
    }

    /// The project badge in a pane header, once a workspace holds several
    /// projects, so each pane says where it runs.
    pub(super) fn pane_project_badge(&self, id: i64, cx: &App) -> Option<AnyElement> {
        let project = self.badged_pane_project(id, cx)?;
        let working_copy = self.session_working_copy(id, cx);
        let worktree = (monocode_core::files::path_key(working_copy)
            != monocode_core::files::path_key(&project))
        .then(|| project_name(working_copy));
        Some(
            div()
                .debug_selector({
                    let name = project_name(&project);
                    move || format!("pane-project-{name}")
                })
                .flex_shrink_0()
                .max_w(px(160.))
                .h(px(18.))
                .px_1p5()
                .rounded(px(5.))
                .bg(content(0.06))
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(11.))
                .text_color(content(0.6))
                .child(self.project_mark(&project, 10.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(self.project_label(&project)),
                )
                .when_some(worktree, |d, worktree| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(content(0.4))
                            .child(worktree),
                    )
                })
                .into_any(),
        )
    }
}

fn projects_label(count: usize) -> String {
    if count == 1 {
        "1 project".into()
    } else {
        format!("{count} projects")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    const OTHER: &str = "/work/another-project";
    const MONOCODE: &str = "/work/monocode";

    /// Both fixture projects open as separate tabs in one workspace.
    fn workspace_window(cx: &mut TestAppContext) -> (gpui::WindowHandle<Shell>, i64, i64) {
        let (window, a, b) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell
                    .workspace
                    .create_named_workspace("Workspace", vec![MONOCODE.into(), OTHER.into()]);
                shell.open(a, window, cx);
                shell.open(b, window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        (window, a, b)
    }

    fn visible_sessions(shell: &Shell, cx: &App) -> Vec<i64> {
        shell
            .visible_tabs(cx)
            .into_iter()
            .filter_map(|tab| tab.session())
            .collect()
    }

    #[gpui::test]
    fn picking_a_project_or_a_workspace_keeps_their_tabs_apart(cx: &mut TestAppContext) {
        let (window, a, b) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                // One project at a time: only the selected project's tab shows.
                assert_eq!(visible_sessions(shell, cx), vec![b]);
                // A new workspace starts from the focused project and its tab.
                shell.new_named_workspace(window, cx);
                let id = shell.workspace.named_workspace().unwrap().id.clone();
                assert_eq!(
                    shell.workspace.named_workspace().unwrap().projects,
                    vec![MONOCODE]
                );
                assert_eq!(visible_sessions(shell, cx), vec![b]);
                // Picking a project leaves the workspace; its tab stays there.
                shell.pick_project(OTHER.into(), window, cx);
                assert!(shell.workspace.named_workspace().is_none());
                assert_eq!(shell.workspace.project, OTHER);
                assert_eq!(visible_sessions(shell, cx), vec![a]);
                // Picking the workspace brings its tab and focus back.
                shell.enter_named_workspace(&id, window, cx);
                assert_eq!(visible_sessions(shell, cx), vec![b]);
                assert_eq!(shell.workspace.active, Some(b));
                // Deleting it returns the tab to its project.
                shell.delete_named_workspace(window, cx);
                assert!(shell.workspace.named_workspaces.is_empty());
                assert_eq!(shell.workspace.project, MONOCODE);
                assert_eq!(visible_sessions(shell, cx), vec![b]);
            })
            .unwrap();
    }

    #[gpui::test]
    fn focusing_a_mixed_split_pane_targets_its_project_without_moving_panes(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = workspace_window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.sessions[&a].update(cx, |chat, _| {
                    chat.settings.worktree_cwd = Some("/work/another-project-feature".into())
                });
                shell.workspace.place(a, b, Edge::Right);
                shell.persist(cx);
                let tab = shell.workspace.tab_of(b).unwrap();
                let order = shell.workspace.layout(tab).leaf_ids();
                assert_eq!(order, vec![b, a]);

                shell.focus_pane(b, window, cx);
                assert_eq!(shell.workspace.project, MONOCODE);
                assert_eq!(shell.git.read(cx).project(), MONOCODE);
                assert_eq!(shell.terminal_cwd(cx), MONOCODE);

                shell.focus_pane(a, window, cx);
                // Explorer, Changes and new terminals follow the pane's own
                // working copy; the dock belongs to its project.
                assert_eq!(shell.workspace.project, OTHER);
                assert_eq!(
                    shell.git.read(cx).project(),
                    "/work/another-project-feature"
                );
                assert_eq!(shell.terminal_cwd(cx), "/work/another-project-feature");
                let tab = shell.workspace.tab_of(a).unwrap();
                assert_eq!(shell.workspace.layout(tab).leaf_ids(), order);
                // The mixed tab stays on the strip whichever pane is focused.
                assert_eq!(visible_sessions(shell, cx), vec![tab]);
                // Sessions keep their own execution directories.
                assert_eq!(shell.sessions[&b].read(cx).summary.cwd, MONOCODE);
                assert_eq!(shell.sessions[&a].read(cx).summary.cwd, OTHER);
            })
            .unwrap();
    }

    #[gpui::test]
    fn switching_workspaces_restores_layout_focus_and_drafts(cx: &mut TestAppContext) {
        let (window, a, b) = workspace_window(cx);
        let first = window
            .update(cx, |shell, window, cx| {
                shell.workspace.place(a, b, Edge::Right);
                shell.persist(cx);
                shell.focus_pane(b, window, cx);
                shell.focus_pane(a, window, cx);
                shell.sessions[&b].update(cx, |chat, cx| {
                    chat.input
                        .update(cx, |input, cx| input.set_text("Unsent plan".into(), cx))
                });
                shell.new_named_workspace(window, cx);
                shell.workspace.named_workspaces[0].id.clone()
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                let second = shell.workspace.named_workspace().unwrap().clone();
                assert_eq!(second.name, "Workspace 2");
                assert_eq!(second.projects, vec![OTHER]);
                // The new workspace starts with its own tab, not the split.
                let shown = visible_sessions(shell, cx);
                assert!(!shown.contains(&shell.workspace.tab_of(a).unwrap()));
                assert_eq!(shown.len(), 1);

                shell.enter_named_workspace(&first, window, cx);
                let tab = shell.workspace.tab_of(a).unwrap();
                assert_eq!(visible_sessions(shell, cx), vec![tab]);
                assert_eq!(shell.workspace.layout(tab).leaf_ids(), vec![b, a]);
                assert_eq!(shell.workspace.active, Some(a));
                assert_eq!(shell.workspace.project, OTHER);
                assert_eq!(
                    shell.sessions[&b].read(cx).input.read(cx).content,
                    "Unsent plan"
                );
                // The second workspace's tab still exists, only out of view.
                shell.enter_named_workspace(&second.id, window, cx);
                assert_eq!(visible_sessions(shell, cx), shown);
            })
            .unwrap();
    }

    #[gpui::test]
    fn opening_a_session_from_another_workspace_moves_its_tab(cx: &mut TestAppContext) {
        let (window, a, b) = workspace_window(cx);
        window
            .update(cx, |shell, window, cx| {
                let first = shell.workspace.named_workspace().unwrap().id.clone();
                shell.new_named_workspace(window, cx);
                let second = shell.workspace.named_workspace().unwrap().id.clone();
                shell.enter_named_workspace(&first, window, cx);
                shell.enter_named_workspace(&second, window, cx);
                shell.open(b, window, cx);
                let tab = shell.workspace.tab_of(b).unwrap();
                assert!(visible_sessions(shell, cx).contains(&tab));
                assert!(shell.workspace.is_workspace_member(MONOCODE));
                shell.enter_named_workspace(&first, window, cx);
                assert!(!visible_sessions(shell, cx).contains(&tab));
                assert!(visible_sessions(shell, cx).contains(&a));
            })
            .unwrap();
    }

    #[gpui::test]
    fn removing_a_project_only_removes_membership(cx: &mut TestAppContext) {
        let (window, a, b) = workspace_window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.workspace.place(a, b, Edge::Right);
                shell.persist(cx);
                shell.focus_pane(b, window, cx);
                shell.focus_pane(a, window, cx);
                assert_eq!(shell.workspace.project, OTHER);
                shell.remove_named_workspace_member(OTHER, window, cx);
                let workspace = shell.workspace.named_workspace().unwrap();
                assert_eq!(workspace.projects, vec![MONOCODE]);
                // Its pane left this layout; its session and project remain.
                assert_eq!(shell.workspace.tab_of(a), None);
                assert!(shell.sessions.contains_key(&a));
                assert!(shell.session_summaries.contains_key(&a));
                assert!(shell.workspace.projects.iter().any(|p| p == OTHER));
                // Focus moved to a pane of a remaining project.
                assert_eq!(shell.workspace.active, Some(b));
                assert_eq!(shell.workspace.project, MONOCODE);
                assert_eq!(shell.git.read(cx).project(), MONOCODE);
                // The last project cannot leave.
                shell.remove_named_workspace_member(MONOCODE, window, cx);
                assert_eq!(
                    shell.workspace.named_workspace().unwrap().projects,
                    vec![MONOCODE]
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn sidebar_lists_member_sessions_and_searches_the_workspace(cx: &mut TestAppContext) {
        let (window, a, b) = workspace_window(cx);
        window
            .update(cx, |shell, window, cx| {
                // The selected project is MONOCODE, but the listed session
                // with messages belongs to the other member.
                assert_eq!(shell.workspace.project, MONOCODE);
                assert_eq!(shell.sidebar_session_ids(cx), vec![a]);
                shell.toggle_named_workspace_group(OTHER, cx);
                assert!(
                    shell
                        .workspace
                        .named_workspace()
                        .unwrap()
                        .collapsed
                        .contains(OTHER)
                );
                shell.query = "missing".into();
                assert!(shell.sidebar_session_ids(cx).is_empty());
                shell.query.clear();
                // Once it leaves the workspace, its sessions leave the list.
                shell.open(b, window, cx);
                shell.remove_named_workspace_member(OTHER, window, cx);
                assert!(shell.sidebar_session_ids(cx).is_empty());
            })
            .unwrap();
    }

    /// Drive the rendered shell: mixed panes, the switcher menu, membership
    /// and collapsing, in the original and the new layout.
    fn rendered_workspace_flow(cx: &mut TestAppContext, classic: bool) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, a, b) = workspace_window(cx);
        cx.simulate_window_resize(window.into(), gpui::size(px(1500.), px(900.)));
        window
            .update(cx, |shell, window, cx| {
                shell.workspace.classic_layout = classic;
                shell.workspace.sidebar_visible = true;
                shell.workspace.project_rail_open = true;
                shell.workspace.right_panel_open = false;
                shell.workspace.place(a, b, Edge::Right);
                shell.open(b, window, cx);
                shell.layout_changed(cx);
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let click = |selector: &'static str, visual: &mut gpui::VisualTestContext| {
            visual.refresh().unwrap();
            let bounds = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
            visual.refresh().unwrap();
        };
        visual.refresh().unwrap();
        for selector in [
            "named-workspace-button",
            "workspace-project-monocode",
            "workspace-project-another-project",
            "pane-project-monocode",
            "pane-project-another-project",
        ] {
            assert!(visual.debug_bounds(selector).is_some(), "{selector}");
        }
        if classic {
            assert!(visual.debug_bounds("rail-workspace-workspace-1").is_some());
        }
        // Clicking the other pane focuses its project; panes stay put.
        click(format!("split-header-{a}").leak(), visual);
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.workspace.project, OTHER);
                assert_eq!(shell.git.read(cx).project(), OTHER);
                let tab = shell.workspace.tab_of(a).unwrap();
                assert_eq!(shell.workspace.layout(tab).leaf_ids(), vec![b, a]);
            })
            .unwrap();
        // Hovering a project's heading offers a new session there.
        visual.refresh().unwrap();
        let heading = visual
            .debug_bounds("workspace-project-another-project")
            .unwrap();
        visual.simulate_mouse_move(heading.center(), None, Default::default());
        let count = window
            .update(visual, |shell, _, _| shell.sessions.len())
            .unwrap();
        click("workspace-project-new-another-project", visual);
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.sessions.len(), count + 1);
                let next = shell.workspace.active.unwrap();
                assert!(![a, b].contains(&next));
                assert_eq!(shell.sessions[&next].read(cx).summary.cwd, OTHER);
                assert_eq!(shell.workspace.project, OTHER);
                // A tab of its own; the heading stays expanded.
                assert_eq!(shell.workspace.tab_of(next), Some(next));
                assert!(visible_sessions(shell, cx).contains(&next));
                let workspace = shell.workspace.named_workspace().unwrap();
                assert!(!workspace.collapsed.contains(OTHER));
                assert_eq!(
                    shell
                        .workspace
                        .layout(shell.workspace.tab_of(a).unwrap())
                        .leaf_ids(),
                    vec![b, a]
                );
            })
            .unwrap();
        window
            .update(visual, |shell, window, cx| {
                let next = shell.workspace.active.unwrap();
                shell.close_pane(next, window, cx);
                shell.focus_pane(a, window, cx);
            })
            .unwrap();
        visual.run_until_parked();
        // Collapse a project's group from its heading.
        click("workspace-project-another-project", visual);
        window
            .update(visual, |shell, _, _| {
                let workspace = shell.workspace.named_workspace().unwrap();
                assert!(workspace.collapsed.contains(OTHER));
            })
            .unwrap();
        // A second workspace from the picker, then back to the first.
        click("named-workspace-button", visual);
        click("menu-action-switch-project", visual);
        click("project-picker-new-workspace", visual);
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.workspace.named_workspaces.len(), 2);
                assert!(!visible_sessions(shell, cx).contains(&shell.workspace.tab_of(a).unwrap()));
            })
            .unwrap();
        click("named-workspace-button", visual);
        click("menu-action-switch-project", visual);
        click("project-picker-workspace-0", visual);
        window
            .update(visual, |shell, _, cx| {
                let tab = shell.workspace.tab_of(a).unwrap();
                assert_eq!(visible_sessions(shell, cx), vec![tab]);
                assert_eq!(shell.workspace.layout(tab).leaf_ids(), vec![b, a]);
                assert_eq!(shell.workspace.active, Some(a));
            })
            .unwrap();
        // The member toggle keeps the menu open and removes only membership.
        click("named-workspace-button", visual);
        click("menu-action-edit-workspace", visual);
        click("menu-action-member:1", visual);
        assert!(visual.debug_bounds("menu-action-member:0").is_some());
        window
            .update(visual, |shell, _, _| {
                assert_eq!(
                    shell.workspace.named_workspace().unwrap().projects,
                    vec![MONOCODE]
                );
                assert_eq!(shell.workspace.tab_of(a), None);
                assert!(shell.sessions.contains_key(&a));
                assert_eq!(shell.workspace.project, MONOCODE);
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_blank_session_title_starts_it_in_another_workspace_project(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, a, b) = workspace_window(cx);
        cx.simulate_window_resize(window.into(), gpui::size(px(1500.), px(900.)));
        window
            .update(cx, |shell, _, cx| {
                // Only blank sessions in a workspace of several projects choose.
                assert!(shell.sessions[&b].read(cx).project_choice());
                assert!(!shell.sessions[&a].read(cx).project_choice());
                shell.sessions[&b].update(cx, |chat, cx| {
                    chat.input
                        .update(cx, |input, cx| input.set_text("Unsent plan".into(), cx))
                });
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for selector in ["empty-session-project", "menu-action-project:1"] {
            visual.refresh().unwrap();
            let bounds = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
        }
        window
            .update(visual, |shell, _, cx| {
                let next = shell.workspace.active.unwrap();
                assert_ne!(next, b);
                let chat = shell.sessions[&next].read(cx);
                assert_eq!(chat.summary.cwd, OTHER);
                assert_eq!(chat.input.read(cx).content.to_string(), "Unsent plan");
                assert_eq!(shell.workspace.project, OTHER);
                // The new session takes the blank's pane; the blank keeps its
                // own directory and gives up its draft.
                assert_eq!(shell.workspace.tab_of(b), None);
                assert!(
                    visible_sessions(shell, cx).contains(&shell.workspace.tab_of(next).unwrap())
                );
                let blank = shell.sessions[&b].read(cx);
                assert_eq!(blank.summary.cwd, MONOCODE);
                assert!(blank.input.read(cx).content.is_empty());
                assert!(shell.sessions[&next].read(cx).project_choice());
            })
            .unwrap();
        // A single project has nothing to choose between.
        window
            .update(visual, |shell, window, cx| {
                shell.pick_project(MONOCODE.into(), window, cx);
                shell.persist(cx);
                assert!(!shell.sessions[&b].read(cx).project_choice());
            })
            .unwrap();
    }

    #[gpui::test]
    fn rendered_workspace_flow_in_the_new_layout(cx: &mut TestAppContext) {
        rendered_workspace_flow(cx, false);
    }

    #[gpui::test]
    fn rendered_workspace_flow_in_the_original_layout(cx: &mut TestAppContext) {
        rendered_workspace_flow(cx, true);
    }
}
