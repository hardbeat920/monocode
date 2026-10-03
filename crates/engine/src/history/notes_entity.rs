//! The `Notes` entity: the notes.ts cache and store calls (`loadNotes`,
//! `getNote`, `upsertNote`, `deleteNote`, `createNote`, `applyNotesToTurn`),
//! the image saves of noteImages.ts, and the model state of
//! src/features/notes/ui/NotesView.tsx: the list, filter, and selection, and
//! the editor's save model (pending edits, the 400 ms autosave, the per-note
//! save queue, project moves, and retry).
//!
//! The window events become `NotesEvent::AddToChat` (from
//! `requestAddNoteToChat`) and `Notes::notes_changed` (for
//! `NOTES_CHANGED_EVENT`). Also ports App.tsx `onAddNoteToChat` and
//! `onNoteCardDismiss` (lines 2339-2384 and 2424-2433).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AsyncApp, Context, EventEmitter, Task, WeakEntity};
use monocode_core::RuntimeMode;
use monocode_core::js;
use monocode_core::notes::NoteComposerCard;

use super::host::HistoryHost;
use super::note_images::{
    MarkdownInsertion, NO_IMAGES_ERROR, NONE_SAVED_ERROR, NoteImageAsset, NoteImageData,
    insert_note_images_markdown, is_image_name,
};
use super::notes::{
    NewNote, Note, NoteUpsert, apply_notes_to_text, can_add_note_to_chat, create_note_upsert,
    normalize_note_tags, note_composer_card, note_slugs_in_text, note_source_project, note_title,
};
use super::notes_backend::NotesBackend;
use super::paths::looks_like_project;
use crate::runtime::Engine;

/// The pause after typing before a note saves.
pub const NOTE_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// What `Notes` tells the app.
#[derive(Debug, Clone, PartialEq)]
pub enum NotesEvent {
    /// `ADD_NOTE_TO_CHAT_EVENT`: start a chat with this note
    /// (`open_note_chat`).
    AddToChat(NoteComposerCard),
}

type SharedNotes = Shared<Task<Vec<Note>>>;

/// Field edits not saved yet (`Edits`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NoteEdits {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

/// An unsaved project choice. The version tells two choices of the same
/// path apart, as object identity did.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectChange {
    path: String,
    version: u64,
}

/// One mounted note editor. An editor that left the screen lives on until
/// its queued saves finish, as the TypeScript closures did.
struct EditorState {
    /// `noteRef`: the latest note this editor knows.
    note: Note,
    edits: NoteEdits,
    project_change: Option<ProjectChange>,
    save_error: Option<String>,
    image_busy: bool,
    /// Set by Delete so the unmount save does nothing.
    skip_save: bool,
    save_timer: Option<Task<()>>,
    /// Queued saves that still read this editor.
    pending: usize,
    /// The note was blank when the editor opened (source mode first).
    blank: bool,
}

/// `noteSaveQueues`: saves of one note run in order, each seeing the note
/// the previous one saved.
struct SaveQueue {
    tail: Shared<Task<()>>,
    saved: Rc<RefCell<Option<Note>>>,
    job: u64,
}

enum SaveJob {
    Persist(u64),
    Delete,
}

enum SavePlan {
    Unchanged {
        current: Note,
        changes: NoteEdits,
        project: Option<ProjectChange>,
    },
    Save {
        upsert: NoteUpsert,
        current: Note,
        changes: NoteEdits,
        project: Option<ProjectChange>,
    },
}

/// What a view shows for the open note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteEditorView {
    pub note_id: String,
    pub slug: String,
    pub updated_at: i64,
    /// The title field, with unsaved edits.
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    /// The project, with an unsaved move.
    pub source_cwd: Option<String>,
    pub save_error: Option<String>,
    pub image_busy: bool,
    /// The note was blank when it opened, so it starts in source mode.
    pub blank: bool,
    pub can_add_to_chat: bool,
}

/// Notes: the store cache plus the notes page.
pub struct Notes {
    backend: Arc<dyn NotesBackend>,
    /// The notes.ts module cache.
    cache: Option<Vec<Note>>,
    inflight: Option<(u64, SharedNotes)>,
    next_load: u64,
    // The page.
    open: bool,
    notes: Vec<Note>,
    loading: bool,
    error: Option<String>,
    query: String,
    selected_id: Option<String>,
    creating: bool,
    editors: HashMap<u64, EditorState>,
    current_editor: Option<u64>,
    next_editor: u64,
    next_project_change: u64,
    save_queues: HashMap<String, SaveQueue>,
    next_job: u64,
}

impl EventEmitter<NotesEvent> for Notes {}

impl Notes {
    pub fn new(backend: Arc<dyn NotesBackend>, _cx: &mut Context<Self>) -> Self {
        Self {
            backend,
            cache: None,
            inflight: None,
            next_load: 0,
            open: false,
            notes: Vec::new(),
            loading: true,
            error: None,
            query: String::new(),
            selected_id: None,
            creating: false,
            editors: HashMap::new(),
            current_editor: None,
            next_editor: 0,
            next_project_change: 0,
            save_queues: HashMap::new(),
            next_job: 0,
        }
    }

    pub fn backend(&self) -> &Arc<dyn NotesBackend> {
        &self.backend
    }

    // The notes.ts cache and store calls.

    /// `peekNotes`.
    pub fn peek_notes(&self) -> Option<&[Note]> {
        self.cache.as_deref()
    }

    /// `invalidateNotes`.
    pub fn invalidate_notes(&mut self) {
        self.cache = None;
    }

    /// `loadNotes`: the cache, the listing in flight, or a new listing. A
    /// failed listing resolves to the cache, or to no notes.
    pub fn load_notes(&mut self, refresh: bool, cx: &mut Context<Self>) -> SharedNotes {
        if !refresh && let Some(cache) = self.cache.clone() {
            return Task::ready(cache).shared();
        }
        if !refresh && let Some((_, inflight)) = self.inflight.as_ref() {
            return inflight.clone();
        }
        let id = self.next_load;
        self.next_load += 1;
        let list = self.backend.list();
        let loading = cx
            .spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let listed = list.await;
                this.update(cx, |this, _| {
                    let notes = match listed {
                        Ok(notes) => {
                            this.cache = Some(notes.clone());
                            notes
                        }
                        Err(_) => this.cache.get_or_insert_with(Vec::new).clone(),
                    };
                    if this
                        .inflight
                        .as_ref()
                        .is_some_and(|(pending, _)| *pending == id)
                    {
                        this.inflight = None;
                    }
                    notes
                })
                .unwrap_or_default()
            })
            .shared();
        self.inflight = Some((id, loading.clone()));
        loading
    }

    /// `getNote`.
    pub fn get_note(&self, id: &str, cx: &mut Context<Self>) -> Task<Result<Option<Note>, String>> {
        let get = self.backend.get(id.to_string());
        cx.spawn(async move |_, _| get.await)
    }

    /// `upsertNote`: save and drop the cache.
    pub fn upsert_note(
        &mut self,
        note: NoteUpsert,
        cx: &mut Context<Self>,
    ) -> Task<Result<Note, String>> {
        let upsert = self.backend.upsert(note);
        cx.spawn(async move |this, cx| {
            let saved = upsert.await?;
            this.update(cx, |this, _| this.cache = None).ok();
            Ok(saved)
        })
    }

    /// `deleteNote`: delete and drop the cache.
    pub fn delete_note(&mut self, id: &str, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let delete = self.backend.delete(id.to_string());
        cx.spawn(async move |this, cx| {
            delete.await?;
            this.update(cx, |this, _| this.cache = None).ok();
            Ok(())
        })
    }

    /// `createNote`.
    pub fn create_note(
        &mut self,
        input: &NewNote,
        cx: &mut Context<Self>,
    ) -> Task<Result<Note, String>> {
        let upsert = create_note_upsert(uuid::Uuid::new_v4().to_string(), input);
        self.upsert_note(upsert, cx)
    }

    /// `applyNotesToTurn`: add every `@note/slug` the text mentions.
    pub fn apply_notes_to_turn(&mut self, text: &str, cx: &mut Context<Self>) -> Task<String> {
        if note_slugs_in_text(text).is_empty() {
            return Task::ready(text.to_string());
        }
        let notes = self.load_notes(false, cx);
        let text = text.to_string();
        cx.spawn(async move |_, _| apply_notes_to_text(&text, &notes.await))
    }

    /// `requestAddNoteToChat`: a note with text asks the app for a chat.
    pub fn request_add_note_to_chat(&mut self, note: &Note, cx: &mut Context<Self>) {
        if !can_add_note_to_chat(note) {
            return;
        }
        cx.emit(NotesEvent::AddToChat(note_composer_card(note)));
    }

    /// `saveNoteImagesFromPaths`: images dropped as files.
    pub fn save_images_from_paths(
        &mut self,
        note_id: &str,
        paths: &[String],
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<NoteImageAsset>, String>> {
        let mut seen = HashSet::new();
        let images: Vec<ImageSource> = paths
            .iter()
            .filter(|path| !js::trim(path).is_empty() && seen.insert(path.as_str()))
            .filter(|path| is_image_name(path.rsplit(['/', '\\']).next().unwrap_or(path)))
            .map(|path| ImageSource::Path(path.clone()))
            .collect();
        self.save_image_sources(note_id, images, cx)
    }

    /// `saveNoteImagesFromFiles` for pasted image data.
    pub fn save_images_from_data(
        &mut self,
        note_id: &str,
        images: Vec<NoteImageData>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<NoteImageAsset>, String>> {
        let images = images.into_iter().map(ImageSource::Data).collect();
        self.save_image_sources(note_id, images, cx)
    }

    /// `saveNoteImageAttachments`: copy each image into the note. Data is
    /// written to a temp file first and removed after.
    fn save_image_sources(
        &mut self,
        note_id: &str,
        images: Vec<ImageSource>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<NoteImageAsset>, String>> {
        if images.is_empty() {
            return Task::ready(Err(NO_IMAGES_ERROR.into()));
        }
        let backend = self.backend.clone();
        let note_id = note_id.to_string();
        cx.spawn(async move |_, _| {
            let mut saved = Vec::new();
            let mut failure: Option<String> = None;
            for image in images {
                let (source, temporary) = match image {
                    ImageSource::Path(path) => (path, false),
                    ImageSource::Data(data) => {
                        (backend.write_temp(data.name, data.data).await?, true)
                    }
                };
                match backend.save_image(note_id.clone(), source.clone()).await {
                    Ok(asset) => saved.push(asset),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
                if temporary {
                    let _ = backend.delete_temp(source).await;
                }
            }
            if saved.is_empty() {
                return Err(failure.unwrap_or_else(|| NONE_SAVED_ERROR.into()));
            }
            Ok(saved)
        })
    }

    // The page.

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_creating(&self) -> bool {
        self.creating
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// `visible`: notes matching the filter by title, body, slug, tag, or
    /// project name.
    pub fn visible(&self) -> Vec<Note> {
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

    /// `selected`: the selected note, even when the filter hides it.
    pub fn selected(&self) -> Option<Note> {
        let id = self.selected_id.as_deref()?;
        self.visible()
            .into_iter()
            .find(|note| note.id == id)
            .or_else(|| self.notes.iter().find(|note| note.id == id).cloned())
    }

    /// Show the notes page and list the notes.
    pub fn open_page(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.refresh(cx).detach();
    }

    /// Leave the notes page. The open editor saves, as on unmount.
    pub fn close_page(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.release_editor(cx);
        cx.notify();
    }

    /// `NOTES_CHANGED_EVENT`: something outside the page wrote notes.
    pub fn notes_changed(&mut self, cx: &mut Context<Self>) {
        self.cache = None;
        if self.open {
            self.refresh(cx).detach();
        }
    }

    /// The page's `refresh`: list again and keep the selection when it still
    /// exists.
    pub fn refresh(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let load = self.load_notes(true, cx);
        cx.spawn(async move |this, cx| {
            let next = load.await;
            this.update(cx, |this, cx| {
                this.error = None;
                this.selected_id = this
                    .selected_id
                    .clone()
                    .filter(|preferred| next.iter().any(|note| note.id == *preferred))
                    .or_else(|| next.first().map(|note| note.id.clone()));
                this.notes = next;
                this.loading = false;
                this.sync_editor(cx);
                cx.notify();
            })
            .ok();
        })
    }

    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_string();
        self.sync_editor(cx);
        cx.notify();
    }

    pub fn select(&mut self, note_id: &str, cx: &mut Context<Self>) {
        self.selected_id = Some(note_id.to_string());
        self.sync_editor(cx);
        cx.notify();
    }

    /// `onCreate`: a new untitled note in the active project.
    pub fn create(&mut self, cwd: Option<&str>, cx: &mut Context<Self>) -> Task<()> {
        if self.creating {
            return Task::ready(());
        }
        self.creating = true;
        cx.notify();
        let create = self.create_note(
            &NewNote {
                title: Some("Untitled".into()),
                body: Some(String::new()),
                source_cwd: cwd
                    .filter(|cwd| looks_like_project(cwd))
                    .map(str::to_string),
                ..NewNote::default()
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let created = create.await;
            let created = match created {
                Ok(note) => {
                    let load = this.update(cx, |this, cx| this.load_notes(true, cx));
                    match load {
                        Ok(load) => Ok((note, load.await)),
                        Err(_) => return,
                    }
                }
                Err(error) => Err(error),
            };
            this.update(cx, |this, cx| {
                match created {
                    Ok((note, notes)) => {
                        this.notes = notes;
                        this.selected_id = Some(note.id);
                        this.query.clear();
                    }
                    Err(error) => this.error = Some(error),
                }
                this.creating = false;
                this.sync_editor(cx);
                cx.notify();
            })
            .ok();
        })
    }

    /// `onSaved`: put a saved note back in the list, newest first.
    fn on_saved(&mut self, note: &Note, cx: &mut Context<Self>) {
        for item in &mut self.notes {
            if item.id == note.id {
                *item = note.clone();
            }
        }
        self.notes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        self.sync_editor(cx);
        cx.notify();
    }

    /// `onDelete`: delete, list again, and select the first note when the
    /// deleted one was selected.
    fn delete_and_reload(&mut self, id: &str, cx: &mut Context<Self>) -> Task<()> {
        let delete = self.delete_note(id, cx);
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let result = match delete.await {
                Ok(()) => match this.update(cx, |this, cx| this.load_notes(true, cx)) {
                    Ok(load) => Ok(load.await),
                    Err(_) => return,
                },
                Err(error) => Err(error),
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(next) => {
                        if this.selected_id.as_deref() == Some(id.as_str()) {
                            this.selected_id = next.first().map(|note| note.id.clone());
                        }
                        this.notes = next;
                    }
                    Err(error) => this.error = Some(error),
                }
                this.sync_editor(cx);
                cx.notify();
            })
            .ok();
        })
    }

    /// `onAddToChat`: ask for a chat with the editor's draft, then leave.
    pub fn add_to_chat(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.editor_draft() else {
            return;
        };
        self.request_add_note_to_chat(&draft, cx);
        self.close_page(cx);
    }

    // The editor.

    /// The open editor.
    pub fn editor(&self) -> Option<NoteEditorView> {
        let editor = self.editors.get(&self.current_editor?)?;
        let title = editor
            .edits
            .title
            .clone()
            .unwrap_or_else(|| editor.note.title.clone());
        let body = editor
            .edits
            .body
            .clone()
            .unwrap_or_else(|| editor.note.body.clone());
        Some(NoteEditorView {
            note_id: editor.note.id.clone(),
            slug: editor.note.slug.clone(),
            updated_at: editor.note.updated_at,
            can_add_to_chat: !js::trim(&body).is_empty(),
            tags: editor
                .edits
                .tags
                .clone()
                .unwrap_or_else(|| editor.note.tags.clone()),
            source_cwd: editor
                .project_change
                .as_ref()
                .map(|change| change.path.clone())
                .or_else(|| editor.note.source_cwd.clone()),
            save_error: editor.save_error.clone(),
            image_busy: editor.image_busy,
            blank: editor.blank,
            title,
            body,
        })
    }

    /// `draft`: the note as the editor shows it.
    fn editor_draft(&self) -> Option<Note> {
        let view = self.editor()?;
        let editor = self.editors.get(&self.current_editor?)?;
        let title = js::trim(&view.title);
        Some(Note {
            title: if title.is_empty() {
                note_title(&view.body)
            } else {
                title.to_string()
            },
            body: view.body,
            tags: view.tags,
            source_cwd: view.source_cwd,
            ..editor.note.clone()
        })
    }

    /// Mount, update, or unmount the editor to follow the selection. A new
    /// note gets a fresh editor (`key={note.id}`); the old one saves.
    fn sync_editor(&mut self, cx: &mut Context<Self>) {
        let selected = if self.open { self.selected() } else { None };
        let current_note = self
            .current_editor
            .and_then(|id| self.editors.get(&id))
            .map(|editor| editor.note.id.clone());
        match (selected, current_note) {
            (Some(note), Some(current)) if note.id == current => {
                if let Some(editor) = self.current_editor.and_then(|id| self.editors.get_mut(&id)) {
                    editor.note = note;
                }
            }
            (selected, _) => {
                self.release_editor(cx);
                if let Some(note) = selected {
                    let id = self.next_editor;
                    self.next_editor += 1;
                    let blank = js::trim(&note.body).is_empty() && note.title == "Untitled";
                    self.editors.insert(
                        id,
                        EditorState {
                            note,
                            edits: NoteEdits::default(),
                            project_change: None,
                            save_error: None,
                            image_busy: false,
                            skip_save: false,
                            save_timer: None,
                            pending: 0,
                            blank,
                        },
                    );
                    self.current_editor = Some(id);
                }
            }
        }
    }

    /// Unmount the current editor: save now, and drop it once its saves end.
    fn release_editor(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_editor.take() else {
            return;
        };
        self.save_editor_now(id, cx);
        self.drop_idle_editor(id);
    }

    fn drop_idle_editor(&mut self, id: u64) {
        if self.current_editor != Some(id)
            && self
                .editors
                .get(&id)
                .is_some_and(|editor| editor.pending == 0)
        {
            self.editors.remove(&id);
        }
    }

    fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut NoteEdits)) -> Option<u64> {
        let id = self.current_editor?;
        let editor = self.editors.get_mut(&id)?;
        change(&mut editor.edits);
        cx.notify();
        Some(id)
    }

    /// Typing in the title field.
    pub fn edit_title(&mut self, title: &str, cx: &mut Context<Self>) {
        if let Some(id) = self.edit(cx, |edits| edits.title = Some(title.to_string())) {
            self.schedule_save(id, cx);
        }
    }

    /// The title field lost focus: fill a blank title from the body and save.
    pub fn commit_title(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.editor() else {
            return;
        };
        let trimmed = js::trim(&view.title);
        let next = if trimmed.is_empty() {
            note_title(&view.body)
        } else {
            trimmed.to_string()
        };
        if next != view.title {
            self.edit(cx, |edits| edits.title = Some(next));
        }
        if let Some(id) = self.current_editor {
            self.save_editor_now(id, cx);
        }
    }

    /// Typing in the body.
    pub fn edit_body(&mut self, body: &str, cx: &mut Context<Self>) {
        if let Some(id) = self.edit(cx, |edits| edits.body = Some(body.to_string())) {
            self.schedule_save(id, cx);
        }
    }

    /// Replace the tags.
    pub fn set_tags(&mut self, tags: Vec<String>, cx: &mut Context<Self>) {
        if let Some(id) = self.edit(cx, |edits| edits.tags = Some(tags)) {
            self.schedule_save(id, cx);
        }
    }

    /// `addTag`: normalize the input with the current tags.
    pub fn add_tag(&mut self, input: &str, cx: &mut Context<Self>) {
        let Some(view) = self.editor() else {
            return;
        };
        let mut all = view.tags.clone();
        all.push(input.to_string());
        let next = normalize_note_tags(&all);
        if next != view.tags {
            self.set_tags(next, cx);
        }
    }

    pub fn remove_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        let Some(view) = self.editor() else {
            return;
        };
        self.set_tags(
            view.tags.into_iter().filter(|item| item != tag).collect(),
            cx,
        );
    }

    /// Move the note to another project and save now.
    pub fn choose_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(id) = self.current_editor else {
            return;
        };
        let version = self.next_project_change;
        self.next_project_change += 1;
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.project_change = Some(ProjectChange {
                path: path.to_string(),
                version,
            });
        }
        self.save_editor_now(id, cx);
        cx.notify();
    }

    /// The Retry button of a failed save.
    pub fn retry_save(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_editor else {
            return;
        };
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.save_error = None;
        }
        self.save_editor_now(id, cx);
        cx.notify();
    }

    /// Delete the open note, after any save queued before it.
    pub fn delete_current(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_editor else {
            return;
        };
        let Some(editor) = self.editors.get_mut(&id) else {
            return;
        };
        editor.skip_save = true;
        editor.save_timer = None;
        let note_id = editor.note.id.clone();
        self.enqueue_save(&note_id, SaveJob::Delete, cx);
    }

    /// Images dropped or pasted at `start..end` of the body: save them, put
    /// their markdown in, and autosave. Resolves to the new cursor.
    pub fn insert_images(
        &mut self,
        load: Task<Result<Vec<NoteImageAsset>, String>>,
        start: usize,
        end: usize,
        cx: &mut Context<Self>,
    ) -> Task<Option<usize>> {
        let Some(id) = self.current_editor else {
            return Task::ready(None);
        };
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.image_busy = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            this.update(cx, |this, cx| {
                let editor = this.editors.get_mut(&id)?;
                editor.image_busy = false;
                cx.notify();
                let images = match loaded {
                    Ok(images) => images,
                    Err(error) => {
                        editor.save_error = Some(error);
                        return None;
                    }
                };
                let body = editor
                    .edits
                    .body
                    .clone()
                    .unwrap_or_else(|| editor.note.body.clone());
                let MarkdownInsertion { value, cursor } =
                    insert_note_images_markdown(&body, start, end, &images);
                editor.edits.body = Some(value);
                editor.save_error = None;
                this.schedule_save(id, cx);
                Some(cursor)
            })
            .ok()
            .flatten()
        })
    }

    fn schedule_save(&mut self, id: u64, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(NOTE_SAVE_DEBOUNCE);
        let task = cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| {
                if let Some(editor) = this.editors.get_mut(&id) {
                    editor.save_timer = None;
                }
                this.save_editor_now(id, cx);
            })
            .ok();
        });
        if let Some(editor) = self.editors.get_mut(&id) {
            editor.save_timer = Some(task);
        }
    }

    /// `saveNow`: cancel the debounce and queue a save.
    fn save_editor_now(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(editor) = self.editors.get_mut(&id) else {
            return;
        };
        editor.save_timer = None;
        let note_id = editor.note.id.clone();
        self.enqueue_save(&note_id, SaveJob::Persist(id), cx)
    }

    /// `enqueueNoteSave`.
    fn enqueue_save(&mut self, note_id: &str, job: SaveJob, cx: &mut Context<Self>) {
        let job_id = self.next_job;
        self.next_job += 1;
        let (previous, saved) = match self.save_queues.get(note_id) {
            Some(queue) => (queue.tail.clone(), queue.saved.clone()),
            None => (Task::ready(()).shared(), Rc::default()),
        };
        if let SaveJob::Persist(editor) = &job
            && let Some(editor) = self.editors.get_mut(editor)
        {
            editor.pending += 1;
        }
        let key = note_id.to_string();
        let note_id = note_id.to_string();
        let latest = saved.clone();
        let tail = cx
            .spawn(async move |this, cx| {
                previous.await;
                let before = latest.borrow().clone();
                let result = match &job {
                    SaveJob::Persist(editor) => persist(&this, *editor, before, cx).await,
                    SaveJob::Delete => {
                        if let Ok(delete) =
                            this.update(cx, |this, cx| this.delete_and_reload(&note_id, cx))
                        {
                            delete.await;
                        }
                        None
                    }
                };
                if let Some(note) = result {
                    *latest.borrow_mut() = Some(note);
                }
                this.update(cx, |this, _| {
                    // The last save of a note ends its queue, so the next
                    // save starts from the editor's note again. The entry
                    // stays, because this task is its tail.
                    if let Some(queue) = this.save_queues.get(&note_id)
                        && queue.job == job_id
                    {
                        queue.saved.borrow_mut().take();
                    }
                    if let SaveJob::Persist(editor) = job {
                        if let Some(state) = this.editors.get_mut(&editor) {
                            state.pending -= 1;
                        }
                        this.drop_idle_editor(editor);
                    }
                })
                .ok();
            })
            .shared();
        self.save_queues.insert(
            key,
            SaveQueue {
                tail,
                saved,
                job: job_id,
            },
        );
    }

    /// The plan of one save: what changed since `latest` (or the editor's
    /// note). `None` when Delete made the editor stop saving.
    fn save_plan(&mut self, editor: u64, latest: Option<Note>) -> Option<SavePlan> {
        let state = self.editors.get(&editor)?;
        if state.skip_save {
            return None;
        }
        let current = latest.unwrap_or_else(|| state.note.clone());
        let changes = state.edits.clone();
        let next_body = changes.body.clone().unwrap_or_else(|| current.body.clone());
        let title = changes
            .title
            .clone()
            .unwrap_or_else(|| current.title.clone());
        let trimmed = js::trim(&title);
        let next_title = if trimmed.is_empty() {
            note_title(&next_body)
        } else {
            trimmed.to_string()
        };
        let next_tags = changes.tags.clone().unwrap_or_else(|| current.tags.clone());
        let project = state.project_change.clone();
        let unchanged = next_title == current.title
            && next_body == current.body
            && next_tags == current.tags
            && project
                .as_ref()
                .is_none_or(|change| Some(&change.path) == current.source_cwd.as_ref());
        if unchanged {
            return Some(SavePlan::Unchanged {
                current,
                changes,
                project,
            });
        }
        Some(SavePlan::Save {
            upsert: NoteUpsert {
                id: current.id.clone(),
                title: next_title,
                body: next_body,
                tags: next_tags,
                source_session_id: None,
                source_cwd: project.as_ref().map(|change| change.path.clone()),
            },
            current,
            changes,
            project,
        })
    }

    /// `acceptSaved`: a finished save clears only the edits it carried.
    fn accept_saved(
        &mut self,
        editor: u64,
        saved: &Note,
        changes: &NoteEdits,
        project: &Option<ProjectChange>,
    ) {
        let Some(state) = self.editors.get_mut(&editor) else {
            return;
        };
        state.note = saved.clone();
        if state.edits.title == changes.title {
            state.edits.title = None;
        }
        if state.edits.body == changes.body {
            state.edits.body = None;
        }
        if state.edits.tags == changes.tags {
            state.edits.tags = None;
        }
        if state.project_change == *project {
            state.project_change = None;
        }
        state.save_error = None;
    }
}

enum ImageSource {
    Path(String),
    Data(NoteImageData),
}

/// `persist`: save the editor's edits on top of the latest saved note.
/// Resolves to the note the next queued save should build on.
async fn persist(
    this: &WeakEntity<Notes>,
    editor: u64,
    latest: Option<Note>,
    cx: &mut AsyncApp,
) -> Option<Note> {
    let plan = this
        .update(cx, |this, _| this.save_plan(editor, latest))
        .ok()
        .flatten()?;
    match plan {
        SavePlan::Unchanged {
            current,
            changes,
            project,
        } => {
            this.update(cx, |this, cx| {
                this.accept_saved(editor, &current, &changes, &project);
                cx.notify();
            })
            .ok();
            Some(current)
        }
        SavePlan::Save {
            upsert,
            current,
            changes,
            project,
        } => {
            let save = this
                .update(cx, |this, cx| this.upsert_note(upsert, cx))
                .ok()?;
            match save.await {
                Ok(saved) => {
                    this.update(cx, |this, cx| {
                        this.accept_saved(editor, &saved, &changes, &project);
                        this.on_saved(&saved, cx);
                    })
                    .ok();
                    Some(saved)
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        if let Some(state) = this.editors.get_mut(&editor) {
                            state.save_error = Some(error);
                        }
                        cx.notify();
                    })
                    .ok();
                    Some(current)
                }
            }
        }
    }
}

/// `onAddNoteToChat`: a new chat in the note's project (or the fallback
/// project) titled after the note, with the note chip, opened in a new tab.
/// `fallback_cwds` are the active session's, the session defaults', and the
/// sidebar project, in that order.
pub fn add_note_to_chat(
    card: &NoteComposerCard,
    fallback_cwds: &[Option<&str>],
    runtime_mode: Option<RuntimeMode>,
    host: &dyn HistoryHost,
    cx: &mut App,
) -> Option<String> {
    if card.id.is_empty() {
        return None;
    }
    let cwd = card
        .source_cwd
        .as_deref()
        .filter(|cwd| looks_like_project(cwd))
        .or_else(|| {
            fallback_cwds
                .iter()
                .flatten()
                .copied()
                .find(|cwd| !cwd.is_empty())
        })
        .unwrap_or("")
        .to_string();
    let mut session = host.new_default_session(&cwd, runtime_mode, cx);
    let title = js::trim(&card.title);
    if !title.is_empty() {
        session.title = title.to_string();
    }
    session.note_card = Some(card.clone());
    let id = session.id.clone();
    Engine::sessions(cx).update(cx, |sessions, cx| {
        sessions.insert(session, cx);
    });
    host.open_note_chat(&id, &cwd, cx);
    Some(id)
}

/// `onNoteCardDismiss`: drop a session's note chip.
pub fn dismiss_note_card(session_id: &str, cx: &mut App) {
    Engine::sessions(cx).update(cx, |sessions, cx| {
        let has_card = sessions
            .get(session_id)
            .is_some_and(|session| session.note_card.is_some());
        if has_card {
            sessions.update(session_id, cx, |session| session.note_card = None);
        }
    });
}

#[cfg(test)]
#[path = "notes_entity_tests.rs"]
mod tests;
