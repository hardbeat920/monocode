//! The note commands notes.ts and noteImages.ts called through `invoke`, as
//! a trait. `StoreNotesBackend` runs them over `monocode-store` and
//! `monocode-git` on the background executor; tests use a fake.
//!
//! The runtime's `SessionBackend` has no note commands (NEEDS.md), so the
//! app builds this from the same `SessionStore`.

use std::path::PathBuf;
use std::sync::Arc;

use futures::FutureExt;
use gpui::BackgroundExecutor;
use monocode_store::notes::{self, Note, NoteImageAsset, NoteUpsert};
use monocode_store::session_store::SessionStore;

use crate::runtime::StoreFuture;

/// The note commands of src-tauri, one method per `invoke` name.
pub trait NotesBackend: Send + Sync + 'static {
    /// `notes_list`: newest first.
    fn list(&self) -> StoreFuture<Vec<Note>>;
    /// `notes_get`.
    fn get(&self, id: String) -> StoreFuture<Option<Note>>;
    /// `notes_upsert`.
    fn upsert(&self, note: NoteUpsert) -> StoreFuture<Note>;
    /// `notes_delete`, which also removes the note's images.
    fn delete(&self, id: String) -> StoreFuture<()>;
    /// `notes_save_image`: copy an image into the note's asset folder.
    fn save_image(&self, note_id: String, source_path: String) -> StoreFuture<NoteImageAsset>;
    /// `write_attachment`: a temp file for pasted image data (base64).
    fn write_temp(&self, name: String, data: String) -> StoreFuture<String>;
    /// `delete_path`: remove a temp file.
    fn delete_temp(&self, path: String) -> StoreFuture<()>;
}

/// `NotesBackend` over `monocode.db` and the app data directory.
pub struct StoreNotesBackend {
    store: Arc<SessionStore>,
    data_dir: PathBuf,
    executor: BackgroundExecutor,
}

impl StoreNotesBackend {
    pub fn new(store: Arc<SessionStore>, data_dir: PathBuf, executor: BackgroundExecutor) -> Self {
        Self {
            store,
            data_dir,
            executor,
        }
    }

    fn run<T: Send + 'static>(
        &self,
        op: impl FnOnce(&SessionStore, &std::path::Path) -> Result<T, String> + Send + 'static,
    ) -> StoreFuture<T> {
        let store = self.store.clone();
        let data_dir = self.data_dir.clone();
        self.executor
            .spawn(async move { op(&store, &data_dir) })
            .boxed()
    }
}

impl NotesBackend for StoreNotesBackend {
    fn list(&self) -> StoreFuture<Vec<Note>> {
        self.run(|store, _| notes::notes_list(store))
    }

    fn get(&self, id: String) -> StoreFuture<Option<Note>> {
        self.run(move |store, _| notes::notes_get(store, id))
    }

    fn upsert(&self, note: NoteUpsert) -> StoreFuture<Note> {
        self.run(move |store, _| notes::notes_upsert(store, note))
    }

    fn delete(&self, id: String) -> StoreFuture<()> {
        self.run(move |store, data_dir| notes::notes_delete(data_dir, store, id))
    }

    fn save_image(&self, note_id: String, source_path: String) -> StoreFuture<NoteImageAsset> {
        self.run(move |_, data_dir| notes::notes_save_image(data_dir, note_id, source_path))
    }

    fn write_temp(&self, name: String, data: String) -> StoreFuture<String> {
        self.run(move |_, _| monocode_git::fs::write_attachment(name, data))
    }

    fn delete_temp(&self, path: String) -> StoreFuture<()> {
        self.run(move |_, _| monocode_git::fs::delete_path(path))
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory `NotesBackend` that records commands, like the
    //! TypeScript tests' `invoke` mock.

    use std::collections::{HashMap, VecDeque};

    use futures::channel::oneshot;
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct State {
        notes: Vec<Note>,
        commands: Vec<String>,
        upserts: Vec<NoteUpsert>,
        fail_next: VecDeque<(String, String)>,
        holds: HashMap<String, VecDeque<oneshot::Receiver<()>>>,
        clock: i64,
    }

    /// Releases one held command.
    pub struct Hold(Option<oneshot::Sender<()>>);

    impl Hold {
        pub fn release(mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[derive(Default)]
    pub struct FakeNotes {
        state: Arc<Mutex<State>>,
    }

    impl FakeNotes {
        pub fn with(notes: Vec<Note>) -> Arc<Self> {
            let fake = Self::default();
            fake.state.lock().notes = notes;
            fake.state.lock().clock = 100;
            Arc::new(fake)
        }

        pub fn notes(&self) -> Vec<Note> {
            self.state.lock().notes.clone()
        }

        pub fn note(&self, id: &str) -> Note {
            self.notes()
                .into_iter()
                .find(|note| note.id == id)
                .expect("note")
        }

        pub fn set_note(&self, note: Note) {
            let mut state = self.state.lock();
            state.notes.retain(|existing| existing.id != note.id);
            state.notes.insert(0, note);
        }

        pub fn commands(&self) -> Vec<String> {
            self.state.lock().commands.clone()
        }

        pub fn upserts(&self) -> Vec<NoteUpsert> {
            self.state.lock().upserts.clone()
        }

        /// The next call of `command` fails with `message`.
        pub fn fail_next(&self, command: &str, message: &str) {
            self.state
                .lock()
                .fail_next
                .push_back((command.to_string(), message.to_string()));
        }

        /// The next call of `command` waits until released.
        pub fn hold_next(&self, command: &str) -> Hold {
            let (sender, receiver) = oneshot::channel();
            self.state
                .lock()
                .holds
                .entry(command.to_string())
                .or_default()
                .push_back(receiver);
            Hold(Some(sender))
        }

        fn call<T: Send + 'static>(
            &self,
            command: &str,
            op: impl FnOnce(&mut State) -> Result<T, String> + Send + 'static,
        ) -> StoreFuture<T> {
            let (hold, failure) = {
                let mut state = self.state.lock();
                state.commands.push(command.to_string());
                let hold = state.holds.get_mut(command).and_then(VecDeque::pop_front);
                let failure = match state.fail_next.iter().position(|(c, _)| c == command) {
                    Some(index) => state.fail_next.remove(index).map(|(_, message)| message),
                    None => None,
                };
                (hold, failure)
            };
            let state = self.state.clone();
            async move {
                if let Some(hold) = hold {
                    let _ = hold.await;
                }
                if let Some(message) = failure {
                    return Err(message);
                }
                op(&mut state.lock())
            }
            .boxed()
        }
    }

    impl NotesBackend for FakeNotes {
        fn list(&self) -> StoreFuture<Vec<Note>> {
            self.call("notes_list", |state| {
                let mut notes = state.notes.clone();
                notes.sort_by(|a, b| {
                    b.updated_at
                        .cmp(&a.updated_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                Ok(notes)
            })
        }

        fn get(&self, id: String) -> StoreFuture<Option<Note>> {
            self.call("notes_get", move |state| {
                Ok(state.notes.iter().find(|note| note.id == id).cloned())
            })
        }

        fn upsert(&self, note: NoteUpsert) -> StoreFuture<Note> {
            self.call("notes_upsert", move |state| {
                state.upserts.push(note.clone());
                state.clock += 1;
                let now = state.clock;
                let existing = state.notes.iter().position(|saved| saved.id == note.id);
                let saved = match existing {
                    Some(index) => {
                        let previous = &state.notes[index];
                        Note {
                            id: note.id.clone(),
                            slug: previous.slug.clone(),
                            title: note.title.clone(),
                            body: note.body.clone(),
                            tags: note.tags.clone(),
                            source_session_id: note
                                .source_session_id
                                .clone()
                                .or_else(|| previous.source_session_id.clone()),
                            source_cwd: note
                                .source_cwd
                                .clone()
                                .or_else(|| previous.source_cwd.clone()),
                            created_at: previous.created_at,
                            updated_at: now,
                        }
                    }
                    None => Note {
                        id: note.id.clone(),
                        slug: note.title.to_lowercase().replace(' ', "-"),
                        title: note.title.clone(),
                        body: note.body.clone(),
                        tags: note.tags.clone(),
                        source_session_id: note.source_session_id.clone(),
                        source_cwd: note.source_cwd.clone(),
                        created_at: now,
                        updated_at: now,
                    },
                };
                match existing {
                    Some(index) => state.notes[index] = saved.clone(),
                    None => state.notes.push(saved.clone()),
                }
                Ok(saved)
            })
        }

        fn delete(&self, id: String) -> StoreFuture<()> {
            self.call("notes_delete", move |state| {
                state.notes.retain(|note| note.id != id);
                Ok(())
            })
        }

        fn save_image(&self, note_id: String, source_path: String) -> StoreFuture<NoteImageAsset> {
            self.call("notes_save_image", move |_| {
                let name = source_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&source_path)
                    .to_string();
                Ok(NoteImageAsset {
                    markdown_path: format!("/note-assets/{note_id}/1-{name}"),
                    name,
                })
            })
        }

        fn write_temp(&self, name: String, _data: String) -> StoreFuture<String> {
            self.call("write_attachment", move |_| {
                Ok(format!("/tmp/attachments/{name}"))
            })
        }

        fn delete_temp(&self, _path: String) -> StoreFuture<()> {
            self.call("delete_path", |_| Ok(()))
        }
    }
}
