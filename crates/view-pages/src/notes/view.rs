//! Port of src/features/notes/ui/NotesView.tsx: the notes list with its
//! filter and resizable width, and the note editor with the project picker,
//! title, tags, actions, the Preview and Source tabs, and image drops.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, ExternalPaths, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_base::input::EditorState;
use gpui_component::input::{InputEvent, InputState};
use monocode_markdown::{MarkdownView, default_image_source};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::skill_document_preview::markdown_style;

use super::card::note_card;
use super::data::{NoteEditorView, NotesData, NotesPage};
use super::model::MAX_NOTE_TAGS;
use super::source::{source_editor, source_style};
use crate::data::ProjectsData;
use crate::format::{format_relative_time, looks_like_project, now_ms};
use crate::widgets::{
    MarkdownMode, MarkdownModes, PageChrome, ProjectPicker, ProjectPickerMode, page_tab,
    page_title_bar, plain_input, spinner_icon,
};

const MIN_WIDTH: f32 = 240.;
const MAX_WIDTH: f32 = 420.;
const DEFAULT_WIDTH: f32 = 280.;
/// `min-h-[448px]` on the editor's content area.
const CONTENT_MIN_HEIGHT: f32 = 448.;
/// `leading-5` in the source field.
const SOURCE_LINE_HEIGHT: f32 = 20.;

/// `rememberedWidth`: the list width across visits.
#[derive(Clone, Copy)]
struct RememberedWidth(f32);

impl gpui::Global for RememberedWidth {}

type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// A list resize in progress.
#[derive(Clone, Copy)]
struct Resize {
    start_x: Pixels,
    start_width: f32,
}

pub struct NotesView {
    data: Rc<dyn NotesData>,
    projects: Rc<dyn ProjectsData>,
    chrome: PageChrome,
    cwd: Option<String>,
    on_close: Option<CloseFn>,
    page: NotesPage,
    list_width: f32,
    resize: Option<Resize>,
    focus: FocusHandle,
    query: Entity<InputState>,
    title: Entity<InputState>,
    tag: Entity<InputState>,
    source: Entity<EditorState>,
    preview: Entity<MarkdownView>,
    /// The scheme the preview's style was built for.
    preview_dark: bool,
    picker: Entity<ProjectPicker>,
    list_scroll: ScrollHandle,
    detail_scroll: ScrollHandle,
    /// The note the fields show.
    editor_note: Option<String>,
    /// Field text as last synced from the data, to skip echoes.
    synced_title: String,
    synced_body: String,
    _subscriptions: Vec<Subscription>,
}

impl NotesView {
    pub fn new(
        data: Rc<dyn NotesData>,
        projects: Rc<dyn ProjectsData>,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter notes"));
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Untitled"));
        let tag = cx.new(|cx| InputState::new(window, cx).placeholder("Add tag…"));
        let source = source_editor("", window, cx);
        let preview = cx.new(|cx| {
            let mut view = MarkdownView::new(cx);
            view.set_style(markdown_style(Theme::of(cx)), cx);
            let images = data.clone();
            view.set_image_resolver(move |url| images.image_source(url), cx);
            view
        });
        let rail_cwd = cwd.map(str::to_string);
        let picker_data = data.clone();
        let picker = cx.new(|cx| {
            let mut picker = ProjectPicker::new("~", projects.clone(), window, cx)
                .mode(ProjectPickerMode::Move)
                .on_select(move |path, _, cx| picker_data.choose_project(path, cx));
            picker.set_rail_cwd(rail_cwd.as_deref(), cx);
            picker
        });
        let weak = cx.weak_entity();
        let changes = data.subscribe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.page = this.data.page(cx);
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        let query_events = cx.subscribe_in(&query, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                this.data.set_query(&value, cx);
            }
        });
        let title_events =
            cx.subscribe_in(
                &title,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        if value != this.synced_title {
                            this.synced_title = value.clone();
                            this.data.edit_title(&value, cx);
                        }
                    }
                    InputEvent::PressEnter { .. } => {
                        this.focus.focus(window, cx);
                        this.data.commit_title(cx);
                    }
                    InputEvent::Blur => this.data.commit_title(cx),
                    _ => {}
                },
            );
        let tag_events =
            cx.subscribe_in(&tag, window, |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    let value = input.read(cx).value().to_string();
                    if let Some(tag) = value.strip_suffix(',') {
                        let tag = tag.to_string();
                        this.add_tag(&tag, window, cx);
                    }
                }
                InputEvent::PressEnter { .. } => {
                    let value = input.read(cx).value().to_string();
                    this.add_tag(&value, window, cx);
                }
                InputEvent::Blur => {
                    let value = input.read(cx).value().to_string();
                    if !monocode_core::js::trim(&value).is_empty() {
                        this.add_tag(&value, window, cx);
                    }
                }
                _ => {}
            });
        let source_events = cx.subscribe_in(&source, window, |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                if value != this.synced_body {
                    this.synced_body = value.clone();
                    this.data.edit_body(&value, cx);
                }
            }
        });
        data.open_page(cx);
        let page = data.page(cx);
        let list_width = cx
            .try_global::<RememberedWidth>()
            .map_or(DEFAULT_WIDTH, |width| width.0);
        Self {
            data,
            projects,
            chrome: PageChrome::default(),
            cwd: cwd.map(str::to_string),
            on_close: None,
            page,
            list_width,
            resize: None,
            focus: cx.focus_handle(),
            query,
            title,
            tag,
            source,
            preview,
            preview_dark: Theme::of(cx).is_dark(),
            picker,
            list_scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            editor_note: None,
            synced_title: String::new(),
            synced_body: String::new(),
            _subscriptions: vec![
                changes,
                query_events,
                title_events,
                tag_events,
                source_events,
            ],
        }
    }

    pub fn chrome(mut self, chrome: PageChrome) -> Self {
        self.chrome = chrome;
        self
    }

    /// `onClose`: Escape and Add to chat leave the page.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn page(&self) -> &NotesPage {
        &self.page
    }

    /// The active project changed while the window kept this page cached.
    pub fn set_cwd(&mut self, cwd: Option<&str>, cx: &mut Context<Self>) {
        if self.cwd.as_deref() != cwd {
            self.cwd = cwd.map(str::to_owned);
            self.picker
                .update(cx, |picker, cx| picker.set_rail_cwd(cwd, cx));
            cx.notify();
        }
    }

    pub fn list_width(&self) -> f32 {
        self.list_width
    }

    pub fn source_editor(&self) -> &Entity<EditorState> {
        &self.source
    }

    pub fn title_input(&self) -> &Entity<InputState> {
        &self.title
    }

    pub fn tag_input(&self) -> &Entity<InputState> {
        &self.tag
    }

    pub fn picker(&self) -> &Entity<ProjectPicker> {
        &self.picker
    }

    /// The open note's Preview or Source tab.
    pub fn mode(&self, cx: &App) -> MarkdownMode {
        self.page
            .editor
            .as_ref()
            .map_or(MarkdownMode::Preview, |editor| {
                MarkdownModes::get(&editor.note_id, cx)
            })
    }

    pub fn set_mode(&mut self, mode: MarkdownMode, cx: &mut Context<Self>) {
        if let Some(editor) = self.page.editor.as_ref() {
            MarkdownModes::set(&editor.note_id, mode, cx);
            cx.notify();
        }
    }

    fn close(&self, window: &mut Window, cx: &mut App) {
        self.data.close_page(cx);
        if let Some(close) = self.on_close.clone() {
            close(window, cx);
        }
    }

    fn add_tag(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.tag
            .update(cx, |field, cx| field.set_value("", window, cx));
        self.data.add_tag(input, cx);
    }

    /// Put the data's editor into the fields: a new note resets them, and an
    /// outside write replaces text the user has not changed.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = Theme::of(cx).is_dark();
        if dark != self.preview_dark {
            self.preview_dark = dark;
            let style = markdown_style(Theme::of(cx));
            self.preview
                .update(cx, |view, cx| view.set_style(style, cx));
        }
        let Some(editor) = self.page.editor.clone() else {
            self.editor_note = None;
            return;
        };
        let new_note = self.editor_note.as_deref() != Some(editor.note_id.as_str());
        if new_note {
            self.editor_note = Some(editor.note_id.clone());
            if editor.blank {
                // New untitled notes open in source so typing is not behind
                // the preview.
                MarkdownModes::set(&editor.note_id, MarkdownMode::Source, cx);
            }
            self.tag
                .update(cx, |field, cx| field.set_value("", window, cx));
        }
        if new_note || editor.title != self.synced_title {
            self.synced_title = editor.title.clone();
            let title = editor.title.clone();
            self.title.update(cx, |field, cx| {
                if field.value() != title {
                    field.set_value(title, window, cx);
                }
            });
        }
        if new_note || editor.body != self.synced_body {
            self.synced_body = editor.body.clone();
            let body = editor.body.clone();
            self.source.update(cx, |field, cx| {
                if field.value() != body {
                    field.set_value(body, window, cx);
                }
            });
        }
        let body = editor.body.clone();
        self.preview.update(cx, |view, cx| {
            if view.text() != body {
                view.set_text(&body, cx);
            }
        });
        let cwd = editor.source_cwd.clone().unwrap_or_else(|| "~".into());
        self.picker
            .update(cx, |picker, cx| picker.set_cwd(&cwd, cx));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
            cx.stop_propagation();
            self.close(window, cx);
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(resize) = self.resize else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.finish_resize(cx);
            return;
        }
        let scale = f32::from(window.rem_size()) / 16.0;
        let delta = f32::from(event.position.x - resize.start_x) / scale;
        let max = MAX_WIDTH.min((f32::from(window.viewport_size().width) / scale * 0.5).round());
        self.list_width = (resize.start_width + delta).clamp(MIN_WIDTH, max.max(MIN_WIDTH));
        cx.notify();
    }

    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if self.resize.take().is_some() {
            cx.set_global(RememberedWidth(self.list_width));
            cx.notify();
        }
    }

    /// Dropped files: save the images into the note at the source field's
    /// selection, or at the end of the body.
    fn drop_paths(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        if paths.is_empty() || self.page.editor.is_none() {
            return;
        }
        let range = if self.mode(cx) == MarkdownMode::Source {
            self.source.read(cx).selected_range()
        } else {
            let end = self.synced_body.len();
            end..end
        };
        let task = self
            .data
            .insert_image_paths(paths, range.start, range.end, cx);
        let source = self.source.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Some(cursor) = task.await {
                source
                    .update_in(cx, |field, window, cx| {
                        field.focus(window, cx);
                        field.set_selected_range(cursor..cursor, cx);
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn render_list(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let page = &self.page;
        let creating = page.creating;
        let new_button = div()
            .id("new-note")
            .debug_selector(|| "new-note".into())
            .flex()
            .flex_none()
            .size(u(24.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .group("new-note")
            .tooltip(monocode_ui::widgets::tooltip("New note"))
            .map(|button| {
                let hover = theme.content(0.10);
                if creating {
                    button.opacity(0.4).child(spinner_icon(
                        "new-note-spinner",
                        14.,
                        theme.content(0.45),
                    ))
                } else {
                    let ink = theme.colors.content;
                    button
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let cwd = this.cwd.clone();
                            this.data.create(cwd.as_deref(), cx);
                        }))
                        .child(
                            icon(IconName::Plus)
                                .size(u(14.))
                                .text_color(theme.content(0.45))
                                .group_hover("new-note", move |s| s.text_color(ink)),
                        )
                }
            });
        let toolbar = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(4.))
            .px(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h(u(28.))
                    .items_center()
                    .child(
                        div().absolute().left(u(8.)).child(
                            icon(IconName::Search)
                                .size(u(12.))
                                .text_color(theme.content(0.50)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .size_full()
                            .items_center()
                            .pl(u(28.))
                            .pr(u(8.))
                            .text_px(theme.text.label)
                            .child(plain_input(&self.query, None, cx)),
                    ),
            )
            .child(new_button);
        let mut list = div()
            .id("notes-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll);
        let message = |text: String| {
            div()
                .px(u(12.))
                .py(u(8.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child(text)
        };
        if let (Some(error), true) = (page.error.clone(), page.notes.is_empty()) {
            list = list.child(message(error));
        } else if page.loading && page.notes.is_empty() {
            list = list.child(div().flex().justify_center().py(u(40.)).child(spinner_icon(
                "notes-loading",
                16.,
                theme.content(0.40),
            )));
        } else if page.visible.is_empty() {
            list = list.child(message(
                if monocode_core::js::trim(&page.query).is_empty() {
                    "No notes yet. Save a turn from the transcript, or create one here.".into()
                } else {
                    "No matching notes".into()
                },
            ));
        } else {
            let selected = page.selected().map(|note| note.id.clone());
            let now = now_ms();
            let mut items = div().flex().flex_col().gap(u(2.)).p(u(6.));
            for note in page.visible.iter().cloned() {
                let active = selected.as_deref() == Some(note.id.as_str());
                let mark = note
                    .source_cwd
                    .as_deref()
                    .filter(|cwd| looks_like_project(cwd))
                    .map(|cwd| self.projects.mark(cwd, cx));
                let id = note.id.clone();
                items = items.child(
                    div()
                        .id(gpui::ElementId::Name(
                            format!("note-row-{}", note.id).into(),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| this.data.select(&id, cx)))
                        .child(note_card(note, active, mark, now)),
                );
            }
            list = list.child(items);
        }
        let dragging = self.resize.is_some();
        let handle_hover = theme.content(0.10);
        let handle = div()
            .id("notes-resize")
            .debug_selector(|| "notes-resize".into())
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-1.))
            .w(u(theme.metrics.resize_handle_width))
            .cursor_col_resize()
            .map(|el| {
                if dragging {
                    el.bg(theme.content(0.15))
                } else {
                    el.hover(move |s| s.bg(handle_hover))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    if event.click_count >= 2 {
                        this.list_width = DEFAULT_WIDTH;
                        cx.set_global(RememberedWidth(DEFAULT_WIDTH));
                        cx.notify();
                        return;
                    }
                    this.resize = Some(Resize {
                        start_x: event.position.x,
                        start_width: this.list_width,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .w(u(self.list_width))
            .border_r_1()
            .border_color(theme.colors.stroke)
            .child(toolbar)
            .child(list)
            .child(handle)
    }

    fn render_tags(
        &self,
        editor: &NoteEditorView,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mut row = div()
            .flex()
            .flex_wrap()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .debug_selector(|| "note-tags".into())
            .child(
                div()
                    .mr(u(2.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child("Tags"),
            );
        for tag in &editor.tags {
            let remove = tag.clone();
            let hover = theme.content(0.10);
            let ink = theme.colors.content;
            let group = format!("remove-tag-{tag}");
            row =
                row.child(
                    div()
                        .flex()
                        .h(u(24.))
                        .max_w(u(192.))
                        .items_center()
                        .gap(u(4.))
                        .pl(u(8.))
                        .pr(u(4.))
                        .rounded(u(theme.radius.md))
                        .bg(theme.content(0.08))
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.70))
                        .child(div().min_w_0().truncate().child(format!("#{tag}")))
                        .child(
                            div()
                                .id(gpui::ElementId::Name(group.clone().into()))
                                .debug_selector({
                                    let tag = tag.clone();
                                    move || format!("remove-tag {tag}")
                                })
                                .flex()
                                .flex_none()
                                .size(u(16.))
                                .items_center()
                                .justify_center()
                                .rounded(u(theme.radius.sm))
                                .group(group.clone())
                                .hover(move |s| s.bg(hover))
                                .tooltip(monocode_ui::widgets::tooltip(format!("Remove #{tag}")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.data.remove_tag(&remove, cx)
                                }))
                                .child(
                                    icon(IconName::X)
                                        .size(u(10.))
                                        .text_color(theme.content(0.40))
                                        .group_hover(group, move |s| s.text_color(ink)),
                                ),
                        ),
                );
        }
        if editor.tags.len() < MAX_NOTE_TAGS {
            let last = editor.tags.last().cloned();
            let tag_input = self.tag.clone();
            row = row.child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(u(80.))
                    .h(u(24.))
                    .items_center()
                    .px(u(4.))
                    .text_px(theme.text.caption)
                    .debug_selector(|| "note-tag-input".into())
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "backspace"
                            && tag_input.read(cx).value().is_empty()
                            && let Some(last) = last.clone()
                        {
                            this.data.remove_tag(&last, cx);
                        }
                    }))
                    .child(plain_input(&self.tag, Some(theme.content(0.35)), cx)),
            );
        }
        row
    }

    fn render_editor(
        &self,
        editor: &NoteEditorView,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let time = format_relative_time(editor.updated_at, now_ms());
        let mode = self.mode(cx);
        let ink = theme.colors.content;
        let mut meta = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.50))
            .child(
                icon(IconName::File)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            )
            .child("Note");
        if !editor.slug.is_empty() {
            meta = meta.child(div().min_w_0().truncate().child(editor.slug.clone()));
        }
        meta = meta.child(self.picker.clone());
        let can_add = editor.can_add_to_chat;
        let add_to_chat = div()
            .id("note-add-to-chat")
            .debug_selector(|| "note-add-to-chat".into())
            .flex()
            .h(u(26.))
            .items_center()
            .gap(u(4.))
            .px(u(12.))
            .rounded(u(theme.radius.md))
            .bg(ink)
            .text_px(theme.text.label)
            .text_color(theme.colors.background_base)
            .map(|button| {
                if can_add {
                    let hover = theme.content(0.80);
                    button.hover(move |s| s.bg(hover)).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.data.add_to_chat(cx);
                            if let Some(close) = this.on_close.clone() {
                                close(window, cx);
                            }
                        },
                    ))
                } else {
                    button.opacity(0.4)
                }
            })
            .child("Add to chat");
        let danger = theme.colors.danger;
        let delete_hover = theme.content(0.10);
        let delete = div()
            .id("note-delete")
            .debug_selector(|| "note-delete".into())
            .flex()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .px(u(12.))
            .rounded(u(theme.radius.md))
            .text_px(theme.text.label)
            .text_color(theme.content(0.70))
            .group("note-delete")
            .hover(move |s| s.bg(delete_hover).text_color(danger))
            .on_click(cx.listener(|this, _, _, cx| this.data.delete_current(cx)))
            .child(
                icon(IconName::Trash2)
                    .size(u(14.))
                    .text_color(theme.content(0.70))
                    .group_hover("note-delete", move |s| s.text_color(danger)),
            )
            .child("Delete");
        let mut header = div().flex().flex_col().gap(u(12.)).child(meta).child(
            div()
                .w_full()
                .h(u(25.))
                .flex()
                .items_center()
                .text_px(theme.text.title)
                .semibold()
                .leading(theme.leading.tight)
                .debug_selector(|| "note-title".into())
                .child(plain_input(&self.title, Some(theme.content(0.35)), cx)),
        );
        if !time.is_empty() {
            header = header.child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(format!("Updated {time}")),
            );
        }
        header = header.child(self.render_tags(editor, theme, cx)).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(u(8.))
                .pt(u(4.))
                .child(add_to_chat)
                .child(delete),
        );
        if let Some(error) = editor.save_error.clone() {
            header = header.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .text_px(theme.text.label)
                    .text_color(monocode_ui::color::with_alpha(theme.colors.danger, 0.9))
                    .debug_selector(|| "note-save-error".into())
                    .child(format!("Could not save note: {error}"))
                    .child(
                        div()
                            .id("note-retry")
                            .debug_selector(|| "note-retry".into())
                            .flex_none()
                            .underline()
                            .on_click(cx.listener(|this, _, _, cx| this.data.retry_save(cx)))
                            .child("Retry"),
                    ),
            );
        }
        let tabs = div()
            .flex()
            .h(u(36.))
            .items_stretch()
            .gap(u(16.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                page_tab(
                    "note-tab-preview",
                    "Preview",
                    mode == MarkdownMode::Preview,
                    theme,
                )
                .debug_selector(|| "note-tab-preview".into())
                .on_click(cx.listener(|this, _, _, cx| this.set_mode(MarkdownMode::Preview, cx))),
            )
            .child(
                page_tab(
                    "note-tab-source",
                    "Source",
                    mode == MarkdownMode::Source,
                    theme,
                )
                .debug_selector(|| "note-tab-source".into())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_mode(MarkdownMode::Source, cx);
                    let source = this.source.clone();
                    source.update(cx, |field, cx| field.focus(window, cx));
                })),
            );
        let content: gpui::AnyElement = match mode {
            MarkdownMode::Source => {
                let style = source_style(theme);
                self.source.update(cx, |field, _| {
                    field.set_editor_style(style);
                    field.set_editor_paddings(gpui::Edges::default());
                });
                let lines = editor.body.split('\n').count().max(1) as f32;
                let height =
                    (lines * SOURCE_LINE_HEIGHT + SOURCE_LINE_HEIGHT).max(CONTENT_MIN_HEIGHT);
                let _ = window;
                div()
                    .w_full()
                    .h(u(height))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.body)
                    .line_height(u(SOURCE_LINE_HEIGHT))
                    .text_color(theme.content(0.85))
                    .debug_selector(|| "note-source".into())
                    .child(self.source.clone())
                    .into_any_element()
            }
            MarkdownMode::Preview if !monocode_core::js::trim(&editor.body).is_empty() => div()
                .w_full()
                .debug_selector(|| "note-preview".into())
                .child(self.preview.clone())
                .into_any_element(),
            MarkdownMode::Preview => div()
                .text_px(theme.text.body)
                .text_color(theme.content(0.45))
                .child("No description")
                .into_any_element(),
        };
        let busy = editor.image_busy;
        let accent_border = theme.accent(0.60);
        let accent_fill = theme.accent(0.05);
        let overlay = |text: &'static str, visible: bool| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.lg))
                .bg(monocode_ui::color::with_alpha(
                    theme.colors.background_base,
                    0.8,
                ))
                .text_px(theme.text.label)
                .text_color(theme.content(0.70))
                .when(!visible, |el| el.invisible())
                .child(text)
        };
        let drop_zone = div()
            .id("note-drop-zone")
            .debug_selector(|| "note-drop-zone".into())
            .group("note-drop")
            .relative()
            .min_h(u(CONTENT_MIN_HEIGHT))
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(gpui::transparent_black())
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.border_color(accent_border).bg(accent_fill)
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_paths(paths, window, cx)
            }))
            .child(content)
            .child(if busy {
                overlay("Adding images…", true)
            } else {
                overlay("Drop images here", false)
                    .group_drag_over::<ExternalPaths>("note-drop", |s| s.visible())
            });
        div()
            .id("note-detail")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .child(
                div()
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(u(1024.))
                    .gap(u(20.))
                    .px(u(32.))
                    .py(u(32.))
                    .child(header)
                    .child(tabs)
                    .child(drop_zone),
            )
    }

    fn render_empty(&self, theme: &Theme) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .justify_center()
            .px(u(24.))
            .child(
                icon(IconName::File)
                    .size(u(24.))
                    .mb(u(12.))
                    .text_color(theme.content(0.30)),
            )
            .child(
                div()
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.45))
                    .child("Select a note"),
            )
    }
}

impl Focusable for NotesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_fields(window, cx);
        let theme = Theme::of(cx).clone();
        let editor = self.page.editor.clone();
        let detail = match editor {
            Some(editor) => self
                .render_editor(&editor, &theme, window, cx)
                .into_any_element(),
            None => self.render_empty(&theme).into_any_element(),
        };
        let mut root = div()
            .id("notes-view")
            .key_context("NotesView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.finish_resize(cx)),
            )
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(page_title_bar(
                &self.chrome,
                IconName::File,
                div().min_w_0().truncate().child("Notes"),
                &theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_list(&theme, cx))
                    .child(detail),
            );
        if self.resize.is_some() {
            root = root.cursor_col_resize();
        }
        root
    }
}

/// Default for [`NotesData::image_source`]: note assets need the data
/// directory, so only web, file, and data URLs load.
pub fn default_note_image(url: &str) -> Option<gpui::ImageSource> {
    if url.starts_with(super::NOTE_IMAGE_PREFIX) {
        return None;
    }
    default_image_source(url)
}
