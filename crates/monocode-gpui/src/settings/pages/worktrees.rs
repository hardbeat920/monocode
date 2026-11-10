use crate::{
    settings::{
        SettingsView,
        management::Target,
        widgets::{note, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
impl SettingsView {
    pub(super) fn worktrees_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let project = self.project_picker(cx);
        let ready = !self.discovery_loading
            && self.discovery.worktrees.error.is_none()
            && !self.discovery.worktrees.main.is_empty();
        let mut page=vec![div().relative().child(self.reveal_probe("project-worktrees")).flex().items_center().gap_1().child(project).child(secondary_button("create-settings-worktree","Create worktree",false,!ready).child(icon("plus",12.)).child("Create worktree").on_click(cx.listener(move|this,_,window,cx|{if ready{this.open_management(Target::Worktree,window,cx);}}))).into_any_element(),div().mt_4().flex().items_center().justify_between().gap_3().child(div().min_w_0().flex_1().text_size(px(12.)).line_height(px(18.)).text_color(content(0.5)).child("Sessions can share a worktree. Deleting one keeps its sessions by default and discards uncommitted changes. Its branch and commits are kept.")).child(self.refresh_button("refresh-settings-worktrees","Refresh",cx)).into_any_element()];
        if self.selected_project.is_empty() {
            page.push(note("Add a project to manage its worktrees."));
        } else if self.discovery_loading {
            page.push(note("Loading worktrees…"));
        } else if let Some(error) = &self.discovery.worktrees.error {
            page.push(
                div()
                    .id("settings-page-error")
                    .mt_4()
                    .role(gpui::Role::Alert)
                    .text_size(px(12.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            );
        } else if self.discovery.worktrees.trees.is_empty() {
            page.push(
                div()
                    .mt_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(content(0.1))
                    .px_4()
                    .py_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(icon("folder-tree", 20.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("No additional worktrees"),
                    )
                    .child(
                        div().text_size(px(12.)).text_color(content(0.5)).child(
                            "Create a worktree to work on another branch in a separate folder.",
                        ),
                    )
                    .into_any_element(),
            );
        } else {
            let trees = self.discovery.worktrees.trees.clone();
            page.push(
                div()
                    .mt_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(content(0.1))
                    .overflow_hidden()
                    .children(trees.into_iter().enumerate().map(|(ix, tree)| {
                        let t = &tree.tree;
                        let path = std::path::PathBuf::from(&t.path);
                        let count = self
                            .ctx
                            .sessions
                            .iter()
                            .filter(|(_, p, _)| {
                                monocode_core::files::is_equal_or_inside(p, &t.path)
                            })
                            .count();
                        let status = if t.missing {
                            "Missing folder"
                        } else if tree.dirty == Some(true) {
                            "Uncommitted changes"
                        } else if tree.dirty == Some(false) {
                            "Clean"
                        } else {
                            "Status unavailable"
                        };
                        let blocked = tree.locked || t.branch.is_none();
                        div()
                            .p_4()
                            .flex()
                            .items_start()
                            .gap_3()
                            .border_b_1()
                            .border_color(content(0.1))
                            .child(icon("folder-tree", 16.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(
                                                path.file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                                    .into_owned(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .text_size(px(11.))
                                            .text_color(content(0.4))
                                            .child(t.path.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt_2()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .text_size(px(11.))
                                            .text_color(content(0.55))
                                            .child(icon("git-branch", 12.))
                                            .child(
                                                t.branch
                                                    .as_ref()
                                                    .map(|b| format!("Current branch: {b}"))
                                                    .unwrap_or_else(|| {
                                                        format!(
                                                            "Detached at {}",
                                                            t.head
                                                                .chars()
                                                                .take(7)
                                                                .collect::<String>()
                                                        )
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt_2()
                                            .text_size(px(11.))
                                            .text_color(content(0.55))
                                            .child(format!(
                                                "{count} {} in this worktree · {status}{}{}",
                                                if count == 1 { "session" } else { "sessions" },
                                                if tree.unpushed > 0 {
                                                    format!(
                                                        " · {} unpublished commits",
                                                        tree.unpushed
                                                    )
                                                } else {
                                                    String::new()
                                                },
                                                if tree.locked { " · Locked" } else { "" }
                                            )),
                                    ),
                            )
                            .child(
                                secondary_button(
                                    SharedString::from(format!("reveal-tree-{ix}")),
                                    format!("Reveal {}", t.branch.as_deref().unwrap_or("worktree")),
                                    false,
                                    t.missing,
                                )
                                .child(icon("folder-open", 16.))
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                            )
                            .child(
                                secondary_button(
                                    SharedString::from(format!("delete-tree-{ix}")),
                                    format!("Delete {}", t.branch.as_deref().unwrap_or("worktree")),
                                    false,
                                    blocked,
                                )
                                .child(icon("trash", 16.))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if !blocked {
                                            this.open_management(
                                                Target::DeleteTree(tree.clone()),
                                                window,
                                                cx,
                                            );
                                        }
                                    },
                                )),
                            )
                    }))
                    .into_any_element(),
            );
        }
        if ready {
            page.push(note(format!(
                "New worktrees are created in {}.",
                self.discovery.worktrees.default_root.display()
            )));
        }
        page
    }
}
