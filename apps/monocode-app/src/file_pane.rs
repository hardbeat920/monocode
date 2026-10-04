//! Native file surfaces and their workspace tab strip.
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, WeakEntity, Window, div,
};
use monocode_app::boot::AppServices;
use monocode_engine::{
    remote::RemoteGlobal,
    runtime::Engine,
    submit::Submit,
    workspace::{Terminals, Workspace, WorkspaceEvent},
};
use monocode_layout::{EditorPane, find_surface_pane};
use monocode_ui::Theme;
use monocode_view_files::{
    EditorNavigation, EditorSettings, ExternalSurface, FilePane as NativeFilePane, FilePaneEvent,
    SurfaceRequest,
};
use monocode_view_workbench::panes::{
    pane_tree::PaneTree,
    surface_tabs::{SurfaceTabActions, SurfaceTabs, SurfaceTabsEvent, SurfaceTabsProps},
};
use std::{rc::Rc, sync::Arc};

pub struct FilePane {
    pane_id: String,
    workspace: WeakEntity<Workspace>,
    pane: Option<Entity<NativeFilePane>>,
    tabs: Entity<SurfaceTabs>,
    tree: Option<WeakEntity<PaneTree>>,
    _subscriptions: Vec<Subscription>,
}

impl FilePane {
    pub fn new(
        pane_id: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tabs = cx
            .new(|cx| SurfaceTabs::new(SurfaceTabsProps::default(), Rc::new(NativeTabActions), cx));
        let mut this = Self {
            pane_id,
            workspace,
            pane: None,
            tabs: tabs.clone(),
            tree: None,
            _subscriptions: Vec::new(),
        };
        this._subscriptions.push(cx.subscribe_in(
            &tabs,
            window,
            |this, _, event: &SurfaceTabsEvent, window, cx| {
                if let SurfaceTabsEvent::PaneDragStart { position } = event {
                    if let Some(tree) = this.tree.as_ref().and_then(|tree| tree.upgrade()) {
                        tree.update(cx, |tree, cx| {
                            tree.start_pane_drag(&this.pane_id, *position, window, cx)
                        });
                    }
                    return;
                }
                let Some(workspace) = this.workspace.upgrade() else {
                    return;
                };
                let pane_id = this.pane_id.clone();
                let dock = this.is_dock(cx);
                workspace.update(cx, |workspace, cx| match event {
                    SurfaceTabsEvent::Select(id) => {
                        if dock {
                            workspace.select_project_terminal(id, cx)
                        } else {
                            workspace.select_file_surface(&pane_id, id, cx)
                        }
                    }
                    SurfaceTabsEvent::Close(id) => {
                        if dock {
                            workspace.close_project_terminal(id, cx).detach()
                        } else {
                            workspace.close_file(&pane_id, id, cx).detach()
                        }
                    }
                    SurfaceTabsEvent::CloseOthers(id) => {
                        if dock {
                            workspace.close_other_project_terminals(id, cx).detach()
                        } else {
                            workspace.close_other_files(&pane_id, id, cx).detach()
                        }
                    }
                    SurfaceTabsEvent::Pin(id) => workspace.pin_file(id, cx),
                    SurfaceTabsEvent::Reorder { ids, .. } => {
                        if dock {
                            workspace.reorder_project_terminals(ids, cx)
                        } else {
                            workspace.reorder_files(&pane_id, ids, cx)
                        }
                    }
                    SurfaceTabsEvent::PaneDragStart { .. } => {}
                });
            },
        ));
        if let Some(workspace) = this.workspace.upgrade() {
            this._subscriptions
                .push(cx.observe_in(&workspace, window, |this, _, window, cx| {
                    this.sync(window, cx)
                }));
            this._subscriptions.push(cx.subscribe_in(
                &workspace,
                window,
                |this, _, event: &WorkspaceEvent, window, cx| {
                    if let WorkspaceEvent::EditorNavigation(target) = event
                        && let Some(pane) = &this.pane
                    {
                        let navigation = EditorNavigation {
                            path: target.path.clone(),
                            line: target.line.max(1) as usize,
                            column: target.column.map(|column| column.max(1) as usize),
                            token: target.token as u64,
                        };
                        pane.update(cx, |pane, cx| {
                            pane.set_navigation(Some(navigation), window, cx)
                        });
                    }
                },
            ));
        }
        let sessions = Engine::sessions(cx);
        this._subscriptions
            .push(cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.sync(window, cx)
            }));
        this.sync(window, cx);
        this
    }

    pub fn pane_id(&self) -> &str {
        &self.pane_id
    }
    pub fn set_tree(&mut self, tree: Option<WeakEntity<PaneTree>>, _: &mut Context<Self>) {
        self.tree = tree;
    }

    fn is_dock(&self, cx: &App) -> bool {
        self.workspace.upgrade().is_some_and(|workspace| {
            workspace
                .read(cx)
                .terminals()
                .read(cx)
                .docks()
                .iter()
                .any(|dock| dock.pane.id == self.pane_id)
        })
    }

    fn model(&self, cx: &App) -> Option<EditorPane> {
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        workspace
            .tabs()
            .iter()
            .find_map(|tab| find_surface_pane(tab, &self.pane_id).map(|(_, pane)| pane.clone()))
            .or_else(|| {
                workspace
                    .terminals()
                    .read(cx)
                    .docks()
                    .iter()
                    .find(|dock| dock.pane.id == self.pane_id)
                    .map(|dock| dock.pane.clone())
            })
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model(cx) else { return };
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let focused = workspace
            .read(cx)
            .active_tab()
            .is_some_and(|tab| tab.focused_id == self.pane_id)
            || self.is_dock(cx) && workspace.read(cx).terminals().read(cx).is_focused();
        let settings = AppServices::try_global(cx).map(|services| &services.kv);
        let unified = settings.is_some_and(|kv| {
            monocode_settings::settings_store::load_diff_viewer(kv)
                == monocode_core::settings::DiffViewer::Unified
        });
        let editor_settings = settings
            .map(|kv| EditorSettings {
                autosave: monocode_settings::settings_store::load_autosave(kv),
                format_on_save: monocode_settings::settings_store::load_format_on_save(kv),
            })
            .unwrap_or_default();
        let sessions = Rc::new(Engine::sessions(cx).read(cx).all().to_vec());
        let show_tabs = self.is_dock(cx)
            || workspace.read(cx).active_tab().is_none_or(|tab| {
                tab.editor_panes.len() + tab.terminal_panes.len() != 1
                    || !monocode_layout::leaf_ids(&tab.layout)
                        .iter()
                        .all(|id| id == &self.pane_id)
            });
        let props = SurfaceTabsProps {
            files: model.files.clone(),
            active_file_id: model.active_file_id.clone(),
            dirty_file_ids: workspace.read(cx).dirty_files().clone(),
            file_error_counts: workspace
                .read(cx)
                .file_error_counts()
                .iter()
                .map(|(id, count)| (id.clone(), (*count).max(0) as usize))
                .collect(),
            can_pin: !self.is_dock(cx),
            can_drag_pane: !self.is_dock(cx),
            ..Default::default()
        };
        self.tabs.update(cx, |tabs, cx| tabs.set_props(props, cx));
        if self.pane.is_none() {
            let data = crate::adapters::files::app_files(cx);
            let factory = Rc::new(external_surface);
            let pane =
                cx.new(|cx| NativeFilePane::new(data, model.clone(), Some(factory), window, cx));
            let tabs = self.tabs.clone();
            pane.update(cx, |pane, cx| {
                pane.set_tab_strip(
                    Some(Rc::new(move |_, _, _| tabs.clone().into_any_element())),
                    cx,
                )
            });
            if let Some(source) = crate::session_threads::model_menu_source(cx) {
                pane.update(cx, |pane, cx| pane.set_plan_model_source(source, cx));
            }
            let target = self.workspace.clone();
            self._subscriptions.push(cx.subscribe(
                &pane,
                move |_, _, event: &FilePaneEvent, cx| {
                    let Some(workspace) = target.upgrade() else {
                        return;
                    };
                    match event {
                        FilePaneEvent::Focus { pane_id } => {
                            workspace.update(cx, |workspace, cx| {
                                let dock_file = workspace
                                    .terminals()
                                    .read(cx)
                                    .docks()
                                    .iter()
                                    .find(|dock| &dock.pane.id == pane_id)
                                    .map(|dock| dock.pane.active_file_id.clone());
                                if let Some(file) = dock_file {
                                    workspace.select_project_terminal(&file, cx)
                                } else {
                                    workspace.focus_pane(pane_id, cx)
                                }
                            })
                        }
                        FilePaneEvent::DirtyChanged { file_id, dirty } => workspace
                            .update(cx, |workspace, cx| {
                                workspace.file_dirty_change(file_id, *dirty, cx)
                            }),
                        FilePaneEvent::OpenFile { path } => workspace
                            .update(cx, |workspace, cx| {
                                workspace.open_file(path, None, Default::default(), cx)
                            })
                            .detach(),
                        FilePaneEvent::UpdatePlan {
                            session_id,
                            block_id,
                            text,
                        } => update_plan(session_id, block_id, text, cx),
                        FilePaneEvent::BuildPlan {
                            session_id,
                            block_id,
                            target,
                        } => build_plan(session_id, block_id, target.clone(), cx),
                        FilePaneEvent::AddToChat(selection) => {
                            Submit::global(cx).update(cx, |submit, cx| {
                                submit.request_add_to_chat(
                                    monocode_engine::submit::chat_context::ChatContextItem::Code {
                                        path: selection.path.clone(),
                                        start_line: selection.start_line as i64,
                                        end_line: selection.end_line as i64,
                                    },
                                    cx,
                                )
                            });
                        }
                    }
                },
            ));
            self.pane = Some(pane);
        }
        self.pane.as_ref().unwrap().update(cx, |pane, cx| {
            pane.set_pane(model, window, cx);
            pane.set_unified_diffs(unified, window, cx);
            pane.set_sessions(sessions, window, cx);
            pane.set_settings(editor_settings, window, cx);
            pane.set_show_tabs(show_tabs, cx);
            pane.set_focused(focused, window, cx);
        });
        cx.notify();
    }
}

impl Render for FilePane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .children(self.pane.clone())
    }
}

fn remote_plan_session(session_id: &str, cx: &App) -> Option<monocode_core::Session> {
    Engine::sessions(cx)
        .read(cx)
        .get(session_id)
        .filter(|session| {
            monocode_layout::paths::is_remote_project_path(&session.cwd)
                || RemoteGlobal::is_remote_session(session, cx)
        })
        .cloned()
}

fn update_plan(session_id: &str, block_id: &str, text: &str, cx: &mut App) {
    // Remote plan sources are read-only, as in the original file editor.
    if remote_plan_session(session_id, cx).is_some() {
        return;
    }
    Submit::global(cx).update(cx, |submit, cx| {
        submit.update_plan(session_id, block_id, text, cx)
    });
}

fn build_plan(
    session_id: &str,
    block_id: &str,
    target: Option<monocode_core::block::PlanBuildTarget>,
    cx: &mut App,
) {
    if let Some(shell) = remote_plan_session(session_id, cx) {
        if let Some(sessions) = RemoteGlobal::try_global(cx).map(|remote| remote.sessions.clone()) {
            if sessions.read(cx).session(session_id).is_none() {
                sessions.update(cx, |sessions, cx| {
                    sessions.open(&shell, false, cx);
                });
            }
            RemoteGlobal::build_plan(session_id, block_id, target.as_ref(), cx);
        }
        return;
    }
    Submit::global(cx).update(cx, |submit, cx| {
        submit.build_plan(session_id, block_id, target, cx)
    });
}

fn external_surface(
    request: &SurfaceRequest<'_>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyView> {
    let file = request.file;
    match request.kind {
        ExternalSurface::Terminal => {
            let pty = Terminals::global(cx).attach(&file.id, &file.cwd, cx);
            let theme = terminal_theme(cx);
            let terminal = cx.new(|cx| {
                let terminal = monocode_terminal_view::TerminalView::new(pty, theme, window, cx);
                let appearance = cx.observe_global::<Theme>(|terminal, cx| {
                    let theme = terminal_theme(cx);
                    terminal.set_theme(theme, cx);
                });
                cx.on_release(move |_, _| drop(appearance)).detach();
                terminal
            });
            cx.subscribe(
                &terminal,
                |_, event: &monocode_terminal_view::TerminalEvent, cx| {
                    if let monocode_terminal_view::TerminalEvent::OpenUrl(url) = event {
                        cx.open_url(url);
                    }
                },
            )
            .detach();
            Some(terminal.into())
        }
        ExternalSurface::Agent => {
            let id = file.agent.as_ref()?.session_id.clone();
            Some(cx.new(|cx| WorkerSurface::new(id, cx)).into())
        }
        ExternalSurface::ReleaseNotes => {
            let view = cx.new(monocode_markdown::MarkdownView::new);
            let markdown = include_str!("../../../CHANGELOG.md");
            view.update(cx, |view, cx| view.set_text(markdown, cx));
            Some(view.into())
        }
        _ => crate::adapters::scm::diff_surface(request, window, cx),
    }
}

pub fn terminal_theme(cx: &App) -> monocode_terminal_view::TerminalTheme {
    let theme = Theme::of(cx);
    let mut terminal = if theme.is_dark() {
        monocode_terminal_view::TerminalTheme::dark()
    } else {
        monocode_terminal_view::TerminalTheme::light()
    };
    terminal.foreground = theme.colors.content.into();
    terminal.base_background = theme.colors.background_base.into();
    terminal.cursor = theme.colors.accent.into();
    terminal.font_family = theme.fonts.mono.clone();
    terminal
}

struct NativeTabActions;
impl SurfaceTabActions for NativeTabActions {
    fn open_with_default_app(&self, path: &str, cx: &mut App) -> Task<Result<(), String>> {
        native_path_action(path, false, cx)
    }
    fn reveal(&self, path: &str, cx: &mut App) -> Task<Result<(), String>> {
        native_path_action(path, true, cx)
    }
}
fn native_path_action(path: &str, reveal: bool, cx: &App) -> Task<Result<(), String>> {
    let path = path.to_string();
    cx.background_spawn(async move {
        let mut command = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("explorer")
        } else {
            std::process::Command::new("xdg-open")
        };
        if reveal && cfg!(target_os = "macos") {
            command.arg("-R");
        }
        let status = command
            .arg(path)
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("The file action exited with {status}"))
        }
    })
}

struct WorkerSurface {
    session_id: String,
    view: Entity<monocode_view_workbench::panes::agent_tab_view::AgentTabView>,
    transcript: Entity<monocode_view_transcript::transcript::TranscriptView>,
    _subscription: Subscription,
}
impl WorkerSurface {
    fn new(session_id: String, cx: &mut Context<Self>) -> Self {
        let view =
            cx.new(|_| monocode_view_workbench::panes::agent_tab_view::AgentTabView::new("Agent"));
        let transcript = cx.new(monocode_view_transcript::transcript::TranscriptView::new);
        let sessions = Engine::sessions(cx);
        let subscription = cx.observe(&sessions, |this, _, cx| this.sync(cx));
        let mut this = Self {
            session_id,
            view,
            transcript,
            _subscription: subscription,
        };
        this.sync(cx);
        this
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let session = Engine::sessions(cx).read(cx).get(&self.session_id).cloned();
        if let Some(session) = session {
            let model = monocode_view_workbench::panes::agent_tab_view::AgentTabSession {
                id: session.id.clone(),
                title: session.title.clone(),
                harness: session.harness,
                cwd: session.cwd.clone(),
                model_name: session.model.clone(),
            };
            self.transcript.update(cx, |view, cx| {
                view.set_config(
                    monocode_view_transcript::transcript::TranscriptConfig {
                        managed: true,
                        ..Default::default()
                    },
                    cx,
                );
                view.set_session(Arc::new(session), cx);
            });
            let transcript = self.transcript.clone().into();
            self.view.update(cx, |view, cx| {
                view.set_session(Some(model), cx);
                view.set_transcript(Some(transcript), cx);
            });
        } else {
            self.view.update(cx, |view, cx| view.set_session(None, cx));
        }
        cx.notify();
    }
}
impl Render for WorkerSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.view.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_engine::workspace::terminals::PtyBackend;
    use monocode_terminal::pty::PtyEvents;
    use monocode_terminal_view::TerminalView;
    use monocode_ui::{AppearanceSettings, ThemePreference, set_appearance};
    use monocode_view_files::{LocalFiles, file_pane::Surface};
    use parking_lot::Mutex;

    #[derive(Debug, PartialEq, Eq)]
    enum PtyCall {
        Spawn(String, String),
        Write(String, Vec<u8>),
        Kill(String),
        KillAll,
    }

    #[derive(Default)]
    struct ControlledPty {
        calls: Mutex<Vec<PtyCall>>,
        events: Mutex<Option<Arc<dyn PtyEvents>>>,
    }

    impl ControlledPty {
        fn output(&self, id: &str, bytes: &[u8]) {
            self.events.lock().as_ref().unwrap().data(id, bytes);
        }
    }

    impl PtyBackend for ControlledPty {
        fn spawn(&self, id: &str, cwd: &str, _: u16, _: u16) -> Result<(), String> {
            self.calls
                .lock()
                .push(PtyCall::Spawn(id.into(), cwd.into()));
            self.output(id, b"ready\r\n");
            Ok(())
        }

        fn write(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
            self.calls
                .lock()
                .push(PtyCall::Write(id.into(), bytes.into()));
            Ok(())
        }

        fn resize(&self, _: &str, _: u16, _: u16) -> Result<(), String> {
            Ok(())
        }

        fn status(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn kill(&self, id: &str) -> Result<(), String> {
            self.calls.lock().push(PtyCall::Kill(id.into()));
            Ok(())
        }

        fn kill_all(&self) -> Result<(), String> {
            self.calls.lock().push(PtyCall::KillAll);
            Ok(())
        }
    }

    #[gpui::test]
    fn cached_terminal_follows_appearance_without_restarting_the_pty(cx: &mut TestAppContext) {
        cx.skip_drawing();
        let backend = Arc::new(ControlledPty::default());
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(
                AppearanceSettings {
                    theme_preference: ThemePreference::Dark,
                    ..Default::default()
                },
                cx,
            );
            let backend = backend.clone();
            Terminals::init_with(
                move |events| {
                    *backend.events.lock() = Some(events);
                    backend
                },
                cx,
            );
        });
        let file = monocode_layout::new_terminal_file("/isolated-project", None, None);
        let id = file.id.clone();
        let window = cx.add_window(|window, cx| {
            NativeFilePane::new(
                Rc::new(LocalFiles::new()),
                monocode_layout::new_editor_pane(file),
                Some(Rc::new(external_surface)),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        let terminal = window
            .update(cx, |pane, _, _| match pane.surface(&id) {
                Some(Surface::External(view)) => view.clone().downcast::<TerminalView>().unwrap(),
                _ => panic!("the terminal tab must use the native terminal factory"),
            })
            .unwrap();
        let (original, text, grid) = terminal.read_with(cx, |terminal, _| {
            (
                terminal.theme().clone(),
                terminal.emulator().screen_text(),
                terminal.grid_size(),
            )
        });
        assert!(text.contains("ready"));
        terminal.update(cx, |terminal, cx| terminal.input(b"before", cx));
        cx.run_until_parked();
        cx.update(|cx| {
            set_appearance(
                AppearanceSettings {
                    theme_preference: ThemePreference::Light,
                    accent_color: Some("#cc5500".into()),
                    ..Default::default()
                },
                cx,
            );
        });
        cx.run_until_parked();
        window
            .update(cx, |pane, _, cx| {
                let Some(Surface::External(cached)) = pane.surface(&id) else {
                    panic!("an appearance change must retain the terminal surface");
                };
                assert_eq!(cached.entity_id(), terminal.entity_id());
                let cached = terminal.read(cx);
                assert_eq!(cached.theme(), &terminal_theme(cx));
                assert_ne!(cached.theme(), &original);
                assert_eq!(cached.emulator().screen_text(), text);
                assert_eq!(cached.grid_size(), grid);
                assert!(!cached.has_exited());
                assert_eq!(Terminals::global(cx).open_ids(), [id.clone()].into());
            })
            .unwrap();
        backend.output(&id, b"still attached\r\n");
        terminal.update(cx, |terminal, cx| terminal.input(b"after", cx));
        cx.run_until_parked();
        assert!(terminal.read_with(cx, |terminal, _| {
            terminal.emulator().screen_text().contains("still attached")
        }));
        assert_eq!(
            backend.calls.lock().as_slice(),
            [
                PtyCall::Spawn(id.clone(), "/isolated-project".into()),
                PtyCall::Write(id.clone(), b"before".to_vec()),
                PtyCall::Write(id, b"after".to_vec()),
            ]
        );
    }
}
