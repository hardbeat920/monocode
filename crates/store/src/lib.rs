//! SQLite storage shared by the Tauri app and the GPUI app: sessions,
//! checkpoints, notes, reminders, automations, and the Cursor session reader.
//! Moved from `src-tauri/src`.

pub mod automations;
pub mod checkpoint;
pub mod cursor_store;
pub mod notes;
pub mod reminders;
pub mod session_store;

/// Change notices for data that several windows show. The Tauri app emits
/// `monocode:reminders-changed` and `monocode:automations-changed`.
pub trait StoreEvents: Send + Sync {
    fn reminders_changed(&self);
    fn automations_changed(&self);
}
