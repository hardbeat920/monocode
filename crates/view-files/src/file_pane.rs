//! Port of src/features/files/ui/FilePane.tsx: one editor or terminal pane.
//! It shows the pane's tab strip and the active tab's surface, and keeps the
//! surfaces of background tabs alive, as the React pane kept them mounted
//! and hidden.
//!
//! This crate draws file editors, image and PDF viewers, and plan tabs.
//! Surfaces that belong to other view crates (agent transcripts, terminals,
//! release notes, and the commit, working tree, and session diffs) come from
//! a [`SurfaceFactory`] the owner passes in, and the tab strip
//! (`SurfaceTabs`) comes from a [`TabStrip`] builder.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_core::Session;
use monocode_core::block::PlanBuildTarget;
use monocode_editor::viewer::{is_image_path, is_pdf_path};
use monocode_layout::{
    EditorPane, FilePaneTab, is_agent_tab, is_changes_tab, is_commit_tab, is_plan_tab,
    is_release_notes_tab, is_review_tab, is_session_changes_tab, is_terminal_tab,
};
use monocode_view_transcript::threads::ModelMenuSource;

use crate::binary_view::BinaryFileSurface;
use crate::data::{EditorNavigation, FilesData};
use crate::file_editor::{EditorCodeSelection, EditorSettings, FileEditorEvent, FileEditorSurface};
use crate::paths::editor_paths_equal;
use crate::plan_surface::{PlanSurface, PlanSurfaceEvent};

/// A surface another crate draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalSurface {
    /// `AgentTabView`: an orchestration worker's transcript.
    Agent,
    /// `ReleaseNotesSurface`.
    ReleaseNotes,
    /// `TerminalView`.
    Terminal,
    /// `SessionChangesDiff`, over the whole pane.
    SessionChanges,
    /// `CommitDiff`, over the whole pane.
    Commit,
    /// `WorkingTreeDiff`, over the whole pane.
    WorkingTreeDiff,
}

/// What the factory is asked to build.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceRequest<'a> {
    pub kind: ExternalSurface,
    pub file: &'a FilePaneTab,
    pub pane_id: &'a str,
}

/// Builds the surfaces this crate does not own. Each view is built once per
/// tab and kept while the tab exists; `None` leaves the tab empty.
pub type SurfaceFactory = Rc<dyn Fn(&SurfaceRequest<'_>, &mut Window, &mut App) -> Option<AnyView>>;

/// Draws the pane's tab strip (`SurfaceTabs`).
pub type TabStrip = Rc<dyn Fn(&EditorPane, &mut Window, &mut App) -> AnyElement>;

/// What the pane reports to its owner.
#[derive(Debug, Clone, PartialEq)]
pub enum FilePaneEvent {
    /// `onFocus`: a mouse down anywhere in the pane.
    Focus { pane_id: String },
    /// `onDirtyChange`.
    DirtyChanged { file_id: String, dirty: bool },
    /// `onOpenFile`: a link in a preview.
    OpenFile { path: String },
    /// `onUpdatePlan`.
    UpdatePlan {
        session_id: String,
        block_id: String,
        text: String,
    },
    /// `onBuildPlan`.
    BuildPlan {
        session_id: String,
        block_id: String,
        target: Option<PlanBuildTarget>,
    },
    /// Add to chat on a selection in an editor tab.
    AddToChat(EditorCodeSelection),
}

/// The surface kept for one tab.
pub enum Surface {
    Editor(Entity<FileEditorSurface>),
    Binary(Entity<BinaryFileSurface>),
    Plan(Entity<PlanSurface>),
    External(AnyView),
    /// The factory had nothing for this tab.
    Empty,
}

impl Surface {
    fn view(&self) -> Option<AnyView> {
        match self {
            Surface::Editor(view) => Some(view.clone().into()),
            Surface::Binary(view) => Some(view.clone().into()),
            Surface::Plan(view) => Some(view.clone().into()),
            Surface::External(view) => Some(view.clone()),
            Surface::Empty => None,
        }
    }
}

/// Which surface a tab gets, by the branch order of the React pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSurface {
    /// Drawn by the pane-wide review surface instead.
    None,
    External(ExternalSurface),
    Plan,
    Binary,
    Editor,
}

/// The tab's surface kind. `unified_review` hides review tabs behind the
/// working tree diff.
pub fn tab_surface(file: &FilePaneTab, unified_review: bool) -> TabSurface {
    if is_commit_tab(file)
        || is_changes_tab(file)
        || is_session_changes_tab(file)
        || (unified_review && is_review_tab(file))
    {
        return TabSurface::None;
    }
    if is_agent_tab(file) {
        TabSurface::External(ExternalSurface::Agent)
    } else if is_plan_tab(file) {
        TabSurface::Plan
    } else if is_release_notes_tab(file) {
        TabSurface::External(ExternalSurface::ReleaseNotes)
    } else if is_terminal_tab(file) {
        TabSurface::External(ExternalSurface::Terminal)
    } else if is_image_path(&file.path) || is_pdf_path(&file.path) {
        TabSurface::Binary
    } else {
        TabSurface::Editor
    }
}

/// The pane-wide review surface for the active tab, if any.
pub fn review_surface(
    active: Option<&FilePaneTab>,
    unified_diffs: bool,
) -> Option<ExternalSurface> {
    let active = active?;
    if is_session_changes_tab(active) {
        return Some(ExternalSurface::SessionChanges);
    }
    if is_commit_tab(active) && active.commit.is_some() {
        return Some(ExternalSurface::Commit);
    }
    if is_changes_tab(active) || (unified_diffs && is_review_tab(active)) {
        return Some(ExternalSurface::WorkingTreeDiff);
    }
    None
}

/// The pane view.
pub struct FilePane {
    data: Rc<dyn FilesData>,
    pane: EditorPane,
    focused: bool,
    show_tabs: bool,
    /// `loadDiffViewer() === "unified"`.
    unified_diffs: bool,
    settings: EditorSettings,
    sessions: Rc<Vec<Session>>,
    plan_model_source: Option<Rc<dyn ModelMenuSource>>,
    navigation: Option<EditorNavigation>,
    factory: Option<SurfaceFactory>,
    tab_strip: Option<TabStrip>,
    surfaces: HashMap<String, Surface>,
    review: Option<(String, ExternalSurface, Option<AnyView>)>,
    _subscriptions: HashMap<String, Subscription>,
}

impl EventEmitter<FilePaneEvent> for FilePane {}

impl FilePane {
    pub fn new(
        data: Rc<dyn FilesData>,
        pane: EditorPane,
        factory: Option<SurfaceFactory>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            data,
            pane,
            focused: false,
            show_tabs: true,
            unified_diffs: false,
            settings: EditorSettings::default(),
            sessions: Rc::default(),
            plan_model_source: None,
            navigation: None,
            factory,
            tab_strip: None,
            surfaces: HashMap::new(),
            review: None,
            _subscriptions: HashMap::new(),
        };
        this.sync(window, cx);
        this
    }

    pub fn pane(&self) -> &EditorPane {
        &self.pane
    }

    /// The surface kept for tab `file_id`.
    pub fn surface(&self, file_id: &str) -> Option<&Surface> {
        self.surfaces.get(file_id)
    }

    /// The pane-wide review surface shown for the active tab.
    pub fn review(&self) -> Option<ExternalSurface> {
        self.review.as_ref().map(|(_, kind, _)| *kind)
    }

    pub fn show_tabs(&self) -> bool {
        self.show_tabs && self.tab_strip.is_some()
    }

    pub fn set_pane(&mut self, pane: EditorPane, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = pane;
        self.sync(window, cx);
    }

    pub fn set_focused(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.focused = focused;
        self.sync(window, cx);
    }

    /// `showTabs`: false when the title bar already names a standalone file.
    pub fn set_show_tabs(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_tabs = show;
        cx.notify();
    }

    pub fn set_tab_strip(&mut self, strip: Option<TabStrip>, cx: &mut Context<Self>) {
        self.tab_strip = strip;
        cx.notify();
    }

    /// The `diffViewer` setting is "unified".
    pub fn set_unified_diffs(
        &mut self,
        unified: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unified_diffs = unified;
        self.sync(window, cx);
    }

    pub fn set_settings(
        &mut self,
        settings: EditorSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings;
        for surface in self.surfaces.values() {
            if let Surface::Editor(editor) = surface {
                editor.update(cx, |editor, cx| editor.set_settings(settings, window, cx));
            }
        }
    }

    /// The sessions plan tabs read.
    pub fn set_sessions(
        &mut self,
        sessions: Rc<Vec<Session>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sessions = sessions.clone();
        for surface in self.surfaces.values() {
            if let Surface::Plan(plan) = surface {
                plan.update(cx, |plan, cx| {
                    plan.set_sessions(sessions.clone(), window, cx)
                });
            }
        }
        cx.notify();
    }

    pub fn set_plan_model_source(
        &mut self,
        source: Rc<dyn ModelMenuSource>,
        cx: &mut Context<Self>,
    ) {
        self.plan_model_source = Some(source.clone());
        for surface in self.surfaces.values() {
            if let Surface::Plan(plan) = surface {
                plan.update(cx, |plan, cx| plan.set_model_source(source.clone(), cx));
            }
        }
    }

    /// `editorNavigation`: reveal a location in the tab showing its file.
    pub fn set_navigation(
        &mut self,
        navigation: Option<EditorNavigation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation = navigation;
        self.sync(window, cx);
    }

    fn unified_review(&self) -> bool {
        let active = self.active_file();
        active.is_some_and(|active| {
            !is_session_changes_tab(active)
                && (is_changes_tab(active) || (self.unified_diffs && is_review_tab(active)))
        })
    }

    fn active_file(&self) -> Option<&FilePaneTab> {
        self.pane
            .files
            .iter()
            .find(|file| file.id == self.pane.active_file_id)
    }

    /// Build surfaces for new tabs, drop those of closed tabs, and pass the
    /// pane's state to each.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let unified_review = self.unified_review();
        let live: HashSet<String> = self.pane.files.iter().map(|file| file.id.clone()).collect();
        self.surfaces.retain(|id, _| live.contains(id));
        self._subscriptions.retain(|id, _| live.contains(id));

        let files = self.pane.files.clone();
        for file in &files {
            let kind = tab_surface(file, unified_review);
            let stale = match (self.surfaces.get(&file.id), kind) {
                (None, _) => true,
                (Some(Surface::Editor(_)), TabSurface::Editor) => false,
                (Some(Surface::Binary(binary)), TabSurface::Binary) => {
                    binary.read(cx).path() != file.path
                }
                (Some(Surface::Plan(_)), TabSurface::Plan) => false,
                (Some(Surface::External(_) | Surface::Empty), TabSurface::External(_)) => false,
                (Some(_), TabSurface::None) => true,
                _ => true,
            };
            if stale {
                self.surfaces.remove(&file.id);
                self._subscriptions.remove(&file.id);
                if kind != TabSurface::None {
                    let surface = self.build_surface(file, kind, window, cx);
                    self.surfaces.insert(file.id.clone(), surface);
                }
            }
            if let Some(Surface::Editor(editor)) = self.surfaces.get(&file.id) {
                let active = self.focused && file.id == self.pane.active_file_id;
                let navigation = self
                    .navigation
                    .clone()
                    .filter(|navigation| editor_paths_equal(&file.path, &navigation.path));
                let show_diff = file.review == Some(true);
                let path = file.path.clone();
                editor.update(cx, |editor, cx| {
                    editor.set_path(path, window, cx);
                    editor.set_show_diff(show_diff, cx);
                    editor.set_active(active, window, cx);
                    editor.set_navigation(navigation, window, cx);
                });
            }
        }

        let review = review_surface(self.active_file(), self.unified_diffs);
        let key = self.pane.active_file_id.clone();
        self.review = match (review, self.review.take()) {
            (None, _) => None,
            (Some(kind), Some((id, current, view))) if id == key && current == kind => {
                Some((id, kind, view))
            }
            (Some(kind), _) => {
                let view = self.active_file().cloned().and_then(|file| {
                    let factory = self.factory.clone()?;
                    let request = SurfaceRequest {
                        kind,
                        file: &file,
                        pane_id: &self.pane.id,
                    };
                    factory(&request, window, cx)
                });
                Some((key, kind, view))
            }
        };
        cx.notify();
    }

    fn build_surface(
        &mut self,
        file: &FilePaneTab,
        kind: TabSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Surface {
        let file_id = file.id.clone();
        match kind {
            TabSurface::None => Surface::Empty,
            TabSurface::External(external) => {
                let request = SurfaceRequest {
                    kind: external,
                    file,
                    pane_id: &self.pane.id,
                };
                match self
                    .factory
                    .clone()
                    .and_then(|factory| factory(&request, window, cx))
                {
                    Some(view) => Surface::External(view),
                    None => Surface::Empty,
                }
            }
            TabSurface::Plan => {
                let sessions = self.sessions.clone();
                let file = file.clone();
                let plan = cx.new(|cx| PlanSurface::new(file, sessions, window, cx));
                if let Some(source) = &self.plan_model_source {
                    plan.update(cx, |plan, cx| plan.set_model_source(source.clone(), cx));
                }
                let subscription = cx.subscribe(&plan, |_, _, event: &PlanSurfaceEvent, cx| {
                    cx.emit(match event.clone() {
                        PlanSurfaceEvent::Update {
                            session_id,
                            block_id,
                            text,
                        } => FilePaneEvent::UpdatePlan {
                            session_id,
                            block_id,
                            text,
                        },
                        PlanSurfaceEvent::Build {
                            session_id,
                            block_id,
                            target,
                        } => FilePaneEvent::BuildPlan {
                            session_id,
                            block_id,
                            target,
                        },
                    })
                });
                self._subscriptions.insert(file_id, subscription);
                Surface::Plan(plan)
            }
            TabSurface::Binary => {
                let data = self.data.clone();
                let (path, cwd) = (file.path.clone(), file.cwd.clone());
                Surface::Binary(cx.new(|cx| BinaryFileSurface::new(data, path, cwd, cx)))
            }
            TabSurface::Editor => {
                let data = self.data.clone();
                let (path, cwd) = (file.path.clone(), file.cwd.clone());
                let settings = self.settings;
                let editor = cx.new(|cx| {
                    let mut editor = FileEditorSurface::new(data, path, cwd, window, cx);
                    editor.set_settings(settings, window, cx);
                    editor
                });
                let id = file_id.clone();
                let subscription =
                    cx.subscribe(&editor, move |_, _, event: &FileEditorEvent, cx| {
                        cx.emit(match event.clone() {
                            FileEditorEvent::DirtyChanged(dirty) => FilePaneEvent::DirtyChanged {
                                file_id: id.clone(),
                                dirty,
                            },
                            FileEditorEvent::OpenFile(path) => FilePaneEvent::OpenFile { path },
                            FileEditorEvent::AddToChat(selection) => {
                                FilePaneEvent::AddToChat(selection)
                            }
                        })
                    });
                self._subscriptions.insert(file_id, subscription);
                Surface::Editor(editor)
            }
        }
    }
}

impl Render for FilePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane_id = self.pane.id.clone();
        let tabs = self
            .tab_strip
            .clone()
            .filter(|_| self.show_tabs)
            .map(|strip| strip(&self.pane, window, cx));
        let review = self
            .review
            .as_ref()
            .and_then(|(_, _, view)| view.clone())
            .map(|view| div().absolute().top_0().left_0().size_full().child(view));
        let active = self
            .surfaces
            .get(&self.pane.active_file_id)
            .and_then(Surface::view)
            .filter(|_| {
                self.active_file().is_some_and(|file| {
                    tab_surface(file, self.unified_review()) != TabSurface::None
                })
            })
            .map(|view| div().absolute().top_0().left_0().size_full().child(view));
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _, _, cx| {
                    cx.emit(FilePaneEvent::Focus {
                        pane_id: pane_id.clone(),
                    })
                }),
            )
            .children(tabs)
            .child(
                div()
                    .relative()
                    .min_h_0()
                    .flex_1()
                    .children(review)
                    .when_some(active, |body, active| body.child(active)),
            )
    }
}

#[cfg(test)]
#[path = "file_pane_tests.rs"]
mod tests;
