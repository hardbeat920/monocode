//! The window's pane tree and project terminal dock.
use crate::{file_pane::FilePane, remote_pane::RemotePane, session_pane::SessionPane};
use gpui::{
    AnyView, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    MouseMoveEvent, ParentElement as _, Pixels, Point, Render, StatefulInteractiveElement as _,
    Styled as _, Subscription, WeakEntity, Window, div,
};
use monocode_core::session::session_display_title;
use monocode_engine::{runtime::Engine, workspace::Workspace};
use monocode_layout::{find_surface_pane, leaf_ids, project_terminal::DockSide};
use monocode_ui::widgets::spinner;
use monocode_ui::{IconName, Theme, icon, u};
use monocode_view_inbox::pr::linked_panel::{
    LinkedPanelEvent, LinkedPanelProps, LinkedWorkItemPanel,
};
use monocode_view_workbench::panes::pane_tree::{
    PaneDragSource, PaneLeaf, PaneLeafKind, PaneTree, PaneTreeEvent,
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[cfg(test)]
#[path = "remote_workspace_tests.rs"]
mod remote_tests;

#[derive(Clone)]
enum SessionView {
    Local(Entity<SessionPane>),
    Remote(Entity<RemotePane>),
}
impl SessionView {
    fn list_navigation_allowed(&self, cx: &gpui::App) -> bool {
        match self {
            Self::Local(pane) => pane.read(cx).list_navigation_allowed(cx),
            Self::Remote(pane) => pane.read(cx).list_navigation_allowed(cx),
        }
    }

    fn session_shortcuts_blocked(&self, cx: &gpui::App) -> bool {
        match self {
            Self::Local(pane) => pane.read(cx).session_shortcuts_blocked(cx),
            Self::Remote(pane) => pane.read(cx).session_shortcuts_blocked(cx),
        }
    }

    fn new(
        id: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<WorkspaceArea>,
    ) -> Self {
        let remote = Engine::sessions(cx)
            .read(cx)
            .get(&id)
            .is_some_and(|session| monocode_layout::paths::is_remote_project_path(&session.cwd));
        if remote {
            Self::Remote(cx.new(|cx| RemotePane::new(id, workspace, window, cx)))
        } else {
            Self::Local(cx.new(|cx| SessionPane::new(id, workspace, window, cx)))
        }
    }
    fn set_focused(&self, focused: bool, window: &mut Window, cx: &mut Context<WorkspaceArea>) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.set_focused(focused, window, cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.set_focused(focused, window, cx)),
        }
    }
    fn set_visible(&self, visible: bool, window: &mut Window, cx: &mut Context<WorkspaceArea>) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.set_visible(visible, window, cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.set_visible(visible, window, cx)),
        }
    }
    fn focus_composer(&self, window: &mut Window, cx: &mut Context<WorkspaceArea>) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.focus_composer(window, cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.focus_composer(window, cx)),
        }
    }
    fn switch_model(&self, window: &mut Window, cx: &mut Context<WorkspaceArea>) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.switch_model(window, cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.switch_model(window, cx)),
        }
    }
    fn toggle_workspace_mode(&self, cx: &mut Context<WorkspaceArea>) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.toggle_workspace_mode(cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.toggle_workspace_mode(cx)),
        }
    }
    fn add_to_chat(
        &self,
        item: &monocode_engine::workspace::chat_context::ChatContextItem,
        window: &mut Window,
        cx: &mut Context<WorkspaceArea>,
    ) {
        match self {
            Self::Local(pane) => pane.update(cx, |pane, cx| pane.add_to_chat(item, window, cx)),
            Self::Remote(pane) => pane.update(cx, |pane, cx| pane.add_to_chat(item, window, cx)),
        }
    }
    fn view(self) -> AnyView {
        match self {
            Self::Local(view) => view.into(),
            Self::Remote(view) => view.into(),
        }
    }
}

#[derive(Clone, Copy)]
struct DockResize {
    side: DockSide,
    span: i64,
    start: Point<Pixels>,
}

pub struct WorkspaceArea {
    workspace: Option<Entity<Workspace>>,
    tree: Option<Entity<PaneTree>>,
    sessions: HashMap<String, SessionView>,
    files: HashMap<String, Entity<FilePane>>,
    dock: Option<Entity<FilePane>>,
    resize: Option<DockResize>,
    linked_panels: HashMap<String, Entity<LinkedWorkItemPanel>>,
    active_linked_panel: Option<String>,
    composer_target: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl WorkspaceArea {
    fn focused_session_view(&self, cx: &gpui::App) -> Option<&SessionView> {
        let tab = self.workspace.as_ref()?.read(cx).active_tab()?;
        self.sessions.get(&tab.focused_id)
    }

    pub fn list_navigation_allowed(&self, cx: &gpui::App) -> bool {
        self.focused_session_view(cx)
            .is_some_and(|pane| pane.list_navigation_allowed(cx))
    }

    pub fn session_shortcuts_blocked(&self, cx: &gpui::App) -> bool {
        self.focused_session_view(cx)
            .is_some_and(|pane| pane.session_shortcuts_blocked(cx))
    }

    pub fn new() -> Self {
        Self {
            workspace: None,
            tree: None,
            sessions: HashMap::new(),
            files: HashMap::new(),
            dock: None,
            resize: None,
            linked_panels: HashMap::new(),
            active_linked_panel: None,
            composer_target: None,
            _subscriptions: Vec::new(),
        }
    }

    fn attach(
        &mut self,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions
            .push(cx.observe_in(&workspace, window, |this, _, window, cx| {
                this.sync(window, cx)
            }));
        let sessions = Engine::sessions(cx);
        self._subscriptions
            .push(cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.sync(window, cx)
            }));
        if let Some(inbox) = monocode_engine::inbox::inbox::Inbox::try_global(cx) {
            self._subscriptions
                .push(cx.observe_in(&inbox, window, |this, _, window, cx| this.sync(window, cx)));
        }
        self.workspace = Some(workspace);
        self.sync(window, cx);
    }

    fn active_session_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<SessionView> {
        if self.workspace.is_none()
            && let Some(workspace) = crate::slots::window_workspace_for(window, cx)
        {
            self.attach(workspace, window, cx);
        }
        let workspace = self.workspace.clone()?;
        let tab = workspace.read(cx).active_tab()?;
        let sessions = Engine::sessions(cx);
        let session_id = if sessions.read(cx).get(&tab.focused_id).is_some() {
            Some(tab.focused_id.clone())
        } else {
            leaf_ids(&tab.layout)
                .into_iter()
                .find(|id| sessions.read(cx).get(id).is_some())
        }?;
        Some(
            self.sessions
                .entry(session_id.clone())
                .or_insert_with(|| SessionView::new(session_id, workspace.downgrade(), window, cx))
                .clone(),
        )
    }

    pub fn toggle_workspace_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_session_pane(window, cx) {
            pane.toggle_workspace_mode(cx);
        }
    }

    pub fn switch_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_session_pane(window, cx) {
            pane.switch_model(window, cx);
        }
    }

    pub fn add_to_chat(
        &mut self,
        item: &monocode_engine::workspace::chat_context::ChatContextItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.active_session_pane(window, cx) {
            pane.add_to_chat(item, window, cx);
        }
    }

    pub fn add_to_chat_to(
        &mut self,
        session_id: &str,
        item: &monocode_engine::workspace::chat_context::ChatContextItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.is_none()
            && let Some(workspace) = crate::slots::window_workspace_for(window, cx)
        {
            self.attach(workspace, window, cx);
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let pane = self
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| {
                SessionView::new(session_id.to_string(), workspace.downgrade(), window, cx)
            })
            .clone();
        pane.add_to_chat(item, window, cx);
    }

    fn sync_linked_panel(
        &mut self,
        tab: &monocode_layout::WorkspaceTab,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(inbox) = monocode_engine::inbox::inbox::Inbox::try_global(cx) else {
            return;
        };
        let ids = leaf_ids(&tab.layout);
        let active = inbox
            .read(cx)
            .active_linked_panel(&tab.focused_id, &ids)
            .cloned();
        let retained = inbox
            .read(cx)
            .linked_panels()
            .iter()
            .map(|panel| panel.session_id.clone())
            .collect::<HashSet<_>>();
        self.linked_panels.retain(|id, _| retained.contains(id));
        let visible = !workspace.read(cx).full_page_open();
        for (id, panel) in &self.linked_panels {
            let shown = visible
                && active
                    .as_ref()
                    .is_some_and(|active| &active.session_id == id);
            panel.update(cx, |panel, cx| panel.set_visible(shown, cx));
        }
        self.active_linked_panel = active.as_ref().map(|panel| panel.session_id.clone());
        let Some(active) = active else { return };
        if let Some(panel) = self.linked_panels.get(&active.session_id) {
            panel.update(cx, |panel, cx| panel.set_target(active.item, window, cx));
            return;
        }
        let services = Rc::new(crate::adapters::inbox::InboxAdapter::new(
            workspace.clone(),
            cx,
        ));
        let projects = monocode_engine::projects::ProjectsGlobal::try_global(cx)
            .map(|projects| {
                projects
                    .projects
                    .read(cx)
                    .recents()
                    .iter()
                    .map(|project| monocode_view_inbox::data::InboxProjectOption {
                        path: project.path.clone(),
                        name: monocode_layout::paths::project_name(&project.path),
                        mark: Default::default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let props = LinkedPanelProps {
            cwd: active.cwd,
            projects,
            visible,
            can_repair: true,
        };
        let panel = cx.new(|cx| LinkedWorkItemPanel::new(services, active.item, props, window, cx));
        let session_id = active.session_id.clone();
        self._subscriptions
            .push(cx.subscribe(&panel, move |_, _, event, cx| match event {
                LinkedPanelEvent::Close => inbox.update(cx, |inbox, cx| {
                    inbox.close_linked_work_item_panel(&session_id, cx)
                }),
                LinkedPanelEvent::OpenSession(id) => {
                    let history = monocode_engine::history::HistoryPackage::global(cx)
                        .history
                        .clone();
                    history
                        .update(cx, |history, cx| history.select_session(id, cx))
                        .detach();
                }
            }));
        self.linked_panels.insert(active.session_id, panel);
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let Some(tab) = workspace.read(cx).active_tab().cloned() else {
            return;
        };
        self.sync_linked_panel(&tab, &workspace, window, cx);
        if self.tree.is_none() {
            let tree = cx.new(|cx| PaneTree::new(tab.layout.clone(), tab.focused_id.clone(), cx));
            tree.update(cx, |tree, _| {
                tree.set_title_tab_hit_test(Some(Rc::new(
                    crate::shell::title_bar::title_tab_hit_test,
                )))
            });
            let target = workspace.downgrade();
            self._subscriptions.push(cx.subscribe_in(
                &tree,
                window,
                move |_, _, event: &PaneTreeEvent, window, cx| {
                    if let PaneTreeEvent::TitleTabDropChanged(drop) = event {
                        crate::shell::title_bar::set_title_tab_drop(drop.clone(), window, cx);
                        return;
                    }
                    let Some(workspace) = target.upgrade() else {
                        return;
                    };
                    if let PaneTreeEvent::Drop {
                        source: PaneDragSource::Session(host_id),
                        pane_id,
                        edge,
                    } = event
                        && Engine::sessions(cx).read(cx).get(host_id).is_none()
                    {
                        let cwd = workspace.read(cx).git_cwd(cx);
                        if monocode_layout::paths::is_remote_project_path(&cwd)
                            && let Some(remote) =
                                monocode_engine::remote::RemoteGlobal::try_global(cx)
                        {
                            let (connections, remote_sessions) =
                                (remote.connections.clone(), remote.sessions.clone());
                            let existing = Engine::sessions(cx)
                                .read(cx)
                                .all()
                                .iter()
                                .find(|session| {
                                    monocode_layout::paths::same_project_path(&session.cwd, &cwd)
                                        && connections
                                            .read(cx)
                                            .remote_session_for(&session.id)
                                            .as_deref()
                                            == Some(host_id)
                                })
                                .map(|session| session.id.clone());
                            let shell_id = existing.unwrap_or_else(|| {
                                let mut session =
                                    workspace.read(cx).new_default_session(&cwd, None);
                                if let Some(summary) = connections
                                    .read(cx)
                                    .cached_remote_session_summary(&cwd, host_id)
                                {
                                    session.harness = summary.harness;
                                    session.title = summary.title;
                                    session.model = summary.model.unwrap_or(session.model);
                                    session.runtime_mode =
                                        summary.runtime_mode.unwrap_or(session.runtime_mode);
                                }
                                let id = session.id.clone();
                                Engine::sessions(cx)
                                    .update(cx, |sessions, cx| sessions.insert(session, cx));
                                remote_sessions
                                    .update(cx, |sessions, cx| sessions.bind_tab(&id, host_id, cx));
                                id
                            });
                            workspace
                                .update(cx, |workspace, cx| {
                                    workspace.place_session_on_pane(&shell_id, pane_id, *edge, cx)
                                })
                                .detach();
                            return;
                        }
                    }
                    workspace.update(cx, |workspace, cx| match event {
                        PaneTreeEvent::Focus { pane_id } => workspace.focus_pane(pane_id, cx),
                        PaneTreeEvent::Close { session_id } => {
                            workspace.close_pane(Some(session_id), cx).detach()
                        }
                        PaneTreeEvent::Ratio {
                            split_id,
                            index,
                            ratio,
                        } => {
                            let tab = workspace.active_tab_id().to_string();
                            workspace.set_ratio(&tab, split_id, *index, *ratio, cx);
                        }
                        PaneTreeEvent::MovePane {
                            from_id,
                            to_id,
                            edge,
                        } => workspace.move_pane(from_id, to_id, *edge, cx),
                        PaneTreeEvent::DetachPane {
                            pane_id,
                            target_tab_id,
                            position,
                        } => workspace.detach_pane(pane_id, target_tab_id, *position, cx),
                        PaneTreeEvent::Drop {
                            source,
                            pane_id,
                            edge,
                        } => match source {
                            PaneDragSource::Session(id) => workspace
                                .place_session_on_pane(id, pane_id, *edge, cx)
                                .detach(),
                            PaneDragSource::WorkspaceTab(id) => {
                                workspace.place_tab_on_pane(id, pane_id, *edge, cx)
                            }
                        },
                        PaneTreeEvent::TitleTabDropChanged(_) => {}
                    });
                },
            ));
            self.tree = Some(tree);
        }
        let tree = self.tree.clone().unwrap();
        let live: HashSet<String> = workspace
            .read(cx)
            .tabs()
            .iter()
            .flat_map(|tab| leaf_ids(&tab.layout))
            .collect();
        let current_remote = Engine::sessions(cx)
            .read(cx)
            .all()
            .iter()
            .filter_map(|session| {
                monocode_layout::paths::is_remote_project_path(&session.cwd)
                    .then_some(session.id.clone())
            })
            .collect::<HashSet<_>>();
        self.sessions.retain(|id, pane| {
            live.contains(id)
                && matches!(pane, SessionView::Remote(_)) == current_remote.contains(id)
        });
        self.files.retain(|id, _| live.contains(id));
        let covered = workspace.read(cx).full_page_open();
        let visible = leaf_ids(&tab.layout);
        for (id, pane) in &self.sessions {
            pane.set_focused(!covered && id == &tab.focused_id, window, cx);
            pane.set_visible(visible.contains(id) && !covered, window, cx);
        }
        let composer_target = (!covered && workspace.read(cx).composer_focused())
            .then(|| tab.focused_id.clone())
            .filter(|id| find_surface_pane(&tab, id).is_none());
        let focus_changed = self.composer_target != composer_target;
        self.composer_target = composer_target.clone();
        let mut leaves = Vec::new();
        for id in leaf_ids(&tab.layout) {
            if find_surface_pane(&tab, &id).is_some() {
                let file = self
                    .files
                    .entry(id.clone())
                    .or_insert_with(|| {
                        cx.new(|cx| FilePane::new(id.clone(), workspace.downgrade(), window, cx))
                    })
                    .clone();
                file.update(cx, |file, cx| file.set_tree(Some(tree.downgrade()), cx));
                leaves.push(PaneLeaf {
                    id,
                    kind: PaneLeafKind::Surface,
                    view: file.into(),
                });
            } else {
                let session = self
                    .sessions
                    .entry(id.clone())
                    .or_insert_with(|| {
                        SessionView::new(id.clone(), workspace.downgrade(), window, cx)
                    })
                    .clone();
                session.set_focused(!covered && id == tab.focused_id, window, cx);
                session.set_visible(!covered, window, cx);
                if focus_changed && composer_target.as_ref() == Some(&id) {
                    session.focus_composer(window, cx);
                }
                let title = Engine::sessions(cx)
                    .read(cx)
                    .get(&id)
                    .map(|session| session_display_title(&session.title, session.harness))
                    .unwrap_or_default();
                leaves.push(PaneLeaf {
                    id,
                    kind: PaneLeafKind::Session {
                        title: title.into(),
                    },
                    view: session.view(),
                });
            }
        }
        tree.update(cx, |tree, cx| {
            tree.set_layout(tab.layout, tab.focused_id, cx);
            tree.set_leaves(leaves, cx);
        });
        let project = workspace.read(cx).project_cwd().to_string();
        let docks = workspace.read(cx).terminals().clone();
        let dock_id = docks
            .read(cx)
            .dock(&project)
            .filter(|dock| dock.open)
            .map(|dock| dock.pane.id.clone());
        if let Some(id) = dock_id {
            let changed = self
                .dock
                .as_ref()
                .is_none_or(|dock| dock.read(cx).pane_id() != id);
            if changed {
                self.dock = Some(cx.new(|cx| FilePane::new(id, workspace.downgrade(), window, cx)));
            }
        } else {
            self.dock = None;
        }
        cx.notify();
    }
}

impl Render for WorkspaceArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.workspace.is_none()
            && let Some(workspace) = crate::slots::window_workspace_for(window, cx)
        {
            self.attach(workspace, window, cx);
        }
        let theme = Theme::of(cx).clone();
        let Some(tree) = self.tree.clone() else {
            return div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(spinner("workspace-loading").color(theme.content(0.45)))
                .into_any_element();
        };
        let content = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(tree.clone());
        let mut body = div()
            .id("workspace-area")
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .on_drag_move::<PaneDragSource>(cx.listener(
                |this, event: &gpui::DragMoveEvent<PaneDragSource>, _, cx| {
                    if let Some(tree) = &this.tree {
                        let source = event.drag(cx).clone();
                        tree.update(cx, |tree, cx| {
                            tree.external_drag_move(source, event.event.position, cx);
                        });
                    }
                },
            ))
            .on_drop(cx.listener(|this, source: &PaneDragSource, window, cx| {
                if let Some(tree) = &this.tree {
                    let position = window.mouse_position();
                    tree.update(cx, |tree, cx| {
                        tree.external_drag_move(source.clone(), position, cx);
                        tree.external_drag_end(position, true, cx);
                    });
                }
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    if let Some(tree) = &this.tree {
                        tree.update(cx, |tree, cx| {
                            tree.external_drag_end(event.position, false, cx)
                        });
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                let Some(resize) = this.resize else { return };
                if event.pressed_button != Some(MouseButton::Left) {
                    this.resize = None;
                    return;
                }
                let scale = Theme::of(cx).ui_scale();
                let delta = if matches!(resize.side, DockSide::Top | DockSide::Bottom) {
                    f32::from(event.position.y - resize.start.y)
                } else {
                    f32::from(event.position.x - resize.start.x)
                } / scale;
                let delta = if matches!(resize.side, DockSide::Bottom | DockSide::Right) {
                    -delta
                } else {
                    delta
                };
                let size = window.viewport_size();
                let viewport = monocode_layout::project_terminal::Viewport {
                    width: f32::from(size.width) as f64 / scale as f64,
                    height: f32::from(size.height) as f64 / scale as f64,
                };
                if let Some(workspace) = &this.workspace {
                    workspace.update(cx, |workspace, cx| {
                        workspace.set_project_terminal_size(
                            resize.span as f64 + delta as f64,
                            Some(viewport),
                            cx,
                        )
                    });
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.resize = None),
            );
        if let Some(workspace) = self.workspace.clone()
            && let Some((side, span)) = workspace.read(cx).dock_layout(cx)
            && let Some(dock) = self.dock.clone()
        {
            let vertical = matches!(side, DockSide::Top | DockSide::Bottom);
            let before = matches!(side, DockSide::Top | DockSide::Left);
            let mut toolbar = div()
                .flex()
                .flex_none()
                .h(u(26.0))
                .items_center()
                .px(u(7.0))
                .gap(u(5.0))
                .bg(theme.colors.body_glass)
                .child(div().flex_1().text_size(u(11.0)).child("Terminal"));
            for (target, glyph) in [
                (DockSide::Top, IconName::PanelTop),
                (DockSide::Bottom, IconName::PanelBottom),
                (DockSide::Left, IconName::PanelLeft),
                (DockSide::Right, IconName::PanelRight),
            ] {
                let workspace = workspace.clone();
                toolbar = toolbar.child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "dock-side-{}",
                            target.as_str()
                        )))
                        .cursor_pointer()
                        .p(u(2.0))
                        .text_color(if target == side {
                            theme.colors.accent
                        } else {
                            theme.content(0.5)
                        })
                        .child(icon(glyph).size(u(12.0)))
                        .on_click(move |_, window, cx| {
                            let size = window.viewport_size();
                            let scale = Theme::of(cx).ui_scale() as f64;
                            let viewport = monocode_layout::project_terminal::Viewport {
                                width: f32::from(size.width) as f64 / scale,
                                height: f32::from(size.height) as f64 / scale,
                            };
                            workspace.update(cx, |workspace, cx| {
                                workspace.set_project_terminal_side(target, Some(viewport), cx)
                            });
                        }),
                );
            }
            let new = workspace.clone();
            let hide = workspace.clone();
            toolbar = toolbar
                .child(
                    div()
                        .id("dock-new")
                        .cursor_pointer()
                        .p(u(2.0))
                        .child(icon(IconName::Plus).size(u(12.0)))
                        .on_click(move |_, _, cx| {
                            new.update(cx, |workspace, cx| workspace.new_terminal(cx))
                        }),
                )
                .child(
                    div()
                        .id("dock-hide")
                        .cursor_pointer()
                        .p(u(2.0))
                        .child(icon(IconName::X).size(u(12.0)))
                        .on_click(move |_, _, cx| {
                            hide.update(cx, |workspace, cx| workspace.hide_project_terminal(cx))
                        }),
                );
            let panel = div()
                .flex()
                .flex_col()
                .flex_none()
                .min_h_0()
                .min_w_0()
                .child(toolbar)
                .child(dock);
            let panel = if vertical {
                panel.h(u(span as f32))
            } else {
                panel.w(u(span as f32))
            };
            let grip = div()
                .id("dock-resize")
                .flex_none()
                .bg(theme.colors.stroke)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        this.resize = Some(DockResize {
                            side,
                            span,
                            start: event.position,
                        });
                        cx.stop_propagation();
                    }),
                );
            let grip = if vertical {
                grip.h(u(3.0)).w_full().cursor_row_resize()
            } else {
                grip.w(u(3.0)).h_full().cursor_col_resize()
            };
            body = if vertical {
                body.flex_col()
            } else {
                body.flex_row()
            };
            body = if before {
                body.child(panel).child(grip).child(content)
            } else {
                body.child(content).child(grip).child(panel)
            };
        } else {
            body = body.child(content);
        }
        if let Some(panel) = self
            .active_linked_panel
            .as_ref()
            .and_then(|id| self.linked_panels.get(id))
            .cloned()
        {
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(body)
                .child(panel)
                .into_any_element()
        } else {
            body.into_any_element()
        }
    }
}
