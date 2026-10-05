//! Port of src/features/projects/model/chatBackground.ts: copy a chat
//! background image into app data, globally or for one project.
//!
//! The file picker belongs to the view; it passes the picked path, or
//! `None` when the user cancelled. `projectChatBackgroundSrc` becomes the
//! image path plus the revision to reload it by.

use std::sync::Arc;

use gpui::{BackgroundExecutor, Task};

use super::backend::ProjectsBackend;

/// The image types both background pickers offer.
pub const BACKGROUND_IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
/// The global background picker's title.
pub const CHAT_BACKGROUND_PICKER_TITLE: &str = "Choose chat background";
/// The project background picker's title.
pub const PROJECT_CHAT_BACKGROUND_PICKER_TITLE: &str = "Choose project chat background";

/// `pickAndSaveChatBackground` after the picker. `Ok(None)` when nothing
/// was picked.
pub fn pick_and_save_chat_background(
    backend: &Arc<dyn ProjectsBackend>,
    picked: Option<&str>,
    executor: &BackgroundExecutor,
) -> Task<Result<Option<String>, String>> {
    let Some(source) = picked.filter(|source| !source.is_empty()) else {
        return Task::ready(Ok(None));
    };
    let (backend, source) = (backend.clone(), source.to_string());
    executor.spawn(async move { backend.save_chat_background(&source).map(Some) })
}

/// `removeChatBackground`.
pub fn remove_chat_background(
    backend: &Arc<dyn ProjectsBackend>,
    executor: &BackgroundExecutor,
) -> Task<Result<(), String>> {
    let backend = backend.clone();
    executor.spawn(async move { backend.remove_chat_background() })
}

/// `pickAndSaveProjectChatBackground` after the picker.
pub fn pick_and_save_project_chat_background(
    backend: &Arc<dyn ProjectsBackend>,
    project: &str,
    picked: Option<&str>,
    executor: &BackgroundExecutor,
) -> Task<Result<Option<String>, String>> {
    let Some(source) = picked.filter(|source| !source.is_empty()) else {
        return Task::ready(Ok(None));
    };
    let (backend, project, source) = (backend.clone(), project.to_string(), source.to_string());
    executor.spawn(async move {
        backend
            .save_project_chat_background(&project, &source)
            .map(Some)
    })
}

/// `clearProjectChatBackground`: delete the project's copied image.
pub fn clear_project_chat_background(
    backend: &Arc<dyn ProjectsBackend>,
    project: &str,
    executor: &BackgroundExecutor,
) -> Task<Result<(), String>> {
    let (backend, project) = (backend.clone(), project.to_string());
    executor.spawn(async move { backend.remove_project_chat_background(&project) })
}

/// `projectChatBackgroundSrc`: the image and the revision to reload it by.
/// The webview needed an asset URL with `?v=`; GPUI loads the path and
/// reloads when the revision moves.
pub fn project_chat_background_src(path: &str, revision: i64) -> (String, i64) {
    (path.to_string(), revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::FakeBackend;
    use gpui::TestAppContext;
    use serde_json::json;

    fn setup() -> (Arc<FakeBackend>, Arc<dyn ProjectsBackend>) {
        let fake = FakeBackend::new();
        let backend: Arc<dyn ProjectsBackend> = fake.clone();
        (fake, backend)
    }

    #[gpui::test]
    async fn copies_a_picked_image_into_app_storage(cx: &mut TestAppContext) {
        let (fake, backend) = setup();
        fake.set_saved_path("/app-data/backgrounds/chat-background.webp");
        let saved =
            pick_and_save_chat_background(&backend, Some("/Pictures/aurora.webp"), &cx.executor())
                .await;
        assert_eq!(
            saved,
            Ok(Some("/app-data/backgrounds/chat-background.webp".into()))
        );
        assert_eq!(
            fake.calls("save_chat_background"),
            [json!({ "sourcePath": "/Pictures/aurora.webp" })]
        );
    }

    #[gpui::test]
    async fn leaves_the_current_background_alone_when_picking_is_cancelled(
        cx: &mut TestAppContext,
    ) {
        let (fake, backend) = setup();
        assert_eq!(
            pick_and_save_chat_background(&backend, None, &cx.executor()).await,
            Ok(None)
        );
        assert!(fake.commands().is_empty());
    }

    #[gpui::test]
    async fn removes_the_saved_background(cx: &mut TestAppContext) {
        let (fake, backend) = setup();
        remove_chat_background(&backend, &cx.executor())
            .await
            .unwrap();
        assert_eq!(fake.commands(), ["remove_chat_background"]);
    }

    #[gpui::test]
    async fn copies_a_picked_image_into_project_specific_app_storage(cx: &mut TestAppContext) {
        let (fake, backend) = setup();
        fake.set_saved_path("/app-data/backgrounds/project-abc.png");
        let saved = pick_and_save_project_chat_background(
            &backend,
            "/work/agent-terminal",
            Some("/Pictures/grid.png"),
            &cx.executor(),
        )
        .await;
        assert_eq!(
            saved,
            Ok(Some("/app-data/backgrounds/project-abc.png".into()))
        );
        assert_eq!(
            fake.calls("save_project_chat_background"),
            [json!({ "project": "/work/agent-terminal", "sourcePath": "/Pictures/grid.png" })]
        );
    }

    #[gpui::test]
    async fn removes_only_the_selected_projects_saved_background(cx: &mut TestAppContext) {
        let (fake, backend) = setup();
        clear_project_chat_background(&backend, "/work/agent-terminal", &cx.executor())
            .await
            .unwrap();
        assert_eq!(
            fake.calls("remove_project_chat_background"),
            [json!({ "project": "/work/agent-terminal" })]
        );
    }

    #[test]
    fn cache_busts_project_background_images_after_replacement() {
        assert_eq!(
            project_chat_background_src("/app-data/background.png", 42),
            ("/app-data/background.png".to_string(), 42)
        );
    }
}
