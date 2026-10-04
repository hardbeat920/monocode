//! Project menus persist appearance, grouping, notifications, and local folder actions.
use super::Shell;
use crate::adapters::settings::{AccountsAdapter, SettingsAdapter};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, Point, Render, Styled as _, WeakEntity, Window,
    deferred, div,
};
use monocode_app::boot::AppServices;
use monocode_engine::attention::{Attention, notification_projects};
use monocode_engine::projects::{Projects, ProjectsGlobal, project_groups::ProjectGroup};
use monocode_layout::{
    paths::{is_remote_project_path, project_key, project_name},
    tab_groups::resolve_tab_group_color,
};
use monocode_process::external_editor::ExternalEditor;
use monocode_ui::{IconName, Theme, u};
use monocode_view_settings::accounts::{
    host::NotificationsHost,
    mute_control::{DatePickerEvent, NotificationMuteDatePicker},
    notification_model::{
        PreferencePatch, notification_mute_actions, notification_mute_deadline,
        notification_mute_status,
    },
};
use monocode_view_settings::settings::project_background_dialog::ProjectBackgroundDialog;
use monocode_view_workbench::panes::tab_group_menu::{
    SubmenuEntry, TabGroupMenu, TabGroupMenuEvent, TabGroupMenuExtraItem, TabGroupMenuProps,
};
use std::rc::Rc;

#[derive(Clone)]
enum MenuTarget {
    Project(String),
    Group(String),
}

struct ProjectActionContext {
    projects: Entity<Projects>,
    menu: WeakEntity<TabGroupMenu>,
    position: Point<Pixels>,
}

fn submenu_item(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
    checked: bool,
) -> SubmenuEntry {
    SubmenuEntry::Item {
        id: id.into(),
        label: label.into(),
        disabled: false,
        checked,
    }
}

fn editor_submenu(editors: &[ExternalEditor]) -> Vec<SubmenuEntry> {
    if editors.is_empty() {
        return vec![SubmenuEntry::Item {
            id: "external-editor:none".into(),
            label: "No supported editors found".into(),
            disabled: true,
            checked: false,
        }];
    }
    editors
        .iter()
        .map(|editor| {
            submenu_item(
                format!("external-editor:{}", editor.id()),
                editor.name().to_string(),
                false,
            )
        })
        .collect()
}

fn group_submenu(groups: &[ProjectGroup], current: Option<&str>) -> Vec<SubmenuEntry> {
    let mut entries = vec![submenu_item("project-group:new", "New group", false)];
    if !groups.is_empty() {
        entries.push(SubmenuEntry::Separator);
    }
    entries.extend(groups.iter().map(|group| {
        submenu_item(
            format!("project-group:{}", group.id),
            group.name.clone(),
            current == Some(group.id.as_str()),
        )
    }));
    if !groups.is_empty() {
        entries.push(SubmenuEntry::Separator);
    }
    entries.push(submenu_item(
        "project-group:none",
        "Ungrouped",
        current.is_none(),
    ));
    entries
}

impl Shell {
    pub fn show_project_menu(
        &mut self,
        project: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_menu.is_none() {
            self.project_menu_return_focus = window.focused(cx);
        }
        let Some(global) = ProjectsGlobal::try_global(cx) else {
            return;
        };
        let projects = global.projects.clone();
        let path = project.to_string();
        let key = project_key(project);
        let seed = project_name(project);
        let (labels, colors, custom, logos, mascots) = projects.update(cx, |projects, _| {
            (
                projects.labels(),
                projects.colors(),
                projects.custom_colors(),
                projects.logos(),
                projects.mascots(),
            )
        });
        let model = projects.read(cx);
        let pinned = model.pinned().iter().any(|pinned| {
            monocode_core::paths::path_key(pinned) == monocode_core::paths::path_key(project)
        });
        let mut groups =
            TabGroupMenuExtraItem::new("project-group", "Move to group", IconName::FolderTree);
        groups.submenu = Some(group_submenu(
            model.groups(),
            model.group_id_for_path(project).as_deref(),
        ));
        let local = !is_remote_project_path(project);
        let notification = notification_projects::known_notification_project(model.kv(), project);
        let notification_id = notification.as_ref().map(|project| project.id.clone());
        let accounts = AccountsAdapter::new();
        let now = Attention::now(cx);
        let preferences = accounts.preferences(cx);
        let muted = notification_id
            .as_ref()
            .and_then(|id| notification_mute_status(preferences.get(id), now));
        let mut reveal = TabGroupMenuExtraItem::new(
            "reveal",
            if cfg!(target_os = "macos") {
                "Reveal in Finder"
            } else if cfg!(windows) {
                "Reveal in File Explorer"
            } else {
                "Open Containing Folder"
            },
            IconName::FolderOpen,
        );
        reveal.disabled = !local;
        let mut editor =
            TabGroupMenuExtraItem::new("external-editor", "Open in editor", IconName::AppWindow);
        editor.disabled = true;
        editor.submenu = Some(vec![SubmenuEntry::Item {
            id: "external-editor:loading".into(),
            label: if local {
                "Looking for editors"
            } else {
                "Available for local folders"
            }
            .into(),
            disabled: true,
            checked: false,
        }]);
        let mut mute = TabGroupMenuExtraItem::new(
            "notifications-mute",
            "Mute notifications",
            IconName::BellOff,
        );
        mute.sep_before = true;
        mute.disabled = notification_id.is_none();
        mute.submenu = Some(
            notification_mute_actions(now)
                .iter()
                .map(|action| submenu_item(action.id, action.label.clone(), false))
                .collect(),
        );
        let mut extras = vec![
            TabGroupMenuExtraItem::new("background", "Background image", IconName::ImagePlus),
            groups,
            TabGroupMenuExtraItem::new(
                if pinned { "unpin" } else { "pin" },
                if pinned {
                    "Unpin project"
                } else {
                    "Pin project"
                },
                if pinned {
                    IconName::PinOff
                } else {
                    IconName::Pin
                },
            ),
            reveal,
            editor,
            mute,
            TabGroupMenuExtraItem::new(
                "notifications-settings",
                "Notification settings",
                IconName::Settings,
            ),
        ];
        if notification_projects::looks_like_project(project) {
            let mut archive =
                TabGroupMenuExtraItem::new("archive-project", "Archive", IconName::Archive);
            archive.sep_before = true;
            let mut remove =
                TabGroupMenuExtraItem::new("remove-project", "Delete", IconName::Trash2);
            remove.danger = true;
            extras.extend([archive, remove]);
        }
        let props = TabGroupMenuProps {
            position,
            group_id: key.clone(),
            label: labels.get(&key).cloned().unwrap_or(seed.clone()),
            color_index: colors.get(&key).copied(),
            custom_color: custom.get(&key).cloned(),
            current_color: monocode_view_quick::model::appearance::hex_color(
                &resolve_tab_group_color(&key, Some(&colors), Some(&custom), Some(&seed)),
            ),
            logo_path: logos.get(&key).cloned(),
            logo_project: Some(path.clone()),
            mascot_name: mascots.get(&key).cloned(),
            mascot_project: seed,
            show_actions: false,
            leading_action: muted.map(|description| {
                let mut item = TabGroupMenuExtraItem::new(
                    "notifications-resume",
                    "Resume notifications",
                    IconName::BellOff,
                );
                item.description = Some(description.into());
                item
            }),
            extra_items: extras,
        };
        self.install_project_menu(MenuTarget::Project(path), props, projects, window, cx);
        if local {
            let target = self.project_menu.as_ref().unwrap().downgrade();
            let loading = cx.background_spawn(async {
                monocode_process::external_editor::list_external_editors()
            });
            cx.spawn(async move |_, cx| {
                let editors = loading.await;
                target
                    .update(cx, |menu, cx| {
                        let mut props = menu.props().clone();
                        if let Some(item) = props
                            .extra_items
                            .iter_mut()
                            .find(|item| item.id == "external-editor")
                        {
                            item.disabled = false;
                            item.submenu = Some(editor_submenu(&editors));
                        }
                        menu.set_props(props, cx);
                    })
                    .ok();
            })
            .detach();
        }
    }

    pub fn create_project_group(
        &mut self,
        project: Option<&str>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = ProjectsGlobal::projects(cx);
        let group = projects.update(cx, |projects, cx| {
            let group = projects.create_group(cx);
            if let Some(project) = project {
                projects.set_group_assignment(project, Some(&group.id), cx);
            }
            group
        });
        self.show_project_group_menu(&group.id, position, window, cx);
    }

    pub fn show_project_group_menu(
        &mut self,
        id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_menu.is_none() {
            self.project_menu_return_focus = window.focused(cx);
        }
        let projects = ProjectsGlobal::projects(cx);
        let Some(group) = projects
            .read(cx)
            .groups()
            .iter()
            .find(|group| group.id == id)
            .cloned()
        else {
            return;
        };
        let mut delete =
            TabGroupMenuExtraItem::new("delete-project-group", "Delete group", IconName::Trash2);
        delete.description = Some("Projects will become ungrouped".into());
        delete.danger = true;
        let props = TabGroupMenuProps {
            position,
            group_id: group.id.clone(),
            label: group.name.clone(),
            color_index: group
                .color_index
                .and_then(|index| usize::try_from(index).ok()),
            custom_color: group.custom_color.clone(),
            current_color: monocode_view_quick::model::appearance::hex_color(
                &monocode_engine::projects::project_groups::project_group_color(&group),
            ),
            mascot_name: group.mascot.clone(),
            mascot_project: group.id.clone(),
            show_actions: false,
            extra_items: vec![delete],
            ..Default::default()
        };
        self.install_project_menu(MenuTarget::Group(group.id), props, projects, window, cx);
    }

    fn install_project_menu(
        &mut self,
        target: MenuTarget,
        props: TabGroupMenuProps,
        projects: Entity<Projects>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| TabGroupMenu::new(props.clone(), window, cx));
        let color_target = target.clone();
        let color_key = props.group_id.clone();
        let color_projects = projects.clone();
        let color = cx.new(|cx| {
            monocode_view_settings::settings::color_picker::ColorPicker::new(
                &props.current_color,
                window,
                cx,
            )
            .on_change(move |value, _, cx| {
                color_projects.update(cx, |projects, cx| match &color_target {
                    MenuTarget::Project(_) => {
                        projects.save_custom_color(&color_key, Some(value), cx)
                    }
                    MenuTarget::Group(id) => projects.update_group(
                        id,
                        |mut group| {
                            group.color_index = None;
                            group.custom_color = Some(value.into());
                            group
                        },
                        cx,
                    ),
                });
            })
        });
        menu.update(cx, |menu, cx| {
            menu.set_custom_picker(Some(color.into()), cx);
            menu.set_on_extra_pick(Some(Rc::new(|id, _, _| {
                !id.starts_with("external-editor:")
            })));
        });
        self.project_menu_subscription = Some(cx.subscribe_in(
            &menu,
            window,
            move |this, source, event, window, cx| {
                if this
                    .project_menu
                    .as_ref()
                    .is_none_or(|current| current.entity_id() != source.entity_id())
                {
                    return;
                }
                match event {
                    TabGroupMenuEvent::Rename { group_id, label } => {
                        projects.update(cx, |projects, cx| match &target {
                            MenuTarget::Project(_) => projects.save_label(group_id, label, cx),
                            MenuTarget::Group(id) => projects.update_group(
                                id,
                                |mut group| {
                                    if !label.trim().is_empty() {
                                        group.name = label.trim().into();
                                    }
                                    group
                                },
                                cx,
                            ),
                        })
                    }
                    TabGroupMenuEvent::ColorChange {
                        group_id,
                        color_index,
                    } => projects.update(cx, |projects, cx| match &target {
                        MenuTarget::Project(_) => projects.save_color(group_id, *color_index, cx),
                        MenuTarget::Group(id) => projects.update_group(
                            id,
                            |mut group| {
                                group.color_index = color_index.map(|index| index as i64);
                                group.custom_color = None;
                                group
                            },
                            cx,
                        ),
                    }),
                    TabGroupMenuEvent::MascotChange { group_id, name } => {
                        projects.update(cx, |projects, cx| match &target {
                            MenuTarget::Project(_) => {
                                projects.save_mascot(group_id, name.as_deref(), cx)
                            }
                            MenuTarget::Group(id) => projects.update_group(
                                id,
                                |mut group| {
                                    group.mascot = name.clone();
                                    group
                                },
                                cx,
                            ),
                        })
                    }
                    TabGroupMenuEvent::PickLogo { project } => {
                        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Choose project logo".into()),
                        });
                        let projects = projects.clone();
                        let project = project.clone();
                        cx.spawn(async move |_, cx| {
                            if let Ok(Ok(Some(paths))) = picked.await
                                && let Some(path) = paths.first()
                            {
                                let path = path.to_string_lossy().to_string();
                                let save = cx.update(|cx| {
                                    projects.update(cx, |projects, cx| {
                                        projects.set_project_logo(&project, &path, cx)
                                    })
                                });
                                if let Err(error) = save.await {
                                    cx.update(|cx| {
                                        monocode_app::bridge::dialogs::alert(&error, true, cx)
                                    });
                                }
                            }
                        })
                        .detach();
                    }
                    TabGroupMenuEvent::ClearLogo { project } => {
                        projects
                            .update(cx, |projects, cx| {
                                projects.clear_project_logo(&project_key(project), cx)
                            })
                            .detach();
                    }
                    TabGroupMenuEvent::ExtraPick(action) => match &target {
                        MenuTarget::Project(project) => this.pick_project_action(
                            project,
                            action,
                            ProjectActionContext {
                                projects: projects.clone(),
                                menu: source.downgrade(),
                                position: props.position,
                            },
                            window,
                            cx,
                        ),
                        MenuTarget::Group(id) if action == "delete-project-group" => {
                            projects.update(cx, |projects, cx| projects.delete_group(id, cx));
                        }
                        MenuTarget::Group(_) => {}
                    },
                    TabGroupMenuEvent::Close => {
                        this.project_menu = None;
                        this.project_menu_subscription = None;
                        if this.project_dialog.is_none()
                            && let Some(focus) = this.project_menu_return_focus.take()
                        {
                            focus.focus(window, cx);
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            },
        ));
        self.project_menu = Some(menu);
        cx.notify();
    }

    fn pick_project_action(
        &mut self,
        project: &str,
        id: &str,
        action: ProjectActionContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ProjectActionContext {
            projects,
            menu,
            position,
        } = action;
        match id {
            "pin" | "unpin" => projects.update(cx, |projects, cx| projects.toggle_pin(project, cx)),
            "project-group:new" => self.create_project_group(Some(project), position, window, cx),
            "project-group:none" => projects.update(cx, |projects, cx| {
                projects.set_group_assignment(project, None, cx)
            }),
            "background" => self.show_project_background(project, window, cx),
            "notifications-settings" => {
                self.open_page(crate::slots::Page::Settings, cx);
                crate::pages::settings::reveal_project_notifications(project, window, cx);
            }
            "reveal" if !is_remote_project_path(project) => {
                let path = project.to_string();
                let opening =
                    cx.background_spawn(async move { monocode_git::fs::reveal_path(path) });
                report_task(opening, cx);
            }
            "archive-project" | "remove-project" => {
                let purge = id == "remove-project";
                let message = if purge {
                    "Remove this project from MonoCode and close its tabs? Project files stay on disk."
                } else {
                    "Archive this project and close its tabs?"
                };
                let confirm = monocode_app::bridge::dialogs::confirm(
                    message,
                    if purge { "Delete" } else { "Archive" },
                    cx,
                );
                let project = project.to_string();
                cx.spawn(async move |_, cx| {
                    if confirm.await {
                        cx.update(|cx| {
                            monocode_engine::projects::actions::on_remove_project(
                                &project, purge, cx,
                            )
                        });
                    }
                })
                .detach();
            }
            _ if id.starts_with("project-group:") => {
                let group = &id["project-group:".len()..];
                if projects
                    .read(cx)
                    .groups()
                    .iter()
                    .any(|entry| entry.id == group)
                {
                    projects.update(cx, |projects, cx| {
                        projects.set_group_assignment(project, Some(group), cx)
                    });
                }
            }
            _ if id.starts_with("external-editor:") && !is_remote_project_path(project) => {
                let editor = id["external-editor:".len()..].to_string();
                let cwd = project.to_string();
                let opening = cx.background_spawn(async move {
                    monocode_process::external_editor::open_in_external_editor(editor, cwd)
                });
                let shell = cx.weak_entity();
                let window = window.window_handle();
                cx.spawn(async move |_, cx| match opening.await {
                    Ok(()) => {
                        cx.update_window(window, |_, window, cx| {
                            shell
                                .update(cx, |shell, cx| {
                                    if shell.project_menu.as_ref().is_some_and(|current| {
                                        current.entity_id() == menu.entity_id()
                                    }) {
                                        shell.project_menu = None;
                                        shell.project_menu_subscription = None;
                                        if let Some(focus) = shell.project_menu_return_focus.take()
                                        {
                                            focus.focus(window, cx);
                                        }
                                        cx.notify();
                                    }
                                })
                                .ok();
                        })
                        .ok();
                    }
                    Err(error) => {
                        cx.update(|cx| monocode_app::bridge::dialogs::alert(&error, true, cx));
                    }
                })
                .detach();
            }
            _ if id.starts_with("mute:") || id == "notifications-resume" => {
                let Some(notification) = notification_projects::known_notification_project(
                    projects.read(cx).kv(),
                    project,
                ) else {
                    return;
                };
                if id == "mute:custom" {
                    self.show_project_mute_picker(notification.id, window, cx);
                    return;
                }
                let mute = if id == "notifications-resume" {
                    None
                } else {
                    notification_mute_deadline(id, Attention::now(cx))
                };
                if mute.is_none() && id != "notifications-resume" {
                    return;
                }
                if let Err(error) = AccountsAdapter::new().update_preferences(
                    &[notification.id],
                    &PreferencePatch::mute(mute),
                    cx,
                ) {
                    monocode_app::bridge::dialogs::alert(&error, true, cx);
                }
            }
            _ => {}
        }
    }

    fn close_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_dialog = None;
        self.project_dialog_subscription = None;
        if let Some(focus) = self.project_dialog_return_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn show_project_background(
        &mut self,
        project: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = ProjectsGlobal::projects(cx);
        let key = project_key(project);
        let name = projects
            .update(cx, |projects, _| projects.labels().get(&key).cloned())
            .unwrap_or_else(|| project_name(project));
        let kv = AppServices::global(cx).kv.clone();
        let host = Rc::new(SettingsAdapter::new(cx));
        self.project_dialog_return_focus = self
            .project_menu_return_focus
            .take()
            .or_else(|| window.focused(cx));
        let weak = cx.weak_entity();
        let dialog = cx.new(|cx| {
            ProjectBackgroundDialog::new(key, name, kv, host, cx).on_close(move |window, cx| {
                weak.update(cx, |shell, cx| shell.close_project_dialog(window, cx))
                    .ok();
            })
        });
        dialog.read(cx).focus_handle(cx).focus(window, cx);
        self.project_dialog = Some(dialog.into());
        self.project_dialog_subscription = None;
        cx.notify();
    }

    fn show_project_mute_picker(
        &mut self,
        project_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project_dialog_return_focus = self
            .project_menu_return_focus
            .take()
            .or_else(|| window.focused(cx));
        let picker = cx.new(|cx| {
            NotificationMuteDatePicker::new(
                Rc::new(AccountsAdapter::new()),
                vec![project_id],
                window,
                cx,
            )
        });
        self.project_dialog_subscription = Some(cx.subscribe_in(
            &picker,
            window,
            |shell, _, _: &DatePickerEvent, window, cx| shell.close_project_dialog(window, cx),
        ));
        let shell = cx.weak_entity();
        self.project_dialog = Some(cx.new(|_| ProjectMuteDialog { picker, shell }).into());
        cx.notify();
    }

    pub fn open_project_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open project".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let path = monocode_platform::path_to_js(path);
                this.update(cx, |shell, cx| {
                    ProjectsGlobal::projects(cx)
                        .update(cx, |projects, cx| projects.remember_project(&path, cx));
                    shell.select_project(&path, cx);
                })
                .ok();
            }
        })
        .detach();
    }
}

fn report_task(task: gpui::Task<Result<(), String>>, cx: &mut Context<Shell>) {
    cx.spawn(async move |_, cx| {
        if let Err(error) = task.await {
            cx.update(|cx| monocode_app::bridge::dialogs::alert(&error, true, cx));
        }
    })
    .detach();
}

struct ProjectMuteDialog {
    picker: Entity<NotificationMuteDatePicker>,
    shell: WeakEntity<Shell>,
}
impl Render for ProjectMuteDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let close = self.shell.clone();
        let key_close = close.clone();
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(theme.colors.modal_overlay)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    close
                        .update(cx, |shell, cx| shell.close_project_dialog(window, cx))
                        .ok();
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
                        key_close
                            .update(cx, |shell, cx| shell.close_project_dialog(window, cx))
                            .ok();
                        cx.stop_propagation();
                    }
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(u(320.))
                        .max_w_full()
                        .p(u(16.))
                        .rounded(u(theme.radius.xl))
                        .bg(theme.colors.body_glass)
                        .border_1()
                        .border_color(theme.colors.modal_border)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(self.picker.clone()),
                ),
        )
        .with_priority(theme.layer.dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_menu_marks_the_current_assignment_and_keeps_ungrouped_available() {
        let groups = vec![
            ProjectGroup::new("one", "One", false),
            ProjectGroup::new("two", "Two", true),
        ];
        let entries = group_submenu(&groups, Some("two"));
        let selected: Vec<_> = entries
            .iter()
            .filter_map(|entry| match entry {
                SubmenuEntry::Item {
                    id, checked: true, ..
                } => Some(id.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(selected, vec!["project-group:two"]);
        assert!(entries.iter().any(|entry| matches!(entry, SubmenuEntry::Item { id, disabled: false, .. } if id == "project-group:none")));
        assert!(group_submenu(&groups, None).iter().any(|entry| matches!(entry, SubmenuEntry::Item { id, checked: true, .. } if id == "project-group:none")));
    }
}
