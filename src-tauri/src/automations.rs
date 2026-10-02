//! Tauri commands over `monocode_store::automations`.
use tauri::{AppHandle, State};

use crate::session_store::{SessionStore, TauriStoreEvents};
use monocode_store::automations::{
    self, Automation, AutomationEventClaim, AutomationRun, AutomationUpsert, DueAutomationRun,
};

pub(crate) const CHANGED: &str = "monocode:automations-changed";

#[tauri::command(async)]
pub fn automations_list(store: State<'_, SessionStore>) -> Result<Vec<Automation>, String> {
    automations::automations_list(&store)
}

#[tauri::command(async)]
pub fn automations_upsert(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation: AutomationUpsert,
) -> Result<Automation, String> {
    automations::automations_upsert(&TauriStoreEvents(app.clone()), &store, automation)
}

#[tauri::command(async)]
pub fn automations_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    automations::automations_delete(&TauriStoreEvents(app.clone()), &store, id)
}

#[tauri::command(async)]
pub fn automation_runs_list(
    store: State<'_, SessionStore>,
    automation_id: String,
) -> Result<Vec<AutomationRun>, String> {
    automations::automation_runs_list(&store, automation_id)
}

#[tauri::command(async)]
pub fn automation_runs_recover(
    app: AppHandle,
    store: State<'_, SessionStore>,
    started_before: i64,
    now: i64,
) -> Result<Vec<DueAutomationRun>, String> {
    automations::automation_runs_recover(
        &TauriStoreEvents(app.clone()),
        &store,
        started_before,
        now,
    )
}

#[tauri::command(async)]
pub fn automation_run_now(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    now: i64,
) -> Result<AutomationRun, String> {
    automations::automation_run_now(&TauriStoreEvents(app.clone()), &store, automation_id, now)
}

#[tauri::command(async)]
pub fn automations_claim_due(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    expected_next_run_at: i64,
    next_run_at: i64,
    now: i64,
) -> Result<Option<DueAutomationRun>, String> {
    automations::automations_claim_due(
        &TauriStoreEvents(app.clone()),
        &store,
        automation_id,
        expected_next_run_at,
        next_run_at,
        now,
    )
}

#[tauri::command(async)]
pub fn automations_claim_event(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    claim: AutomationEventClaim,
    now: i64,
) -> Result<Option<DueAutomationRun>, String> {
    automations::automations_claim_event(
        &TauriStoreEvents(app.clone()),
        &store,
        automation_id,
        claim,
        now,
    )
}

#[tauri::command(async)]
pub fn automation_run_update(
    app: AppHandle,
    store: State<'_, SessionStore>,
    run_id: String,
    status: String,
    session_id: Option<String>,
    error: Option<String>,
    now: i64,
) -> Result<AutomationRun, String> {
    automations::automation_run_update(
        &TauriStoreEvents(app.clone()),
        &store,
        run_id,
        status,
        session_id,
        error,
        now,
    )
}
