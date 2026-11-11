use crate::{
    project_settings::{Event, button},
    runtime::{Database, locked},
    theme::*,
};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, PathPromptOptions, Role, Window, div,
    prelude::*, px,
};
use monocode_core::{project_folders::ProjectFolders, provider::Kind};

pub(crate) struct FoldersDialog {
    project: String,
    provider: Kind,
    store: Database,
    folders: ProjectFolders,
    error: Option<String>,
    busy: bool,
    focus: FocusHandle,
}
impl EventEmitter<Event> for FoldersDialog {}
impl Focusable for FoldersDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl FoldersDialog {
    pub(crate) fn new(
        project: String,
        provider: Kind,
        store: Database,
        cx: &mut Context<Self>,
    ) -> Self {
        let result = locked(&store).and_then(|store| store.project_folders(&project));
        let (folders, error) = match result {
            Ok(folders) => (folders, None),
            Err(error) => (ProjectFolders::default(), Some(error)),
        };
        Self {
            project,
            provider,
            store,
            folders,
            error,
            busy: false,
            focus: cx.focus_handle(),
        }
    }
    fn add(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add project folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = picker.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match picked {
                    Ok(Ok(Some(paths))) if !paths.is_empty() => {
                        let result = locked(&this.store)
                            .and_then(|store| store.add_project_folder(&this.project, &paths[0]));
                        this.apply(result);
                    }
                    Ok(Ok(_)) => {}
                    _ => this.error = Some("Could not open the folder picker.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn apply(&mut self, result: Result<ProjectFolders, String>) {
        match result {
            Ok(folders) => {
                self.folders = folders;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}
impl Render for FoldersDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let supported = ProjectFolders::supported(self.provider, &self.project);
        div().id("project-folders-dialog").role(Role::Dialog).aria_label("Additional folders")
            .track_focus(&self.focus).max_h(px(window.viewport_size().height.as_f32() - 40.)).overflow_y_scroll().p_4().flex().flex_col().gap_3().text_color(content(1.))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| { if event.keystroke.key == "escape" { cx.emit(Event::Close); cx.stop_propagation(); } }))
            .child(div().text_size(px(16.)).child("Additional folders"))
            .child(div().text_size(px(12.)).child(format!("Default folder (unchanged): {}", self.project)))
            .child(div().text_size(px(12.)).child("Local Codex CLI 0.167.1 or newer only. Other providers and remote projects are not supported. Changes apply on the next regular turn; the current turn keeps its folders. Selected access permissions still apply. Removing a folder does not erase conversation history or restrict Full access."))
            .children(self.folders.additional.clone().into_iter().enumerate().map(|(ix, path)| {
                let label = path.display().to_string();
                div().flex().items_center().gap_2().child(div().flex_1().text_size(px(12.)).child(label))
                    .child(button(("remove-project-folder", ix), "Remove").on_click(cx.listener(move |this, _, _, cx| {
                        let result = locked(&this.store).and_then(|store| store.remove_project_folder(&this.project, &path));
                        this.apply(result); cx.notify();
                    })))
            }))
            .when_some(self.error.clone(), |el, error| el.child(div().text_size(px(12.)).child(error)))
            .child(div().flex().gap_2()
                .when(supported && !self.busy, |el| el.child(button("add-project-folder", "Add folder").on_click(cx.listener(|this, _, _, cx| this.add(cx)))))
                .child(button("close-project-folders", "Done").on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close)))))
    }
}
