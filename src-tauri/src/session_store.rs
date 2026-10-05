//! Tauri commands over `monocode_store::session_store`.
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

pub(crate) use monocode_store::session_store::SessionStore;
use monocode_store::session_store::{
    self, InFlightSession, SessionRecord, SessionSearchOptions, SessionSearchResult,
    SessionSummary, SessionUpsert,
};
use monocode_store::StoreEvents;

/// Emits the change events every window listens to.
pub(crate) struct TauriStoreEvents(pub AppHandle);

impl StoreEvents for TauriStoreEvents {
    fn reminders_changed(&self) {
        let _ = self.0.emit(crate::reminders::CHANGED, ());
    }

    fn automations_changed(&self) {
        let _ = self.0.emit(crate::automations::CHANGED, ());
    }
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    session_store::init_with(
        || crate::app_data_dir(app),
        SessionStore::open,
        |store| {
            app.manage(store);
        },
    )
}

#[tauri::command(async)]
pub fn session_upsert(
    store: State<'_, SessionStore>,
    session: SessionUpsert,
) -> Result<SessionSummary, String> {
    session_store::session_upsert(&store, session)
}

#[tauri::command(async)]
pub fn session_list_by_project(
    store: State<'_, SessionStore>,
    cwd: String,
) -> Result<Vec<SessionSummary>, String> {
    session_store::session_list_by_project(&store, cwd)
}

#[tauri::command(async)]
pub fn session_rebase_project(
    store: State<'_, SessionStore>,
    from_cwd: String,
    to_cwd: String,
) -> Result<(), String> {
    session_store::session_rebase_project(&store, from_cwd, to_cwd)
}

#[tauri::command(async)]
pub fn session_list_linked(store: State<'_, SessionStore>) -> Result<Vec<SessionSummary>, String> {
    session_store::session_list_linked(&store)
}

#[tauri::command(async)]
pub fn session_get(
    store: State<'_, SessionStore>,
    session_id: String,
) -> Result<Option<SessionRecord>, String> {
    session_store::session_get(&store, session_id)
}

#[tauri::command(async)]
pub fn session_search(
    store: State<'_, SessionStore>,
    options: SessionSearchOptions,
) -> Result<SessionSearchResult, String> {
    session_store::session_search(&store, options)
}

#[tauri::command(async)]
pub fn cancel_session_search(search_owner: String) {
    session_store::cancel_session_search(search_owner)
}

#[tauri::command(async)]
pub fn session_delete(
    store: State<'_, SessionStore>,
    app: AppHandle,
    session_id: String,
    image_paths: Vec<String>,
) -> Result<(), String> {
    let data_dir = crate::app_data_dir(&app)?;
    session_store::session_delete(
        &store,
        &data_dir,
        &TauriStoreEvents(app.clone()),
        session_id,
        image_paths,
    )
}

#[tauri::command(async)]
pub fn session_set_archived(
    store: State<'_, SessionStore>,
    session_id: String,
    archived: bool,
) -> Result<(), String> {
    session_store::session_set_archived(&store, session_id, archived)
}

#[tauri::command(async)]
pub fn session_set_pinned(
    store: State<'_, SessionStore>,
    session_id: String,
    pinned: bool,
) -> Result<(), String> {
    session_store::session_set_pinned(&store, session_id, pinned)
}

#[tauri::command(async)]
pub fn session_set_linked_work_item(
    store: State<'_, SessionStore>,
    session_id: String,
    linked_work_item: Option<Value>,
) -> Result<(), String> {
    session_store::session_set_linked_work_item(&store, session_id, linked_work_item)
}

#[tauri::command(async)]
pub fn session_set_in_flight(
    store: State<'_, SessionStore>,
    sessions: Vec<InFlightSession>,
) -> Result<(), String> {
    session_store::session_set_in_flight(&store, sessions)
}

/// Read the quit snapshot without clearing it. Vite/dev reloads must not
/// consume the only copy.
#[tauri::command(async)]
pub fn session_list_in_flight(
    store: State<'_, SessionStore>,
) -> Result<Vec<InFlightSession>, String> {
    session_store::session_list_in_flight(&store)
}

/// Read and clear the quit snapshot so a restored window cannot take it twice.
#[tauri::command(async)]
pub fn session_take_in_flight(
    store: State<'_, SessionStore>,
) -> Result<Vec<InFlightSession>, String> {
    session_store::session_take_in_flight(&store)
}

#[tauri::command(async)]
pub fn workspace_set_snapshot(
    store: State<'_, SessionStore>,
    snapshot: Value,
) -> Result<(), String> {
    session_store::workspace_set_snapshot(&store, snapshot)
}

#[tauri::command(async)]
pub fn workspace_get_snapshot(store: State<'_, SessionStore>) -> Result<Option<Value>, String> {
    session_store::workspace_get_snapshot(&store)
}
