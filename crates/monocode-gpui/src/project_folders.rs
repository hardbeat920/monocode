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
use std::path::PathBuf;

enum FolderRow {
    Default,
    Additional { index: usize, path: PathBuf },
}

struct FolderPathTooltip(String);
impl Render for FolderPathTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(480.))
            .px_2()
            .py_1()
            .rounded_md()
            .bg(pane_fill())
            .border_1()
            .border_color(content(0.15))
            .text_size(px(12.))
            .text_color(content(1.))
            .child(self.0.clone())
    }
}

pub(crate) struct FoldersDialog {
    project: String,
    provider: Kind,
    store: Database,
    folders: ProjectFolders,
    error: Option<String>,
    busy: bool,
    focus: FocusHandle,
    details_open: bool,
    details_focus: FocusHandle,
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
            details_open: false,
            details_focus: cx.focus_handle(),
        }
    }
    fn toggle_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.details_open = !self.details_open;
        window.focus(&self.details_focus, cx);
        cx.notify();
    }
    fn folder_row(&self, row: FolderRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let path = match &row {
            FolderRow::Default => PathBuf::from(&self.project),
            FolderRow::Additional { path, .. } => path.clone(),
        };
        let full_path = path.display().to_string();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| full_path.clone());
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(content(0.1))
            .bg(content(0.03))
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon("folder", 18.).text_color(content(0.6))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(
                        div()
                            .id(match &row {
                                FolderRow::Default => "default-folder-path".to_owned(),
                                FolderRow::Additional { index, .. } => {
                                    format!("additional-folder-path-{index}")
                                }
                            })
                            .role(Role::Label)
                            .aria_label(full_path.clone())
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(content(0.55))
                            .tooltip(move |_, cx| {
                                cx.new(|_| FolderPathTooltip(full_path.clone())).into()
                            })
                            .child(path.display().to_string()),
                    ),
            )
            .child(match row {
                FolderRow::Default => div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(content(0.06))
                    .text_size(px(11.))
                    .text_color(content(0.6))
                    .child("Default")
                    .into_any_element(),
                FolderRow::Additional { index, path } => {
                    button(("remove-project-folder", index), "Remove")
                        .flex_shrink_0()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let result = locked(&this.store).and_then(|store| {
                                store.remove_project_folder(&this.project, &path)
                            });
                            this.apply(result);
                            cx.notify();
                        }))
                        .into_any_element()
                }
            })
            .into_any_element()
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
            .track_focus(&self.focus).relative().max_h(px(window.viewport_size().height.as_f32() - 40.))
            .overflow_y_scroll().p_4().flex().flex_col().gap_2().text_color(content(1.))

            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" { cx.emit(Event::Close); cx.stop_propagation(); }
            }))
            .child(div().flex_shrink_0().text_size(px(16.)).font_weight(gpui::FontWeight::SEMIBOLD).child("Additional folders"))
            .child(div().flex_shrink_0().flex().flex_col().gap_2()
                .child(self.folder_row(FolderRow::Default, cx))
                .children(self.folders.additional.iter().cloned().enumerate().map(|(index, path)|
                    self.folder_row(FolderRow::Additional { index, path }, cx))))
            .when(!supported, |el| el.child(div().flex_shrink_0().text_size(px(12.)).text_color(content(0.6))
                .child("Adding folders requires local Codex CLI 0.167.1 or newer; other providers and remote projects are not supported.")))
            .child(button("project-folders-details", "How folders work")
                .flex_shrink_0().h(px(28.)).border_color(content(0.)).aria_expanded(self.details_open).track_focus(&self.details_focus).tab_index(0)
                .justify_start().when(self.details_focus.is_focused(window), |el| el.border_color(content(0.6)))
                .child(icon(if self.details_open { "chevron-up" } else { "chevron-down" }, 14.))
                .on_click(cx.listener(|this, _, window, cx| this.toggle_details(window, cx))))
            .when(self.details_open, |el| el.child(div().flex_shrink_0().flex().flex_col().gap_2().text_size(px(12.)).text_color(content(0.6))
                .child("Additional folders require local Codex CLI 0.167.1 or newer. Other providers and remote projects are not supported.")
                .child("Changes apply on the next regular turn; the current turn keeps its folders. Selected access permissions still apply.")
                .child("Removing a folder does not erase conversation history or restrict Full access.")))
            .when_some(self.error.clone(), |el, error| el.child(div().flex_shrink_0().text_size(px(12.)).child(error)))
            .child(div().flex_shrink_0().flex().gap_2()
                .when(supported && !self.busy, |el| el.child(button("add-project-folder", "Add folder").on_click(cx.listener(|this, _, _, cx| this.add(cx)))))
                .child(button("close-project-folders", "Done").on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close)))))
    }
}
