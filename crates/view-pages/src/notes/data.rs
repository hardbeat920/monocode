//! What the notes page reads and changes.
//!
//! The engine's history package owns the page model in its `Notes` entity:
//! the list, filter, selection, and the editor's save model (pending edits,
//! the 400 ms autosave, the per-note save queue, project moves, and retry).
//! [`NotesData`] mirrors that entity's API, so the engine implements it by
//! forwarding. The view keeps only what React kept in the DOM: the text
//! fields, the preview or source choice, the list width, and the drag
//! state.

use gpui::{App, Subscription, Task};

pub use monocode_store::notes::Note;

use crate::data::Listener;

/// `NoteEditorView` in the engine: what the open note's editor shows.
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

/// One read of the page.
#[derive(Debug, Clone, Default)]
pub struct NotesPage {
    /// Every note, newest first.
    pub notes: Vec<Note>,
    /// The notes the filter shows.
    pub visible: Vec<Note>,
    pub selected_id: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub query: String,
    pub creating: bool,
    /// The open note's editor.
    pub editor: Option<NoteEditorView>,
}

impl NotesPage {
    /// `selected`: the selected note, even when the filter hides it.
    pub fn selected(&self) -> Option<&Note> {
        let id = self.selected_id.as_deref()?;
        self.visible
            .iter()
            .find(|note| note.id == id)
            .or_else(|| self.notes.iter().find(|note| note.id == id))
    }
}

/// The notes page's model. See the module docs.
pub trait NotesData: 'static {
    fn page(&self, cx: &App) -> NotesPage;
    /// Runs after every change to the page.
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;

    /// Show the page and list the notes.
    fn open_page(&self, cx: &mut App);
    /// Leave the page. The open editor saves.
    fn close_page(&self, cx: &mut App);
    fn set_query(&self, query: &str, cx: &mut App);
    fn select(&self, note_id: &str, cx: &mut App);
    /// `onCreate`: a new untitled note in `cwd` when it is a project.
    fn create(&self, cwd: Option<&str>, cx: &mut App);

    /// Typing in the title field.
    fn edit_title(&self, title: &str, cx: &mut App);
    /// The title field lost focus: fill a blank title from the body and save.
    fn commit_title(&self, cx: &mut App);
    /// Typing in the body.
    fn edit_body(&self, body: &str, cx: &mut App);
    /// `addTag`: normalize the input with the current tags.
    fn add_tag(&self, input: &str, cx: &mut App);
    fn remove_tag(&self, tag: &str, cx: &mut App);
    /// Move the note to another project and save now.
    fn choose_project(&self, path: &str, cx: &mut App);
    /// The Retry button of a failed save.
    fn retry_save(&self, cx: &mut App);
    /// Delete the open note, after any save queued before it.
    fn delete_current(&self, cx: &mut App);
    /// `onAddToChat`: ask for a chat with the editor's note.
    fn add_to_chat(&self, cx: &mut App);
    /// Images dropped at `start..end` (byte offsets) of the body: save them
    /// to the note, insert their markdown, and autosave. Resolves to the new
    /// cursor offset.
    fn insert_image_paths(
        &self,
        paths: Vec<String>,
        start: usize,
        end: usize,
        cx: &mut App,
    ) -> Task<Option<usize>>;
    /// The image an URL in the preview shows. Note assets
    /// (`/note-assets/...`) live in the data directory, which only the host
    /// knows; the default loads web, file, and data URLs.
    fn image_source(&self, url: &str) -> Option<gpui::ImageSource> {
        super::view::default_note_image(url)
    }
}
