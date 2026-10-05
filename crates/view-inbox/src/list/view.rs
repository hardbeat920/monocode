//! Port of `InboxView` from src/features/inbox/ui/InboxView.tsx: the Inbox
//! page. A title bar, the resizable list pane (source tabs, filter field,
//! filter, mark read, and refresh buttons, cards), the selected item's
//! detail, and the Ask panel beside it.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{Input, InputEvent, InputState};
use monocode_ui::widgets::{icon_button, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{
    InboxListData, InboxListState, InboxProvider, InboxServices, InboxSource, ListedItem,
};
use crate::list::card::inbox_card;
use crate::list::connect_menu::inbox_connect_menu;
use crate::list::discussion::{CloseDiscussion, InboxDiscussionPanel};
use crate::list::filters_menu::{
    FilterAction, apply_filter_action, filter_entries, inbox_filters_menu,
};
use crate::model::{inbox_source_label, is_tracker_source};
use crate::pr::detail::{DetailMode, DetailProps, InboxDetailEvent, InboxDetailView, empty_detail};
use crate::style::{
    PaneResize, PopoverAlign, ResizeEdge, loader, popover_below, provider_mark, resize_handle,
    square_button,
};

/// `LIST_PAGE_SIZE`.
pub const LIST_PAGE_SIZE: usize = 32;
const MIN_WIDTH: f32 = 240.;
const MAX_WIDTH: f32 = 420.;
const DEFAULT_WIDTH: f32 = 280.;

thread_local! {
    /// `rememberedWidth`.
    static REMEMBERED_WIDTH: Cell<f32> = const { Cell::new(DEFAULT_WIDTH) };
}

/// `listWindowSize`.
pub fn list_window_size(total: usize, requested: usize) -> usize {
    if total == 0 {
        return 0;
    }
    total.min(LIST_PAGE_SIZE.max(requested))
}

/// The page's props.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InboxViewConfig {
    /// The page sits beside the project rail (no traffic-light inset, no
    /// back and sidebar buttons).
    pub beside_rail: bool,
    pub compact_rail: bool,
    /// `onClose` is wired: show Back.
    pub can_close: bool,
    /// `onToggleSidebar` is wired.
    pub can_toggle_sidebar: bool,
    /// `onStart` is wired.
    pub can_start: bool,
    /// `onRepairChecks` is wired.
    pub can_repair: bool,
}

/// What the page asks its owner for.
#[derive(Debug, Clone, PartialEq)]
pub enum InboxViewEvent {
    Close,
    ToggleSidebar,
    /// Settings on the card where this source connects.
    OpenIntegrations(InboxSource),
    OpenSession(String),
}

/// The empty list's message, from the React ternary chain.
pub fn empty_list_message(state: &InboxListState, search: &str, project_count: usize) -> String {
    let source = state.source;
    let label = inbox_source_label(source);
    let tracker = is_tracker_source(source);
    let search_narrowed = !search.trim().is_empty();
    let narrowed = search_narrowed || state.filters_active;
    if narrowed {
        if search_narrowed {
            return if tracker {
                format!("No matching {label} issues")
            } else if source == InboxProvider::Gitlab {
                "No matching issues or merge requests".into()
            } else {
                "No matching issues or pull requests".into()
            };
        }
        if tracker {
            return format!("No {label} issues match these filters");
        }
        if matches!(source, InboxProvider::Gitlab | InboxProvider::AzureDevops) {
            return if state.filters.assigned_to_me {
                "Nothing needs your attention".into()
            } else if source == InboxProvider::Gitlab {
                "No GitLab items match these filters".into()
            } else {
                "No ADO items match these filters".into()
            };
        }
        return "No issues or pull requests match these filters".into();
    }
    if tracker {
        return format!("No {label} issues");
    }
    if project_count == 0 {
        return "Open a project to fill the inbox".into();
    }
    if source == InboxProvider::Gitlab {
        "No matching issues or merge requests".into()
    } else {
        "No matching issues or pull requests".into()
    }
}

/// The selected card: the selected key if it is listed, else the first
/// card unless a linked target is still loading.
pub fn resolve_selection<'a>(
    visible: &'a [ListedItem],
    selected_key: Option<&str>,
    target_key: Option<&str>,
) -> Option<&'a ListedItem> {
    let by_key = visible
        .iter()
        .find(|listed| Some(listed.key.as_str()) == selected_key);
    let waiting = target_key.is_some() && selected_key == target_key;
    by_key.or(if waiting { None } else { visible.first() })
}

pub struct InboxView {
    services: Rc<dyn InboxServices>,
    list: Rc<dyn InboxListData>,
    config: InboxViewConfig,
    state: InboxListState,
    search: Entity<InputState>,
    search_text: String,
    selected_key: Option<String>,
    filter_menu_open: bool,
    connect_menu_open: bool,
    list_limit: usize,
    resize: PaneResize,
    revision: u64,
    detail: Option<(String, Entity<InboxDetailView>, Subscription)>,
    discussion: Option<(String, Entity<InboxDiscussionPanel>, Subscription)>,
    discussion_open: bool,
    list_scroll: ScrollHandle,
    animate: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InboxViewEvent> for InboxView {}

impl InboxView {
    pub fn new(
        services: Rc<dyn InboxServices>,
        list: Rc<dyn InboxListData>,
        config: InboxViewConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter inbox"));
        let search_events = cx.subscribe_in(&search, window, |this, search, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_text = search.read(cx).value().to_string();
                this.reset_window();
                this.sync(window, cx);
            }
        });
        let weak = cx.entity().downgrade();
        let handle = window.window_handle();
        let list_sub = list.subscribe(
            Box::new(move |cx| {
                let weak = weak.clone();
                let _ = handle.update(cx, |_, window, cx| {
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |view, cx| view.sync(window, cx));
                    }
                });
            }),
            cx,
        );
        let state = list.state(cx);
        let width = REMEMBERED_WIDTH.with(Cell::get);
        let mut view = Self {
            services,
            selected_key: state.target_selection_key.clone(),
            list,
            config,
            state,
            search,
            search_text: String::new(),
            filter_menu_open: false,
            connect_menu_open: false,
            list_limit: LIST_PAGE_SIZE,
            resize: PaneResize::new(width, DEFAULT_WIDTH, MIN_WIDTH, ResizeEdge::Right),
            revision: 0,
            detail: None,
            discussion: None,
            discussion_open: false,
            list_scroll: ScrollHandle::new(),
            animate: true,
            _subscriptions: vec![search_events, list_sub],
        };
        view.sync(window, cx);
        view
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        if let Some((_, detail, _)) = &self.detail {
            detail.update(cx, |detail, cx| detail.set_animate(animate, cx));
        }
    }

    pub fn state(&self) -> &InboxListState {
        &self.state
    }

    pub fn visible_items(&self, cx: &App) -> Vec<ListedItem> {
        if !self.state.visible_sources.contains(&self.state.source) {
            return Vec::new();
        }
        self.list.visible_items(&self.search_text, cx)
    }

    pub fn selected_key(&self) -> Option<&str> {
        self.selected_key.as_deref()
    }

    pub fn detail(&self) -> Option<&Entity<InboxDetailView>> {
        self.detail.as_ref().map(|(_, detail, _)| detail)
    }

    pub fn discussion(&self) -> Option<&Entity<InboxDiscussionPanel>> {
        self.discussion.as_ref().map(|(_, panel, _)| panel)
    }

    fn reset_window(&mut self) {
        self.list_limit = LIST_PAGE_SIZE;
        self.list_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
    }

    fn detail_props(&self, listed: &ListedItem) -> DetailProps {
        DetailProps {
            cwd: self.state.cwd.clone(),
            projects: self.state.projects.clone(),
            related_sessions: listed.related_sessions.clone(),
            mode: DetailMode::Inbox,
            visible: true,
            revision: self.revision,
            can_discuss: true,
            can_start: self.config.can_start,
            can_repair: self.config.can_repair,
        }
    }

    /// Reads the list again and keeps the selection and the panes in step.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.list.state(cx);
        let narrowing_changed = state.filters != self.state.filters
            || state.source != self.state.source
            || state.hidden_linear_team_ids != self.state.hidden_linear_team_ids
            || state.hidden_jira_project_ids != self.state.hidden_jira_project_ids;
        self.state = state;
        if narrowing_changed {
            self.reset_window();
        }
        let visible = self.visible_items(cx);
        let target_key = self.state.target_selection_key.clone();
        let selected = resolve_selection(
            &visible,
            self.selected_key.as_deref(),
            target_key.as_deref(),
        )
        .cloned();
        match &selected {
            None => {
                if target_key.is_none() {
                    self.selected_key = None;
                }
            }
            Some(listed) => {
                let waiting = target_key.is_some()
                    && self.selected_key == target_key
                    && Some(&listed.key) != target_key.as_ref();
                if !waiting && Some(&listed.key) != self.selected_key.as_ref() {
                    self.selected_key = Some(listed.key.clone());
                }
            }
        }
        self.sync_detail(selected, window, cx);
        cx.notify();
    }

    fn sync_detail(
        &mut self,
        selected: Option<ListedItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(listed) = selected else {
            self.detail = None;
            self.discussion = None;
            return;
        };
        let props = self.detail_props(&listed);
        match &self.detail {
            Some((key, detail, _)) if *key == listed.key => {
                let item = listed.item.clone();
                detail.update(cx, |detail, cx| {
                    detail.set_item(item, cx);
                    detail.set_props(props, cx);
                });
            }
            _ => {
                let services = self.services.clone();
                let item = listed.item.clone();
                let animate = self.animate;
                let detail = cx.new(|cx| {
                    let mut detail = InboxDetailView::new(services, item, props, window, cx);
                    detail.set_animate(animate, cx);
                    detail
                });
                let subscription = cx.subscribe(
                    &detail,
                    |this, _, event: &InboxDetailEvent, cx| match event {
                        InboxDetailEvent::Discuss => {
                            this.discussion_open = true;
                            cx.notify();
                        }
                        InboxDetailEvent::OpenSession(id) => {
                            cx.emit(InboxViewEvent::OpenSession(id.clone()))
                        }
                        InboxDetailEvent::ItemChanged(item) => {
                            this.list.update_item(item.as_ref().clone(), cx)
                        }
                    },
                );
                self.detail = Some((listed.key.clone(), detail, subscription));
            }
        }
        // The Ask panel follows the selected item; a new item remounts it.
        if self.discussion_open {
            match &self.discussion {
                Some((key, panel, _)) if *key == listed.key => {
                    let item = listed.item.clone();
                    panel.update(cx, |panel, cx| panel.set_item(item, cx));
                }
                _ => {
                    let services = self.services.clone();
                    let item = listed.item.clone();
                    let panel = cx.new(|cx| InboxDiscussionPanel::new(services, item, window, cx));
                    let subscription = cx.subscribe(&panel, |this, _, _: &CloseDiscussion, cx| {
                        this.discussion_open = false;
                        this.discussion = None;
                        cx.notify();
                    });
                    self.discussion = Some((listed.key.clone(), panel, subscription));
                }
            }
        } else {
            self.discussion = None;
        }
    }

    /// A card was clicked.
    pub fn select(&mut self, listed: &ListedItem, window: &mut Window, cx: &mut Context<Self>) {
        self.list.mark_item_seen(listed, cx);
        self.selected_key = Some(listed.key.clone());
        self.sync(window, cx);
    }

    /// A source tab.
    pub fn set_source(&mut self, source: InboxSource, cx: &mut Context<Self>) {
        self.list.set_source(source, cx);
    }

    /// "Mark all as read" for the open tab.
    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        if self.state.source_has_unseen {
            self.list.mark_source_read(cx);
        }
    }

    /// The Refresh button.
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revision += 1;
        self.list.refresh(cx);
        self.sync(window, cx);
    }

    /// Typing in the filter field, for tests and the gallery.
    pub fn set_search(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            search.set_value(text.to_string(), window, cx)
        });
        self.search_text = text.to_string();
        self.reset_window();
        self.sync(window, cx);
    }

    /// Opens or closes the filter menu.
    pub fn toggle_filter_menu(&mut self, cx: &mut Context<Self>) {
        self.filter_menu_open = !self.filter_menu_open;
        cx.notify();
    }

    /// Opens or closes the connect menu.
    pub fn toggle_connect_menu(&mut self, cx: &mut Context<Self>) {
        self.connect_menu_open = !self.connect_menu_open;
        cx.notify();
    }

    /// Opens the Ask panel for the selected item.
    pub fn open_discussion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.discussion_open = true;
        self.sync(window, cx);
    }

    fn apply_filter(&mut self, action: FilterAction, cx: &mut Context<Self>) {
        let change = apply_filter_action(&action, self.state.source, &self.state);
        if let Some(filters) = change.filters {
            self.list.set_filters(filters, cx);
        }
        if let Some(ids) = change.hidden_linear_team_ids {
            self.list.set_hidden_linear_team_ids(ids, cx);
        }
        if let Some(ids) = change.hidden_jira_project_ids {
            self.list.set_hidden_jira_project_ids(ids, cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        cx.stop_propagation();
        if self.filter_menu_open {
            self.filter_menu_open = false;
            cx.notify();
            return;
        }
        if self.connect_menu_open {
            self.connect_menu_open = false;
            cx.notify();
            return;
        }
        if self.config.can_close {
            cx.emit(InboxViewEvent::Close);
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mac = cfg!(target_os = "macos");
        let mut bar = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.title_bar_height))
            .items_center()
            .border_b_1()
            .border_color(theme.colors.stroke);
        if mac && self.config.compact_rail {
            bar = bar.child(div().flex_none().w(u(16.)));
        }
        if mac && !self.config.beside_rail {
            bar = bar.child(div().flex_none().w(u(theme.metrics.traffic_light_inset)));
        }
        if !self.config.beside_rail && (self.config.can_close || self.config.can_toggle_sidebar) {
            let modifier = if mac { "⌘" } else { "Ctrl+" };
            let mut nav = div().flex().flex_none().items_center().px(u(6.));
            if self.config.can_close {
                nav = nav.child(
                    icon_button("inbox-back", IconName::ChevronLeft)
                        .tooltip(format!("Back ({modifier}[)"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(InboxViewEvent::Close))),
                );
            }
            if self.config.can_toggle_sidebar {
                nav = nav.child(
                    icon_button("inbox-toggle-sidebar", IconName::PanelLeft)
                        .tooltip(format!("Toggle Sidebar ({modifier}B)"))
                        .on_click(
                            cx.listener(|_, _, _, cx| cx.emit(InboxViewEvent::ToggleSidebar)),
                        ),
                );
            }
            bar = bar.child(nav);
        }
        bar.child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(u(8.))
                .px(u(12.))
                .text_px(theme.text.body)
                .child(
                    icon(IconName::Inbox)
                        .size(u(14.))
                        .text_color(theme.content(0.45)),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.colors.content)
                        .child("Inbox"),
                ),
        )
        .into_any_element()
    }

    fn render_source_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut row = div()
            .flex()
            .flex_none()
            .h(u(36.))
            .items_center()
            .gap(gpui::px(1.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(8.));
        let sources = self.state.visible_sources.clone();
        if !sources.is_empty() {
            let mut tabs = div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(gpui::px(1.))
                .flex_basis(gpui::relative(0.))
                .flex_grow(sources.len() as f32);
            for source in sources {
                let selected = self.state.source == source;
                let hover_bg = theme.content(0.05);
                let ink = theme.colors.content;
                let mut tab = div()
                    .id(SharedString::from(format!(
                        "inbox-source-{}",
                        inbox_source_label(source)
                    )))
                    .flex()
                    .h(u(24.))
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .text_px(theme.text.label)
                    .leading(theme.leading.none)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .child(provider_mark(
                                source,
                                14.,
                                if selected {
                                    theme.colors.content
                                } else {
                                    theme.content(0.50)
                                },
                            ))
                            .child(inbox_source_label(source)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set_source(source, cx)));
                tab = if selected {
                    tab.bg(theme.colors.selection).text_color(ink)
                } else {
                    tab.text_color(theme.content(0.50))
                        .hover(move |s| s.bg(hover_bg).text_color(ink))
                };
                tabs = tabs.child(tab);
            }
            row = row.child(tabs);
        }
        if !self.state.connectable_sources.is_empty() {
            let open = self.connect_menu_open;
            let hover_bg = theme.content(0.05);
            let ink = theme.colors.content;
            let mut button = div()
                .id("inbox-add-connection")
                .flex()
                .h(u(24.))
                .min_w_0()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .px(u(8.))
                .text_px(theme.text.label)
                .leading(theme.leading.none)
                .tooltip(tooltip("Connect an inbox source"))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_connect_menu(cx)))
                .child(icon(IconName::Plus).size(u(14.)).text_color(if open {
                    ink
                } else {
                    theme.content(0.40)
                }))
                .child(div().min_w_0().truncate().child("Add connection"));
            button = if open {
                button.bg(theme.colors.selection).text_color(ink)
            } else {
                button
                    .text_color(theme.content(0.40))
                    .hover(move |s| s.bg(hover_bg).text_color(ink))
            };
            let mut cell = div().relative().flex().flex_1().min_w_0().child(button);
            if open {
                let weak = cx.entity().downgrade();
                let close_weak = weak.clone();
                cell = cell.child(popover_below(
                    PopoverAlign::Start,
                    4.,
                    inbox_connect_menu(
                        &self.state.connectable_sources,
                        self.animate,
                        Rc::new(move |source, _, cx| {
                            if let Some(view) = weak.upgrade() {
                                view.update(cx, |_, cx| {
                                    cx.emit(InboxViewEvent::OpenIntegrations(source))
                                });
                            }
                        }),
                        Rc::new(move |_, cx| {
                            if let Some(view) = close_weak.upgrade() {
                                view.update(cx, |view, cx| {
                                    view.connect_menu_open = false;
                                    cx.notify();
                                });
                            }
                        }),
                        cx,
                    ),
                    cx,
                ));
            }
            row = row.child(cell);
        }
        row.into_any_element()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let filters_highlight = self.filter_menu_open || self.state.filters_active;
        let filter_ink = if filters_highlight {
            theme.colors.content
        } else {
            theme.content(0.45)
        };
        let mut filter_cell = div().relative().flex_none().child(
            square_button(
                "inbox-filter",
                icon(IconName::ListFilter)
                    .size(u(12.))
                    .text_color(filter_ink)
                    .into_any_element(),
                filters_highlight,
                false,
                cx,
            )
            .tooltip(tooltip("Filter inbox"))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_filter_menu(cx))),
        );
        if self.filter_menu_open {
            let weak = cx.entity().downgrade();
            let close_weak = weak.clone();
            let services = self.services.clone();
            filter_cell = filter_cell.child(popover_below(
                PopoverAlign::End,
                2.,
                inbox_filters_menu(
                    filter_entries(&self.state),
                    self.animate,
                    move |project, cx| services.project_mark(&project.mark, 14., cx),
                    Rc::new(move |action, _, cx| {
                        if let Some(view) = weak.upgrade() {
                            view.update(cx, |view, cx| view.apply_filter(action, cx));
                        }
                    }),
                    Rc::new(move |_, cx| {
                        if let Some(view) = close_weak.upgrade() {
                            view.update(cx, |view, cx| {
                                view.filter_menu_open = false;
                                cx.notify();
                            });
                        }
                    }),
                    cx,
                ),
                cx,
            ));
        }
        let has_unseen = self.state.source_has_unseen;
        let mut mark_read = square_button(
            "inbox-mark-read",
            icon(IconName::CheckCheck)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element(),
            false,
            !has_unseen,
            cx,
        )
        .tooltip(tooltip("Mark all as read"));
        if has_unseen {
            let list = self.list.clone();
            mark_read = mark_read.on_click(move |_, _, cx| list.mark_source_read(cx));
        }
        let busy = self.state.loading || self.state.revalidating;
        let refresh_glyph = if busy {
            loader("inbox-refreshing", 14., theme.content(0.45))
        } else {
            icon(IconName::RefreshCw)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element()
        };
        let refresh = square_button("inbox-refresh", refresh_glyph, false, false, cx)
            .tooltip(tooltip("Refresh"))
            .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx)));
        div()
            .flex()
            .flex_none()
            .h(u(36.))
            .items_center()
            .gap(u(4.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(8.))
            .child(
                div()
                    .relative()
                    .flex()
                    .h(u(28.))
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .child(
                        div().absolute().left(u(8.)).child(
                            icon(IconName::Search)
                                .size(u(12.))
                                .text_color(theme.content(0.50)),
                        ),
                    )
                    .child(
                        div()
                            .size_full()
                            .pl(u(28.))
                            .pr(u(8.))
                            .text_px(theme.text.label)
                            .text_color(theme.colors.content)
                            .child(
                                Input::new(&self.search)
                                    .appearance(false)
                                    .h_full()
                                    .p_0()
                                    .text_px(theme.text.label),
                            ),
                    ),
            )
            .child(filter_cell)
            .child(mark_read)
            .child(refresh)
            .into_any_element()
    }

    fn render_list_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let message = |text: String| {
            div()
                .px(u(12.))
                .py(u(8.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child(text)
                .into_any_element()
        };
        let no_sources = self.state.visible_sources.is_empty();
        if no_sources {
            return div()
                .px(u(12.))
                .py(u(12.))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .child("Add a connection to start using the Inbox.")
                .into_any_element();
        }
        let visible = self.visible_items(cx);
        if let Some(error) = self.state.source_error.clone()
            && visible.is_empty()
        {
            return message(error);
        }
        if self.state.loading && self.state.item_count == 0 {
            return div()
                .flex()
                .justify_center()
                .py(u(40.))
                .child(loader("inbox-list-loading", 16., theme.content(0.40)))
                .into_any_element();
        }
        if visible.is_empty() {
            return message(empty_list_message(
                &self.state,
                &self.search_text,
                self.state.projects.len(),
            ));
        }
        let shown = list_window_size(visible.len(), self.list_limit);
        let has_more = shown < visible.len();
        let selected = self.selected_key.clone();
        let mut list = div().flex().flex_col().gap(u(2.)).p(u(6.));
        for listed in visible.into_iter().take(shown) {
            let active = selected.as_deref() == Some(listed.key.as_str());
            let id = ElementId::Name(format!("inbox-card:{}", listed.key).into());
            let entity = cx.entity().downgrade();
            let clicked = listed.clone();
            list = list.child(
                inbox_card(id, listed, active, self.services.clone()).on_select(
                    move |_, window, cx| {
                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, cx| view.select(&clicked, window, cx));
                        }
                    },
                ),
            );
        }
        if has_more {
            // The sentinel: once it scrolls within 240px of view, mount
            // another page.
            let scroll = self.list_scroll.clone();
            let weak = cx.entity().downgrade();
            let margin = gpui::px(240. * theme.ui_scale());
            list = list.child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        let viewport = scroll.bounds();
                        if bounds.top() <= viewport.bottom() + margin
                            && let Some(view) = weak.upgrade()
                        {
                            window.defer(cx, move |_, cx| {
                                view.update(cx, |view, cx| {
                                    view.list_limit += LIST_PAGE_SIZE;
                                    cx.notify();
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .h(gpui::px(1.))
                .w_full(),
            );
        }
        list.into_any_element()
    }

    fn render_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let scale = theme.ui_scale();
        let viewport = f32::from(window.viewport_size().width) / scale;
        let max = MAX_WIDTH.min((viewport * 0.5).round());
        let width = self.resize.width.clamp(MIN_WIDTH, max.max(MIN_WIDTH));
        let no_sources = self.state.visible_sources.is_empty();
        let body = self.render_list_body(cx);
        let mut pane = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .min_h_0()
            .w(u(width))
            .border_r_1()
            .border_color(theme.colors.stroke)
            .child(self.render_source_row(cx));
        if !no_sources {
            pane = pane.child(self.render_toolbar(cx));
        }
        if let Some(error) = self.state.read_status_error.clone() {
            pane = pane.child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        pane.child(
            div()
                .id("inbox-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.list_scroll)
                .child(body),
        )
        .child(
            resize_handle(
                "inbox-resize",
                ResizeEdge::Right,
                6.,
                self.resize.dragging(),
                cx,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count >= 2 {
                        let width = this.resize.reset();
                        REMEMBERED_WIDTH.with(|cell| cell.set(width));
                    } else {
                        this.resize.begin(f32::from(event.position.x));
                    }
                    cx.notify();
                }),
            ),
        )
        .into_any_element()
    }
}

impl Render for InboxView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let scale = theme.ui_scale();
        let viewport = f32::from(window.viewport_size().width) / scale;
        let max = MAX_WIDTH.min((viewport * 0.5).round());
        let list = self.render_list(window, cx);
        let detail: AnyElement = match &self.detail {
            Some((_, detail, _)) => detail.clone().into_any_element(),
            None => empty_detail(cx),
        };
        let mut main = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(div().flex_1().min_h_0().min_w_0().child(detail));
        if self.discussion_open
            && let Some((_, panel, _)) = &self.discussion
        {
            main = main.child(panel.clone());
        }
        div()
            .id("inbox-view")
            .key_context("InboxView")
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if this.resize.drag_to(f32::from(event.position.x), scale, max) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(width) = this.resize.end() {
                        REMEMBERED_WIDTH.with(|cell| cell.set(width));
                        cx.notify();
                    }
                }),
            )
            .child(self.render_title_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(list)
                    .child(main),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{InboxItem, InboxKind};

    fn listed(key: &str) -> ListedItem {
        ListedItem {
            key: key.into(),
            item: InboxItem::github(InboxKind::Issue, "acme/web", 1, key),
            unseen: false,
            related_sessions: Vec::new(),
            project_mark: Default::default(),
        }
    }

    #[test]
    fn selects_the_first_card_unless_a_target_is_loading() {
        let visible = [listed("a"), listed("b")];
        assert_eq!(resolve_selection(&visible, None, None).unwrap().key, "a");
        assert_eq!(
            resolve_selection(&visible, Some("b"), None).unwrap().key,
            "b"
        );
        assert!(resolve_selection(&visible, Some("t"), Some("t")).is_none());
        assert_eq!(
            resolve_selection(&visible, Some("x"), Some("t"))
                .unwrap()
                .key,
            "a"
        );
    }

    #[test]
    fn explains_an_empty_list() {
        let mut state = InboxListState::default();
        assert_eq!(
            empty_list_message(&state, "", 0),
            "Open a project to fill the inbox"
        );
        assert_eq!(
            empty_list_message(&state, "bug", 1),
            "No matching issues or pull requests"
        );
        state.source = InboxProvider::Linear;
        assert_eq!(empty_list_message(&state, "", 1), "No Linear issues");
        assert_eq!(
            empty_list_message(&state, "x", 1),
            "No matching Linear issues"
        );
        state.source = InboxProvider::Gitlab;
        state.filters_active = true;
        state.filters.assigned_to_me = true;
        assert_eq!(
            empty_list_message(&state, "", 1),
            "Nothing needs your attention"
        );
        state.filters.assigned_to_me = false;
        assert_eq!(
            empty_list_message(&state, "", 1),
            "No GitLab items match these filters"
        );
    }

    #[test]
    fn mounts_lists_a_page_at_a_time() {
        assert_eq!(list_window_size(0, 32), 0);
        assert_eq!(list_window_size(10, 32), 10);
        assert_eq!(list_window_size(100, 32), 32);
        assert_eq!(list_window_size(100, 64), 64);
    }
}
