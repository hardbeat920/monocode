use super::*;
use monocode_engine::history::session_folders::SessionListEntry;

impl SessionList {
    pub(super) fn render_list_entry(
        &self,
        entry: &SessionListEntry,
        data: &ListData,
        index: &mut usize,
        now: i64,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut cards = |rows: &[monocode_engine::runtime::session_store::SessionSummary],
                         cx: &mut Context<Self>| {
            let mut body = div().flex().flex_col().gap(u(2.0));
            for row in rows {
                if let Some(card) = data.sessions.iter().find(|card| card.id == row.id) {
                    let selected = data.active_session_id.as_deref() == Some(&card.id)
                        || data.selected_ids.contains(&card.id);
                    body = body
                        .child(self.render_session_card(*index, card, selected, now, theme, cx));
                    *index += 1;
                }
            }
            body
        };
        match entry {
            SessionListEntry::Session { session } => {
                cards(std::slice::from_ref(session.as_ref()), cx).into_any_element()
            }
            SessionListEntry::Folder { folder, sessions } => {
                let collapsed = folder.collapsed && !data.search_narrowed;
                let label = if self
                    .editing
                    .as_ref()
                    .is_some_and(|(id, is_folder)| *is_folder && id == &folder.id)
                {
                    text_field(&self.rename_input).into_any_element()
                } else {
                    div()
                        .flex_1()
                        .truncate()
                        .child(folder.name.clone())
                        .into_any_element()
                };
                let header = div()
                    .id(gpui::SharedString::from(format!(
                        "folder-header-{}",
                        folder.id
                    )))
                    .flex()
                    .items_center()
                    .gap(u(6.0))
                    .p(u(6.0))
                    .text_px(theme.text.caption)
                    .medium()
                    .child(
                        icon(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .size(u(12.0)),
                    )
                    .child(icon(IconName::Folder).size(u(13.0)))
                    .child(label)
                    .child(
                        div()
                            .text_color(theme.content(0.45))
                            .child(sessions.len().to_string()),
                    )
                    .on_click({
                        let id = folder.id.clone();
                        let was = folder.collapsed;
                        cx.listener(move |this, _, _, cx| {
                            if this
                                .editing
                                .as_ref()
                                .is_some_and(|(target, _)| target == &id)
                            {
                                return;
                            }
                            if let Some(history) = this.history(cx) {
                                history.update(cx, |history, cx| {
                                    history.set_folder_collapsed(&id, !was, cx)
                                });
                            }
                        })
                    })
                    .drag_over::<monocode_view_workbench::panes::pane_tree::PaneDragSource>({ let color = theme.accent(0.2); move |style, _, _, _| style.bg(color) })
                    .on_drop({ let target = folder.id.clone(); cx.listener(move |this, source: &monocode_view_workbench::panes::pane_tree::PaneDragSource, window, cx| {
                        if let monocode_view_workbench::panes::pane_tree::PaneDragSource::Session(id) = source {
                            this.folder_drop(id, monocode_engine::history::session_folders::SessionListDropTarget::Folder { id: target.clone() }, window, cx);
                        }
                    }) })
                    .on_mouse_down(MouseButton::Right, {
                        let id = folder.id.clone();
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.folder_menu = Some((id.clone(), event.position));
                            cx.notify();
                        })
                    });
                let palette = folder
                    .color_index
                    .filter(|index| *index > 0)
                    .and_then(|index| {
                        monocode_layout::tab_groups::TAB_GROUP_COLORS.get(index as usize)
                    })
                    .copied();
                let color = folder
                    .custom_color
                    .as_deref()
                    .or(palette)
                    .and_then(monocode_view_settings::accounts::style::parse_css_color)
                    .unwrap_or(theme.colors.accent);
                let mut group = div()
                    .flex()
                    .flex_col()
                    .rounded(u(theme.radius.md))
                    .bg(monocode_ui::color::with_alpha(color, 0.1))
                    .child(header);
                if !collapsed {
                    group = group.child(cards(sessions, cx));
                }
                group.into_any_element()
            }
            SessionListEntry::Pinned {
                collapsed,
                sessions,
            }
            | SessionListEntry::Reminders {
                collapsed,
                sessions,
            } => {
                let pinned = matches!(entry, SessionListEntry::Pinned { .. });
                let collapsed = *collapsed && !data.search_narrowed;
                let header = div()
                    .id(if pinned {
                        "pinned-header"
                    } else {
                        "reminders-header"
                    })
                    .flex()
                    .items_center()
                    .gap(u(6.0))
                    .p(u(6.0))
                    .text_px(theme.text.caption)
                    .medium()
                    .text_color(theme.content(0.6))
                    .child(
                        icon(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .size(u(12.0)),
                    )
                    .child(
                        icon(if pinned {
                            IconName::Pin
                        } else {
                            IconName::Clock
                        })
                        .size(u(13.0)),
                    )
                    .child(if pinned { "Pinned" } else { "Reminders" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(history) = this.history(cx) {
                            history.update(cx, |history, cx| {
                                if pinned {
                                    history.set_pinned_collapsed(!collapsed, cx)
                                } else {
                                    history.set_reminders_collapsed(!collapsed, cx)
                                }
                            });
                        }
                    }));
                let mut group = div().flex().flex_col().child(header);
                if !collapsed {
                    group = group.child(cards(sessions, cx));
                }
                group.into_any_element()
            }
        }
    }
}
