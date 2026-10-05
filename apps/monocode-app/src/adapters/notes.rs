//! The notes page reads the history entity and preserves its autosave queue.
use gpui::{App, Entity, ImageSource, Subscription, Task};
use monocode_engine::history::Notes;
use monocode_view_pages::Listener;
use monocode_view_pages::notes::{NoteEditorView, NotesData, NotesPage};

pub struct AppNotesData {
    pub notes: Entity<Notes>,
    pub data_dir: std::path::PathBuf,
}
impl NotesData for AppNotesData {
    fn page(&self, cx: &App) -> NotesPage {
        let notes = self.notes.read(cx);
        NotesPage {
            notes: notes.notes().to_vec(),
            visible: notes.visible(),
            selected_id: notes.selected_id().map(str::to_owned),
            loading: notes.is_loading(),
            error: notes.error().map(str::to_owned),
            query: notes.query().to_owned(),
            creating: notes.is_creating(),
            editor: notes.editor().map(|editor| NoteEditorView {
                note_id: editor.note_id,
                slug: editor.slug,
                updated_at: editor.updated_at,
                title: editor.title,
                body: editor.body,
                tags: editor.tags,
                source_cwd: editor.source_cwd,
                save_error: editor.save_error,
                image_busy: editor.image_busy,
                blank: editor.blank,
                can_add_to_chat: editor.can_add_to_chat,
            }),
        }
    }
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.notes, move |_, cx| listener(cx))
    }
    fn open_page(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.open_page(cx));
    }
    fn close_page(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.close_page(cx));
    }
    fn commit_title(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.commit_title(cx));
    }
    fn retry_save(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.retry_save(cx));
    }
    fn delete_current(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.delete_current(cx));
    }
    fn add_to_chat(&self, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.add_to_chat(cx));
    }
    fn set_query(&self, value: &str, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.set_query(value, cx));
    }
    fn select(&self, value: &str, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.select(value, cx));
    }
    fn edit_title(&self, value: &str, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.edit_title(value, cx));
    }
    fn edit_body(&self, value: &str, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.edit_body(value, cx));
    }
    fn add_tag(&self, value: &str, cx: &mut App) {
        self.notes.update(cx, |notes, cx| notes.add_tag(value, cx));
    }
    fn remove_tag(&self, value: &str, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.remove_tag(value, cx));
    }
    fn choose_project(&self, value: &str, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.choose_project(value, cx));
    }
    fn create(&self, cwd: Option<&str>, cx: &mut App) {
        self.notes
            .update(cx, |notes, cx| notes.create(cwd, cx))
            .detach();
    }
    fn insert_image_paths(
        &self,
        paths: Vec<String>,
        start: usize,
        end: usize,
        cx: &mut App,
    ) -> Task<Option<usize>> {
        self.notes.update(cx, |notes, cx| {
            let Some(editor) = notes.editor() else {
                return Task::ready(None);
            };
            let load = notes.save_images_from_paths(&editor.note_id, &paths, cx);
            notes.insert_images(load, start, end, cx)
        })
    }
    fn image_source(&self, url: &str) -> Option<ImageSource> {
        if url.starts_with("/note-assets/") {
            let path =
                monocode_store::notes::notes_image_path(&self.data_dir, url.to_owned()).ok()?;
            Some(std::path::PathBuf::from(path).into())
        } else {
            monocode_view_pages::notes::view::default_note_image(url)
        }
    }
}
