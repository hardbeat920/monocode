//! An editor surface pane: its file tabs and the active file, drawn with
//! `monocode-editor`. Port of the editor half of SurfaceTabs.tsx and
//! FilePane.tsx for M1: a file opens in the code editor (Cmd+S saves it), a
//! session's changes and the working tree's changes open as diffs. Plans,
//! commits, terminals, and images land with their feature ports.

use std::path::Path;

use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div,
};
use monocode_editor::{CodeEditor, ColorScheme, DiffFile, DiffView, EditorTheme, SaveRequest};
use monocode_engine::runtime::Engine;
use monocode_engine::workspace::{Workspace, WorkspaceEvent};
use monocode_layout::{EditorPane, FilePaneTab, GitFileDiffKind, find_surface_pane};
use monocode_ui::widgets::{icon_button, spinner, tooltip};
use monocode_ui::{Theme, UiStyled as _, file_type_icon, u};

/// What the pane shows for its active file.
enum Content {
    Loading,
    Editor(Entity<CodeEditor>),
    Diff(Entity<DiffView>),
    Message(SharedString),
}

pub struct FilePane {
    pane_id: String,
    workspace: WeakEntity<Workspace>,
    /// The active file the content was loaded for, by id and path.
    loaded: Option<(String, String)>,
    content: Content,
    load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The editor colors from the app theme.
fn editor_theme(cx: &gpui::App) -> EditorTheme {
    let theme = Theme::of(cx);
    let scheme = if theme.is_dark() {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    EditorTheme::new(scheme, theme.colors.background_base, theme.colors.content)
}

/// `path` relative to `cwd`, for git and checkpoint reads.
fn relative_to(path: &str, cwd: &str) -> String {
    Path::new(path)
        .strip_prefix(cwd)
        .map(|relative| relative.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

impl FilePane {
    pub fn new(
        pane_id: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        if let Some(entity) = workspace.upgrade() {
            subscriptions
                .push(cx.observe_in(&entity, window, |this, _, window, cx| this.sync(window, cx)));
            subscriptions.push(cx.subscribe_in(
                &entity,
                window,
                |this, _, event: &WorkspaceEvent, window, cx| {
                    if let WorkspaceEvent::EditorNavigation(target) = event
                        && let Content::Editor(editor) = &this.content
                        && this
                            .loaded
                            .as_ref()
                            .is_some_and(|(_, path)| *path == target.path)
                    {
                        let line = target.line.max(1) as usize;
                        let column = target.column.map(|column| column.max(1) as usize);
                        editor.update(cx, |editor, cx| {
                            editor.reveal_position(line, column, window, cx)
                        });
                    }
                },
            ));
        }
        let mut pane = Self {
            pane_id,
            workspace,
            loaded: None,
            content: Content::Loading,
            load: None,
            _subscriptions: subscriptions,
        };
        pane.sync(window, cx);
        pane
    }

    /// This pane's files, from whichever tab holds it.
    fn pane(&self, cx: &gpui::App) -> Option<EditorPane> {
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        workspace
            .tabs()
            .iter()
            .find_map(|tab| find_surface_pane(tab, &self.pane_id).map(|(_, pane)| pane.clone()))
    }

    fn active_file(&self, cx: &gpui::App) -> Option<FilePaneTab> {
        let pane = self.pane(cx)?;
        pane.files
            .iter()
            .find(|file| file.id == pane.active_file_id)
            .or(pane.files.first())
            .cloned()
    }

    /// Load the active file when it changed.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.active_file(cx) else {
            return;
        };
        let key = (file.id.clone(), file.path.clone());
        if self.loaded.as_ref() == Some(&key) {
            return;
        }
        // A session review that moved to another file scrolls instead of
        // reloading.
        if let (Content::Diff(diff), Some((id, _))) = (&self.content, &self.loaded)
            && *id == file.id
        {
            let target = relative_to(&file.path, &file.cwd);
            diff.update(cx, |diff, cx| diff.scroll_to_file(&target, cx));
            self.loaded = Some(key);
            cx.notify();
            return;
        }
        self.loaded = Some(key);
        self.content = Content::Loading;
        self.load = Some(self.start_load(file, window, cx));
        cx.notify();
    }

    fn start_load(
        &mut self,
        file: FilePaneTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if file.terminal == Some(true) {
            return self.message("Terminal tabs land with the terminal dock.", cx);
        }
        if file.plan.is_some() || file.commit.is_some() || file.release_notes.is_some() {
            return self.message("This tab lands with its feature port.", cx);
        }
        if file.agent.is_some() {
            return self.message("Worker transcripts land with orchestration.", cx);
        }
        if let Some(source) = file.session_changes.clone() {
            return self.load_session_changes(source.session_id, file, window, cx);
        }
        if file.changes == Some(true) {
            return self.load_working_tree(file, window, cx);
        }
        if file.review == Some(true) {
            let staged = file.change_kind == Some(GitFileDiffKind::Staged);
            return self.load_file_diff(file, staged, window, cx);
        }
        self.load_editor(file, window, cx)
    }

    fn message(&mut self, text: &'static str, cx: &mut Context<Self>) -> Task<()> {
        self.content = Content::Message(text.into());
        cx.notify();
        Task::ready(())
    }

    fn show_diff(&mut self, files: Vec<DiffFile>, focus: Option<String>, cx: &mut Context<Self>) {
        if files.is_empty() {
            self.content = Content::Message("No changes".into());
            cx.notify();
            return;
        }
        let theme = editor_theme(cx);
        let diff = cx.new(|cx| DiffView::new(files, theme, cx));
        if let Some(focus) = focus {
            diff.update(cx, |diff, cx| diff.scroll_to_file(&focus, cx));
        }
        self.content = Content::Diff(diff);
        cx.notify();
    }

    /// A session's changes from its checkpoint, every file in one review.
    fn load_session_changes(
        &mut self,
        session_id: String,
        file: FilePaneTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let checkpoints = Engine::checkpoints(cx);
        let cwd = file.cwd.clone();
        let status = checkpoints.status(&session_id, &cwd);
        let focus = (file.path != file.cwd).then(|| relative_to(&file.path, &cwd));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(status) = status.await else {
                this.update(cx, |this, cx| {
                    this.content = Content::Message("Could not read this session's changes".into());
                    cx.notify();
                })
                .ok();
                return;
            };
            let mut files = Vec::new();
            for changed in status.files {
                let diff = cx
                    .update(|_, _| checkpoints.file_diff(&session_id, &cwd, &changed.relative))
                    .ok();
                let Some(diff) = diff else {
                    return;
                };
                if let Ok(diff) = diff.await {
                    let mut file =
                        DiffFile::from_texts(diff.relative.clone(), &diff.original, &diff.current);
                    file.binary = diff.binary;
                    file.too_large = diff.too_large;
                    files.push(file);
                }
            }
            this.update(cx, |this, cx| this.show_diff(files, focus, cx))
                .ok();
        })
    }

    /// Every working-tree change in one review.
    fn load_working_tree(
        &mut self,
        file: FilePaneTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let cwd = file.cwd.clone();
        let focus = (file.path != file.cwd).then(|| relative_to(&file.path, &cwd));
        cx.spawn_in(window, async move |this, cx| {
            let files = smol::unblock(move || {
                let index = monocode_git::fs::git_diff_files(cwd.clone());
                index
                    .files
                    .into_iter()
                    .filter_map(|changed| {
                        let staged = changed.staged && !changed.unstaged;
                        monocode_git::fs::git_file_diff(cwd.clone(), changed.relative, staged).ok()
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let files = files
                .into_iter()
                .map(|diff| {
                    let mut file =
                        DiffFile::from_texts(diff.relative.clone(), &diff.original, &diff.current);
                    file.binary = diff.binary;
                    file.too_large = diff.too_large;
                    file
                })
                .collect();
            this.update(cx, |this, cx| this.show_diff(files, focus, cx))
                .ok();
        })
    }

    /// One file's staged or unstaged diff.
    fn load_file_diff(
        &mut self,
        file: FilePaneTab,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let cwd = file.cwd.clone();
        let relative = relative_to(&file.path, &cwd);
        cx.spawn_in(window, async move |this, cx| {
            let diff =
                smol::unblock(move || monocode_git::fs::git_file_diff(cwd, relative, staged)).await;
            this.update_in(cx, |this, window, cx| match diff {
                Ok(diff) => {
                    let mut file =
                        DiffFile::from_texts(diff.relative.clone(), &diff.original, &diff.current);
                    file.binary = diff.binary;
                    file.too_large = diff.too_large;
                    this.show_diff(vec![file], None, cx);
                }
                // A file outside the repository has no diff: show the file.
                Err(_) => this.load = Some(this.load_editor(file, window, cx)),
            })
            .ok();
        })
    }

    /// A file in the code editor. Cmd+S writes it back.
    fn load_editor(
        &mut self,
        file: FilePaneTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let path = file.path.clone();
        cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let text = smol::unblock(move || monocode_git::fs::read_text_file(read_path)).await;
            this.update_in(cx, |this, window, cx| match text {
                Ok(text) => {
                    let theme = editor_theme(cx);
                    let editor = cx.new(|cx| {
                        let mut editor = CodeEditor::new(path.clone(), &text, theme, window, cx);
                        editor.on_save(std::rc::Rc::new(
                            |request: SaveRequest, _: &mut Window, cx: &mut gpui::App| {
                                let path = request.path.to_string();
                                let contents = request.contents;
                                cx.background_spawn(async move {
                                    monocode_git::fs::write_text_file(path, contents)
                                        .map_err(anyhow::Error::msg)
                                })
                            },
                        ));
                        editor
                    });
                    this.content = Content::Editor(editor);
                    this.reveal_pending(window, cx);
                    cx.notify();
                }
                Err(error) => {
                    this.content = Content::Message(error.into());
                    cx.notify();
                }
            })
            .ok();
        })
    }

    /// Apply a navigation the workspace asked for before the file loaded.
    fn reveal_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(target) = workspace.read(cx).editor_navigation().cloned() else {
            return;
        };
        if let Content::Editor(editor) = &self.content
            && self
                .loaded
                .as_ref()
                .is_some_and(|(_, path)| *path == target.path)
        {
            let line = target.line.max(1) as usize;
            let column = target.column.map(|column| column.max(1) as usize);
            editor.update(cx, |editor, cx| {
                editor.reveal_position(line, column, window, cx)
            });
        }
    }

    fn select(&mut self, file_id: &str, cx: &mut Context<Self>) {
        let pane_id = self.pane_id.clone();
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.select_file_surface(&pane_id, file_id, cx)
            });
        }
    }

    fn close(&mut self, file_id: &str, cx: &mut Context<Self>) {
        let pane_id = self.pane_id.clone();
        if let Some(workspace) = self.workspace.upgrade() {
            workspace
                .update(cx, |workspace, cx| {
                    workspace.close_file(&pane_id, file_id, cx)
                })
                .detach();
        }
    }

    fn render_tabs(&self, pane: &EditorPane, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let mut strip = div()
            .id(SharedString::from(format!("surface-tabs-{}", self.pane_id)))
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(2.))
            .px(u(6.))
            .border_b_1()
            .border_color(c.stroke)
            .overflow_x_scroll();
        for file in &pane.files {
            let active = file.id == pane.active_file_id;
            let name = if file.session_changes.is_some() {
                "Session changes".to_string()
            } else if file.changes == Some(true) {
                "Changes".to_string()
            } else {
                file_name(&file.path)
            };
            let select_id = file.id.clone();
            let close_id = file.id.clone();
            let group = SharedString::from(format!("surface-tab-{}", file.id));
            let mut tab = div()
                .id(SharedString::from(format!("surface-tab-{}", file.id)))
                .group(group.clone())
                .flex()
                .flex_none()
                .items_center()
                .gap(u(6.))
                .h(u(26.))
                .pl(u(8.))
                .pr(u(4.))
                .rounded(u(theme.radius.md))
                .text_px(theme.text.label)
                .child(file_type_icon(name.clone()).size(14.))
                .child({
                    let label = div().max_w(u(180.)).truncate().child(name.clone());
                    if file.preview == Some(true) {
                        label.italic()
                    } else {
                        label
                    }
                })
                .child(
                    icon_button(
                        SharedString::from(format!("surface-close-{}", file.id)),
                        monocode_ui::IconName::X,
                    )
                    .size(18.)
                    .icon_size(11.)
                    .tooltip("Close")
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.close(&close_id, cx)),
                    ),
                )
                .tooltip(tooltip(file.path.clone()))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.select(&select_id, cx)),
                );
            if active {
                tab = tab.bg(c.selection).text_color(c.content);
            } else {
                let hover = theme.content(0.05);
                tab = tab
                    .text_color(theme.content(0.55))
                    .hover(move |s| s.bg(hover));
            }
            strip = strip.child(tab);
        }
        strip
    }
}

impl Render for FilePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tabs = self
            .pane(cx)
            .map(|pane| self.render_tabs(&pane, cx).into_any_element());
        let body: AnyElement = match &self.content {
            Content::Loading => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(spinner(SharedString::from(format!(
                    "file-{}",
                    self.pane_id
                ))))
                .into_any_element(),
            Content::Editor(editor) => editor.clone().into_any_element(),
            Content::Diff(diff) => diff.clone().into_any_element(),
            Content::Message(text) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_px(theme.text.body)
                .text_color(theme.content(0.45))
                .child(text.clone())
                .into_any_element(),
        };
        let pane_id = self.pane_id.clone();
        let workspace = self.workspace.clone();
        div()
            .id(SharedString::from(format!("file-pane-{}", self.pane_id)))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| workspace.focus_pane(&pane_id, cx));
                }
            })
            .children(tabs)
            .child(div().flex().flex_col().flex_1().min_h_0().child(body))
    }
}
