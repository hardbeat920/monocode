//! Port of src/features/projects/model/projectData.ts: everything stored
//! for a project, removed when the project is deleted and moved when its
//! folder is renamed.

use gpui::{App, AppContext, Task};
use monocode_core::project_providers::{PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviders};
use monocode_layout::paths::project_key;
use monocode_settings::Kv;

use super::recents::normalize_project_path;
use super::{ProjectsGlobal, chat_background, project_groups, project_sidebar_tab};
use crate::runtime::engine::Engine;

/// Change the stored per-project provider overrides, saving only on change.
fn update_project_providers(kv: &Kv, update: impl FnOnce(&mut ProjectProviders)) {
    let mut providers =
        ProjectProviders::parse(kv.get_item(PROJECT_PROVIDER_SETTINGS_KEY).as_deref());
    let before = providers.clone();
    update(&mut providers);
    if providers != before {
        kv.set_item(PROJECT_PROVIDER_SETTINGS_KEY, &providers.to_json());
    }
}

/// `projectSessionCount`: saved chats filed under the project, for the
/// confirm prompt.
pub fn project_session_count(path: &str, cx: &App) -> Task<usize> {
    let Some(engine) = Engine::try_global(cx) else {
        return Task::ready(0);
    };
    let list = engine.writer.list_sessions_by_project(path);
    cx.background_spawn(async move { list.await.map(|sessions| sessions.len()).unwrap_or(0) })
}

/// `removeProjectData`: the project's saved chats, its logo and background
/// images, and every setting stored under its path.
pub fn remove_project_data(path: &str, cx: &mut App) -> Task<()> {
    let normalized = normalize_project_path(path);
    let key = project_key(&normalized);
    let writer = Engine::try_global(cx).map(|engine| engine.writer.clone());
    let Some(global) = ProjectsGlobal::try_global(cx) else {
        return Task::ready(());
    };
    let projects = global.projects.clone();
    let backend = global.backend.clone();
    cx.spawn(async move |cx| {
        if let Some(writer) = writer {
            let sessions = writer
                .list_sessions_by_project(&normalized)
                .await
                .unwrap_or_default();
            for session in sessions {
                let _ = writer.delete_session(&session.id, Vec::new()).await;
            }
        }
        // Drops the copied image from app data; the stored entry goes with it.
        let clear_logo = cx
            .update(|cx| projects.update(cx, |projects, cx| projects.clear_project_logo(&key, cx)));
        let _ = clear_logo.await;
        let executor = cx.background_executor().clone();
        let _ = chat_background::clear_project_chat_background(&backend, &key, &executor).await;
        cx.update(|cx| {
            projects.update(cx, |projects, cx| {
                projects.clear_chat_background_setting(&key, cx);
                let mut store = projects.store();
                projects
                    .appearance
                    .clear_tab_group_settings(&mut store, &key);
                projects.emit_appearance_events(cx);
                project_groups::remove_project_group_assignment(&projects.kv, &normalized);
                update_project_providers(&projects.kv, |providers| {
                    providers.clear_project_providers(&key)
                });
                project_sidebar_tab::clear_project_sidebar_tab(&projects.kv, &normalized);
                projects.paths_changed(cx);
            })
        });
    })
}

/// `rebaseProjectData`: move a project's stored settings after the folder
/// resolver found a rename.
pub fn rebase_project_data(from: &str, to: &str, cx: &mut App) {
    let old_key = project_key(&normalize_project_path(from));
    let new_key = project_key(&normalize_project_path(to));
    if let Some(global) = ProjectsGlobal::try_global(cx) {
        let projects = global.projects.clone();
        projects.update(cx, |projects, cx| {
            let mut store = projects.store();
            projects
                .appearance
                .rebase_project_tab_group_settings(&mut store, from, to);
            projects.emit_appearance_events(cx);
            project_groups::rebase_project_group_assignment(&projects.kv, from, to);
            projects
                .backgrounds
                .rebase(&projects.kv, &old_key, &new_key);
            projects.emit_background_events(cx);
            update_project_providers(&projects.kv, |providers| {
                providers.rebase_project_providers(&old_key, &new_key)
            });
            project_sidebar_tab::rebase_project_sidebar_tab(&projects.kv, from, to);
            projects.paths_changed(cx);
        });
    }
    ProjectsGlobal::hooks(cx).rebase_session_folder_settings(from, to, cx);
}
