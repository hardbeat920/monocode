//! Tauri commands over `monocode_store::notes`.
use tauri::{AppHandle, State};

use crate::session_store::SessionStore;
use monocode_store::notes::{self, Note, NoteImageAsset, NoteUpsert};

#[tauri::command(async)]
pub fn notes_list(store: State<'_, SessionStore>) -> Result<Vec<Note>, String> {
    notes::notes_list(&store)
}

#[tauri::command(async)]
pub fn notes_get(store: State<'_, SessionStore>, id: String) -> Result<Option<Note>, String> {
    notes::notes_get(&store, id)
}

#[tauri::command(async)]
pub fn notes_upsert(store: State<'_, SessionStore>, note: NoteUpsert) -> Result<Note, String> {
    notes::notes_upsert(&store, note)
}

#[tauri::command(async)]
pub fn notes_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    notes::notes_delete(&data_dir, &store, id)
}

#[tauri::command]
pub async fn notes_save_image(
    app: AppHandle,
    note_id: String,
    source_path: String,
) -> Result<NoteImageAsset, String> {
    let data_dir = crate::app_data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        notes::notes_save_image(&data_dir, note_id, source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn notes_image_path(app: AppHandle, asset: String) -> Result<String, String> {
    let data_dir = crate::app_data_dir(&app)?;
    notes::notes_image_path(&data_dir, asset)
}
