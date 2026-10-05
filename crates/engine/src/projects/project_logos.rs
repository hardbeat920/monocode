//! Port of src/features/projects/model/projectLogos.ts: a project's logo
//! image on the rail.
//!
//! The file picker (`pickImageFile`) belongs to the view, which passes the
//! picked path to `Projects::set_project_logo`. `projectLogoSrc` becomes
//! the logo path plus `Projects::logo_display_revision` as a cache key.

use gpui::{AppContext, Context, Task};
use monocode_layout::paths::project_key;
use monocode_layout::tab_groups::JsRecord;

use super::Projects;

/// The image types the logo picker offers.
pub const LOGO_IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
/// The logo picker's title.
pub const LOGO_PICKER_TITLE: &str = "Choose project logo";

/// `droppableLogoFile`: the file a project is about to stop showing, or
/// `None` when it must stay.
///
/// Logos are filed on disk under a stem derived from the project key, so a
/// logo saved before the keys became paths sits under a stem nothing
/// derives anymore, and the stored path is the only handle left to it.
/// Migrated projects that shared a folder name also share that one file, so
/// it only goes when the last project pointing at it lets go.
pub fn droppable_logo_file(
    logos: &JsRecord<String>,
    project: &str,
    keep: Option<&str>,
) -> Option<String> {
    let previous = logos.get(project).filter(|previous| !previous.is_empty())?;
    if Some(previous.as_str()) == keep {
        return None;
    }
    let shared = logos
        .iter()
        .any(|(key, path)| key != project && path == previous);
    (!shared).then(|| previous.clone())
}

/// `projectLogoSrc`: the logo file and the revision to reload it by.
pub fn project_logo_src(path: Option<&str>, revision: u64) -> Option<(String, u64)> {
    path.filter(|path| !path.is_empty())
        .map(|path| (path.to_string(), revision))
}

impl Projects {
    /// `pickAndSetProjectLogo` after the picker: copy `source_path` into app
    /// data under the project's key and show it. Resolves to the saved path.
    pub fn set_project_logo(
        &mut self,
        project_path: &str,
        source_path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, String>> {
        let project = project_key(project_path);
        let logos = self.logos();
        let backend = self.backend.clone();
        let source = source_path.to_string();
        cx.spawn(async move |this, cx| {
            let save_backend = backend.clone();
            let save_project = project.clone();
            let path = cx
                .background_spawn(
                    async move { save_backend.save_project_logo(&save_project, &source) },
                )
                .await?;
            if let Some(stale) = droppable_logo_file(&logos, &project, Some(&path)) {
                let _ = cx
                    .background_spawn(async move { backend.forget_logo_file(&stale) })
                    .await;
            }
            this.update(cx, |this, cx| {
                let mut store = this.store();
                this.appearance
                    .save_tab_group_logo(&mut store, &project, Some(&path));
                this.appearance.notify_tab_group_logos_changed();
                this.emit_appearance_events(cx);
            })
            .ok();
            Ok(path)
        })
    }

    /// `clearProjectLogo`: remove the project's logo, and its file once no
    /// other project shows it. `project` is the project key.
    pub fn clear_project_logo(
        &mut self,
        project: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let stale = droppable_logo_file(&self.logos(), project, None);
        let backend = self.backend.clone();
        let project = project.to_string();
        cx.spawn(async move |this, cx| {
            let remove_backend = backend.clone();
            let remove_project = project.clone();
            cx.background_spawn(async move { remove_backend.remove_project_logo(&remove_project) })
                .await?;
            if let Some(stale) = stale {
                let _ = cx
                    .background_spawn(async move { backend.forget_logo_file(&stale) })
                    .await;
            }
            this.update(cx, |this, cx| {
                let mut store = this.store();
                this.appearance
                    .save_tab_group_logo(&mut store, &project, None);
                this.appearance.notify_tab_group_logos_changed();
                this.emit_appearance_events(cx);
            })
            .ok();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::FakeBackend;
    use crate::projects::{ProjectsConfig, ProjectsGlobal};
    use gpui::TestAppContext;
    use monocode_layout::tab_groups::KEY_VERSION_KEY;
    use monocode_settings::Kv;
    use std::sync::Arc;

    const SHARED: &str = "/logos/agentbase.png";

    fn finance() -> String {
        project_key("/Users/me/cortex-finance/agentbase")
    }

    fn cortex() -> String {
        project_key("/Users/me/cortex/agentbase")
    }

    fn record(entries: &[(&str, &str)]) -> JsRecord<String> {
        let mut logos = JsRecord::new();
        for (key, path) in entries {
            logos.insert(*key, path.to_string());
        }
        logos
    }

    fn init(cx: &mut TestAppContext) -> (Arc<FakeBackend>, gpui::Entity<Projects>) {
        let kv = Kv::in_memory();
        kv.set_item(KEY_VERSION_KEY, "2");
        let backend = FakeBackend::new();
        cx.update(|cx| {
            ProjectsGlobal::init(
                ProjectsConfig {
                    kv,
                    backend: backend.clone(),
                    clock: crate::projects::system_clock(),
                },
                cx,
            )
        });
        let projects = cx.update(|cx| ProjectsGlobal::projects(cx));
        (backend, projects)
    }

    #[gpui::test]
    async fn opens_in_a_folder_and_saves_the_logo_under_its_project_key(cx: &mut TestAppContext) {
        for (directory, key) in [
            ("D:\\Work\\My Project", "d:/work/my project"),
            ("D:\\", "d:"),
            ("/Users/me/My Project", "/Users/me/My Project"),
            ("\\\\server\\share\\My Project", "//server/share/my project"),
        ] {
            let (backend, projects) = init(cx);
            backend.set_saved_path("/logos/saved.png");
            let result = projects
                .update(cx, |projects, cx| {
                    projects.set_project_logo(directory, "C:\\Pictures\\logo.png", cx)
                })
                .await;
            assert_eq!(result.as_deref(), Ok("/logos/saved.png"));
            assert_eq!(
                backend.calls("save_project_logo"),
                [serde_json::json!({ "project": key, "sourcePath": "C:\\Pictures\\logo.png" })]
            );
            let logos = projects.update(cx, |projects, _| projects.logos());
            assert_eq!(logos.get(key).map(String::as_str), Some("/logos/saved.png"));
        }
    }

    #[test]
    fn keeps_a_file_another_project_still_shows() {
        let logos = record(&[(&finance(), SHARED), (&cortex(), SHARED)]);
        assert_eq!(droppable_logo_file(&logos, &finance(), None), None);
    }

    #[test]
    fn drops_a_file_only_this_project_points_at() {
        let logos = record(&[(&finance(), SHARED), (&cortex(), "/logos/other.png")]);
        assert_eq!(
            droppable_logo_file(&logos, &finance(), None).as_deref(),
            Some(SHARED)
        );
    }

    #[test]
    fn keeps_the_file_the_project_is_about_to_point_at() {
        let logos = record(&[(&finance(), SHARED)]);
        assert_eq!(droppable_logo_file(&logos, &finance(), Some(SHARED)), None);
    }

    #[test]
    fn has_nothing_to_drop_for_a_project_without_a_logo() {
        assert_eq!(
            droppable_logo_file(&JsRecord::new(), &finance(), None),
            None
        );
    }

    #[gpui::test]
    async fn leaves_the_shared_file_on_disk_for_the_other_project(cx: &mut TestAppContext) {
        let (backend, projects) = init(cx);
        projects.update(cx, |projects, _| {
            let mut store = projects.store();
            projects
                .appearance
                .save_tab_group_logo(&mut store, &finance(), Some(SHARED));
            projects
                .appearance
                .save_tab_group_logo(&mut store, &cortex(), Some(SHARED));
        });

        projects
            .update(cx, |projects, cx| {
                projects.clear_project_logo(&finance(), cx)
            })
            .await
            .unwrap();

        assert!(backend.calls("forget_logo_file").is_empty());
        assert_eq!(
            backend.calls("remove_project_logo"),
            [serde_json::json!({ "project": finance() })]
        );
    }

    #[gpui::test]
    async fn removes_the_file_once_the_last_project_lets_go_of_it(cx: &mut TestAppContext) {
        let (backend, projects) = init(cx);
        projects.update(cx, |projects, _| {
            let mut store = projects.store();
            projects
                .appearance
                .save_tab_group_logo(&mut store, &finance(), Some(SHARED));
            projects
                .appearance
                .save_tab_group_logo(&mut store, &cortex(), Some(SHARED));
        });

        projects
            .update(cx, |projects, cx| {
                projects.clear_project_logo(&finance(), cx)
            })
            .await
            .unwrap();
        let revision = projects.read_with(cx, |projects, _| projects.logo_display_revision());
        projects
            .update(cx, |projects, cx| {
                projects.clear_project_logo(&cortex(), cx)
            })
            .await
            .unwrap();

        assert_eq!(
            backend.calls("forget_logo_file"),
            [serde_json::json!({ "path": SHARED })]
        );
        assert_eq!(
            projects.read_with(cx, |projects, _| projects.logo_display_revision()),
            revision + 1
        );
        assert_eq!(
            project_logo_src(Some("/l.png"), 3),
            Some(("/l.png".into(), 3))
        );
        assert_eq!(project_logo_src(None, 3), None);
    }
}
