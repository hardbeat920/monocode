//! Port of src/features/files/ui/FilePicker.tsx: Go to File, and the
//! command palette when the query starts with `>`.
//!
//! The React picker stayed mounted and reset on each `open`. Here the owner
//! creates a `FilePicker` to open it and drops it on [`FilePickerEvent::Close`].

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, MouseMoveEvent, ParentElement, Pixels, Point,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    Window, anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};
use gpui_base::input::Input;
use gpui_component::input::{
    Enter, Escape, IndentInline, InputEvent, InputState, MoveDown, MoveUp,
};
use monocode_core::Platform;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};

use crate::data::{FileOpenOptions, FilesData, ProjectFile, RankedFile};
use crate::fuzzy::fuzzy_match;
use crate::match_text::match_text;
use crate::paths::looks_like_project;

/// `Action`: a palette command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAction {
    pub id: SharedString,
    pub label: SharedString,
    pub hint: Option<SharedString>,
}

/// `RankedAction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedAction {
    pub action: PaletteAction,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// The search field's placeholder.
pub const PLACEHOLDER: &str = "Go to File (type > for commands)";

/// `reloadActionHint`.
pub fn reload_action_hint(module: &str, shift: &str) -> String {
    format!("{module}{shift}R")
}

/// `ACTIONS`.
pub fn palette_actions(platform: Platform) -> Vec<PaletteAction> {
    vec![PaletteAction {
        id: "reload".into(),
        label: "Reload MonoCode".into(),
        hint: Some(reload_action_hint(platform.mod_label(), platform.shift_label()).into()),
    }]
}

/// `paletteMode`: a leading `>` (after trimming) turns on command mode.
pub fn is_palette_query(query: &str) -> bool {
    query.trim().starts_with('>')
}

/// `actionQuery`.
fn action_query(query: &str) -> &str {
    if is_palette_query(query) {
        query.trim()[1..].trim()
    } else {
        ""
    }
}

/// `actionResults`.
pub fn rank_actions(actions: &[PaletteAction], query: &str) -> Vec<RankedAction> {
    if !is_palette_query(query) {
        return Vec::new();
    }
    let needle = action_query(query);
    if needle.is_empty() {
        return actions
            .iter()
            .map(|action| RankedAction {
                action: action.clone(),
                score: 0,
                positions: Vec::new(),
            })
            .collect();
    }
    let mut ranked: Vec<RankedAction> = actions
        .iter()
        .filter_map(|action| {
            fuzzy_match(needle, &action.label).map(|hit| RankedAction {
                action: action.clone(),
                score: hit.score,
                positions: hit.positions,
            })
        })
        .collect();
    ranked.sort_by_key(|ranked| std::cmp::Reverse(ranked.score));
    ranked
}

/// `emptyLabel`: the message shown instead of a list, if any.
#[allow(clippy::too_many_arguments)]
pub fn empty_label(
    cwd: &str,
    query: &str,
    loading: bool,
    error: Option<&str>,
    file_count: usize,
    match_count: usize,
    palette_mode: bool,
    action_count: usize,
) -> Option<String> {
    if palette_mode {
        return (action_count == 0).then(|| "No matching commands".into());
    }
    if let Some(error) = error
        && file_count == 0
    {
        return Some(error.to_string());
    }
    if !looks_like_project(cwd) {
        return Some("Open a project to search files".into());
    }
    if loading && file_count == 0 {
        return Some("Indexing files…".into());
    }
    if file_count == 0 {
        return Some("No files found".into());
    }
    if match_count == 0 {
        return Some(if query.trim().is_empty() {
            "Type a file name to search".into()
        } else {
            "No matching files".into()
        });
    }
    None
}

/// What the picker asks its owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePickerEvent {
    /// `onOpenFile(path, undefined, { exact: true })`.
    OpenFile {
        path: String,
        options: FileOpenOptions,
    },
    /// `onRunAction(id)`.
    RunAction(SharedString),
    /// `onClose`.
    Close,
}

/// The quick open dialog.
pub struct FilePicker {
    data: Rc<dyn FilesData>,
    cwd: String,
    query: Entity<InputState>,
    actions: Vec<PaletteAction>,
    recents: Vec<String>,
    files: Arc<Vec<ProjectFile>>,
    loading: bool,
    error: Option<String>,
    results: Vec<RankedFile>,
    action_results: Vec<RankedAction>,
    active: usize,
    /// The last pointer position over the list, and whether a hover may
    /// move the highlight (the pointer moved since the list changed).
    pointer: Option<Point<Pixels>>,
    pointer_allowed: bool,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
    _load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FilePickerEvent> for FilePicker {}

impl Focusable for FilePicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.read(cx).focus_handle(cx)
    }
}

impl FilePicker {
    /// Open the picker on `cwd`. `open_paths` are the files open in the
    /// workspace; they rank after the remembered recents.
    pub fn new(
        data: Rc<dyn FilesData>,
        cwd: impl Into<String>,
        open_paths: Vec<String>,
        initial_query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(PLACEHOLDER)
                .default_value(initial_query.to_string())
        });
        let subscription = cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.active = 0;
                this.refresh_results(cx);
            }
        });
        let mut recents = Vec::new();
        for path in data
            .recent_opened_files(&cwd, cx)
            .into_iter()
            .chain(open_paths)
        {
            if !recents.contains(&path) {
                recents.push(path);
            }
        }
        let searchable = looks_like_project(&cwd);
        let cached = data.peek_project_files(&cwd, cx);
        let loading = searchable && cached.is_none();
        let mut this = Self {
            files: cached.unwrap_or_default(),
            data,
            cwd,
            query,
            actions: palette_actions(Platform::current()),
            recents,
            loading,
            error: None,
            results: Vec::new(),
            action_results: Vec::new(),
            active: 0,
            pointer: None,
            pointer_allowed: false,
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _load: None,
            _subscriptions: vec![subscription],
        };
        if searchable {
            let load = this.data.load_project_files(&this.cwd, true, cx);
            this._load = Some(cx.spawn(async move |this, cx| {
                let result = load.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(files) => this.files = files,
                        Err(error) => this.error = Some(error),
                    }
                    this.loading = false;
                    this.refresh_results(cx);
                })
                .ok();
            }));
        }
        this.refresh_results(cx);
        let input = this.query.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
        this
    }

    /// Replace the palette's commands. The default is Reload MonoCode.
    pub fn set_actions(&mut self, actions: Vec<PaletteAction>, cx: &mut Context<Self>) {
        self.actions = actions;
        self.refresh_results(cx);
    }

    pub fn query(&self, cx: &App) -> String {
        self.query.read(cx).value().to_string()
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }

    /// `paletteMode`.
    pub fn palette_mode(&self, cx: &App) -> bool {
        is_palette_query(&self.query(cx))
    }

    /// The dialog's accessible name: Command Palette or Go to File.
    pub fn title(&self, cx: &App) -> &'static str {
        if self.palette_mode(cx) {
            "Command Palette"
        } else {
            "Go to File"
        }
    }

    pub fn results(&self) -> &[RankedFile] {
        &self.results
    }

    pub fn action_results(&self) -> &[RankedAction] {
        &self.action_results
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The message shown instead of a list, if any.
    pub fn empty_message(&self, cx: &App) -> Option<String> {
        let query = self.query(cx);
        empty_label(
            &self.cwd,
            &query,
            self.loading,
            self.error.as_deref(),
            self.files.len(),
            self.results.len(),
            is_palette_query(&query),
            self.action_results.len(),
        )
    }

    fn option_count(&self, cx: &App) -> usize {
        if self.palette_mode(cx) {
            self.action_results.len()
        } else {
            self.results.len()
        }
    }

    /// `results` and `actionResults`, recomputed after the query or the file
    /// list changed. Ranking runs only outside palette mode.
    fn refresh_results(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        if is_palette_query(&query) {
            self.results.clear();
        } else {
            self.results = self
                .data
                .rank_project_files(&self.files, &query, &self.recents);
        }
        self.action_results = rank_actions(&self.actions, &query);
        let count = self.option_count(cx);
        self.active = if count == 0 {
            0
        } else {
            self.active.min(count - 1)
        };
        self.pointer_allowed = false;
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.option_count(cx);
        if count == 0 {
            return;
        }
        self.active = ((self.active as isize + delta).rem_euclid(count as isize)) as usize;
        self.pointer_allowed = false;
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    /// Enter: open the highlighted file or run the highlighted command.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.palette_mode(cx) {
            if let Some(action) = self.action_results.get(self.active).cloned() {
                self.run_action(&action.action.id, cx);
            }
        } else if let Some(file) = self.results.get(self.active).cloned() {
            self.pick(&file.file.path, cx);
        }
    }

    /// `pick`.
    pub fn pick(&mut self, path: &str, cx: &mut Context<Self>) {
        self.data.remember_opened_file(&self.cwd, path, cx);
        cx.emit(FilePickerEvent::OpenFile {
            path: path.to_string(),
            options: FileOpenOptions::EXACT,
        });
        cx.emit(FilePickerEvent::Close);
    }

    /// `runAction`.
    pub fn run_action(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        cx.emit(FilePickerEvent::RunAction(id.clone()));
        cx.emit(FilePickerEvent::Close);
    }

    fn hover_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.pointer_allowed || self.active == index {
            return;
        }
        self.active = index;
        cx.notify();
    }

    fn pointer_moved(&mut self, event: &MouseMoveEvent, _: &mut Window, _: &mut Context<Self>) {
        if self.pointer == Some(event.position) {
            return;
        }
        self.pointer = Some(event.position);
        self.pointer_allowed = true;
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::of(cx);
        let highlighted = index == self.active;
        let selection = theme.colors.selection;
        div()
            .id(("picker-option", index))
            .debug_selector(move || format!("picker-option-{index}"))
            .flex()
            .h(u(32.))
            .w_full()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.md))
            .px(u(8.))
            .text_px(theme.text.ui)
            .leading(theme.leading.none)
            .text_color(theme.colors.content)
            .when(highlighted, |row| row.bg(selection))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hover_row(index, cx);
                }
            }))
    }

    fn render_actions(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let theme = Theme::of(cx).clone();
        let query_active = !action_query(&self.query(cx)).is_empty();
        let mut rows = Vec::new();
        for (index, ranked) in self.action_results.iter().enumerate() {
            let id = ranked.action.id.clone();
            let hint = ranked.action.hint.clone().map(|hint| {
                div()
                    .ml_auto()
                    .flex_none()
                    .rounded(u(theme.radius.sm))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.05))
                    .px(u(6.))
                    .py(u(2.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.micro)
                    .text_color(theme.content(0.50))
                    .child(hint)
            });
            rows.push(
                self.render_row(index, cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.run_action(&id, cx)))
                    .child(
                        icon(IconName::RefreshCw)
                            .size(u(16.))
                            .text_color(theme.content(0.50)),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(match_text(
                        ranked.action.label.clone(),
                        &ranked.positions,
                        query_active,
                        theme.colors.accent,
                    )))
                    .children(hint)
                    .into_any_element(),
            );
        }
        rows
    }

    fn render_files(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let theme = Theme::of(cx).clone();
        let query_active = !self.query(cx).trim().is_empty();
        let mut rows = Vec::new();
        for (index, ranked) in self.results.iter().enumerate() {
            let file = &ranked.file;
            let relative: Vec<u16> = file.relative.encode_utf16().collect();
            let slash = relative.iter().rposition(|unit| *unit == u16::from(b'/'));
            let (dir, name_offset) = match slash {
                Some(slash) => (String::from_utf16_lossy(&relative[..slash]), slash + 1),
                None => (String::new(), 0),
            };
            let name_positions: Vec<usize> = ranked
                .positions
                .iter()
                .filter(|pos| **pos >= name_offset)
                .map(|pos| pos - name_offset)
                .collect();
            let dir_positions: Vec<usize> = ranked
                .positions
                .iter()
                .copied()
                .filter(|pos| slash.is_some_and(|slash| *pos < slash))
                .collect();
            let path = file.path.clone();
            rows.push(
                self.render_row(index, cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(&path, cx)))
                    .child(div().flex_none().child(file_type_icon(file.name.clone())))
                    .child(div().min_w_0().flex_1().truncate().child(match_text(
                        file.name.clone(),
                        &name_positions,
                        query_active,
                        theme.colors.accent,
                    )))
                    .when(!dir.is_empty(), |row| {
                        row.child(
                            div()
                                .min_w_0()
                                .max_w(gpui::relative(0.45))
                                .truncate()
                                .font_family(theme.fonts.mono.clone())
                                .text_px(theme.text.caption)
                                .text_color(theme.content(0.40))
                                .child(match_text(
                                    dir.clone(),
                                    &dir_positions,
                                    query_active,
                                    theme.colors.accent,
                                )),
                        )
                    })
                    .into_any_element(),
            );
        }
        rows
    }
}

impl Render for FilePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let viewport = window.viewport_size();
        let rem = window.rem_size();
        let palette = self.palette_mode(cx);
        let empty = self.empty_message(cx);
        let list_max = u(380.).to_pixels(rem).min(viewport.height * 0.5);
        let width = u(560.)
            .to_pixels(rem)
            .min(viewport.width - u(24.).to_pixels(rem));
        let body = match empty {
            Some(message) => div()
                .px(u(12.))
                .pb(u(12.))
                .pt(u(4.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child(message)
                .into_any_element(),
            None => {
                let rows = if palette {
                    self.render_actions(cx)
                } else {
                    self.render_files(cx)
                };
                div()
                    .id(if palette {
                        "picker-commands"
                    } else {
                        "picker-files"
                    })
                    .flex()
                    .flex_col()
                    .max_h(list_max)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(u(6.))
                    .pb(u(6.))
                    .on_mouse_move(cx.listener(Self::pointer_moved))
                    .children(rows)
                    .into_any_element()
            }
        };
        let dialog = div()
            .id("file-picker")
            .debug_selector(|| "file-picker".into())
            .key_context("FilePicker")
            .track_focus(&self.focus_handle)
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.step(1, cx);
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.step(-1, cx);
            }))
            .capture_action(cx.listener(|this, _: &Enter, _, cx| {
                cx.stop_propagation();
                this.confirm(cx);
            }))
            .capture_action(cx.listener(|_, _: &IndentInline, _, cx| cx.stop_propagation()))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.stop_propagation();
                cx.emit(FilePickerEvent::Close);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .flex()
            .flex_col()
            .w(width)
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .text_color(theme.colors.content)
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)))
            .child(
                div().relative().pb(u(6.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .border_b_1()
                        .border_color(theme.colors.stroke)
                        .px(u(8.))
                        .py(u(10.))
                        .text_color(theme.content(0.50))
                        .child(
                            icon(IconName::Search)
                                .size(u(14.))
                                .text_color(theme.content(0.50)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_px(theme.text.body)
                                .text_color(theme.colors.content)
                                .child(Input::new(&self.query)),
                        ),
                ),
            )
            .child(div().relative().child(body));
        let overlay = div()
            .id("file-picker-overlay")
            .w(viewport.width)
            .h(viewport.height)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(FilePickerEvent::Close)),
            )
            .child(
                div()
                    .absolute()
                    .top(viewport.height * 0.12)
                    .left((viewport.width - width) / 2.)
                    .child(dialog),
            );
        deferred(anchored().position(point(px(0.), px(0.))).child(overlay))
            .with_priority(theme.layer.dialog)
    }
}

#[cfg(test)]
#[path = "file_picker_tests.rs"]
mod tests;
