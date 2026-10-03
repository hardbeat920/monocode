//! Port of src/features/projects/model/projectLocation.ts and
//! projectLocationError.ts: remember each project folder's filesystem
//! identity, and use it to follow the folder when it is renamed.
//!
//! The resolve call blocks, so the async functions here run it on the
//! background executor.

use std::sync::Arc;

use gpui::{BackgroundExecutor, Task};
use monocode_core::paths::{display_path, path_key};
use monocode_layout::tab_groups::JsRecord;
use monocode_settings::Kv;
use serde::Serialize;
use serde_json::Value;

use super::backend::{ProjectLocation, ProjectsBackend};
use super::js_object::{parse_object, stringify};
use super::recents::{is_local_project, normalize_project_path, same_project_path};

pub const KEY: &str = "monocode.projectLocations";

/// `StoredProjectLocation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct StoredProjectLocation {
    path: String,
    identity: String,
}

/// `ProjectLocationSync`: where the project is now and whether it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLocationSync {
    pub path: String,
    pub identity: String,
    pub moved: bool,
}

/// `read`.
fn read(kv: &Kv) -> JsRecord<StoredProjectLocation> {
    let raw = kv.get_item(KEY).unwrap_or_else(|| "{}".into());
    let Some(parsed) = parse_object(&raw) else {
        return JsRecord::new();
    };
    let mut locations = JsRecord::new();
    for (key, value) in parsed.iter() {
        let Some(candidate) = value.as_object() else {
            continue;
        };
        let path = candidate.get("path").and_then(Value::as_str);
        let identity = candidate.get("identity").and_then(Value::as_str);
        let (Some(path), Some(identity)) = (
            path.filter(|path| !path.is_empty()),
            identity.filter(|identity| !identity.is_empty()),
        ) else {
            continue;
        };
        locations.insert(
            key,
            StoredProjectLocation {
                path: normalize_project_path(path),
                identity: identity.to_string(),
            },
        );
    }
    locations
}

/// `write`.
fn write(kv: &Kv, locations: &JsRecord<StoredProjectLocation>) {
    kv.set_item(KEY, &stringify(locations));
}

/// `remember`.
fn remember(kv: &Kv, locations: &mut JsRecord<StoredProjectLocation>, location: &ProjectLocation) {
    let path = normalize_project_path(&location.path);
    locations.insert(
        path_key(&path),
        StoredProjectLocation {
            path,
            identity: location.identity.clone(),
        },
    );
    write(kv, locations);
}

/// The identity saved for a project, if any.
pub fn stored_identity(kv: &Kv, path: &str) -> Option<String> {
    read(kv)
        .get(&path_key(&normalize_project_path(path)))
        .map(|location| location.identity.clone())
}

/// `rememberProjectLocation`: capture an identity without searching for a
/// moved folder.
pub fn remember_project_location(
    kv: &Kv,
    backend: &Arc<dyn ProjectsBackend>,
    path: &str,
    executor: &BackgroundExecutor,
) -> Task<Result<(), String>> {
    let normalized = normalize_project_path(path);
    if !is_local_project(&normalized) {
        return Task::ready(Ok(()));
    }
    let (kv, backend) = (kv.clone(), backend.clone());
    executor.spawn(async move {
        let Some(location) = backend.resolve_project_location(&normalized, None)? else {
            return Ok(());
        };
        remember(&kv, &mut read(&kv), &location);
        Ok(())
    })
}

/// `synchronizeProjectLocation`: resolve a missing project among its former
/// siblings and save the result. `Ok(None)` when the folder is gone.
pub fn synchronize_project_location(
    kv: &Kv,
    backend: &Arc<dyn ProjectsBackend>,
    path: &str,
    executor: &BackgroundExecutor,
) -> Task<Result<Option<ProjectLocationSync>, String>> {
    let normalized = normalize_project_path(path);
    if !is_local_project(&normalized) {
        return Task::ready(Ok(Some(ProjectLocationSync {
            path: normalized,
            identity: String::new(),
            moved: false,
        })));
    }
    let (kv, backend) = (kv.clone(), backend.clone());
    executor.spawn(async move {
        let mut locations = read(&kv);
        let old_key = path_key(&normalized);
        let identity = locations
            .get(&old_key)
            .map(|stored| stored.identity.clone());
        let Some(location) = backend.resolve_project_location(&normalized, identity.as_deref())?
        else {
            return Ok(None);
        };
        let resolved = normalize_project_path(&location.path);
        let moved = !same_project_path(&normalized, &resolved);
        if moved {
            locations.remove(&old_key);
        }
        let location = ProjectLocation {
            path: resolved.clone(),
            identity: location.identity,
        };
        remember(&kv, &mut locations, &location);
        Ok(Some(ProjectLocationSync {
            path: resolved,
            identity: location.identity,
            moved,
        }))
    })
}

/// `forgetProjectLocation`.
pub fn forget_project_location(kv: &Kv, path: &str) {
    let mut locations = read(kv);
    let key = path_key(&normalize_project_path(path));
    if locations.remove(&key).is_none() {
        return;
    }
    write(kv, &locations);
}

/// `ProjectNotFoundError`: the user must reconnect the project before they
/// retry the submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNotFoundError {
    pub cwd: String,
}

impl ProjectNotFoundError {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self { cwd: cwd.into() }
    }
}

impl std::fmt::Display for ProjectNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Project folder not found: {}. Reopen the folder to reconnect it.",
            display_path(&self.cwd, None)
        )
    }
}

impl std::error::Error for ProjectNotFoundError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::FakeBackend;
    use gpui::TestAppContext;

    fn location(path: &str, identity: &str) -> Option<ProjectLocation> {
        Some(ProjectLocation {
            path: path.into(),
            identity: identity.into(),
        })
    }

    #[gpui::test]
    async fn records_an_identity_and_uses_it_to_follow_a_rename(cx: &mut TestAppContext) {
        let kv = Kv::in_memory();
        let fake = FakeBackend::new();
        fake.push_location(location("/work/monocode", "unix:1:2"));
        fake.push_location(location("/work/monocode-personal", "unix:1:2"));
        let backend: Arc<dyn ProjectsBackend> = fake.clone();
        let executor = cx.executor();

        remember_project_location(&kv, &backend, "/work/monocode", &executor)
            .await
            .unwrap();
        let synced = synchronize_project_location(&kv, &backend, "/work/monocode", &executor)
            .await
            .unwrap();
        assert_eq!(
            synced,
            Some(ProjectLocationSync {
                path: "/work/monocode-personal".into(),
                identity: "unix:1:2".into(),
                moved: true,
            })
        );
        assert_eq!(
            fake.location_calls().last(),
            Some(&("/work/monocode".to_string(), Some("unix:1:2".to_string())))
        );
        assert_eq!(stored_identity(&kv, "/work/monocode"), None);
        assert_eq!(
            stored_identity(&kv, "/work/monocode-personal").as_deref(),
            Some("unix:1:2")
        );
    }

    #[gpui::test]
    async fn cannot_guess_a_rename_before_an_identity_has_been_recorded(cx: &mut TestAppContext) {
        let kv = Kv::in_memory();
        let fake = FakeBackend::new();
        fake.push_location(None);
        let backend: Arc<dyn ProjectsBackend> = fake.clone();
        let synced = synchronize_project_location(&kv, &backend, "/work/missing", &cx.executor())
            .await
            .unwrap();
        assert_eq!(synced, None);
        assert_eq!(fake.location_calls(), [("/work/missing".to_string(), None)]);
    }

    #[gpui::test]
    async fn forgets_the_saved_identity_when_a_project_is_removed(cx: &mut TestAppContext) {
        let kv = Kv::in_memory();
        let fake = FakeBackend::new();
        fake.push_location(location("/work/repo", "unix:1:2"));
        fake.push_location(None);
        let backend: Arc<dyn ProjectsBackend> = fake.clone();
        let executor = cx.executor();
        remember_project_location(&kv, &backend, "/work/repo", &executor)
            .await
            .unwrap();
        forget_project_location(&kv, "/work/repo");
        synchronize_project_location(&kv, &backend, "/work/repo", &executor)
            .await
            .unwrap();
        assert_eq!(
            fake.location_calls().last(),
            Some(&("/work/repo".to_string(), None))
        );
    }

    #[gpui::test]
    async fn remote_projects_never_touch_the_disk(cx: &mut TestAppContext) {
        let kv = Kv::in_memory();
        let fake = FakeBackend::new();
        let backend: Arc<dyn ProjectsBackend> = fake.clone();
        let synced = synchronize_project_location(
            &kv,
            &backend,
            "remote://env/home/me/app/",
            &cx.executor(),
        )
        .await
        .unwrap();
        assert_eq!(
            synced,
            Some(ProjectLocationSync {
                path: "remote://env/home/me/app".into(),
                identity: String::new(),
                moved: false,
            })
        );
        assert!(fake.location_calls().is_empty());
    }

    #[test]
    fn not_found_error_names_the_folder() {
        assert_eq!(
            ProjectNotFoundError::new("/work/gone/").to_string(),
            "Project folder not found: /work/gone. Reopen the folder to reconnect it."
        );
    }
}
