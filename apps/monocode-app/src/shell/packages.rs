//! The engine packages the shell reads: history (the sidebar list) and
//! projects (the rail and git status). Boot should initialize both; until
//! it does, [`ensure`] does it the same way when the first window opens.
//! It does nothing once the globals exist.

use std::sync::Arc;

use gpui::App;
use monocode_app::boot::AppServices;
use monocode_engine::history::notes_backend::StoreNotesBackend;
use monocode_engine::history::{HistoryConfig, HistoryPackage};
use monocode_engine::projects::ProjectsGlobal;
use monocode_store::session_store::open_in_data_dir;

/// Initialize history and projects when boot has not.
pub fn ensure(cx: &mut App) {
    let Some(services) = AppServices::try_global(cx) else {
        return;
    };
    let kv = services.kv.clone();
    let data_dir = services.data_dir.path.clone();
    let host = services.host.clone();
    let have_history = HistoryPackage::try_global(cx).is_some();
    let have_projects = ProjectsGlobal::try_global(cx).is_some();
    if have_history && have_projects {
        return;
    }
    // The engine keeps its store private, so this opens a second connection
    // to the same monocode.db for notes and the worktree session lookup.
    let store = match open_in_data_dir(&data_dir) {
        Ok(store) => Arc::new(store),
        Err(error) => {
            log::error!("[monocode] shell: opening the session store: {error}");
            return;
        }
    };
    if !have_history {
        let notes = Arc::new(StoreNotesBackend::new(
            store.clone(),
            data_dir.clone(),
            cx.background_executor().clone(),
        ));
        HistoryPackage::init(HistoryConfig::new(kv.clone(), notes, cx), cx);
    }
    if !have_projects {
        let in_use = Arc::new(move |path: &std::path::Path| host.has_working_dir(path));
        ProjectsGlobal::init_native(kv, data_dir, store, Arc::new(|_| false), in_use, cx);
    }
}
