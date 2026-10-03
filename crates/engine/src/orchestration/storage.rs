//! The orchestrator's storage over the real store and control server: the
//! `storage` object in src/features/orchestration/model/orchestration.ts,
//! which called the `control_*` Tauri commands.

use std::sync::Arc;

use gpui::{App, AppContext as _, Task};
use monocode_process::control::{
    ControlHost, control_disable, control_enable, control_scopes, control_write_path,
};
use monocode_store::session_store::{SessionStore, control_load, control_save};

use super::host::OrchestrationStorage;
use super::state::{OrchestrationRun, parse_saved_run};

/// `control_save` and `control_load` in `monocode.db`, plus the control
/// server's grants for `owner`.
pub struct NativeStorage {
    pub store: Arc<SessionStore>,
    pub control: Arc<ControlHost>,
    /// The owner id control grants are issued to.
    pub owner: String,
}

impl OrchestrationStorage for NativeStorage {
    fn save(&self, run: &OrchestrationRun, cx: &App) -> Task<Result<(), String>> {
        let store = self.store.clone();
        let lead_id = run.lead_id.clone();
        let state = serde_json::to_string(run).map_err(|error| error.to_string());
        cx.background_spawn(async move { control_save(&store, lead_id, state?) })
    }

    fn load(&self, id: &str, cx: &App) -> Task<Result<Option<OrchestrationRun>, String>> {
        let store = self.store.clone();
        let id = id.to_string();
        cx.background_spawn(async move {
            match control_load(&store, id.clone())? {
                Some(raw) => parse_saved_run(&raw, &id).map(Some),
                None => Ok(None),
            }
        })
    }

    fn enable(&self, id: &str, cwd: &str, cx: &App) -> Task<Result<String, String>> {
        let control = self.control.clone();
        let (owner, id, cwd) = (self.owner.clone(), id.to_string(), cwd.to_string());
        cx.background_spawn(async move { control_enable(&control, &owner, id, cwd) })
    }

    fn disable(&self, id: &str, cx: &App) -> Task<Result<(), String>> {
        let control = self.control.clone();
        let (owner, id) = (self.owner.clone(), id.to_string());
        cx.background_spawn(async move { control_disable(&control, &owner, id) })
    }

    fn scopes(&self, cwd: &str, files: &[String], cx: &App) -> Task<Result<Vec<String>, String>> {
        let (cwd, files) = (cwd.to_string(), files.to_vec());
        cx.background_spawn(async move { control_scopes(cwd, files) })
    }

    fn resolve_path(&self, path: &str, cx: &App) -> Task<Result<String, String>> {
        let path = path.to_string();
        cx.background_spawn(async move { control_write_path(path) })
    }
}
