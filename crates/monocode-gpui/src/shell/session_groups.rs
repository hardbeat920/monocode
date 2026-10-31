//! Sidebar.tsx's session actions: pins, renames, GitHub links, reminders,
//! folders and deletion, and the grouped list they shape.
use super::*;
use crate::work_item_picker::{Picked, WorkItemPicker};
use chrono::{Local, NaiveDateTime, TimeZone};
use gpui::EventEmitter;
use monocode_core::{
    session_groups::{
        Entry, Groups, WorkItem, build_list, format_reminder, navigation_ids, reminder_at,
        reminder_presets,
    },
    store::Reminder,
};

actions!(
    session_groups,
    [CancelRename, RenameSidebarItem, DeleteSidebarSelection]
);

pub(super) fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", CancelRename, Some("SessionRename")),
        KeyBinding::new("f2", RenameSidebarItem, Some("Workspace && !SessionRename")),
        KeyBinding::new(
            "backspace",
            DeleteSidebarSelection,
            Some("SessionSidebar && !Composer"),
        ),
        KeyBinding::new(
            "delete",
            DeleteSidebarSelection,
            Some("SessionSidebar && !Composer"),
        ),
    ]);
}

/// tabGroups.ts's TAB_GROUP_COLORS; 0 is the default wash.
const FOLDER_COLORS: [(f32, f32, f32); 9] = [
    (210., 0.08, 0.58),
    (211., 0.92, 0.62),
    (12., 0.80, 0.58),
    (45., 0.90, 0.55),
    (142., 0.55, 0.50),
    (330., 0.70, 0.62),
    (280., 0.55, 0.62),
    (175., 0.55, 0.48),
    (25., 0.85, 0.58),
];
fn folder_color(ix: usize) -> Option<gpui::Hsla> {
    FOLDER_COLORS
        .get(ix)
        .map(|&(h, s, l)| gpui::hsla(h / 360., s, l, 1.))
}
/// Sidebar.tsx's REMINDERS_COLOR, amber-500.
const REMINDERS_COLOR: u32 = 0xf59e0b;

fn local_ms(at: NaiveDateTime) -> Option<u64> {
    Local
        .from_local_datetime(&at)
        .earliest()
        .map(|at| at.timestamp_millis().max(0) as u64)
}
fn local_time(ms: u64) -> Option<NaiveDateTime> {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|at| at.with_timezone(&Local).naive_local())
}
pub(super) fn local_reminder_label(ms: u64) -> String {
    local_time(ms).map(format_reminder).unwrap_or_default()
}
pub(super) fn reminder_caption(ms: u64) -> String {
    let Some(at) = local_time(ms) else {
        return String::new();
    };
    let now = Local::now().naive_local();
    if at <= now {
        "Now".into()
    } else if at.date() == now.date() {
        at.format("%-H:%M").to_string()
    } else {
        at.format("%a").to_string()
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum ListDrop {
    Folder(String),
    Session(i64),
}
pub(super) struct FolderDrag {
    id: String,
    start: Point<Pixels>,
    active: bool,
    over: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(super) enum RenameTarget {
    Session(i64),
    Folder(String),
}
pub(super) struct Renaming {
    pub(super) target: RenameTarget,
    input: Entity<input::Input>,
    _events: Vec<Subscription>,
}
pub(super) struct LinkDialog {
    id: i64,
    picker: Entity<WorkItemPicker>,
    _events: Subscription,
}

/// FolderColorSwatches: the folder menu's palette row.
pub(super) struct FolderSwatches {
    focus: FocusHandle,
    selected: usize,
}
pub(super) struct SwatchPicked(Option<usize>);
impl EventEmitter<SwatchPicked> for FolderSwatches {}
impl Render for FolderSwatches {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .px_2()
            .py_1p5()
            .flex()
            .items_center()
            .gap_1p5()
            .children((0..FOLDER_COLORS.len()).map(|ix| {
                let selected = ix == self.selected;
                div()
                    .id(("folder-swatch", ix))
                    .debug_selector(move || format!("folder-color-{ix}"))
                    .role(Role::Button)
                    .aria_label(if ix == 0 {
                        "Default color".to_string()
                    } else {
                        format!("Color {ix}")
                    })
                    .size(px(18.))
                    .rounded_full()
                    .border_2()
                    .border_color(if selected { content(0.9) } else { content(0.) })
                    .hover(|s| s.border_color(content(0.5)))
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .border_2()
                            .border_color(content(0.))
                            .bg(folder_color(ix).unwrap()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = ix;
                        cx.emit(SwatchPicked((ix > 0).then_some(ix)));
                        cx.notify();
                    }))
            }))
    }
}

impl Shell {
    pub(super) fn rename_sidebar_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.sidebar_focus.is_focused(window) {
            self.sidebar_target.clone()
        } else {
            self.session_hover
                .or(self.workspace.active)
                .map(RenameTarget::Session)
        };
        if let Some(target) = target {
            self.start_rename(target, window, cx);
        }
    }
    pub(super) fn delete_sidebar_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<_> = self.selected_sessions.iter().copied().collect();
        for id in ids {
            self.delete_session(id, window, cx);
        }
    }
    pub(super) fn sidebar_drop_at(
        &self,
        position: Point<Pixels>,
        source: i64,
        cx: &App,
    ) -> Option<ListDrop> {
        if !self.workspace.sidebar_visible || self.reminders.iter().any(|r| r.session_id == source)
        {
            return None;
        }
        let groups = self.project_groups();
        for folder in &groups.folders {
            if self
                .folder_bounds
                .get(&folder.id)
                .and_then(AnchorSlot::get)
                .is_some_and(|b| b.contains(&position))
            {
                return Some(ListDrop::Folder(folder.id.clone()));
            }
        }
        for id in self.sidebar_navigation_ids(cx) {
            if id == source || self.reminders.iter().any(|r| r.session_id == id) {
                continue;
            }
            if self
                .session_row_bounds
                .get(&id)
                .and_then(AnchorSlot::get)
                .is_some_and(|b| b.contains(&position))
            {
                return Some(match groups.folder_of(id) {
                    Some(folder) => ListDrop::Folder(folder.id.clone()),
                    None => ListDrop::Session(id),
                });
            }
        }
        None
    }
    pub(super) fn apply_sidebar_drop(
        &mut self,
        source: i64,
        target: ListDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            ListDrop::Folder(folder) => {
                self.update_groups(cx, |g| g.add_to_folder(&folder, source))
            }
            ListDrop::Session(target) if source != target => {
                let folder = format!("{:x}-{source}-{target}", monocode_core::time::now_ms());
                self.update_groups(cx, |g| {
                    g.create_folder(&[source, target], folder.clone());
                });
                self.start_rename(RenameTarget::Folder(folder), window, cx);
            }
            _ => {}
        }
    }
    pub(super) fn move_folder_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.folder_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.folder_drag = None;
            return;
        }
        let delta = event.position - drag.start;
        if !drag.active && delta.x.as_f32().hypot(delta.y.as_f32()) < 5. {
            return;
        }
        drag.active = true;
        self.skip_folder_click = true;
        drag.over = self.folder_bounds.iter().find_map(|(id, slot)| {
            slot.get()
                .filter(|b| b.contains(&event.position))
                .map(|_| id.clone())
        });
        self.notify_sidebar(cx);
    }
    pub(super) fn finish_folder_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.folder_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        if let Some(over) = drag.over {
            self.update_groups(cx, |g| {
                if let Some((from, to)) = g
                    .folders
                    .iter()
                    .position(|f| f.id == drag.id)
                    .zip(g.folders.iter().position(|f| f.id == over))
                {
                    let folder = g.folders.remove(from);
                    g.folders.insert(to, folder);
                }
            });
        }
        self.notify_sidebar(cx);
    }
    pub(super) fn project_groups(&self) -> Groups {
        self.groups
            .get(&self.workspace.project)
            .cloned()
            .unwrap_or_default()
    }
    pub(super) fn update_groups(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Groups),
    ) {
        let project = self.workspace.project.clone();
        let groups = self.groups.entry(project.clone()).or_default();
        change(groups);
        let groups = groups.clone();
        self.store_write("save folders", cx, move |store| {
            store.save_groups(&project, &groups)
        });
        self.invalidate_chrome(cx);
        cx.notify();
    }
    pub(super) fn store_write(
        &mut self,
        what: &'static str,
        cx: &mut Context<Self>,
        write: impl FnOnce(&mut monocode_core::store::Store) -> Result<(), String> + Send + 'static,
    ) {
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx
                .background_executor()
                .spawn(async move { write(&mut *locked(&store)?) });
            if let Err(error) = work.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(format!("Could not {what}: {error}"));
                    cx.notify();
                });
            }
        }));
    }

    /// The project's sidebar entries, reminders first.
    pub(super) fn session_entries(&self, ids: &[i64], cx: &App) -> Vec<Entry> {
        let reminders: Vec<_> = self
            .reminders
            .iter()
            .map(|r| (r.session_id, r.due_at))
            .collect();
        build_list(
            ids,
            |id| self.session_metadata(id, cx).summary.pinned,
            &self.project_groups(),
            &reminders,
        )
    }
    /// The sessions in the order the sidebar shows them, for stepping.
    pub(super) fn sidebar_navigation_ids(&self, cx: &App) -> Vec<i64> {
        let ids = self.sidebar_session_ids(cx);
        let groups = self.project_groups();
        let entries: Vec<_> = self
            .session_entries(&ids, cx)
            .into_iter()
            .filter(|entry| {
                !self.query.is_empty()
                    || match entry {
                        Entry::Folder { id, .. } => groups.folder(id).is_none_or(|f| !f.collapsed),
                        Entry::Pinned { .. } if self.workspace.classic_layout => {
                            !groups.pinned_collapsed
                        }
                        _ => true,
                    }
            })
            .collect();
        navigation_ids(&entries, !self.query.is_empty())
    }

    fn session_menu_ids(&self, id: i64, cx: &App) -> Vec<i64> {
        if !self.selected_sessions.contains(&id) {
            return vec![id];
        }
        self.sidebar_session_ids(cx)
            .into_iter()
            .filter(|id| self.selected_sessions.contains(id))
            .collect()
    }
    pub(super) fn clear_context_selection(&mut self) {
        if std::mem::take(&mut self.context_selection) {
            self.selected_sessions.clear();
        }
    }
    pub(super) fn select_sidebar_session(
        &mut self,
        id: i64,
        modifiers: gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_selection = false;
        self.close_menu(cx);
        self.sidebar_target = Some(RenameTarget::Session(id));
        let additive = modifiers.control || modifiers.platform;
        if modifiers.shift {
            let visible = self.sidebar_navigation_ids(cx);
            let anchor = self
                .selection_anchor
                .filter(|id| visible.contains(id))
                .or(self.workspace.active)
                .unwrap_or(id);
            let range = visible
                .iter()
                .position(|s| *s == anchor)
                .zip(visible.iter().position(|s| *s == id));
            if !additive {
                self.selected_sessions.clear();
            }
            if let Some((a, b)) = range {
                self.selected_sessions.extend(&visible[a.min(b)..=a.max(b)]);
                self.selection_anchor = Some(anchor);
            } else {
                self.selected_sessions.insert(id);
                self.selection_anchor = Some(id);
            }
        } else if additive {
            self.selection_anchor = Some(id);
            if !self.selected_sessions.insert(id) {
                self.selected_sessions.remove(&id);
            }
        } else {
            self.selected_sessions.clear();
            self.selection_anchor = Some(id);
            self.open(id, window, cx);
        }
        if modifiers.shift || additive {
            window.focus(&self.sidebar_focus, cx);
        }
        self.invalidate_chrome(cx);
        cx.notify();
    }
    pub(super) fn session_menu_items(&self, id: i64, cx: &App) -> Vec<MenuItem> {
        let summary = self.session_metadata(id, cx).summary;
        let ids = self.session_menu_ids(id, cx);
        let multiple = ids.len() > 1;
        let groups = self.project_groups();
        let all_pinned = ids
            .iter()
            .all(|id| self.session_metadata(*id, cx).summary.pinned);
        let all_archived = ids
            .iter()
            .all(|id| self.session_metadata(*id, cx).summary.archived);
        let mut items = Vec::new();
        if self.reminders.iter().any(|r| ids.contains(&r.session_id)) {
            items.push(MenuAction::new("reminder:cancel", "Cancel reminder").into());
            items.push(MenuItem::Separator);
        }
        items.push(MenuAction::new("pin", if all_pinned { "Unpin" } else { "Pin" }).into());
        if !multiple {
            items.extend([
                MenuAction::new("rename", "Rename").shortcut("F2").into(),
                MenuAction::new("copy-session-id", "Copy session ID")
                    .submenu(vec![
                        MenuAction::new("copy-harness-session-id", "Harness session ID")
                            .disabled(summary.provider_id.is_none()),
                        MenuAction::new("copy-monocode-session-id", "MonoCode session ID"),
                    ])
                    .into(),
                MenuAction::new(
                    "link-work-item",
                    if summary.work_item.is_some() {
                        "Edit GitHub issue or PR link…"
                    } else {
                        "Link GitHub issue or PR…"
                    },
                )
                .into(),
            ]);
        }
        items.extend([
            MenuAction::new("reminder", "Remind me")
                .submenu(
                    reminder_presets(Local::now().naive_local())
                        .into_iter()
                        .map(|(id, label, enabled)| MenuAction::new(id, label).disabled(!enabled))
                        .collect(),
                )
                .into(),
            MenuItem::Separator,
            MenuAction::new("folder-new", "New folder").into(),
        ]);
        if !groups.folders.is_empty() {
            items.push(MenuItem::Separator);
            items.extend(groups.folders.iter().map(|folder| {
                MenuAction::new(
                    format!("folder-add:{}", folder.id),
                    format!("Add to {}", folder.name),
                )
                .checked(ids.iter().all(|id| folder.session_ids.contains(id)))
                .into()
            }));
        }
        if ids.iter().any(|id| groups.folder_of(*id).is_some()) {
            items.push(
                MenuAction::new(
                    "folder-remove",
                    if multiple {
                        "Remove from folders"
                    } else {
                        "Remove from folder"
                    },
                )
                .into(),
            );
        }
        items.extend([
            MenuItem::Separator,
            MenuAction::new(
                "archive",
                if all_archived { "Unarchive" } else { "Archive" },
            )
            .into(),
            MenuAction::new("delete", "Delete")
                .shortcut("⌫")
                .danger()
                .into(),
        ]);
        items
    }
    pub(super) fn run_session_menu(
        &mut self,
        id: i64,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.session_summaries.contains_key(&id) {
            return;
        }
        let summary = self.session_metadata(id, cx).summary.clone();
        let ids = self.session_menu_ids(id, cx);
        match action {
            "copy-harness-session-id" | "copy-monocode-session-id" => {
                let text = if action == "copy-harness-session-id" {
                    summary.provider_id
                } else {
                    locked(&self.store)
                        .and_then(|store| store.persisted_session_id(summary.id))
                        .ok()
                };
                if let Some(text) = text {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "pin" => {
                let pinned = !ids
                    .iter()
                    .all(|id| self.session_metadata(*id, cx).summary.pinned);
                for id in &ids {
                    if let Some(summary) = self.session_summaries.get_mut(id) {
                        summary.pinned = pinned;
                    }
                    if let Some(view) = self.sessions.get(id) {
                        view.update(cx, |s, _| s.summary.pinned = pinned);
                    }
                }
                self.store_write("pin the session", cx, move |s| s.set_pinned(&ids, pinned));
                self.invalidate_chrome(cx);
            }
            "rename" => self.start_rename(RenameTarget::Session(id), window, cx),
            "link-work-item" => self.open_link_dialog(id, window, cx),
            "reminder:cancel" => {
                let occurrences: Vec<_> = self
                    .reminders
                    .iter()
                    .filter(|reminder| ids.contains(&reminder.session_id))
                    .map(|reminder| (reminder.session_id, reminder.due_at))
                    .collect();
                for (id, due_at) in occurrences {
                    self.clear_notification_reminder(id, due_at, cx);
                }
                self.invalidate_chrome(cx);
            }
            preset if preset.starts_with("reminder:") => {
                let Some(due_at) =
                    reminder_at(preset, Local::now().naive_local()).and_then(local_ms)
                else {
                    return;
                };
                self.reminders.retain(|r| !ids.contains(&r.session_id));
                self.reminders.extend(ids.iter().map(|id| Reminder {
                    session_id: *id,
                    due_at,
                    fired_at: None,
                }));
                let project = self.workspace.project.clone();
                if let Some(groups) = self.groups.get_mut(&project) {
                    groups.reminders_collapsed = false;
                }
                self.update_groups(cx, |_| {});
                self.store_write("set the reminder", cx, move |s| {
                    s.set_reminders(&ids, due_at)
                });
            }
            "folder-new" => {
                let folder = format!("{:x}", monocode_core::time::now_ms() ^ ((id as u64) << 40));
                self.update_groups(cx, |groups| {
                    groups.create_folder(&ids, folder.clone());
                });
                self.start_rename(RenameTarget::Folder(folder), window, cx);
            }
            "folder-remove" => self.update_groups(cx, |groups| groups.remove_sessions(&ids)),
            add if add.starts_with("folder-add:") => {
                let folder = add["folder-add:".len()..].to_string();
                self.update_groups(cx, |groups| {
                    for id in &ids {
                        groups.add_to_folder(&folder, *id);
                    }
                });
            }
            "archive" => {
                let archived = !ids
                    .iter()
                    .all(|id| self.session_metadata(*id, cx).summary.archived);
                for id in ids {
                    if self.session_metadata(id, cx).summary.archived != archived {
                        self.toggle_archive(id, cx);
                    }
                }
            }
            "delete" => {
                for id in ids {
                    self.delete_session(id, window, cx);
                }
            }
            _ => {}
        }
    }
    pub(super) fn folder_menu_items(
        &self,
        folder: &str,
        events: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> Vec<MenuItem> {
        let selected = self
            .project_groups()
            .folder(folder)
            .and_then(|f| f.color_index)
            .unwrap_or(0);
        let swatches = cx.new(|cx| FolderSwatches {
            focus: cx.focus_handle(),
            selected,
        });
        let id = folder.to_string();
        events.push(
            cx.subscribe(&swatches, move |this, _, SwatchPicked(color), cx| {
                let color = *color;
                this.update_groups(cx, |groups| groups.set_color(&id, color));
            }),
        );
        let focus = swatches.read(cx).focus.clone();
        vec![
            MenuItem::Custom(swatches.into(), focus),
            MenuAction::new("rename", "Rename").shortcut("F2").into(),
            MenuItem::Separator,
            MenuAction::new("ungroup", "Ungroup").into(),
        ]
    }
    pub(super) fn run_folder_menu(
        &mut self,
        folder: String,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            "rename" => self.start_rename(RenameTarget::Folder(folder), window, cx),
            "ungroup" => self.update_groups(cx, |groups| groups.dissolve(&folder)),
            _ => {}
        }
    }

    /// Opening a session whose reminder is due dismisses it.
    pub(super) fn dismiss_due_reminder(&mut self, id: i64, cx: &mut Context<Self>) {
        let now = monocode_core::time::now_ms();
        let Some(due) = self
            .reminders
            .iter()
            .find(|r| r.session_id == id && r.due_at <= now)
            .map(|r| r.due_at)
        else {
            return;
        };
        self.clear_notification_reminder(id, due, cx);
    }

    pub(super) fn poll_reminders(&mut self, cx: &mut Context<Self>) {
        if self.notification_state.reminder_error.is_some() {
            return;
        }
        let now = monocode_core::time::now_ms();
        let due: Vec<_> = self
            .reminders
            .iter_mut()
            .filter_map(|reminder| {
                if reminder.due_at > now || reminder.fired_at.is_some() {
                    return None;
                }
                reminder.fired_at = Some(now);
                Some((reminder.session_id, reminder.due_at))
            })
            .collect();
        if due.is_empty() {
            return;
        }
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx
                .background_executor()
                .spawn(async move { locked(&store)?.take_due_reminders(now) });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(claimed) => {
                        this.notification_state.reminder_error = None;
                        for reminder in claimed {
                            // A cancel/reschedule while the store operation was
                            // pending must never deliver an obsolete occurrence.
                            if this.reminders.iter().any(|current| {
                                current.session_id == reminder.session_id
                                    && current.due_at == reminder.due_at
                            }) {
                                this.deliver_reminder_notice(
                                    reminder.session_id,
                                    reminder.due_at,
                                    cx,
                                );
                            }
                        }
                    }
                    Err(error) => {
                        // Re-arm only the occurrences claimed by this request;
                        // a newer schedule has its own independent fired state.
                        for reminder in &mut this.reminders {
                            if due.contains(&(reminder.session_id, reminder.due_at))
                                && reminder.fired_at == Some(now)
                            {
                                reminder.fired_at = None;
                            }
                        }
                        this.notification_state.reminder_error =
                            Some(format!("Could not deliver reminders: {error}"));
                    }
                }
                this.invalidate_chrome(cx);
                cx.notify();
            });
        }));
        self.invalidate_chrome(cx);
    }

    pub(super) fn delete_session(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.review_owners.iter().any(|(pane, owner)| {
            *owner == id
                && (self.workspace.tab_of(*pane).is_some()
                    || self
                        .reviews
                        .get(pane)
                        .is_some_and(|r| r.read(cx).unsaved_files(cx)))
        }) {
            self.error = Some("Close this session's detached file panes before deleting it".into());
            cx.notify();
            return;
        }
        if !self.session_summaries.contains_key(&id) || !self.deleting.insert(id) {
            return;
        }
        self.close_preview(cx);
        if let Some(view) = self.sessions.get(&id) {
            view.update(cx, |session, cx| session.stop_if_busy(cx));
        }
        if self.workspace.tab_of(id).is_some() {
            self.close_pane(id, window, cx);
        }
        if self.workspace.tab_of(id).is_some() {
            self.deleting.remove(&id);
            self.error = Some("Could not delete a session that is still open".into());
            cx.notify();
            return;
        }
        self.invalidate_chrome(cx);
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx.background_executor().spawn(async move {
                locked(&store)?.delete(id)?;
                // Drop the session's recorded turns and their Git refs.
                if let Some(checkpoints) = monocode_core::checkpoint::store() {
                    let _ = checkpoints.exclusive(|c| c.forget(&id.to_string()));
                }
                Ok::<_, String>(())
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.deleting.remove(&id);
                match result {
                    Ok(()) => this.forget_session(id, cx),
                    Err(error) => {
                        this.error = Some(format!("Could not delete the session: {error}"))
                    }
                }
                this.invalidate_chrome(cx);
                this.sync_settings(cx);
                cx.notify();
            });
        }));
    }
    fn forget_session(&mut self, id: i64, cx: &mut Context<Self>) {
        // A tab opened on it while the store was deleting keeps it alive.
        if self.workspace.tab_of(id).is_some() {
            return;
        }
        if self
            .session_preview
            .as_ref()
            .is_some_and(|preview| preview.id == id)
        {
            self.session_preview = None;
            self.preview_close = None;
        }
        self.sessions.remove(&id);
        self.session_summaries.remove(&id);
        self.draft_sessions.remove(&id);
        self.history_recency.remove(&id);
        self.session_marks.remove(&id);
        self.tab_marks.remove(&id);
        self.order.retain(|s| *s != id);
        self.busy_sessions.remove(&id);
        self.unseen_finished.remove(&id);
        self.selected_sessions.remove(&id);
        self.reminders.retain(|r| r.session_id != id);
        self.workspace.project_focus.retain(|_, s| *s != id);
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        if self.session_hover == Some(id) {
            self.session_hover = None;
        }
        if self
            .renaming
            .as_ref()
            .is_some_and(|r| r.target == RenameTarget::Session(id))
        {
            self.renaming = None;
        }
        let projects: Vec<String> = self
            .groups
            .iter()
            .filter(|(_, g)| g.folder_of(id).is_some())
            .map(|(p, _)| p.clone())
            .collect();
        for project in projects {
            let groups = self.groups.get_mut(&project).unwrap();
            groups.remove_sessions(&[id]);
            let groups = groups.clone();
            self.store_write("save folders", cx, move |s| {
                s.save_groups(&project, &groups)
            });
        }
        self.persist(cx);
    }

    fn start_rename(&mut self, target: RenameTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview(cx);
        let text = match &target {
            RenameTarget::Session(id) => self.session_metadata(*id, cx).display_title(),
            RenameTarget::Folder(folder) => self
                .project_groups()
                .folder(folder)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
        };
        let input = cx.new(|cx| {
            let mut input = input::Input::field(
                "Rename. Press Enter to confirm or Escape to cancel.",
                "",
                content(0.35),
                cx,
            );
            input.set_font_size(if self.workspace.classic_layout {
                13.
            } else {
                14.
            });
            input
        });
        let len = text.len();
        input.update(cx, |i, cx| {
            i.set_text(text, cx);
            i.select_range(0..len, cx);
        });
        let focus = input.read(cx).focus_handle(cx);
        let events = vec![
            cx.subscribe_in(
                &input,
                window,
                |this, _, _: &input::Submitted, window, cx| this.finish_rename(true, window, cx),
            ),
            cx.on_blur(&focus, window, |this, window, cx| {
                this.finish_rename(true, window, cx)
            }),
        ];
        window.focus(&focus, cx);
        self.renaming = Some(Renaming {
            target,
            input,
            _events: events,
        });
        self.invalidate_chrome(cx);
        cx.notify();
    }
    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let name = renaming.input.read(cx).content.trim().to_string();
        if commit && !name.is_empty() {
            match renaming.target {
                RenameTarget::Session(id) => {
                    if self.session_summaries.contains_key(&id)
                        && self.session_metadata(id, cx).summary.title != name
                    {
                        if let Some(summary) = self.session_summaries.get_mut(&id) {
                            summary.title = name.clone();
                        }
                        if let Some(session) = self.sessions.get(&id) {
                            session.update(cx, |s, cx| {
                                s.rename(name.clone(), cx);
                            });
                        }
                        self.store_write("rename the session", cx, move |s| s.title(id, &name));
                    }
                }
                RenameTarget::Folder(folder) => {
                    self.update_groups(cx, |groups| groups.rename_folder(&folder, &name))
                }
            }
        }
        if renaming.input.read(cx).focus_handle(cx).is_focused(window) {
            self.focus_active(window, cx);
        }
        self.invalidate_chrome(cx);
        cx.notify();
    }
    /// SessionRenameRow and FolderRenameRow: the name in a focused field.
    pub(super) fn rename_row(
        &self,
        target: &RenameTarget,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let renaming = self.renaming.as_ref().filter(|r| r.target == *target)?;
        if self.workspace.classic_layout {
            let folder = match target {
                RenameTarget::Folder(id) => self.project_groups().folder(id).cloned(),
                _ => None,
            };
            let mut outline = crate::theme::accent_color();
            outline.a = 0.4;
            let input = div()
                .flex_1()
                .min_w_0()
                .h(px(if folder.is_some() { 21.875 } else { 25.875 }))
                .px_2()
                .flex()
                .items_center()
                .rounded_sm()
                .bg(content(0.1))
                .border_1()
                .border_color(outline)
                .text_size(px(13.))
                .line_height(px(17.875))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(content(1.))
                .child(renaming.input.clone());
            let row = div()
                .key_context("SessionRename")
                .on_action(cx.listener(|this, _: &CancelRename, window, cx| {
                    this.finish_rename(false, window, cx)
                }))
                .w_full()
                .flex()
                .items_center()
                .gap_1p5()
                .text_color(content(1.));
            return Some(if let Some(folder) = folder {
                row.px_2()
                    .py(px(6.))
                    .rounded_t_md()
                    .bg(self.classic_group_fill(&Entry::Folder {
                        id: folder.id.clone(),
                        sessions: Vec::new(),
                    }))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("chevron-down", 14.).text_color(content(0.5))),
                    )
                    .child(input)
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .line_height(px(16.5))
                            .text_color(content(0.45))
                            .child(folder.session_ids.len().to_string()),
                    )
                    .into_any_element()
            } else {
                let (active, approval) = match target {
                    RenameTarget::Session(id) => (
                        self.workspace.active == Some(*id),
                        self.session_metadata(*id, cx).needs_approval(),
                    ),
                    _ => (false, false),
                };
                row.px(px(10.))
                    .py_2()
                    .rounded_md()
                    .when(active, |d| d.bg(content(0.1)))
                    .when(approval, |d| {
                        d.bg(gpui::Hsla::from(rgb(0xfbbf24)).opacity(0.1))
                    })
                    .child(input)
                    .into_any_element()
            });
        }
        Some(
            div()
                .key_context("SessionRename")
                .on_action(cx.listener(|this, _: &CancelRename, window, cx| {
                    this.finish_rename(false, window, cx)
                }))
                .h(px(32.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(16.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(matches!(target, RenameTarget::Folder(_)), |d| {
                            d.child(icon("chevron-down", 14.))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(22.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .bg(content(0.1))
                        .border_1()
                        .border_color(rgba(0x4da3f566))
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(1.))
                        .child(renaming.input.clone()),
                )
                .into_any(),
        )
    }

    /// SessionCard's linked issue or PR, opened on GitHub. Hover underlines
    /// it as a link; with no number shown, the line runs under the icon.
    pub(super) fn work_item_badge(
        &self,
        id: i64,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let item = self.session_metadata(id, cx).summary.work_item.clone()?;
        let noun = if item.kind == "pr" { "PR" } else { "issue" };
        let url = item.url.clone();
        Some(
            div()
                .id(("work-item", id as usize))
                .role(Role::Link)
                .aria_label(format!("Open {noun} #{}", item.number))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(2.))
                .px(px(2.))
                .rounded_sm()
                .text_size(px(11.))
                .text_color(rgb(ACCENT))
                .child(
                    icon(
                        if item.kind == "pr" {
                            "git-pull-request"
                        } else {
                            "circle-dot"
                        },
                        12.,
                    )
                    .text_color(rgb(ACCENT)),
                )
                .when(!compact, |d| d.child(format!("#{}", item.number)))
                .cursor_pointer()
                .when(compact, |d| {
                    d.border_b_1()
                        .border_color(gpui::transparent_black())
                        .hover(|s| s.border_color(rgb(ACCENT)))
                })
                .when(!compact, |d| d.hover(|s| s.underline()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Middle, {
                    let url = url.clone();
                    move |_, _, cx| {
                        cx.stop_propagation();
                        cx.open_url(&url);
                    }
                })
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    cx.open_url(&url);
                })
                .into_any(),
        )
    }

    /// A virtualized group renders its header separately from its member cards.
    pub(super) fn sidebar_group_header(&self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let searching = !self.query.is_empty();
        let folders = self.project_groups();
        let (key, name, glyph, color, collapsed, sessions): (
            String,
            String,
            &'static str,
            Option<gpui::Hsla>,
            bool,
            &Vec<i64>,
        ) = match entry {
            Entry::Reminders {
                collapsed,
                sessions,
            } => (
                "reminders".into(),
                "Reminders".into(),
                "clock",
                Some(rgb(REMINDERS_COLOR).into()),
                *collapsed,
                sessions,
            ),
            Entry::Folder { id, sessions } => {
                let Some(folder) = folders.folder(id) else {
                    return div().into_any();
                };
                (
                    format!("folder:{id}"),
                    folder.name.clone(),
                    "folder",
                    folder
                        .custom_color
                        .as_deref()
                        .and_then(|color| color.strip_prefix('#'))
                        .and_then(|color| u32::from_str_radix(color, 16).ok())
                        .map(|color| rgb(color).into())
                        .or_else(|| folder.color_index.and_then(folder_color)),
                    folder.collapsed,
                    sessions,
                )
            }
            Entry::Pinned { sessions } if self.workspace.classic_layout => (
                "pinned".into(),
                "Pinned".into(),
                "pin",
                None,
                folders.pinned_collapsed,
                sessions,
            ),
            Entry::Pinned { .. } | Entry::Session(_) => return div().into_any(),
        };
        let expanded = searching || !collapsed;
        let folder_id = key.strip_prefix("folder:").map(str::to_string);
        folder_id
            .as_ref()
            .and_then(|f| self.rename_row(&RenameTarget::Folder(f.clone()), cx))
            .unwrap_or_else(|| self.group_row(&key, name, glyph, color, expanded, sessions, cx))
    }
    #[allow(clippy::too_many_arguments)]
    fn group_row(
        &self,
        key: &str,
        name: String,
        glyph: &'static str,
        color: Option<gpui::Hsla>,
        expanded: bool,
        sessions: &[i64],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.classic_layout {
            return self.classic_group_row(key, name, glyph, color, expanded, sessions, cx);
        }
        let approval = sessions
            .iter()
            .any(|id| self.session_metadata(*id, cx).needs_approval());
        let done = sessions.iter().any(|id| self.unseen_finished.contains(id));
        let group = SharedString::from(format!("group-row-{key}"));
        let toggle_key = key.to_string();
        let menu_key = key.to_string();
        let drag_key = key.strip_prefix("folder:").map(str::to_string);
        let bounds = drag_key
            .as_ref()
            .and_then(|id| self.folder_bounds.get(id))
            .cloned();
        let drop_target = drag_key.as_ref().is_some_and(|id| {
            self.sidebar_drop == Some(ListDrop::Folder(id.clone()))
                || self
                    .folder_drag
                    .as_ref()
                    .is_some_and(|drag| drag.active && drag.over.as_ref() == Some(id))
        });
        let mark = |side| {
            let glyph = icon(glyph, side);
            match color {
                Some(color) => glyph.text_color(color),
                None => glyph,
            }
        };
        div()
            .id(SharedString::from(format!("session-group-{key}")))
            .debug_selector({
                let key = key.to_string();
                move || format!("session-group-{key}")
            })
            .group(group.clone())
            .role(Role::Button)
            .aria_label(name.clone())
            .aria_expanded(expanded)
            .cursor_pointer()
            .h(px(32.))
            .px_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .text_color(content(0.5))
            .when(expanded, |d| d.bg(content(0.05)).text_color(content(0.8)))
            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
            .when(drop_target, |d| d.bg(content(0.14)))
            .when(
                drag_key.as_ref().is_some_and(|id| {
                    self.folder_drag
                        .as_ref()
                        .is_some_and(|drag| drag.active && &drag.id == id)
                }),
                |d| d.opacity(0.4),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.7)
                    .child(
                        div()
                            .when(expanded, |d| d.invisible())
                            .group_hover(group.clone(), |s| s.invisible())
                            .child(mark(16.)),
                    )
                    .child(
                        div()
                            .absolute()
                            .invisible()
                            .when(expanded, |d| d.visible())
                            .group_hover(group.clone(), |s| s.visible())
                            .child(icon(
                                if expanded {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                16.,
                            )),
                    )
                    .relative(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(name),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .when(!expanded && approval, |d| {
                        d.child(icon("circle-alert", 12.).text_color(rgb(0xfbbf24)))
                    })
                    .when(!expanded && !approval && done, |d| {
                        d.child(icon("check-circle-2", 12.).text_color(rgb(0x2dd4bf)))
                    })
                    .child(sessions.len().to_string()),
            )
            .children(bounds.map(|slot| slot.probe().into_any_element()))
            .relative()
            .when_some(drag_key, |d, id| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, _| {
                        this.skip_folder_click = false;
                        this.folder_drag = Some(FolderDrag {
                            id: id.clone(),
                            start: event.position,
                            active: false,
                            over: None,
                        });
                    }),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if std::mem::take(&mut this.skip_folder_click) {
                    return;
                }
                this.sidebar_target = toggle_key
                    .strip_prefix("folder:")
                    .map(|id| RenameTarget::Folder(id.into()));
                window.focus(&this.sidebar_focus, cx);
                if !this.query.is_empty() {
                    return;
                }
                let key = toggle_key.clone();
                this.update_groups(cx, |groups| match key.as_str() {
                    "reminders" => groups.reminders_collapsed = !groups.reminders_collapsed,
                    folder => groups.toggle_folder(folder.trim_start_matches("folder:")),
                });
            }))
            .when(key.starts_with("folder:"), |d| {
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let folder = menu_key.trim_start_matches("folder:").to_string();
                        let anchor = Anchor::Point(event.position);
                        this.open_menu(MenuTarget::Folder(folder), anchor, window, cx);
                    }),
                )
            })
            .into_any()
    }

    /// Original FolderRow and its enclosing tinted shell. The count and label
    /// retain their original alignment in collapsed and expanded states.
    #[allow(clippy::too_many_arguments)]
    fn classic_group_row(
        &self,
        key: &str,
        name: String,
        glyph: &'static str,
        color: Option<gpui::Hsla>,
        expanded: bool,
        sessions: &[i64],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let approval = sessions
            .iter()
            .any(|id| self.session_metadata(*id, cx).needs_approval());
        let busy = sessions
            .iter()
            .any(|id| self.session_metadata(*id, cx).responding());
        let busy_mark = sessions
            .iter()
            .find(|id| self.session_metadata(**id, cx).responding())
            .and_then(|id| self.session_marks.get(id))
            .cloned();
        let done = sessions.iter().any(|id| self.unseen_finished.contains(id));
        let group = SharedString::from(format!("classic-group-row-{key}"));
        let toggle_key = key.to_owned();
        let menu_key = key.to_owned();
        let folder_id = key.strip_prefix("folder:").map(str::to_owned);
        let bounds = folder_id
            .as_ref()
            .and_then(|id| self.folder_bounds.get(id))
            .cloned();
        let drop_target = folder_id.as_ref().is_some_and(|id| {
            self.sidebar_drop == Some(ListDrop::Folder(id.clone()))
                || self
                    .folder_drag
                    .as_ref()
                    .is_some_and(|drag| drag.active && drag.over.as_ref() == Some(id))
        });
        let mut fill = color.unwrap_or_else(|| content(0.05));
        if color.is_some() {
            fill.a = 0.18;
        }
        div()
            .id(SharedString::from(format!("session-group-{key}")))
            .debug_selector({
                let key = key.to_owned();
                move || format!("session-group-{key}")
            })
            .group(group.clone())
            .role(Role::Button)
            .aria_label(name.clone())
            .aria_expanded(expanded)
            .cursor_pointer()
            .relative()
            .h(px(32.))
            .px_2()
            .rounded_t_md()
            .when(!expanded, |d| d.rounded_b_md())
            .bg(fill)
            .flex()
            .items_center()
            .gap_1p5()
            .text_color(content(if expanded { 1. } else { 0.8 }))
            .hover(|s| s.bg(fill.blend(content(0.1))).text_color(content(1.)))
            .when(drop_target, |d| {
                let mut accent = crate::theme::accent_color();
                accent.a = 0.2;
                d.bg(fill.blend(accent))
            })
            .when(
                folder_id.as_ref().is_some_and(|id| {
                    self.folder_drag
                        .as_ref()
                        .is_some_and(|drag| drag.active && &drag.id == id)
                }),
                |d| d.opacity(0.4),
            )
            .child(
                div()
                    .size(px(16.))
                    .flex_shrink_0()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .when(expanded && glyph == "folder", |d| d.invisible())
                            .group_hover(group.clone(), |s| s.invisible())
                            .child(icon(glyph, 14.).text_color(content(1.))),
                    )
                    .child(
                        div()
                            .absolute()
                            .invisible()
                            .when(expanded && glyph == "folder", |d| d.visible())
                            .group_hover(group.clone(), |s| s.visible())
                            .child(
                                icon(
                                    if expanded {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    14.,
                                )
                                .text_color(content(1.)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .line_height(px(17.875))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(1.))
                    .child(name),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .line_height(px(16.5))
                    .text_color(content(0.45))
                    .when(!expanded && approval, |d| {
                        d.child(icon("circle-alert", 12.).text_color(rgb(0xfbbf24)))
                    })
                    .when(!expanded && !approval && busy, |d| d.children(busy_mark))
                    .when(!expanded && !approval && !busy && done, |d| {
                        d.child(icon("check", 12.).text_color(rgb(0x34d399)))
                    })
                    .child(sessions.len().to_string()),
            )
            .children(bounds.map(|slot| slot.probe().into_any_element()))
            .when_some(folder_id, |d, id| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, _| {
                        this.skip_folder_click = false;
                        this.folder_drag = Some(FolderDrag {
                            id: id.clone(),
                            start: event.position,
                            active: false,
                            over: None,
                        });
                    }),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if std::mem::take(&mut this.skip_folder_click) {
                    return;
                }
                this.sidebar_target = toggle_key
                    .strip_prefix("folder:")
                    .map(|id| RenameTarget::Folder(id.into()));
                window.focus(&this.sidebar_focus, cx);
                if !this.query.is_empty() {
                    return;
                }
                this.update_groups(cx, |groups| match toggle_key.as_str() {
                    "pinned" => groups.pinned_collapsed = !groups.pinned_collapsed,
                    "reminders" => groups.reminders_collapsed = !groups.reminders_collapsed,
                    folder => groups.toggle_folder(folder.trim_start_matches("folder:")),
                });
            }))
            .when(key.starts_with("folder:"), |d| {
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.open_menu(
                            MenuTarget::Folder(menu_key.trim_start_matches("folder:").to_owned()),
                            Anchor::Point(event.position),
                            window,
                            cx,
                        );
                    }),
                )
            })
            .into_any_element()
    }

    fn classic_group_fill(&self, entry: &Entry) -> gpui::Hsla {
        let color = match entry {
            Entry::Reminders { .. } => Some(rgb(REMINDERS_COLOR).into()),
            Entry::Folder { id, .. } => self.project_groups().folder(id).and_then(|folder| {
                folder
                    .custom_color
                    .as_deref()
                    .and_then(|color| color.strip_prefix('#'))
                    .and_then(|color| u32::from_str_radix(color, 16).ok())
                    .map(|color| rgb(color).into())
                    .or_else(|| folder.color_index.and_then(folder_color))
            }),
            _ => None,
        };
        color
            .map(|mut color| {
                color.a = 0.18;
                color
            })
            .unwrap_or_else(|| content(0.05))
    }

    pub(super) fn classic_group_member(
        &self,
        entry: &Entry,
        id: i64,
        first: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folder = matches!(entry, Entry::Folder { .. });
        div()
            .bg(self.classic_group_fill(entry))
            .px_1()
            .when(first, |d| d.pt_1())
            .when(last && !folder, |d| d.pb_1().rounded_b_md())
            .when(!last, |d| d.pb_px())
            .child(self.classic_session_card_at_depth(id, false, true, cx))
            .into_any_element()
    }

    pub(super) fn classic_folder_new_session(
        &self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = Entry::Folder {
            id: id.to_owned(),
            sessions: Vec::new(),
        };
        let folder = id.to_owned();
        div()
            .bg(self.classic_group_fill(&entry))
            .pt_1()
            .rounded_b_md()
            .child(
                div().border_t_1().border_color(content(0.08)).p_1().child(
                    div()
                        .id(SharedString::from(format!("new-session-in-folder-{id}")))
                        .role(Role::Button)
                        .aria_label("New session")
                        .cursor_pointer()
                        .border_1()
                        .border_color(content(0.))
                        .rounded_md()
                        .px(px(10.))
                        .py(px(6.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(13.))
                        .line_height(px(17.875))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(content(0.45))
                        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                        .child(icon("plus", 12.))
                        .child("New session")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.create_in_folder(folder.clone(), cx);
                        })),
                ),
            )
            .into_any_element()
    }

    fn open_link_dialog(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.close_preview(cx);
        let summary = self.session_metadata(id, cx).summary;
        let (cwd, current) = (summary.cwd.clone(), summary.work_item.clone());
        let picker = cx.new(|cx| WorkItemPicker::new(cwd.into(), current, cx));
        let events = cx.subscribe_in(
            &picker,
            window,
            move |this, _, event, window, cx| match event {
                Picked::Item(item) => this.save_link(Some(item.clone()), window, cx),
                Picked::Remove => this.save_link(None, window, cx),
                Picked::Dismissed => this.close_link_dialog(window, cx),
            },
        );
        window.focus(&picker.read(cx).focus(cx), cx);
        self.link_dialog = Some(LinkDialog {
            id,
            picker,
            _events: events,
        });
        cx.notify();
    }
    pub(super) fn close_link_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.link_dialog.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
        }
    }
    /// Links the session to `item`, or with None, removes its link.
    fn save_link(&mut self, item: Option<WorkItem>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &self.link_dialog else {
            return;
        };
        let id = dialog.id;
        if let Some(summary) = self.session_summaries.get_mut(&id) {
            summary.work_item = item.clone();
        }
        if let Some(session) = self.sessions.get(&id) {
            session.update(cx, |s, _| s.summary.work_item = item.clone());
        }
        self.store_write("link the session", cx, move |s| {
            s.set_work_item(id, item.as_ref())
        });
        self.invalidate_chrome(cx);
        self.close_link_dialog(window, cx);
    }
    /// LinkSessionWorkItemDialog as a searchable picker, centred over the window.
    pub(super) fn link_modal(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.link_dialog.as_ref()?;
        let width = 440f32.min(window.viewport_size().width.as_f32() - 24.);
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("link-work-item-scrim"))
                .child(
                    popover("link-work-item-modal", Anchor::Center)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .priority(60)
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| this.close_link_dialog(window, cx));
                        })
                        .child(dialog.picker.clone()),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, WindowHandle};

    fn setup(cx: &mut TestAppContext) -> (WindowHandle<Shell>, i64, i64) {
        cx.update(|cx| {
            crate::shell::bind_keys(cx);
            crate::input::bind(cx);
            crate::popover::bind(cx);
        });
        let (window, a, b) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                for id in [a, b] {
                    shell.sessions[&id].update(cx, |session, _| {
                        session.summary.cwd = shell.workspace.project.clone();
                        session.summary.has_messages = true;
                    });
                }
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        (window, a, b)
    }

    #[gpui::test]
    fn expanded_folders_use_compact_connected_rows_and_keep_operator_children(
        cx: &mut TestAppContext,
    ) {
        let (window, a, b) = setup(cx);
        let (plain, child) = window
            .update(cx, |shell, _, cx| {
                let mut ids = Vec::new();
                for _ in 0..2 {
                    let mut summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", &shell.workspace.project)
                        .unwrap();
                    summary.has_messages = true;
                    ids.push(summary.id);
                    shell.add(summary, cx);
                    shell.sessions[ids.last().unwrap()]
                        .update(cx, |chat, _| chat.summary.has_messages = true);
                }
                let (plain, child) = (ids[0], ids[1]);
                shell.groups.insert(
                    shell.workspace.project.clone(),
                    Groups {
                        folders: vec![monocode_core::session_groups::Folder {
                            id: "visual-folder".into(),
                            name: "Review work".into(),
                            session_ids: vec![a, b],
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                );
                shell.started_by.insert(child, a);
                shell.workspace.active = Some(a);
                shell.invalidate_chrome(cx);
                (plain, child)
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let row = |cx: &mut VisualTestContext, id| {
            cx.debug_bounds(Box::leak(format!("sidebar-session-{id}").into_boxed_str()))
        };
        let header = visual
            .debug_bounds("session-group-folder:visual-folder")
            .unwrap();
        let first = row(visual, a).unwrap();
        let second = row(visual, b).unwrap();
        assert_eq!(first.left() - header.left(), px(30.));
        assert_eq!(first.size.height, px(28.));
        assert_eq!(second.left(), first.left());
        assert_eq!(second.size.height, px(28.));
        assert_eq!(row(visual, plain).unwrap().size.height, px(32.));
        let started = row(visual, child).expect("a folder member keeps its Operator rail");
        assert_eq!(started.left() - first.left(), px(30.));
        window
            .update(visual, |shell, _, cx| {
                shell.change_sidebar_option("compact-session-cards", cx);
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(row(visual, a).unwrap().size.height, px(28.));
        assert_eq!(row(visual, b).unwrap().size.height, px(28.));
        assert_eq!(row(visual, child).unwrap().size.height, px(28.));
        assert_eq!(row(visual, plain).unwrap().size.height, px(60.));
        window
            .update(visual, |_, window, _| {
                let scale = window.scale_factor();
                assert!(
                    window.painted_quads().iter().any(|quad| {
                        (quad.bounds.left().0 - f32::from(header.left()) * scale).abs() < 1.
                            && (quad.bounds.top().0 - f32::from(header.top()) * scale).abs() < 1.
                            && (quad.bounds.size.width.0 - f32::from(header.size.width) * scale)
                                .abs()
                                < 1.
                            && quad
                                .background
                                .as_solid()
                                .is_some_and(|color| (color.a - 0.05).abs() < 0.001)
                    }),
                    "an open folder has a persistent header fill"
                );
                assert!(
                    window.painted_quads().iter().any(|quad| {
                        (quad.bounds.left().0 - f32::from(header.left() + px(16.)) * scale).abs()
                            < 1.
                            && (quad.bounds.size.width.0 - scale).abs() < 0.1
                            && quad.bounds.top().0 > f32::from(header.bottom()) * scale
                            && quad.bounds.top().0 < f32::from(first.bottom()) * scale
                            && quad
                                .background
                                .as_solid()
                                .is_some_and(|color| color == crate::theme::rail_color())
                    }),
                    "the same spine as Operator rows connects folder members"
                );
            })
            .unwrap();
        window
            .update(visual, |shell, _, cx| {
                shell.update_groups(cx, |groups| groups.toggle_folder("visual-folder"))
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(row(visual, a).is_none());
        assert!(row(visual, b).is_none());
        assert!(row(visual, child).is_none());
        window
            .update(visual, |shell, _, cx| {
                shell.query = "new".into();
                shell.notify_sidebar(cx);
            })
            .unwrap();
        visual.refresh().unwrap();
        assert_eq!(
            row(visual, a).unwrap().size.height,
            px(28.),
            "search opens a collapsed folder with the same compact layout"
        );
    }

    #[gpui::test]
    fn sidebar_header_controls_share_the_trailing_alignment(cx: &mut TestAppContext) {
        let (window, _, _) = setup(cx);
        // The header's plus appears once this project has a Mono. The empty
        // state uses a full-width creation row instead.
        window
            .update(cx, |shell, _, cx| {
                if let Some(monos) = &shell.monos {
                    monos.update(cx, |monos, cx| {
                        monos.roster.monos.push(monocode_core::mono::Mono {
                            projects: vec![shell.workspace.project.clone()],
                            ..Default::default()
                        });
                        cx.notify();
                    });
                }
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let options = visual.debug_bounds("sidebar-options").unwrap();
        let mono = visual.debug_bounds("sidebar-mono-button").unwrap();
        let filter = visual.debug_bounds("sidebar-filter-button").unwrap();
        assert!((options.center().x - mono.center().x).abs() <= px(1.));
        assert!((options.center().x - filter.center().x).abs() <= px(1.));
        let project = visual.debug_bounds("sidebar-project-button").unwrap();
        let search = visual.debug_bounds("sidebar-search-field").unwrap();
        assert_eq!(project.left(), search.left());
        assert_eq!(project.size.height, px(28.));
        assert!(project.right() < options.left());
    }

    #[gpui::test]
    fn session_actions_are_enabled_and_bulk_selection_shapes_the_menu(cx: &mut TestAppContext) {
        let (window, a, b) = setup(cx);
        window
            .update(cx, |shell, w, cx| {
                let items = shell.session_menu_items(a, cx);
                for item in items {
                    if let MenuItem::Action(action) = item {
                        assert!(!action.disabled, "{} must be usable", action.label);
                    }
                }
                shell.select_sidebar_session(
                    a,
                    gpui::Modifiers {
                        platform: true,
                        ..Default::default()
                    },
                    w,
                    cx,
                );
                shell.select_sidebar_session(
                    b,
                    gpui::Modifiers {
                        shift: true,
                        ..Default::default()
                    },
                    w,
                    cx,
                );
                assert_eq!(shell.selected_sessions.len(), 2);
                let actions: Vec<_> = shell
                    .session_menu_items(a, cx)
                    .into_iter()
                    .filter_map(|item| match item {
                        MenuItem::Action(action) => Some(action.id),
                        _ => None,
                    })
                    .collect();
                assert!(
                    !actions.iter().any(|id| id == "rename"
                        || id == "copy-session-id"
                        || id == "link-work-item")
                );
                shell.run_menu(MenuTarget::Session(a), "pin", None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions[&a].read(cx).summary.pinned);
                assert!(shell.sessions[&b].read(cx).summary.pinned);
                let stored = locked(&shell.store).unwrap().sessions().unwrap();
                assert!(stored.iter().all(|s| s.pinned));
            })
            .unwrap();
    }

    #[gpui::test]
    fn folders_rename_move_collapse_color_and_ungroup_with_saved_state(cx: &mut TestAppContext) {
        let (window, a, b) = setup(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.run_menu(MenuTarget::Session(a), "folder-new", None, w, cx)
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, cx| {
                let renaming = shell.renaming.as_ref().unwrap();
                renaming
                    .input
                    .update(cx, |input, cx| input.set_text("Work".into(), cx));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        let folder = window
            .update(cx, |shell, w, cx| {
                assert!(shell.renaming.is_none());
                let folder = shell.project_groups().folder_of(a).unwrap().id.clone();
                shell.run_menu(
                    MenuTarget::Session(b),
                    &format!("folder-add:{folder}"),
                    None,
                    w,
                    cx,
                );
                shell.update_groups(cx, |g| {
                    g.toggle_folder(&folder);
                    g.set_color(&folder, Some(3));
                });
                assert!(shell.sidebar_navigation_ids(cx).is_empty());
                shell.query = "new".into();
                assert_eq!(
                    shell.sidebar_navigation_ids(cx).len(),
                    2,
                    "search opens folded groups"
                );
                shell.query.clear();
                folder
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                let saved = locked(&shell.store)
                    .unwrap()
                    .groups(&shell.workspace.project)
                    .unwrap();
                let saved = saved.folder(&folder).unwrap();
                assert_eq!(saved.name, "Work");
                assert_eq!(saved.color_index, Some(3));
                assert!(saved.collapsed);
                assert!(saved.session_ids.contains(&a) && saved.session_ids.contains(&b));
                shell.run_menu(MenuTarget::Session(b), "folder-remove", None, w, cx);
                assert_eq!(
                    shell.project_groups().folder(&folder).unwrap().session_ids,
                    [a]
                );
                shell.run_menu(MenuTarget::Folder(folder.clone()), "ungroup", None, w, cx);
                assert_eq!(shell.sidebar_navigation_ids(cx).len(), 2);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(
                    locked(&shell.store)
                        .unwrap()
                        .groups(&shell.workspace.project)
                        .unwrap()
                        .folders
                        .is_empty()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn session_rename_accepts_enter_and_escape_cancels(cx: &mut TestAppContext) {
        let (window, a, _) = setup(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.run_menu(MenuTarget::Session(a), "rename", None, w, cx);
                shell
                    .renaming
                    .as_ref()
                    .unwrap()
                    .input
                    .update(cx, |i, cx| i.set_text("First name".into(), cx));
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.sessions[&a].read(cx).summary.title, "First name");
                let saved = locked(&shell.store).unwrap().sessions().unwrap();
                assert_eq!(
                    saved.iter().find(|s| s.id == a).unwrap().title,
                    "First name"
                );
                shell.run_menu(MenuTarget::Session(a), "rename", None, w, cx);
                shell
                    .renaming
                    .as_ref()
                    .unwrap()
                    .input
                    .update(cx, |i, cx| i.set_text("Discard".into(), cx));
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.renaming.is_none());
                assert_eq!(shell.sessions[&a].read(cx).summary.title, "First name");
            })
            .unwrap();
    }

    #[gpui::test]
    fn editing_a_link_keeps_remove_link_inside_the_dialog(cx: &mut TestAppContext) {
        let (window, a, _) = setup(cx);
        let open_picker = |cx: &mut TestAppContext| {
            window
                .update(cx, |shell, w, cx| {
                    shell.run_menu(MenuTarget::Session(a), "link-work-item", None, w, cx);
                    shell.link_dialog.as_ref().unwrap().picker.clone()
                })
                .unwrap()
        };
        open_picker(cx).update(cx, |p, cx| {
            p.search_for("https://github.com/acme/app/pull/42", cx);
            p.pick(0, cx);
        });
        cx.run_until_parked();
        open_picker(cx);
        cx.run_until_parked();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let dialog = visual.debug_bounds("work-item-picker").unwrap();
        let remove = visual.debug_bounds("work-item-remove").unwrap();
        assert!(
            remove.right() <= dialog.right(),
            "Remove link at {remove:?} overflows the dialog at {dialog:?}"
        );
    }

    #[gpui::test]
    fn links_and_reminders_validate_persist_and_clear_when_due_session_opens(
        cx: &mut TestAppContext,
    ) {
        let (window, a, _) = setup(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.run_menu(MenuTarget::Session(a), "link-work-item", None, w, cx);
            })
            .unwrap();
        let picker = window
            .read_with(cx, |shell, _| {
                shell.link_dialog.as_ref().unwrap().picker.clone()
            })
            .unwrap();
        let pick = |cx: &mut TestAppContext, text: &str| {
            picker.update(cx, |p, cx| {
                p.search_for(text, cx);
                p.pick(0, cx);
            });
            cx.run_until_parked();
        };
        pick(cx, "invalid");
        window
            .read_with(cx, |shell, _| assert!(shell.link_dialog.is_some()))
            .unwrap();
        pick(cx, "https://github.com/acme/app/pull/42");
        window
            .update(cx, |shell, w, cx| {
                shell.run_menu(MenuTarget::Session(a), "reminder:tomorrow", None, w, cx);
                shell.run_menu(MenuTarget::Session(a), "reminder:1h", None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                let store = locked(&shell.store).unwrap();
                assert_eq!(store.reminders().unwrap().len(), 1);
                assert_eq!(
                    store
                        .sessions()
                        .unwrap()
                        .iter()
                        .find(|s| s.id == a)
                        .unwrap()
                        .work_item
                        .as_ref()
                        .unwrap()
                        .number,
                    42
                );
                drop(store);
                assert!(shell.link_dialog.is_none());
                shell.run_menu(MenuTarget::Session(a), "reminder:cancel", None, w, cx);
                let due_at = monocode_core::time::now_ms().saturating_sub(1);
                shell.reminders.push(Reminder {
                    session_id: a,
                    due_at,
                    fired_at: None,
                });
                shell.store_write("set test reminder", cx, move |store| {
                    store.set_reminders(&[a], due_at)
                });
                shell.poll_reminders(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert!(
                    locked(&shell.store).unwrap().reminders().unwrap()[0]
                        .fired_at
                        .is_some()
                );
                shell.open(a, w, cx);
                assert!(shell.reminders.is_empty());
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(
                    locked(&shell.store)
                        .unwrap()
                        .reminders()
                        .unwrap()
                        .is_empty()
                )
            })
            .unwrap();
    }

    #[gpui::test]
    fn dragging_sessions_creates_a_folder_and_moves_into_an_existing_one(cx: &mut TestAppContext) {
        let (window, a, b) = setup(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let a_row = visual
            .debug_bounds(Box::leak(format!("sidebar-session-{a}").into_boxed_str()))
            .unwrap();
        let b_row = visual
            .debug_bounds(Box::leak(format!("sidebar-session-{b}").into_boxed_str()))
            .unwrap();
        visual.simulate_mouse_down(a_row.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(b_row.center(), Some(MouseButton::Left), Default::default());
        visual.simulate_mouse_up(b_row.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                let groups = shell.project_groups();
                assert_eq!(groups.folders.len(), 1);
                assert_eq!(groups.folders[0].session_ids, [a, b]);
                assert!(shell.renaming.is_some());
                shell.finish_rename(false, w, cx);
                shell.update_groups(cx, |g| g.remove_sessions(&[a]));
            })
            .unwrap();
        visual.refresh().unwrap();
        let folder = window
            .update(visual, |shell, _, _| {
                shell.project_groups().folders[0].id.clone()
            })
            .unwrap();
        let row = visual
            .debug_bounds(Box::leak(format!("sidebar-session-{a}").into_boxed_str()))
            .unwrap();
        let header = visual
            .debug_bounds(Box::leak(
                format!("session-group-folder:{folder}").into_boxed_str(),
            ))
            .unwrap();
        visual.simulate_mouse_down(row.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(header.center(), Some(MouseButton::Left), Default::default());
        visual.simulate_mouse_up(header.center(), MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert!(
                    shell
                        .project_groups()
                        .folder(&folder)
                        .unwrap()
                        .session_ids
                        .contains(&a)
                )
            })
            .unwrap();
    }

    #[gpui::test]
    fn delete_closes_the_session_and_cleans_up_folders_and_reminders(cx: &mut TestAppContext) {
        let (window, a, b) = setup(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.run_menu(MenuTarget::Session(a), "pin", None, w, cx);
                shell.run_menu(MenuTarget::Session(a), "reminder:1h", None, w, cx);
                shell.update_groups(cx, |g| {
                    g.create_folder(&[a], "remove-me".into());
                });
                shell.open(a, w, cx);
                shell.run_menu(MenuTarget::Session(a), "delete", None, w, cx);
                shell.open(a, w, cx);
                assert_ne!(
                    shell.workspace.active,
                    Some(a),
                    "deleting sessions cannot reopen"
                );
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(!shell.sessions.contains_key(&a));
                assert!(shell.sessions.contains_key(&b));
                assert!(shell.project_groups().folders.is_empty());
                let store = locked(&shell.store).unwrap();
                assert!(!store.sessions().unwrap().iter().any(|s| s.id == a));
                assert!(store.load(a).unwrap().rows.is_empty());
                assert!(store.reminders().unwrap().is_empty());
                assert!(
                    store
                        .groups(&shell.workspace.project)
                        .unwrap()
                        .folders
                        .is_empty()
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn folder_headers_reorder_and_the_color_menu_updates_the_saved_folder(cx: &mut TestAppContext) {
        let (window, a, b) = setup(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.update_groups(cx, |g| {
                    g.create_folder(&[a], "first".into());
                    g.create_folder(&[b], "second".into());
                });
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let first = visual
            .debug_bounds("session-group-folder:first")
            .unwrap()
            .center();
        let second = visual
            .debug_bounds("session-group-folder:second")
            .unwrap()
            .center();
        visual.simulate_mouse_down(second, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(first, Some(MouseButton::Left), Default::default());
        visual.simulate_mouse_up(first, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                let g = shell.project_groups();
                assert_eq!(g.folders[0].id, "first");
                assert_eq!(g.folders[1].id, "second");
                assert!(!g.folders[0].collapsed && !g.folders[1].collapsed);
            })
            .unwrap();
        visual.refresh().unwrap();
        let first = visual
            .debug_bounds("session-group-folder:first")
            .unwrap()
            .center();
        visual.simulate_mouse_down(first, MouseButton::Right, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        let swatch = visual.debug_bounds("folder-color-3").unwrap().center();
        visual.simulate_click(swatch, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                let saved = locked(&shell.store)
                    .unwrap()
                    .groups(&shell.workspace.project)
                    .unwrap();
                assert_eq!(saved.folders[0].id, "first");
                assert_eq!(saved.folder("first").unwrap().color_index, Some(3));
            })
            .unwrap();
    }

    #[gpui::test]
    fn f2_renames_the_active_session_and_the_focused_folder(cx: &mut TestAppContext) {
        let (window, a, _) = setup(cx);
        window
            .update(cx, |shell, w, cx| shell.open(a, w, cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "f2");
        cx.run_until_parked();
        window.update(cx, |shell, w, cx| {
            assert!(matches!(shell.renaming.as_ref().unwrap().target, RenameTarget::Session(id) if id == a));
            shell.finish_rename(false, w, cx);
            shell.update_groups(cx, |g| { g.create_folder(&[a], "work".into()); });
        }).unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let folder = visual
            .debug_bounds("session-group-folder:work")
            .unwrap()
            .center();
        visual.simulate_click(folder, Default::default());
        visual.simulate_keystrokes("f2");
        visual.run_until_parked();
        window.update(visual, |shell, _, _| {
            assert!(matches!(&shell.renaming.as_ref().unwrap().target, RenameTarget::Folder(id) if id == "work"));
        }).unwrap();
    }
}
