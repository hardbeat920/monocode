//! Only construct and lay out sidebar cards near the viewport.
use super::*;
use gpui::{ListAlignment, ListState, list};
use monocode_core::session_groups::Entry;
use std::cell::{Cell, RefCell};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Navigation,
    Monos,
    Pinned,
    Folders,
    Group(Entry),
    Gap,
    Heading {
        label: &'static str,
        first: bool,
    },
    Session(i64),
    GroupedSession {
        id: i64,
        first: bool,
        last: bool,
    },
    ClassicGroupMember {
        entry: Entry,
        id: i64,
        first: bool,
        last: bool,
    },
    ClassicFolderNew(String),
    ClassicGap(usize),
    Empty,
}

pub(super) struct SidebarList {
    pub(super) state: ListState,
    rows: Vec<Row>,
    scope: String,
    structure_dirty: Cell<bool>,
    indexed_minute: u64,
    #[cfg(test)]
    structural_rebuilds: usize,
    /// Only anchors constructed for the last viewport, including nested rails.
    visible_anchors: RefCell<Vec<AnchorSlot>>,
    visible_rows: RefCell<Vec<usize>>,
}

impl SidebarList {
    pub(super) fn new(cx: &mut Context<Shell>) -> Self {
        let state =
            ListState::new(0, ListAlignment::Top, px(96.)).with_uniform_item_height(px(33.));
        let owner = cx.weak_entity();
        state.set_scroll_handler(move |_, _, cx| {
            let _ = owner.update(cx, |shell, cx| {
                shell.sidebar_scrolled(cx);
                // Historical rows are retained as cheap IDs, but hit targets
                // only belong to the viewport that was actually laid out.
                shell.sidebar_list.clear_bounds();
                shell.sidebar_frame.update(cx, |_, cx| cx.notify());
            });
        });
        Self {
            state,
            rows: Vec::new(),
            scope: String::new(),
            structure_dirty: Cell::new(true),
            indexed_minute: 0,
            #[cfg(test)]
            structural_rebuilds: 0,
            visible_anchors: RefCell::new(Vec::new()),
            visible_rows: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn invalidate_structure(&self) {
        self.structure_dirty.set(true);
    }

    pub(super) fn remeasure(&self) {
        self.state.remeasure_items(0..self.rows.len());
        self.clear_bounds();
    }

    pub(super) fn track_anchor(&self, anchor: AnchorSlot) {
        self.visible_anchors.borrow_mut().push(anchor);
    }

    fn clear_bounds(&self) {
        for anchor in self.visible_anchors.borrow_mut().drain(..) {
            anchor.set(None);
        }
    }

    fn update(&mut self, scope: String, rows: Vec<Row>) {
        if self.scope != scope {
            self.state.reset_with_uniform_height(rows.len(), px(33.));
            self.scope = scope;
        } else if self.rows != rows {
            // Keep the scroll anchor when sessions are added, regrouped or
            // reordered. Only replace the changed run of list items.
            let prefix = self
                .rows
                .iter()
                .zip(&rows)
                .take_while(|(a, b)| a == b)
                .count();
            let suffix = self.rows[prefix..]
                .iter()
                .rev()
                .zip(rows[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            self.state.splice(
                prefix..self.rows.len() - suffix,
                rows.len() - prefix - suffix,
            );
        }
        self.rows = rows;
        // Navigation and the Mono overview can change height independently.
        self.state.remeasure_items(0..self.rows.len().min(2));
    }
}

fn age_label(updated_at: u64, now: u64) -> &'static str {
    let today = chrono::DateTime::from_timestamp_millis(now.min(i64::MAX as u64) as i64)
        .map(|t| t.with_timezone(&chrono::Local).date_naive());
    let day = chrono::DateTime::from_timestamp_millis(updated_at.min(i64::MAX as u64) as i64)
        .map(|t| t.with_timezone(&chrono::Local).date_naive());
    match today.zip(day).map(|(today, day)| (today - day).num_days()) {
        Some(days) if days <= 0 => "Today",
        Some(1) => "Yesterday",
        Some(2..=6) => "Last 7 days",
        _ => "Older",
    }
}

// Match sidebar-session-scroll: 40px below, and 28px above scrolled Focused rows.
// Mask only the edge bands so the virtual list never needs a full-pane copy.
fn sidebar_edge_mask(
    state: ListState,
    top_fade: bool,
    #[cfg_attr(target_os = "macos", allow(unused_variables))] inset: bool,
    begin: bool,
) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let offset = state.logical_scroll_top();
            let bottom_height = px(40.).min(bounds.size.height);
            let top_height = if top_fade && (offset.item_ix > 0 || offset.offset_in_item > px(0.)) {
                px(28.).min((bounds.size.height - bottom_height).max(px(0.)))
            } else {
                px(0.)
            };
            for (at_top, height) in [(true, top_height), (false, bottom_height)] {
                if height <= px(0.) {
                    continue;
                }
                let band = gpui::Bounds::new(
                    gpui::point(
                        bounds.left(),
                        if at_top {
                            bounds.top()
                        } else {
                            bounds.bottom() - height
                        },
                    ),
                    gpui::size(bounds.size.width, height),
                );
                #[cfg(target_os = "macos")]
                window.paint_quad(gpui::fill(band, gpui::black()).border_color(
                    gpui_apple::metal_renderer::edge_fade_marker(
                        begin,
                        if at_top {
                            height.as_f32() * window.scale_factor()
                        } else {
                            0.
                        },
                        if at_top {
                            0.
                        } else {
                            height.as_f32() * window.scale_factor()
                        },
                    ),
                ));
                #[cfg(not(target_os = "macos"))]
                if !begin {
                    let background = if inset {
                        shell_fill().blend(inset_chrome_wash())
                    } else {
                        shell_fill()
                    };
                    // Without a mask the fade paints the background over the
                    // rows. Over Windows' glass that becomes an opaque band,
                    // so a translucent background keeps the list's hard edge.
                    if cfg!(target_os = "windows") && background.a < 1. {
                        continue;
                    }
                    for y in 0..height.as_f32().ceil() as usize {
                        let mut color = background;
                        color.a = (1. - (y as f32 + 0.5) / height.as_f32()).clamp(0., 1.);
                        let y = if at_top {
                            band.top() + px(y as f32)
                        } else {
                            band.bottom() - px(y as f32 + 1.)
                        };
                        window.paint_quad(gpui::fill(
                            gpui::Bounds::new(
                                gpui::point(band.left(), y),
                                gpui::size(band.size.width, px(1.)),
                            ),
                            color,
                        ));
                    }
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl Shell {
    /// Session membership/order changes independently of scroll position and
    /// hover state. Keep the expensive project index until a data notification
    /// invalidates it; individual cards still read live metadata each render.
    pub(super) fn refresh_sidebar_structure(&mut self, cx: &mut Context<Self>) {
        let scope = self.sidebar_scope();
        let minute = monocode_core::time::now_ms() / 60_000;
        if self.sidebar_list.structure_dirty.replace(false)
            || self.sidebar_list.scope != scope
            || self.sidebar_list.indexed_minute != minute
        {
            // Date/time expiry refreshes membership without resetting the
            // project scope or scroll anchor.
            self.sidebar_list.indexed_minute = minute;
            #[cfg(test)]
            {
                self.sidebar_list.structural_rebuilds += 1;
            }
            let ids = self.sidebar_session_ids(cx);
            self.sync_rails(&ids, cx);
            let entries = self.session_entries(&ids, cx);
            let listed: HashSet<_> = ids.iter().copied().collect();
            self.session_row_bounds.retain(|id, _| listed.contains(id));
            for id in ids {
                self.session_row_bounds.entry(id).or_default();
            }
            let folders: HashSet<_> = entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Folder { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            self.folder_bounds.retain(|id, _| folders.contains(id));
            for id in folders {
                self.folder_bounds.entry(id).or_default();
            }
            self.prepare_sidebar_list(&entries, cx);
        }
        self.sync_sidebar_list_visibility();
        self.sidebar_list.clear_bounds();
    }

    fn sidebar_scope(&self) -> String {
        format!(
            "{}\0{}\0{}\0{:?}\0{}\0{}\0{}",
            self.workspace.project,
            self.working_copy(),
            self.query,
            self.workspace.session_filters,
            self.workspace.preferences.show_archived,
            self.workspace.boxy_layout(),
            self.workspace.classic_layout
        )
    }

    pub(super) fn prepare_sidebar_list(&mut self, entries: &[Entry], cx: &App) {
        if self.workspace.classic_layout {
            self.prepare_classic_sidebar_list(entries);
            return;
        }
        let groups = self.project_groups();
        let now = monocode_core::time::now_ms();
        let mut rows = vec![Row::Navigation, Row::Monos];
        let mut folders = false;
        for entry in entries {
            let (members, collapsed) = match entry {
                Entry::Pinned { sessions } => {
                    // Pins sit between the folders and the dated sessions,
                    // as ordinary cards under a plain label.
                    if folders {
                        rows.push(Row::Gap);
                        folders = false;
                    }
                    rows.push(Row::Pinned);
                    rows.extend(
                        sessions
                            .iter()
                            .filter(|id| !self.rails.children.contains(id))
                            .map(|id| Row::Session(*id)),
                    );
                    rows.push(Row::Gap);
                    continue;
                }
                Entry::Reminders {
                    sessions,
                    collapsed,
                } => (sessions, *collapsed),
                Entry::Folder { id, sessions } => {
                    (sessions, groups.folder(id).is_some_and(|g| g.collapsed))
                }
                Entry::Session(_) => continue,
            };
            if !folders {
                rows.push(Row::Folders);
                folders = true;
            }
            rows.push(Row::Group(entry.clone()));
            if !collapsed || !self.query.is_empty() {
                let members: Vec<_> = members
                    .iter()
                    .copied()
                    .filter(|id| !self.rails.children.contains(id))
                    .collect();
                rows.extend(
                    members
                        .iter()
                        .enumerate()
                        .map(|(ix, id)| Row::GroupedSession {
                            id: *id,
                            first: ix == 0,
                            last: ix + 1 == members.len(),
                        }),
                );
            }
        }
        if folders {
            rows.push(Row::Gap);
        }
        let mut previous = None;
        let mut first = true;
        for entry in entries {
            let Entry::Session(id) = entry else {
                continue;
            };
            if self.rails.children.contains(id) {
                continue;
            }
            let label = age_label(self.session_metadata(*id, cx).summary.updated_at, now);
            if previous != Some(label) {
                rows.push(Row::Heading { label, first });
                previous = Some(label);
                first = false;
            }
            rows.push(Row::Session(*id));
        }
        if first {
            rows.push(Row::Heading {
                label: "Sessions",
                first: true,
            });
            if entries.is_empty() {
                rows.push(Row::Empty);
            }
        }
        self.sidebar_list.update(self.sidebar_scope(), rows);
    }

    fn prepare_classic_sidebar_list(&mut self, entries: &[Entry]) {
        let groups = self.project_groups();
        // Original Sidebar.tsx: the list has p-1.5, gap-0.5 and pb-10.
        // Its project navigation and Mono roster belong to the first rail.
        let mut rows = vec![Row::ClassicGap(6)];
        for (ix, entry) in entries.iter().enumerate() {
            if ix > 0 {
                rows.push(Row::ClassicGap(2));
            }
            let (members, collapsed) = match entry {
                Entry::Session(id) => {
                    rows.push(Row::Session(*id));
                    continue;
                }
                Entry::Pinned { sessions } => (sessions, groups.pinned_collapsed),
                Entry::Reminders {
                    sessions,
                    collapsed,
                } => (sessions, *collapsed),
                Entry::Folder { id, sessions } => (
                    sessions,
                    groups.folder(id).is_some_and(|folder| folder.collapsed),
                ),
            };
            rows.push(Row::Group(entry.clone()));
            let expanded = !collapsed || !self.query.is_empty();
            if expanded {
                let members: Vec<_> = members.to_vec();
                rows.extend(
                    members
                        .iter()
                        .enumerate()
                        .map(|(ix, id)| Row::ClassicGroupMember {
                            entry: entry.clone(),
                            id: *id,
                            first: ix == 0,
                            last: ix + 1 == members.len(),
                        }),
                );
                if let Entry::Folder { id, .. } = entry {
                    rows.push(Row::ClassicFolderNew(id.clone()));
                }
            }
            if expanded || matches!(entries.get(ix + 1), Some(Entry::Session(_))) {
                rows.push(Row::ClassicGap(6));
            }
        }
        if entries.is_empty() {
            rows.push(Row::Empty);
        }
        rows.push(Row::ClassicGap(40));
        self.sidebar_list.update(self.sidebar_scope(), rows);
    }

    fn sync_sidebar_list_visibility(&mut self) {
        // Open rails have variable height. Remeasure only materialized rows,
        // rather than scanning every session in the project every frame.
        for ix in self.sidebar_list.visible_rows.borrow_mut().drain(..) {
            if let Some(
                Row::Session(id)
                | Row::GroupedSession { id, .. }
                | Row::ClassicGroupMember { id, .. },
            ) = self.sidebar_list.rows.get(ix)
                && (self.rails.started.contains_key(id)
                    || self.orchestration_summaries.contains_key(id))
            {
                self.sidebar_list.state.remeasure_items(ix..ix + 1);
            }
        }
    }

    pub(super) fn virtual_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("sidebar-scroll")
            .debug_selector(|| "sidebar-sessions".into())
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| this.sidebar_scrolled(cx)))
            .child(sidebar_edge_mask(
                self.sidebar_list.state.clone(),
                !self.workspace.classic_layout,
                !self.workspace.boxy_layout(),
                true,
            ))
            .child(
                list(
                    self.sidebar_list.state.clone(),
                    cx.processor(|this, ix, _, cx| this.sidebar_list_row(ix, cx)),
                )
                .size_full(),
            )
            .child(sidebar_edge_mask(
                self.sidebar_list.state.clone(),
                !self.workspace.classic_layout,
                !self.workspace.boxy_layout(),
                false,
            ))
            .into_any()
    }

    fn sidebar_list_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.sidebar_list.rows.get(ix).cloned() else {
            return div().into_any();
        };
        self.sidebar_list.visible_rows.borrow_mut().push(ix);
        let content = match row {
            Row::Navigation => return self.sidebar_navigation(cx),
            Row::Monos => {
                return self
                    .monos
                    .clone()
                    .map(|monos| {
                        monos.update(cx, |monos, cx| {
                            monos.rail(Some(self.workspace.project.clone()), cx)
                        })
                    })
                    .unwrap_or_else(|| div().into_any());
            }
            Row::Pinned => section_label("Pinned").into_any(),
            Row::Folders => section_label("Folders").into_any(),
            Row::Group(entry) => {
                if let Entry::Folder { id, .. } = &entry
                    && let Some(anchor) = self.folder_bounds.get(id)
                {
                    self.sidebar_list.track_anchor(anchor.clone());
                }
                self.sidebar_group_header(&entry, cx)
            }
            Row::Gap => return div().h(px(12.)).into_any(),
            Row::Heading { label, first } => section_label(label)
                .when(!first, |d| d.mt_3())
                .when(first, |d| d.child(self.sidebar_filter_button(cx)))
                .into_any(),
            Row::Session(id) => self.session_card(id, cx),
            Row::GroupedSession { id, first, last } => {
                return div()
                    .px_2()
                    .child(
                        div()
                            .ml(px(10.))
                            .when(first, |d| d.pt_1())
                            .child(rails::step(last, self.nested_session_card(id, cx))),
                    )
                    .into_any();
            }
            Row::ClassicGroupMember {
                entry,
                id,
                first,
                last,
            } => {
                return div()
                    .px(px(6.))
                    .child(self.classic_group_member(&entry, id, first, last, cx))
                    .into_any();
            }
            Row::ClassicFolderNew(id) => {
                return div()
                    .px(px(6.))
                    .child(self.classic_folder_new_session(&id, cx))
                    .into_any();
            }
            Row::ClassicGap(height) => return div().h(px(height as f32)).into_any(),
            Row::Empty => div()
                .px_2()
                .py_2()
                .text_size(px(12.))
                .text_color(rgb(MUTED))
                .child(if self.query.is_empty() {
                    "No sessions yet"
                } else {
                    "No matching sessions"
                })
                .into_any(),
        };
        div()
            .when(self.workspace.classic_layout, |d| d.px(px(6.)))
            .when(!self.workspace.classic_layout, |d| d.px_2().pb_px())
            .child(content)
            .into_any()
    }
}

impl Shell {
    pub(super) fn sidebar_options_menu_items(&self) -> Vec<MenuItem> {
        vec![
            MenuAction::new("sidebar-notes", "Notes")
                .icon("note")
                .toggle(self.workspace.sidebar_notes_visible)
                .disabled(!self.workspace.preferences.notes)
                .into(),
            MenuAction::new("sidebar-automations", "Automations")
                .icon("zap")
                .toggle(self.workspace.sidebar_automations_visible)
                .into(),
            MenuAction::new("sidebar-inbox", "Inbox")
                .icon("inbox")
                .toggle(self.workspace.sidebar_inbox_visible)
                .into(),
            MenuAction::new("sidebar-monos", "Monos")
                .icon("ghost")
                .toggle(self.workspace.preferences.monos)
                .into(),
            MenuAction::new("sidebar-activity", "Activity")
                .icon("response-activity")
                .toggle(self.workspace.sidebar_activity_visible)
                .disabled(!self.workspace.preferences.working_agents)
                .into(),
            MenuItem::Separator,
            MenuAction::new("compact-session-cards", "Compact view")
                .icon("list-bullet")
                .toggle(self.workspace.compact_session_cards)
                .into(),
        ]
    }

    pub(super) fn change_sidebar_option(&mut self, action: &str, cx: &mut Context<Self>) {
        let value = match action {
            "sidebar-notes" => &mut self.workspace.sidebar_notes_visible,
            "sidebar-automations" => &mut self.workspace.sidebar_automations_visible,
            "sidebar-inbox" => &mut self.workspace.sidebar_inbox_visible,
            "sidebar-monos" => &mut self.workspace.preferences.monos,
            "sidebar-activity" => &mut self.workspace.sidebar_activity_visible,
            "compact-session-cards" => &mut self.workspace.compact_session_cards,
            _ => return,
        };
        *value = !*value;
        if action == "sidebar-monos" {
            self.apply_preferences(cx);
            self.sync_settings(cx);
        }
        self.close_preview(cx);
        self.sidebar_list.remeasure();
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn sidebar_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .px_2()
            .pt(px(if self.workspace.classic_layout {
                2.
            } else {
                4.
            }))
            .pb(px(if self.workspace.classic_layout {
                8.
            } else {
                12.
            }))
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .id("rail-search")
                    .debug_selector(|| "sidebar-search-field".into())
                    .group("rail-search")
                    .w_full()
                    .h(px(32.))
                    .mb(px(if self.workspace.classic_layout {
                        2.
                    } else {
                        4.
                    }))
                    .px_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.08))
                    // RailSearch's Tailwind shadow-sm, including
                    // its tighter second layer below the border.
                    // The vendored shader keeps it outside the
                    // box, as CSS does.
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), rgba(0x0000001a).into()).blur_radius(px(3.)),
                        BoxShadow::new(px(0.), px(1.), rgba(0x0000001a).into())
                            .blur_radius(px(2.))
                            .spread_radius(px(-1.)),
                    ])
                    .text_color(content(if self.sidebar_hover == Some("Search") {
                        1.
                    } else {
                        0.5
                    }))
                    .font_weight(FontWeight::MEDIUM)
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .on_hover(cx.listener(|this, hovered, _, cx| {
                        this.sidebar_hover = if *hovered { Some("Search") } else { None };
                        this.notify_navigation(cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.focus_search(window, cx);
                        }),
                    )
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .child(
                        icon("search", 16.)
                            .text_color(content(0.35))
                            .group_hover("rail-search", |s| s.text_color(content(0.7))),
                    )
                    .child(div().flex_1().min_w_0().h(px(20.)).child("Search"))
                    .child(
                        // The shortcut span sits outside the label's `font-medium`.
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(content(0.4))
                            .child(if cfg!(target_os = "macos") {
                                "⌘K"
                            } else {
                                "Ctrl K"
                            }),
                    ),
            )
            .children(
                [
                    ("Inbox", "inbox"),
                    ("Notes", "note"),
                    ("Automations", "zap"),
                ]
                .into_iter()
                .filter(|(label, _)| match *label {
                    "Notes" => {
                        self.workspace.preferences.notes && self.workspace.sidebar_notes_visible
                    }
                    "Automations" => self.workspace.sidebar_automations_visible,
                    "Inbox" => self.workspace.sidebar_inbox_visible,
                    _ => true,
                })
                .map(|(label, glyph)| {
                    let glyph = if self.workspace.classic_layout && label == "Notes" {
                        "file"
                    } else {
                        glyph
                    };
                    let selected = self.app_view_label() == Some(label);
                    let unread = label == "Inbox"
                        && self
                            .inbox
                            .as_ref()
                            .is_some_and(|inbox| inbox.read(cx).unread_count() > 0);
                    div()
                        .id(label)
                        .debug_selector(move || format!("sidebar-navigation-{label}"))
                        .role(Role::Button)
                        .aria_label(if unread { "Inbox, new items" } else { label })
                        .aria_selected(selected)
                        .cursor_pointer()
                        .group("rail-action")
                        .h(px(32.))
                        .px_2()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(if selected || self.sidebar_hover == Some(label) {
                            1.
                        } else {
                            0.5
                        }))
                        .when(selected, |d| d.bg(content(0.1)))
                        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                        .child(icon(glyph, 16.).text_color(content(if selected {
                            0.7
                        } else {
                            0.35
                        })))
                        .child(div().flex_1().child(label))
                        .when(unread, |row| {
                            row.child(
                                div()
                                    .size(px(if self.workspace.classic_layout {
                                        8.
                                    } else {
                                        6.
                                    }))
                                    .rounded_full()
                                    .bg(accent_color()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, window, cx| match label {
                            "Inbox" => this.open_inbox(window, cx),
                            "Notes" => this.open_notes(window, cx),
                            "Automations" => this.open_automations(window, cx),
                            _ => (),
                        }))
                        .on_hover(cx.listener(move |this, hovered, _, cx| {
                            if *hovered {
                                this.sidebar_hover = Some(label);
                            } else if this.sidebar_hover == Some(label) {
                                this.sidebar_hover = None;
                            }
                            this.notify_navigation(cx);
                        }))
                }),
            )
            .into_any()
    }
    fn notify_navigation(&self, cx: &mut Context<Self>) {
        if self.workspace.classic_layout {
            self.projects_frame.update(cx, |_, cx| cx.notify());
        } else {
            self.sidebar_frame.update(cx, |_, cx| cx.notify());
        }
    }
    pub(super) fn sidebar_filter_button(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self
            .workspace
            .session_filters
            .active(self.workspace.preferences.show_archived)
            || self.menu_open(&MenuTarget::SessionFilter);
        let button = if self.workspace.classic_layout {
            div()
                .id("filter-sessions")
                .role(Role::Button)
                .aria_label("Filter sessions")
                .aria_expanded(self.menu_open(&MenuTarget::SessionFilter))
                .cursor_pointer()
                .group("classic-filter")
                .size(px(24.))
                .flex_shrink_0()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(content(0.1)))
                .child(
                    icon("filter", 12.)
                        .text_color(content(if active { 1. } else { 0.5 }))
                        .group_hover("classic-filter", |s| s.text_color(content(1.))),
                )
        } else {
            icon_button("filter-sessions", "Filter sessions", "filter")
        };
        button
            .debug_selector(|| "sidebar-filter-button".into())
            .mr(px(if self.workspace.classic_layout {
                0.
            } else {
                -5.
            }))
            .when(active, |d| d.bg(content(0.1)))
            .relative()
            .child(self.filter_anchor.probe())
            .on_click(cx.listener(|this, _, window, cx| {
                if this.menu_open(&MenuTarget::SessionFilter) {
                    this.close_menu(cx);
                } else {
                    this.open_menu(
                        MenuTarget::SessionFilter,
                        Anchor::Slot(this.filter_anchor.clone()),
                        window,
                        cx,
                    );
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::age_label;
    use chrono::TimeZone;
    use gpui::TestAppContext;

    #[gpui::test]
    fn search_field_fills_the_virtual_sidebar_row(cx: &mut TestAppContext) {
        let (window, _, _) = super::super::tests::window(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let sidebar = visual.debug_bounds("sidebar-content").unwrap();
        let search = visual.debug_bounds("sidebar-search-field").unwrap();
        assert_eq!(search.left() - sidebar.left(), px(8.));
        assert_eq!(sidebar.right() - search.right(), px(8.));
    }

    #[gpui::test]
    fn sidebar_options_switch_card_heights_and_persist_without_loading_history(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = super::super::tests::window(cx);
        let (id, loaded) = window
            .update(cx, |shell, _, cx| {
                let mut summary = locked(&shell.store)
                    .unwrap()
                    .create("codex", "/work/monocode")
                    .unwrap();
                summary.has_messages = true;
                summary.title =
                    "A detailed session with a long title that must stay inside its card".into();
                let id = summary.id;
                shell.session_details.insert(
                    id,
                    monocode_core::store::MobileSessionMetadata {
                        model: Some("gpt-6.1-sol".into()),
                        branch: Some("feature/session-cards".into()),
                        ..Default::default()
                    },
                );
                shell.session_summaries.insert(id, summary);
                shell.order.insert(0, id);
                shell.notify_sidebar(cx);
                (id, shell.sessions.len())
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let selector: &'static str = Box::leak(format!("sidebar-session-{id}").into_boxed_str());
        let model_selector: &'static str =
            Box::leak(format!("session-model-{id}").into_boxed_str());
        let workspace_selector: &'static str =
            Box::leak(format!("session-workspace-{id}").into_boxed_str());
        let title_selector: &'static str =
            Box::leak(format!("session-title-{id}").into_boxed_str());
        for boxy in [false, true] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.new_layout_boxy = boxy;
                    shell.layout_changed(cx);
                })
                .unwrap();
            visual.run_until_parked();
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(32.));
            let options = visual.debug_bounds("sidebar-options").unwrap();
            visual.simulate_click(options.center(), Default::default());
            visual.run_until_parked();
            let compact = visual
                .debug_bounds("menu-action-compact-session-cards")
                .unwrap();
            visual.simulate_click(compact.center(), Default::default());
            visual.run_until_parked();
            let row = visual.debug_bounds(selector).unwrap();
            assert_eq!(row.size.height, px(60.));
            let model = visual.debug_bounds(model_selector).unwrap();
            let workspace = visual.debug_bounds(workspace_selector).unwrap();
            let title = visual.debug_bounds(title_selector).unwrap();
            assert!(row.top() <= title.top() && title.bottom() <= workspace.top());
            assert_eq!(model.center().y, workspace.center().y);
            assert!(workspace.right() <= model.left());
            assert!(workspace.bottom() <= row.bottom());
            assert!(
                model.right() <= row.right()
                    && workspace.right() <= row.right()
                    && title.right() <= row.right()
            );
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(shell.sessions.len(), loaded);
                    assert!(!shell.sessions.contains_key(&id));
                    let saved = shell.latest_state.lock().unwrap().clone();
                    let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                    assert!(!restored.compact_session_cards);
                })
                .unwrap();
            // Switches keep the popover open for more choices.
            let compact = visual
                .debug_bounds("menu-action-compact-session-cards")
                .unwrap();
            visual.simulate_click(compact.center(), Default::default());
            visual.run_until_parked();
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(32.));
            assert!(visual.debug_bounds(model_selector).is_none());
            visual.simulate_click(options.center(), Default::default());
            visual.run_until_parked();
        }
    }

    #[gpui::test]
    fn sidebar_visibility_switches_stay_open_preserve_notes_and_survive_restart(
        cx: &mut TestAppContext,
    ) {
        let (window, _, _) = super::super::tests::window(cx);
        window
            .update(cx, |shell, w, cx| shell.open_notes(w, cx))
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let options = visual.debug_bounds("sidebar-options").unwrap();
        visual.simulate_click(options.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let menu = shell.menu.as_ref().unwrap().menu.read(cx);
                let bounds = menu.surface_region().get().unwrap();
                assert_eq!(bounds.left(), options.left());
                assert_eq!(bounds.size.width, px(184.));
            })
            .unwrap();
        let notes = visual.debug_bounds("menu-action-sidebar-notes").unwrap();
        let automations = visual
            .debug_bounds("menu-action-sidebar-automations")
            .unwrap();
        let inbox = visual.debug_bounds("menu-action-sidebar-inbox").unwrap();
        let monos = visual.debug_bounds("menu-action-sidebar-monos").unwrap();
        let activity = visual.debug_bounds("menu-action-sidebar-activity").unwrap();
        let compact = visual
            .debug_bounds("menu-action-compact-session-cards")
            .unwrap();
        assert!(notes.bottom() <= automations.top() && automations.bottom() <= inbox.top());
        assert!(inbox.bottom() <= monos.top() && monos.bottom() <= activity.top());
        assert!(
            activity.bottom() < compact.top(),
            "a divider separates Compact view"
        );
        for (action, navigation) in [
            ("menu-action-sidebar-notes", "sidebar-navigation-Notes"),
            (
                "menu-action-sidebar-automations",
                "sidebar-navigation-Automations",
            ),
            ("menu-action-sidebar-inbox", "sidebar-navigation-Inbox"),
            ("menu-action-sidebar-monos", "new-mono"),
        ] {
            assert!(visual.debug_bounds(navigation).is_some());
            let row = visual.debug_bounds(action).unwrap();
            visual.simulate_click(row.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds(navigation).is_none());
            assert!(
                visual
                    .debug_bounds("menu-action-compact-session-cards")
                    .is_some()
            );
        }
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.active_app_view, Some(app_views::AppView::Notes));
                assert!(shell.notes.is_some());
                assert!(
                    shell.workspace.preferences.notes,
                    "hiding the shortcut leaves Notes enabled"
                );
                let menu = shell.menu.as_ref().unwrap().menu.read(cx);
                for id in [
                    "sidebar-notes",
                    "sidebar-automations",
                    "sidebar-inbox",
                    "sidebar-monos",
                ] {
                    assert_eq!(menu.toggle_state(id), Some(false));
                }
                assert!(!cx.global::<crate::settings::Preferences>().monos);
                let saved = shell.latest_state.lock().unwrap().clone();
                let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                assert!(!restored.sidebar_notes_visible);
                assert!(!restored.sidebar_automations_visible);
                assert!(!restored.sidebar_inbox_visible);
                assert!(!restored.preferences.monos);
            })
            .unwrap();
        for (action, navigation) in [
            ("menu-action-sidebar-notes", "sidebar-navigation-Notes"),
            (
                "menu-action-sidebar-automations",
                "sidebar-navigation-Automations",
            ),
            ("menu-action-sidebar-inbox", "sidebar-navigation-Inbox"),
            ("menu-action-sidebar-monos", "new-mono"),
        ] {
            let row = visual.debug_bounds(action).unwrap();
            visual.simulate_click(row.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds(navigation).is_some());
        }
        visual.simulate_click(options.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                shell.open_settings(crate::settings::Section::Monos, w, cx)
            })
            .unwrap();
        visual.run_until_parked();
        let toggle = visual
            .debug_bounds("settings-toggle-monos-enabled")
            .unwrap();
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(!shell.workspace.preferences.monos);
                shell.close_settings(w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("new-mono").is_none());
        let options = visual.debug_bounds("sidebar-options").unwrap();
        visual.simulate_click(options.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let menu = shell.menu.as_ref().unwrap().menu.read(cx);
                assert_eq!(menu.toggle_state("sidebar-monos"), Some(false));
            })
            .unwrap();
    }

    #[gpui::test]
    fn activity_visibility_switch_preserves_agents_and_survives_restart(cx: &mut TestAppContext) {
        let (window, a, b) = super::super::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                for id in [a, b] {
                    shell.sessions[&id].update(cx, |session, _| session.busy = true);
                }
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for boxy in [false, true] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.new_layout_boxy = boxy;
                    shell.layout_changed(cx);
                })
                .unwrap();
            visual.run_until_parked();
            assert!(visual.debug_bounds("sidebar-activity-card").is_some());
            let options = visual.debug_bounds("sidebar-options").unwrap();
            visual.simulate_click(options.center(), Default::default());
            visual.run_until_parked();
            let activity = visual.debug_bounds("menu-action-sidebar-activity").unwrap();
            visual.simulate_click(activity.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds("sidebar-activity-card").is_none());
            window
                .update(visual, |shell, _, cx| {
                    assert!(shell.workspace.preferences.working_agents);
                    for id in [a, b] {
                        assert!(shell.sessions[&id].read(cx).responding());
                    }
                    let menu = shell.menu.as_ref().unwrap().menu.read(cx);
                    assert_eq!(menu.toggle_state("sidebar-activity"), Some(false));
                    let saved = shell.latest_state.lock().unwrap().clone();
                    let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                    assert!(!restored.sidebar_activity_visible);
                })
                .unwrap();
            let activity = visual.debug_bounds("menu-action-sidebar-activity").unwrap();
            visual.simulate_click(activity.center(), Default::default());
            visual.run_until_parked();
            assert!(visual.debug_bounds("sidebar-activity-card").is_some());
            window
                .update(visual, |shell, _, _| {
                    let saved = shell.latest_state.lock().unwrap().clone();
                    let restored = Workspace::restore(Some(&saved), &[], "/work/monocode");
                    assert!(restored.sidebar_activity_visible);
                })
                .unwrap();
            visual.simulate_click(options.center(), Default::default());
            visual.run_until_parked();
        }
        window
            .update(visual, |shell, _, cx| {
                shell.workspace.preferences.working_agents = false;
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("sidebar-activity-card").is_none());
    }

    #[gpui::test]
    fn scroll_and_hover_redraws_reuse_the_sidebar_index(cx: &mut TestAppContext) {
        let (window, _, _) = super::super::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.refresh_sidebar_structure(cx);
                let rebuilds = shell.sidebar_list.structural_rebuilds;
                for _ in 0..100 {
                    // This is the path used by scroll/hover redraws: visible
                    // cards refresh, while membership/order stays cached.
                    shell.refresh_sidebar_structure(cx);
                }
                assert_eq!(shell.sidebar_list.structural_rebuilds, rebuilds);
                shell.sidebar_list.invalidate_structure();
                shell.refresh_sidebar_structure(cx);
                assert_eq!(shell.sidebar_list.structural_rebuilds, rebuilds + 1);
                // Time expiry is a content refresh, not navigation: retain
                // the user's position instead of snapping back to the active row.
                shell.sidebar_list.state.scroll_to(gpui::ListOffset {
                    item_ix: 1,
                    offset_in_item: px(5.),
                });
                let offset = shell.sidebar_list.state.logical_scroll_top();
                shell.sidebar_list.indexed_minute = 0;
                shell.refresh_sidebar_structure(cx);
                let refreshed = shell.sidebar_list.state.logical_scroll_top();
                assert_eq!(refreshed.item_ix, offset.item_ix);
                assert_eq!(refreshed.offset_in_item, offset.offset_in_item);
                assert_eq!(shell.sidebar_list.structural_rebuilds, rebuilds + 2);
                // Scope changes are also detected without a notification hook.
                shell.query.push_str("unmatched-session-query");
                shell.refresh_sidebar_structure(cx);
                assert_eq!(shell.sidebar_list.structural_rebuilds, rebuilds + 3);
                assert!(shell.sidebar_list.rows.contains(&super::Row::Empty));
            })
            .unwrap();
    }

    #[gpui::test]
    fn large_project_only_lays_out_nearby_session_cards(cx: &mut TestAppContext) {
        let (window, _, _) = super::super::tests::window(cx);
        let started = std::time::Instant::now();
        let (first, last) = window
            .update(cx, |shell, window, cx| {
                let mut summaries = Vec::new();
                {
                    let store = locked(&shell.store).unwrap();
                    for i in 0..1000 {
                        let mut summary = store.create("codex", "/work/many-sessions").unwrap();
                        summary.title =
                            format!("Session {i}: {}", "A long conversation title 🙂 ".repeat(4));
                        summary.has_messages = true;
                        summaries.push(summary);
                    }
                }
                let first = summaries.last().unwrap().id;
                let last = summaries[0].id;
                for summary in summaries {
                    shell.order.push(summary.id);
                    shell.session_summaries.insert(summary.id, summary);
                }
                shell.workspace.open(first);
                shell.select_project("/work/many-sessions".into(), window, cx);
                (first, last)
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, cx| {
                let laid_out = shell
                    .session_row_bounds
                    .values()
                    .filter(|slot| slot.get().is_some())
                    .count();
                eprintln!(
                    "1,000-session project: {laid_out} laid-out cards, {:?} setup + first frame",
                    started.elapsed()
                );
                assert!(laid_out > 0);
                assert!(
                    shell.sessions.len() < 10,
                    "scrolling the sidebar must not construct off-screen conversations"
                );
                assert!(
                    laid_out < 80,
                    "off-screen session cards should not be laid out: {laid_out}"
                );
                assert!(shell.session_row_bounds[&first].get().is_some());
                assert!(shell.session_row_bounds[&last].get().is_none());
                shell.sidebar_list.state.scroll_to_end();
                shell.sidebar_frame.update(cx, |_, cx| cx.notify());
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, cx| {
                assert!(
                    shell.session_row_bounds[&first].get().is_none(),
                    "scrolled-away cards cannot remain hit targets"
                );
                assert!(
                    shell.session_row_bounds[&last].get().is_some(),
                    "the oldest conversation is reachable"
                );
                let members = shell.sidebar_session_ids(cx);
                shell.groups.insert(
                    "/work/many-sessions".into(),
                    monocode_core::session_groups::Groups {
                        folders: vec![monocode_core::session_groups::Folder {
                            id: "large-folder".into(),
                            name: "All history".into(),
                            session_ids: members,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                );
                shell.notify_sidebar(cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, _| {
                let laid_out = shell
                    .session_row_bounds
                    .values()
                    .filter(|slot| slot.get().is_some())
                    .count();
                assert!(
                    laid_out > 0 && laid_out < 80,
                    "expanded folders must also be virtualized: {laid_out}"
                );
                shell.sidebar_list.state.scroll_to_end();
            })
            .unwrap();
        cx.refresh().unwrap();
        let offset = window
            .update(cx, |shell, window, cx| {
                // Resolve scroll_to_end's end sentinel before checking selection.
                let offset = shell.sidebar_list.state.logical_scroll_top();
                shell.open(first - 1, window, cx);
                offset
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, _| {
                // Like the Tauri sidebar, selecting a session (e.g. from a
                // title tab) leaves the user's scroll position alone.
                let after = shell.sidebar_list.state.logical_scroll_top();
                assert_eq!(
                    (after.item_ix, after.offset_in_item),
                    (offset.item_ix, offset.offset_in_item)
                );
                assert!(shell.session_row_bounds[&(first - 1)].get().is_none());
            })
            .unwrap();
    }

    #[test]
    fn sidebar_dates_use_local_calendar_days_and_match_the_tauri_sections() {
        let ms = |day, hour, minute| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
                .single()
                .unwrap()
                .timestamp_millis() as u64
        };
        let now = ms(20, 0, 5);
        assert_eq!(age_label(ms(20, 0, 1), now), "Today");
        assert_eq!(age_label(ms(19, 23, 59), now), "Yesterday");
        assert_eq!(age_label(ms(14, 0, 0), now), "Last 7 days");
        assert_eq!(age_label(ms(13, 23, 59), now), "Older");
        assert_eq!(age_label(0, now), "Older");
        // A local calendar day remains Yesterday across daylight-saving changes.
        assert_eq!(age_label(ms(25, 0, 1), ms(26, 0, 5)), "Yesterday");
    }
}
