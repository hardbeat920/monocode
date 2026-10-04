//! An in-memory [`NotesData`] for the gallery, the tests, and hosts that run
//! the page without the engine. Saves land at once (title edits on commit),
//! and `fail_next_save` makes the next save fail, so the error and retry
//! states can be shown.

use std::rc::Rc;

use gpui::{App, AppContext as _, Entity, Subscription, Task};
use monocode_core::js;

use super::data::{Note, NoteEditorView, NotesData, NotesPage};
use super::model::{normalize_note_tags, note_source_project, note_title};
use crate::data::Listener;
use crate::format::looks_like_project;

#[derive(Default)]
struct Edits {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

struct Editor {
    note: Note,
    edits: Edits,
    project: Option<String>,
    save_error: Option<String>,
    image_busy: bool,
    blank: bool,
}

/// The state behind [`LocalNotes`].
pub struct LocalNotesState {
    notes: Vec<Note>,
    open: bool,
    query: String,
    selected_id: Option<String>,
    error: Option<String>,
    editor: Option<Editor>,
    fail_next: Option<String>,
    next_id: u64,
    /// The chats `add_to_chat` asked for, by note id.
    pub added_to_chat: Vec<String>,
    /// The image paths `insert_image_paths` received.
    pub dropped: Vec<Vec<String>>,
}

/// In-memory notes. Clones share one store.
#[derive(Clone)]
pub struct LocalNotes {
    state: Entity<LocalNotesState>,
}

impl LocalNotes {
    pub fn new(notes: Vec<Note>, cx: &mut App) -> Self {
        let mut notes = notes;
        sort_notes(&mut notes);
        Self {
            state: cx.new(|_| LocalNotesState {
                notes,
                open: false,
                query: String::new(),
                selected_id: None,
                error: None,
                editor: None,
                fail_next: None,
                next_id: 1,
                added_to_chat: Vec::new(),
                dropped: Vec::new(),
            }),
        }
    }

    pub fn rc(self) -> Rc<dyn NotesData> {
        Rc::new(self)
    }

    pub fn state(&self) -> &Entity<LocalNotesState> {
        &self.state
    }

    /// The stored note.
    pub fn note(&self, id: &str, cx: &App) -> Option<Note> {
        self.state
            .read(cx)
            .notes
            .iter()
            .find(|note| note.id == id)
            .cloned()
    }

    /// Make the next save fail with `message`.
    pub fn fail_next_save(&self, message: &str, cx: &mut App) {
        self.state
            .update(cx, |state, _| state.fail_next = Some(message.to_string()));
    }

    /// A write from outside the page, such as the Operator, then
    /// `NOTES_CHANGED_EVENT`.
    pub fn write_from_outside(&self, note: Note, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            match state.notes.iter_mut().find(|item| item.id == note.id) {
                Some(item) => *item = note,
                None => state.notes.push(note),
            }
            sort_notes(&mut state.notes);
            state.sync_editor();
            cx.notify();
        });
    }

    /// Show the image busy overlay, for screenshots.
    pub fn set_image_busy(&self, busy: bool, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            if let Some(editor) = state.editor.as_mut() {
                editor.image_busy = busy;
            }
            cx.notify();
        });
    }

    fn update(&self, cx: &mut App, f: impl FnOnce(&mut LocalNotesState)) {
        self.state.update(cx, |state, cx| {
            f(state);
            cx.notify();
        });
    }
}

fn sort_notes(notes: &mut [Note]) {
    notes.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| monocode_locale::compare(&a.id, &b.id))
    });
}

impl LocalNotesState {
    fn visible(&self) -> Vec<Note> {
        let needle = js::trim(&self.query).to_lowercase();
        if needle.is_empty() {
            return self.notes.clone();
        }
        let tag_needle = needle.strip_prefix('#').unwrap_or(&needle);
        self.notes
            .iter()
            .filter(|note| {
                let project = note_source_project(note.source_cwd.as_deref())
                    .map(|name| name.to_lowercase())
                    .unwrap_or_default();
                note.title.to_lowercase().contains(&needle)
                    || note.body.to_lowercase().contains(&needle)
                    || note.slug.to_lowercase().contains(&needle)
                    || note.tags.iter().any(|tag| tag.contains(tag_needle))
                    || project.contains(&needle)
            })
            .cloned()
            .collect()
    }

    fn selected(&self) -> Option<Note> {
        let id = self.selected_id.as_deref()?;
        self.visible()
            .into_iter()
            .find(|note| note.id == id)
            .or_else(|| self.notes.iter().find(|note| note.id == id).cloned())
    }

    /// Keep the selection when it still exists, else the first note.
    fn refresh_selection(&mut self) {
        self.selected_id = self
            .selected_id
            .clone()
            .filter(|id| self.notes.iter().any(|note| note.id == *id))
            .or_else(|| self.notes.first().map(|note| note.id.clone()));
    }

    fn sync_editor(&mut self) {
        let selected = if self.open { self.selected() } else { None };
        match (selected, self.editor.as_mut()) {
            (Some(note), Some(editor)) if editor.note.id == note.id => editor.note = note,
            (selected, _) => {
                self.save();
                self.editor = selected.map(|note| Editor {
                    blank: js::trim(&note.body).is_empty() && note.title == "Untitled",
                    note,
                    edits: Edits::default(),
                    project: None,
                    save_error: None,
                    image_busy: false,
                });
            }
        }
    }

    /// `persist`: save the pending edits and project move.
    fn save(&mut self) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let current = editor.note.clone();
        let body = editor.edits.body.clone().unwrap_or(current.body.clone());
        let title = editor.edits.title.clone().unwrap_or(current.title.clone());
        let title = match js::trim(&title) {
            "" => note_title(&body),
            trimmed => trimmed.to_string(),
        };
        let tags = editor.edits.tags.clone().unwrap_or(current.tags.clone());
        let project = editor.project.clone();
        let unchanged = title == current.title
            && body == current.body
            && tags == current.tags
            && project
                .as_deref()
                .is_none_or(|path| Some(path) == current.source_cwd.as_deref());
        if unchanged {
            editor.edits = Edits::default();
            editor.project = None;
            editor.save_error = None;
            return;
        }
        if let Some(message) = self.fail_next.take() {
            editor.save_error = Some(message);
            return;
        }
        let saved = Note {
            title,
            body,
            tags,
            source_cwd: project.or(current.source_cwd.clone()),
            updated_at: current.updated_at + 1,
            ..current
        };
        editor.note = saved.clone();
        editor.edits = Edits::default();
        editor.project = None;
        editor.save_error = None;
        if let Some(item) = self.notes.iter_mut().find(|note| note.id == saved.id) {
            *item = saved;
        }
        sort_notes(&mut self.notes);
    }

    fn editor_view(&self) -> Option<NoteEditorView> {
        let editor = self.editor.as_ref()?;
        let body = editor
            .edits
            .body
            .clone()
            .unwrap_or_else(|| editor.note.body.clone());
        Some(NoteEditorView {
            note_id: editor.note.id.clone(),
            slug: editor.note.slug.clone(),
            updated_at: editor.note.updated_at,
            title: editor
                .edits
                .title
                .clone()
                .unwrap_or_else(|| editor.note.title.clone()),
            can_add_to_chat: !js::trim(&body).is_empty(),
            body,
            tags: editor
                .edits
                .tags
                .clone()
                .unwrap_or_else(|| editor.note.tags.clone()),
            source_cwd: editor
                .project
                .clone()
                .or_else(|| editor.note.source_cwd.clone()),
            save_error: editor.save_error.clone(),
            image_busy: editor.image_busy,
            blank: editor.blank,
        })
    }
}

impl NotesData for LocalNotes {
    fn page(&self, cx: &App) -> NotesPage {
        let state = self.state.read(cx);
        NotesPage {
            notes: state.notes.clone(),
            visible: state.visible(),
            selected_id: state.selected_id.clone(),
            loading: false,
            error: state.error.clone(),
            query: state.query.clone(),
            creating: false,
            editor: state.editor_view(),
        }
    }

    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.state, move |_, cx| listener(cx))
    }

    fn open_page(&self, cx: &mut App) {
        self.update(cx, |state| {
            state.open = true;
            state.refresh_selection();
            state.sync_editor();
        });
    }

    fn close_page(&self, cx: &mut App) {
        self.update(cx, |state| {
            state.open = false;
            state.sync_editor();
        });
    }

    fn set_query(&self, query: &str, cx: &mut App) {
        self.update(cx, |state| {
            state.query = query.to_string();
            state.sync_editor();
        });
    }

    fn select(&self, note_id: &str, cx: &mut App) {
        self.update(cx, |state| {
            state.selected_id = Some(note_id.to_string());
            state.sync_editor();
        });
    }

    fn create(&self, cwd: Option<&str>, cx: &mut App) {
        self.update(cx, |state| {
            let id = format!("local-note-{}", state.next_id);
            state.next_id += 1;
            let updated_at = state
                .notes
                .iter()
                .map(|note| note.updated_at)
                .max()
                .unwrap_or(0)
                + 1;
            state.notes.push(Note {
                id: id.clone(),
                slug: format!("untitled-{}", state.next_id - 1),
                title: "Untitled".into(),
                body: String::new(),
                tags: Vec::new(),
                source_session_id: None,
                source_cwd: cwd
                    .filter(|cwd| looks_like_project(cwd))
                    .map(str::to_string),
                created_at: updated_at,
                updated_at,
            });
            sort_notes(&mut state.notes);
            state.selected_id = Some(id);
            state.query.clear();
            state.sync_editor();
        });
    }

    fn edit_title(&self, title: &str, cx: &mut App) {
        self.update(cx, |state| {
            if let Some(editor) = state.editor.as_mut() {
                editor.edits.title = Some(title.to_string());
            }
        });
    }

    fn commit_title(&self, cx: &mut App) {
        self.update(cx, |state| state.save());
    }

    fn edit_body(&self, body: &str, cx: &mut App) {
        self.update(cx, |state| {
            if let Some(editor) = state.editor.as_mut() {
                editor.edits.body = Some(body.to_string());
            }
            state.save();
        });
    }

    fn add_tag(&self, input: &str, cx: &mut App) {
        self.update(cx, |state| {
            let Some(view) = state.editor_view() else {
                return;
            };
            let mut all = view.tags.clone();
            all.push(input.to_string());
            let next = normalize_note_tags(&all);
            if next != view.tags {
                if let Some(editor) = state.editor.as_mut() {
                    editor.edits.tags = Some(next);
                }
                state.save();
            }
        });
    }

    fn remove_tag(&self, tag: &str, cx: &mut App) {
        self.update(cx, |state| {
            let Some(view) = state.editor_view() else {
                return;
            };
            if let Some(editor) = state.editor.as_mut() {
                editor.edits.tags =
                    Some(view.tags.into_iter().filter(|item| item != tag).collect());
            }
            state.save();
        });
    }

    fn choose_project(&self, path: &str, cx: &mut App) {
        self.update(cx, |state| {
            if let Some(editor) = state.editor.as_mut() {
                editor.project = Some(path.to_string());
            }
            state.save();
        });
    }

    fn retry_save(&self, cx: &mut App) {
        self.update(cx, |state| {
            if let Some(editor) = state.editor.as_mut() {
                editor.save_error = None;
            }
            state.save();
        });
    }

    fn delete_current(&self, cx: &mut App) {
        self.update(cx, |state| {
            let Some(editor) = state.editor.take() else {
                return;
            };
            state.notes.retain(|note| note.id != editor.note.id);
            if state.selected_id.as_deref() == Some(editor.note.id.as_str()) {
                state.selected_id = state.notes.first().map(|note| note.id.clone());
            }
            state.sync_editor();
        });
    }

    fn add_to_chat(&self, cx: &mut App) {
        self.update(cx, |state| {
            if let Some(view) = state.editor_view()
                && view.can_add_to_chat
            {
                state.added_to_chat.push(view.note_id);
            }
            state.open = false;
            state.sync_editor();
        });
    }

    fn insert_image_paths(
        &self,
        paths: Vec<String>,
        _start: usize,
        _end: usize,
        cx: &mut App,
    ) -> Task<Option<usize>> {
        self.update(cx, |state| state.dropped.push(paths));
        Task::ready(None)
    }
}
