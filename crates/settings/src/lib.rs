//! Settings storage for the native app.
//!
//! `Kv` replaces the webview's localStorage. It keeps the same `monocode.*`
//! keys and the same stored strings in one JSON file in the app data
//! directory, so each ported module keeps its own load and save code.
//! `import_webkit_local_storage` copies the Tauri app's localStorage into it
//! once. `settings_store`, `app_shortcuts`, `zoom_keybinding`, and
//! `storage_flags` port the storage side of the settings model.

pub mod app_shortcuts;
pub mod import;
pub mod kv;
pub mod settings_store;
pub mod storage_flags;
mod temp;
pub mod zoom_keybinding;

pub use import::{
    APP_IDENTIFIER, ImportCandidate, ImportRecord, ImportReport, ImportStatus, SkippedItem,
    WebviewData, import_record, import_webkit_local_storage,
};
pub use kv::{KV_FILE_NAME, Kv, KvChange, Subscription};
pub use settings_store::{APP_SETTINGS_KEYS, load_app_settings};
