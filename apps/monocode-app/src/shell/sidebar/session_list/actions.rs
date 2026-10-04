use super::*;
use monocode_engine::history::session_filters::{SessionSidebarFilters, SessionTimeFilter};
use monocode_engine::history::sidebar::SessionMenuAction;
use monocode_engine::remote::RemoteGlobal;
use monocode_view_inbox::pr::link_dialog::{LinkDialogEvent, LinkSessionWorkItemDialog};
use serde_json::{Value, json};

impl SessionList {
    fn remote_target(
        &self,
        cx: &App,
    ) -> Option<(
        String,
        String,
        monocode_engine::remote::client::RemoteClient,
    )> {
        let cwd = self.shell.upgrade()?.read(cx).sidebar_cwd(cx);
        if !monocode_layout::paths::is_remote_project_path(&cwd) {
            return None;
        }
        let remote = RemoteGlobal::try_global(cx)?;
        let connections = remote.connections.read(cx);
        let project = connections.remote_project_for(&cwd)?;
        let machine = connections.project_sessions(&cwd).machine?;
        Some((machine.id, project.project_id, remote.client.clone()))
    }

    pub(super) fn remote_change(
        &self,
        ids: Vec<String>,
        patch: Value,
        delete: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let cwd = self
            .shell
            .upgrade()
            .map(|shell| shell.read(cx).sidebar_cwd(cx))
            .unwrap_or_default();
        if !monocode_layout::paths::is_remote_project_path(&cwd) {
            return false;
        }
        let Some((machine, project, client)) = self.remote_target(cx) else {
            monocode_app::bridge::dialogs::alert(
                "Connect this project's machine to change its sessions.",
                true,
                cx,
            );
            return true;
        };
        let confirmation = delete.then(|| {
            monocode_app::bridge::dialogs::confirm(
                "Delete these conversations? This cannot be undone.",
                "Delete",
                cx,
            )
        });
        cx.spawn(async move |_, cx| {
            if let Some(confirmation) = confirmation
                && !confirmation.await
            {
                return;
            }
            for id in ids {
                let mut params = patch.as_object().cloned().unwrap_or_default();
                params.insert("projectId".into(), project.clone().into());
                params.insert("sessionId".into(), id.clone().into());
                if let Err(error) = client
                    .request(
                        &machine,
                        if delete {
                            "sessions.delete"
                        } else {
                            "sessions.update"
                        },
                        params.into(),
                    )
                    .await
                {
                    cx.update(|cx| {
                        monocode_app::bridge::dialogs::alert(
                            &format!("Could not change this conversation.\n\n{error}"),
                            true,
                            cx,
                        )
                    });
                    break;
                }
                if delete {
                    cx.update(|cx| {
                        let Some(remote) = RemoteGlobal::try_global(cx) else {
                            return;
                        };
                        let connections = remote.connections.clone();
                        let ids = Engine::sessions(cx)
                            .read(cx)
                            .all()
                            .iter()
                            .filter(|session| {
                                monocode_layout::paths::same_project_path(&session.cwd, &cwd)
                                    && connections
                                        .read(cx)
                                        .remote_session_for(&session.id)
                                        .as_deref()
                                        == Some(&id)
                            })
                            .map(|session| session.id.clone())
                            .collect::<Vec<_>>();
                        for id in ids {
                            crate::slots::forget_session_in_windows(&id, cx);
                            RemoteGlobal::forget_tab(&id, cx);
                        }
                    });
                }
            }
            cx.update(|cx| {
                if let Some(remote) = RemoteGlobal::try_global(cx) {
                    remote.connections.clone().update(cx, |connections, cx| {
                        connections.refresh_remote_project_sessions(cx)
                    });
                }
            });
        })
        .detach();
        true
    }

    fn start_rename(
        &mut self,
        id: String,
        name: &str,
        folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing = Some((id, folder));
        self.rename_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn folder_drop(
        &mut self,
        source: &str,
        target: monocode_engine::history::session_folders::SessionListDropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(history) = self.history(cx) {
            history.update(cx, |history, cx| {
                history.drop_on_session_list(source, &target, cx)
            });
            let folder = history
                .read(cx)
                .sidebar()
                .renaming_folder_id
                .as_ref()
                .and_then(|id| {
                    history
                        .read(cx)
                        .sidebar()
                        .folders
                        .iter()
                        .find(|folder| &folder.id == id)
                })
                .cloned();
            if let Some(folder) = folder {
                self.start_rename(folder.id, &folder.name, true, window, cx);
            }
        }
    }

    pub(super) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some((id, folder)) = self.editing.take() else {
            return;
        };
        let value = self.rename_input.read(cx).value().trim().to_owned();
        if !value.is_empty() {
            if folder {
                if let Some(history) = self.history(cx) {
                    history.update(cx, |history, cx| history.rename_folder(&id, &value, cx));
                }
            } else if !self.remote_change(vec![id.clone()], json!({ "title": value }), false, cx)
                && let Some(history) = self.history(cx)
            {
                history
                    .update(cx, |history, cx| history.rename_session(&id, &value, cx))
                    .detach();
            }
        }
        cx.notify();
    }

    fn open_link_dialog(
        &mut self,
        session_id: String,
        row: &monocode_engine::runtime::session_store::SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = cx.new(|cx| {
            LinkSessionWorkItemDialog::new(
                row.linked_work_item.clone(),
                session_display_title(&row.title, row.harness),
                window,
                cx,
            )
        });
        self.link_subscription = Some(cx.subscribe(&dialog, move |this, _, event, cx| {
            if let LinkDialogEvent::Save(linked) = event
                && !this.remote_change(
                    vec![session_id.clone()],
                    json!({ "linkedWorkItem": linked }),
                    false,
                    cx,
                )
                && let Some(inbox) = monocode_engine::inbox::inbox::Inbox::try_global(cx)
            {
                inbox.update(cx, |inbox, cx| {
                    inbox.set_session_linked_work_item(&session_id, linked.clone(), cx)
                });
            }
            this.link_dialog = None;
            this.link_subscription = None;
            cx.notify();
        }));
        self.link_dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn render_session_menu(
        &self,
        data: &ListData,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let position = self.session_menu?;
        let row = self
            .menu_session
            .as_ref()
            .and_then(|id| data.listed.iter().find(|row| &row.id == id))
            .cloned();
        let state = self
            .history(cx)
            .map(|history| history.read(cx).session_menu_state(&data.listed))
            .unwrap_or_default();
        let remote_project = self.shell.upgrade().is_some_and(|shell| {
            monocode_layout::paths::is_remote_project_path(&shell.read(cx).sidebar_cwd(cx))
        });
        let mut entries: Vec<MenuEntry> = vec![
            MenuItem::new("open", "Open in new tab")
                .shortcut("⌘↩")
                .into(),
            MenuItem::new("rename", "Rename")
                .shortcut("F2")
                .disabled(state.session_ids.len() > 1)
                .into(),
            MenuItem::new("pin", if state.all_pinned { "Unpin" } else { "Pin" }).into(),
            MenuItem::new("link", "Link issue or pull request")
                .disabled(state.session_ids.len() > 1)
                .into(),
            MenuEntry::Separator,
            MenuItem::new("new-folder", "Move to new folder").into(),
        ];
        if let Some(history) = self.history(cx) {
            for folder in &history.read(cx).sidebar().folders {
                entries.push(
                    MenuItem::new(
                        format!("folder:{}", folder.id),
                        format!("Move to {}", folder.name),
                    )
                    .checked(
                        state
                            .folders_checked
                            .iter()
                            .any(|(id, checked)| id == &folder.id && *checked),
                    )
                    .into(),
                );
            }
        }
        if state.can_remove_from_folders {
            entries.push(MenuItem::new("remove-folder", "Remove from folder").into());
        }
        entries.extend([
            MenuEntry::Separator,
            MenuItem::new("copy-id", "Copy session id").into(),
        ]);
        for (id, label) in [
            ("reminder:1h", "Remind me in 1 hour"),
            ("reminder:3h", "Remind me in 3 hours"),
            ("reminder:evening", "Remind me this evening"),
            ("reminder:tomorrow", "Remind me tomorrow"),
            ("reminder:next-week", "Remind me next week"),
        ] {
            let due = monocode_engine::automations::reminders::reminder_time(id, now_ms());
            let mut item = MenuItem::new(id, label).disabled(remote_project || due.is_none());
            if remote_project {
                item = item.description("Reminders require a saved local conversation.");
            } else if let Some(due) = due {
                item = item.description(
                    monocode_engine::automations::reminders::format_reminder_time(due),
                );
            }
            entries.push(item.into());
        }
        let has_reminder = monocode_engine::automations::AutomationsPackage::try_global(cx)
            .is_some_and(|package| {
                package
                    .reminders
                    .read(cx)
                    .reminders()
                    .iter()
                    .any(|reminder| state.session_ids.contains(&reminder.session_id))
            });
        if has_reminder {
            entries.push(MenuItem::new("reminder:cancel", "Cancel reminder").into());
        }
        entries.extend([
            MenuEntry::Separator,
            MenuItem::new(
                "archive",
                if state.all_archived {
                    "Unarchive"
                } else {
                    "Archive"
                },
            )
            .into(),
            MenuItem::new("delete", "Delete").danger().into(),
        ]);
        let weak = cx.weak_entity();
        let pick = weak.clone();
        Some(context_menu(
            position,
            menu("session-menu", entries).on_pick(move |id, window, cx| {
                pick.update(cx, |this, cx| {
                    this.session_menu = None;
                    let Some(session_id) = this.menu_session.take() else {
                        return;
                    };
                    let ids = if state.session_ids.is_empty() {
                        vec![session_id.clone()]
                    } else {
                        state.session_ids.clone()
                    };
                    let action = match id.as_ref() {
                        "open" => {
                            this.with_shell(cx, |shell, cx| shell.open_session(&session_id, cx));
                            None
                        }
                        "rename" => {
                            if let Some(row) = &row {
                                this.start_rename(
                                    session_id.clone(),
                                    &session_display_title(&row.title, row.harness),
                                    false,
                                    window,
                                    cx,
                                );
                            }
                            None
                        }
                        "link" => {
                            if let Some(row) = &row {
                                this.open_link_dialog(session_id.clone(), row, window, cx);
                            }
                            None
                        }
                        "pin"
                            if this.remote_change(
                                ids.clone(),
                                json!({ "pinned": !state.all_pinned }),
                                false,
                                cx,
                            ) =>
                        {
                            None
                        }
                        "pin" => Some(SessionMenuAction::TogglePin),
                        "archive"
                            if this.remote_change(
                                ids.clone(),
                                json!({ "archived": !state.all_archived }),
                                false,
                                cx,
                            ) =>
                        {
                            None
                        }
                        "archive" => Some(SessionMenuAction::ToggleArchive),
                        "delete" if this.remote_change(ids.clone(), json!({}), true, cx) => None,
                        "delete" => Some(SessionMenuAction::Delete),
                        "new-folder" => Some(SessionMenuAction::NewFolder),
                        "remove-folder" => Some(SessionMenuAction::RemoveFromFolders),
                        "copy-id" => {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(ids.join("\n")));
                            None
                        }
                        "reminder:cancel" => {
                            if let Some(package) =
                                monocode_engine::automations::AutomationsPackage::try_global(cx)
                            {
                                package
                                    .reminders
                                    .clone()
                                    .update(cx, |reminders, cx| {
                                        reminders.cancel(ids.clone(), None, cx)
                                    })
                                    .detach();
                            }
                            None
                        }
                        preset if preset.starts_with("reminder:") && !remote_project => {
                            if let Some(due) =
                                monocode_engine::automations::reminders::reminder_time(
                                    preset,
                                    now_ms(),
                                )
                                && let Some(package) =
                                    monocode_engine::automations::AutomationsPackage::try_global(cx)
                            {
                                package
                                    .reminders
                                    .clone()
                                    .update(cx, |reminders, cx| {
                                        reminders.schedule(ids.clone(), due, cx)
                                    })
                                    .detach();
                                if let Some(history) = this.history(cx) {
                                    history
                                        .update(cx, |history, cx| history.reminder_scheduled(cx));
                                }
                            }
                            None
                        }
                        value if value.starts_with("folder:") => {
                            Some(SessionMenuAction::AddToFolder(value[7..].to_owned()))
                        }
                        _ => None,
                    };
                    if let Some(history) = this.history(cx) {
                        if let Some(action) = action {
                            let data = this.data(cx);
                            history.update(cx, |history, cx| {
                                history.pick_session_menu(action, &data.listed, cx)
                            });
                            let folder = history
                                .read(cx)
                                .sidebar()
                                .renaming_folder_id
                                .as_ref()
                                .and_then(|id| {
                                    history
                                        .read(cx)
                                        .sidebar()
                                        .folders
                                        .iter()
                                        .find(|folder| &folder.id == id)
                                })
                                .cloned();
                            if let Some(folder) = folder {
                                this.start_rename(folder.id, &folder.name, true, window, cx);
                            }
                        } else {
                            history.update(cx, |history, cx| history.close_session_menu(cx));
                        }
                    }
                    cx.notify();
                })
                .ok();
            }),
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.session_menu = None;
                    if let Some(history) = this.history(cx) {
                        history.update(cx, |history, cx| history.close_session_menu(cx));
                    }
                    cx.notify();
                })
                .ok();
            },
            cx,
        ))
    }

    pub(super) fn render_filters_menu(
        &self,
        data: &ListData,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let position = self.filters_menu?;
        let filters = self
            .history(cx)
            .map(|history| history.read(cx).sidebar().filters.clone())
            .unwrap_or_default();
        let mut entries: Vec<MenuEntry> = vec![
            MenuItem::new("archive", "Show archived")
                .checked(filters.show_archived)
                .into(),
            MenuEntry::Separator,
        ];
        for harness in &data.harnesses {
            entries.push(
                MenuItem::new(format!("provider:{harness}"), harness.title().to_owned())
                    .checked(!filters.hidden_harnesses.contains(harness))
                    .into(),
            );
        }
        entries.push(MenuEntry::Separator);
        for (id, label, value) in [
            ("time:all", "All time", SessionTimeFilter::All),
            ("time:today", "Today", SessionTimeFilter::Today),
            ("time:week", "Last 7 days", SessionTimeFilter::SevenDays),
            ("time:month", "Last 30 days", SessionTimeFilter::ThirtyDays),
        ] {
            entries.push(
                MenuItem::new(id, label)
                    .checked(filters.time == value)
                    .into(),
            );
        }
        entries.extend([
            MenuEntry::Separator,
            MenuItem::new("working", "Working")
                .checked(filters.status.working)
                .into(),
            MenuItem::new("approval", "Needs approval")
                .checked(filters.status.needs_approval)
                .into(),
            MenuItem::new("done", "Done")
                .checked(filters.status.done)
                .into(),
            MenuEntry::Separator,
            MenuItem::new("reset", "Reset filters").into(),
        ]);
        let weak = cx.weak_entity();
        let pick = weak.clone();
        Some(context_menu(
            position,
            menu("session-filters-menu", entries).on_pick(move |id, _, cx| {
                pick.update(cx, |this, cx| {
                    if let Some(history) = this.history(cx) {
                        let mut filters = history.read(cx).sidebar().filters.clone();
                        match id.as_ref() {
                            "archive" => filters.show_archived = !filters.show_archived,
                            "working" => filters.status.working = !filters.status.working,
                            "approval" => {
                                filters.status.needs_approval = !filters.status.needs_approval
                            }
                            "done" => filters.status.done = !filters.status.done,
                            "time:all" => filters.time = SessionTimeFilter::All,
                            "time:today" => filters.time = SessionTimeFilter::Today,
                            "time:week" => filters.time = SessionTimeFilter::SevenDays,
                            "time:month" => filters.time = SessionTimeFilter::ThirtyDays,
                            "reset" => filters = SessionSidebarFilters::default(),
                            value if value.starts_with("provider:") => {
                                if let Ok(harness) = value[9..].parse::<HarnessId>() {
                                    if filters.hidden_harnesses.contains(&harness) {
                                        filters.hidden_harnesses.retain(|id| id != &harness);
                                    } else {
                                        filters.hidden_harnesses.push(harness);
                                    }
                                }
                            }
                            _ => {}
                        }
                        history.update(cx, |history, cx| history.set_filters(filters, cx));
                    }
                    this.filters_menu = None;
                    cx.notify();
                })
                .ok();
            }),
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.filters_menu = None;
                    cx.notify();
                })
                .ok();
            },
            cx,
        ))
    }

    pub(super) fn render_folder_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (folder_id, position) = self.folder_menu.clone()?;
        let mut entries: Vec<MenuEntry> = vec![
            MenuItem::new("new", "New session in folder").into(),
            MenuItem::new("rename", "Rename folder").into(),
            MenuItem::new("ungroup", "Ungroup sessions").into(),
        ];
        entries.push(MenuEntry::Separator);
        for index in 0..monocode_layout::tab_groups::TAB_GROUP_COLORS.len() {
            entries.push(
                MenuItem::new(
                    format!("color:{index}"),
                    if index == 0 {
                        "Default color".to_owned()
                    } else {
                        format!("Color {index}")
                    },
                )
                .into(),
            );
        }
        let weak = cx.weak_entity();
        let pick = weak.clone();
        Some(context_menu(
            position,
            menu("session-folder-menu", entries).on_pick(move |id, window, cx| {
                pick.update(cx, |this, cx| {
                    this.folder_menu = None;
                    if let Some(history) = this.history(cx) {
                        match id.as_ref() {
                            "rename" => {
                                let name = history
                                    .read(cx)
                                    .sidebar()
                                    .folders
                                    .iter()
                                    .find(|folder| folder.id == folder_id)
                                    .map(|folder| folder.name.clone());
                                if let Some(name) = name {
                                    this.start_rename(folder_id.clone(), &name, true, window, cx);
                                }
                            }
                            "ungroup" => history
                                .update(cx, |history, cx| history.dissolve_folder(&folder_id, cx)),
                            "new" => {
                                let mut created = None;
                                this.with_shell(cx, |shell, cx| created = shell.new_session(cx));
                                if let Some(created) = created {
                                    history.update(cx, |history, cx| {
                                        history.new_in_folder(&folder_id, &created, cx)
                                    });
                                }
                            }
                            value if value.starts_with("color:") => {
                                if let Ok(index) = value[6..].parse::<i64>() {
                                    history.update(cx, |history, cx| {
                                        history.set_folder_custom_color(&folder_id, None, cx);
                                        history.set_folder_color(&folder_id, Some(index), cx);
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    cx.notify();
                })
                .ok();
            }),
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    this.folder_menu = None;
                    cx.notify();
                })
                .ok();
            },
            cx,
        ))
    }
}
