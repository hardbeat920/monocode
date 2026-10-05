//! The notes page (NotesView.tsx) and the note chip (NoteMiniCard.tsx).

pub mod card;
pub mod data;
pub mod local;
pub mod mini_card;
pub mod model;
pub mod source;
pub mod view;

#[cfg(test)]
mod tests;

pub use data::{Note, NoteEditorView, NotesData, NotesPage};
pub use local::LocalNotes;
pub use mini_card::{NoteMiniCard, note_mini_card};
pub use view::NotesView;

/// `NOTE_IMAGE_PREFIX`: markdown paths of images saved into a note.
pub const NOTE_IMAGE_PREFIX: &str = "/note-assets/";
