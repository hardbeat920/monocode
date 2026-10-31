//! Port of src/features/settings/ui/SettingsView.tsx and
//! src/app/shell/SettingsRail.tsx: the Settings pages, their rail and the
//! breadcrumb, search and actions the inset layout puts in its title bar.
mod data;
mod keymap;
#[cfg(test)]
mod layout_tests;
mod management;
mod model;
mod notification_preferences;
mod pages;
mod palettes;
mod preferences;
mod skill_document;
pub(crate) use palettes::{PALETTES, palette};
pub(crate) mod widgets;

pub(crate) use keymap::apply as apply_keybindings;
pub(crate) use model::Section;
pub(crate) use pages::{ACCENTS, command_shortcut, swatch};
pub(crate) use preferences::{ModelChoice, Preferences, UI_SCALE_PERCENTS};

use crate::tooltip::Tip;
use crate::{
    input::{self, Input},
    popover::{Align, Anchor, AnchorSlot, Side, popover},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseMoveEvent, Role, ScrollHandle, SharedString, Subscription, Task, Window, div, prelude::*,
    px,
};
use std::{cell::RefCell, collections::HashMap, time::Duration};

/// A session the Archive page lists.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArchivedSession {
    pub id: i64,
    pub title: String,
    pub provider: String,
    pub updated_at: u64,
}

/// What Settings shows about the workspace around it.
#[derive(Clone, Debug, Default)]
pub(crate) struct SettingsContext {
    /// The desktop's shared remote connection and SSH setup service.
    pub remote: Option<std::sync::Arc<monocode_core::remote::Connections>>,
    pub monos: Vec<MonoSetting>,
    pub classic: bool,
    pub new_layout_boxy: bool,
    /// The selected project, or empty outside one.
    pub cwd: String,
    pub project_label: String,
    pub archived: Vec<ArchivedSession>,
    /// Projects archived from the rail, as (path, label).
    pub archived_projects: Vec<(String, String)>,
    /// Projects the Worktrees, Providers and Inbox pages offer.
    pub projects: Vec<(String, String)>,
    pub sessions: Vec<(i64, String, bool)>,
    pub quick_error: Option<String>,
    pub catalogs: HashMap<String, Vec<(String, String)>>,
    pub inbox_connections: HashMap<String, (bool, String)>,
    /// Mobile access, or `None` when this desktop cannot offer it.
    pub mobile: Option<monocode_core::mobile::Status>,
    /// The pairing code on show.
    pub mobile_invite: Option<monocode_core::mobile::Invite>,
}
impl PartialEq for SettingsContext {
    fn eq(&self, other: &Self) -> bool {
        match (&self.remote, &other.remote) {
            (Some(a), Some(b)) if !std::sync::Arc::ptr_eq(a, b) => return false,
            (None, Some(_)) | (Some(_), None) => return false,
            _ => {}
        }
        self.monos == other.monos
            && self.classic == other.classic
            && self.new_layout_boxy == other.new_layout_boxy
            && self.cwd == other.cwd
            && self.project_label == other.project_label
            && self.archived == other.archived
            && self.archived_projects == other.archived_projects
            && self.projects == other.projects
            && self.sessions == other.sessions
            && self.quick_error == other.quick_error
            && self.catalogs == other.catalogs
            && self.inbox_connections == other.inbox_connections
            && self.mobile == other.mobile
            && self.mobile_invite == other.mobile_invite
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MonoSetting {
    pub id: String,
    pub name: String,
    pub mascot: String,
    pub color: u32,
    pub projects: Vec<String>,
    pub show: bool,
    pub folders: bool,
}

pub(crate) enum SettingsEvent {
    WhatsNew,
    CheckAppUpdates,
    CheckHarnessUpdates,
    ManageInboxProvider(String),
    Changed(Box<Preferences>),
    /// The Appearance page's Layout picker: true for Focused, false for Classic.
    NewLayout(bool),
    NewLayoutBoxy(bool),
    Section(Section),
    OpenSession(i64),
    Archive(i64, bool),
    Delete(i64),
    UnarchiveProject(String),
    DeleteProject(String),
    ChooseBackground,
    RemoveWorktree {
        cwd: String,
        path: String,
        delete_sessions: bool,
    },
    ResetMono(String),
    MonoSidebar(String, bool, bool),
    /// Read mobile access again: the router and pairing change on their own.
    MobileRefresh,
    MobileConfigure {
        enabled: bool,
        open_router: bool,
        remote_address: String,
    },
    /// Start listening when needed, then make a new pairing code.
    MobilePair,
    MobileCancelInvite,
    MobileRevoke(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SliderKind {
    Hue,
    Saturation,
    DarkLightness,
    SidebarOpacity,
    BackgroundEmpty,
    BackgroundSession,
}
impl SliderKind {
    fn id(self) -> &'static str {
        match self {
            SliderKind::Hue => "hue",
            SliderKind::Saturation => "saturation",
            SliderKind::DarkLightness => "dark-lightness",
            SliderKind::SidebarOpacity => "sidebar-opacity",
            SliderKind::BackgroundEmpty => "background-empty",
            SliderKind::BackgroundSession => "background-session",
        }
    }
    fn label(self) -> &'static str {
        match self {
            SliderKind::Hue => "Hue",
            SliderKind::Saturation => "Saturation",
            SliderKind::DarkLightness => "Dark-mode lightness",
            SliderKind::SidebarOpacity => "Sidebar opacity",
            SliderKind::BackgroundEmpty => "Empty chat background visibility",
            SliderKind::BackgroundSession => "Session background visibility",
        }
    }
    fn range(self) -> (u32, u32) {
        match self {
            SliderKind::Hue => (0, 360),
            SliderKind::Saturation => (0, 100),
            SliderKind::DarkLightness => (0, 30),
            SliderKind::SidebarOpacity => (15, 100),
            SliderKind::BackgroundEmpty | SliderKind::BackgroundSession => (5, 65),
        }
    }
    fn get(self, p: &Preferences) -> u32 {
        match self {
            SliderKind::Hue => p.theme_hue,
            SliderKind::Saturation => p.theme_saturation,
            SliderKind::DarkLightness => p.theme_dark_lightness,
            SliderKind::SidebarOpacity => p.sidebar_opacity,
            SliderKind::BackgroundEmpty => p.chat_background_empty,
            SliderKind::BackgroundSession => p.chat_background_session,
        }
    }
    fn set(self, p: &mut Preferences, value: u32) {
        match self {
            SliderKind::Hue => p.theme_hue = value,
            SliderKind::Saturation => p.theme_saturation = value,
            SliderKind::DarkLightness => p.theme_dark_lightness = value,
            SliderKind::SidebarOpacity => p.sidebar_opacity = value,
            SliderKind::BackgroundEmpty => p.chat_background_empty = value,
            SliderKind::BackgroundSession => p.chat_background_session = value,
        }
    }
}

pub(crate) struct SettingsView {
    pub(crate) section: Section,
    prefs: Preferences,
    notification_state: notification_preferences::State,
    ctx: SettingsContext,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// The row or group search just landed on, flashing for 1.8s.
    revealed: Option<String>,
    pending_scroll: bool,
    _reveal: Option<Task<()>>,
    targets: RefCell<HashMap<&'static str, AnchorSlot>>,
    slider_tracks: RefCell<HashMap<SliderKind, AnchorSlot>>,
    slider_drag: Option<SliderKind>,
    select_open: Option<&'static str>,
    select_active: usize,
    select_focus: FocusHandle,
    select_anchors: RefCell<HashMap<&'static str, AnchorSlot>>,
    search: Entity<Input>,
    search_query: String,
    search_active: usize,
    search_anchor: AnchorSlot,
    keybinding_filter: Entity<Input>,
    keybinding_query: String,
    /// The Keybindings row recording new keys.
    recording: Option<String>,
    recording_preview: String,
    recorder_focus: FocusHandle,
    recording_error: Option<String>,
    management: Option<management::Editor>,
    management_generation: u64,
    binary_anchors: RefCell<HashMap<monocode_core::provider::Kind, AnchorSlot>>,
    mcp_show_all: bool,
    mcp_remove_scopes: HashMap<String, String>,
    mcp_busy: bool,
    mcp_health: Option<Task<()>>,
    discovery: data::Discovery,
    discovery_loading: bool,
    discovery_task: Option<Task<()>>,
    selected_project: String,
    provider_scope: String,
    mcp_filter: String,
    fields: HashMap<&'static str, Entity<Input>>,
    connections: pages::ConnectionsState,
    skill_preview: Option<std::path::PathBuf>,
    skill_preview_text: Option<Result<std::sync::Arc<skill_document::DocumentData>, String>>,
    skill_preview_generation: u64,
    skill_preview_task: Option<Task<()>>,
    skill_documents: HashMap<(std::path::PathBuf, bool), Entity<skill_document::SkillDocument>>,
    skill_surface: AnchorSlot,
    skill_filter: Entity<Input>,
    skill_query: String,
    accent_custom: Entity<Input>,
    accent_custom_open: bool,
    accent_anchor: AnchorSlot,
    mobile: pages::MobileState,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<SettingsEvent> for SettingsView {}
impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsView {
    pub(crate) fn new(
        section: Section,
        prefs: Preferences,
        ctx: SettingsContext,
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(center) = crate::accounts::get(cx) {
            center.update(cx, |center, _| center.hide_emails());
        }
        let search = cx.new(|cx| {
            let mut input = Input::field("Search settings", "Search settings", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let keybinding_filter = cx.new(|cx| {
            let mut input = Input::field("Filter keybindings", "Filter", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let accent_custom = cx.new(|cx| {
            let mut input = Input::field("Custom accent color", "#4da3f5", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let skill_filter = cx.new(|cx| {
            let mut input = Input::field("Filter skills", "Filter", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let mut subscriptions = vec![
            cx.subscribe(&skill_filter, |this, input, _: &input::Changed, cx| {
                this.skill_query = input.read(cx).content.to_string();
                cx.notify();
            }),
            cx.subscribe(&search, |this, input, _: &input::Changed, cx| {
                this.search_query = input.read(cx).content.to_string();
                this.search_active = 0;
                cx.notify();
            }),
            cx.subscribe(&search, |this, _, _: &input::Submitted, cx| {
                let results = model::search(&this.search_query, cx.compositor_name());
                if let Some(result) = results.get(this.search_active).cloned() {
                    this.go(result, cx);
                }
            }),
            cx.subscribe(&keybinding_filter, |this, input, _: &input::Changed, cx| {
                this.keybinding_query = input.read(cx).content.to_string();
                cx.notify();
            }),
            cx.subscribe(&accent_custom, |this, input, _: &input::Changed, cx| {
                let text = input.read(cx).content.trim().to_owned();
                if pages::parse_hex(&text).is_some() {
                    this.update(cx, |p| p.accent_color = Some(text.to_lowercase()));
                }
            }),
        ];
        if let Some(center) = crate::accounts::get(cx) {
            subscriptions.push(cx.observe(&center, |_, _, cx| cx.notify()));
        }
        let recorder = cx.weak_entity();
        subscriptions.push(cx.intercept_keystrokes(move |event, window, cx| {
            let _ = recorder.update(cx, |this, cx| {
                if this.recording.is_some() && this.recorder_focus.is_focused(window) {
                    this.record_key(&event.keystroke, cx);
                    if this.recording.is_none() {
                        window.focus(&this.focus, cx);
                    }
                }
            });
        }));
        let selected_project = ctx.cwd.clone();
        let mut this = Self {
            section,
            prefs,
            notification_state: notification_preferences::State::default(),
            ctx,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            revealed: None,
            pending_scroll: false,
            _reveal: None,
            targets: Default::default(),
            slider_tracks: Default::default(),
            slider_drag: None,
            select_open: None,
            select_active: 0,
            select_focus: cx.focus_handle(),
            select_anchors: Default::default(),
            search,
            search_query: String::new(),
            search_active: 0,
            search_anchor: AnchorSlot::default(),
            keybinding_filter,
            keybinding_query: String::new(),
            recording: None,
            recording_preview: String::new(),
            recorder_focus: cx.focus_handle(),
            recording_error: None,
            management: None,
            management_generation: 0,
            binary_anchors: RefCell::default(),
            mcp_show_all: false,
            mcp_remove_scopes: HashMap::new(),
            mcp_busy: false,
            mcp_health: None,
            discovery: data::Discovery::default(),
            discovery_loading: false,
            discovery_task: None,
            selected_project,
            provider_scope: "global".into(),
            mcp_filter: "all".into(),
            fields: HashMap::new(),
            connections: pages::ConnectionsState::default(),
            skill_preview: None,
            skill_preview_text: None,
            skill_preview_generation: 0,
            skill_preview_task: None,
            skill_documents: HashMap::new(),
            skill_surface: AnchorSlot::default(),
            skill_filter,
            skill_query: String::new(),
            accent_custom,
            accent_custom_open: false,
            accent_anchor: AnchorSlot::default(),
            mobile: pages::MobileState::default(),
            _subscriptions: subscriptions,
        };
        this.watch_mobile(cx);
        this.watch_connections(cx);
        if this.prefs.notifications {
            this.probe_notification_permission(false, cx);
        }
        this.notification_deadline(cx);
        this.refresh(cx);
        this
    }
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.section == Section::Providers
            && let Some(center) = crate::accounts::get(cx)
        {
            center.update(cx, |center, cx| {
                for kind in monocode_core::accounts::PROVIDERS {
                    center.load_provider(kind, true, cx);
                }
            });
        }
        let cwd = self.selected_project.clone();
        self.discovery_loading = true;
        let work = cx
            .background_executor()
            .spawn(async move { data::discover(&cwd) });
        self.discovery_task = Some(cx.spawn(async move |this, cx| {
            let data = work.await;
            let _ = this.update(cx, |this, cx| {
                this.skill_documents.clear();
                this.discovery = data;
                this.discovery_loading = false;
                if this.section == Section::Mcp {
                    this.refresh_mcp_health(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(crate) fn set_context(&mut self, ctx: SettingsContext, cx: &mut Context<Self>) {
        if self.ctx != ctx {
            let remote_changed = match (&self.ctx.remote, &ctx.remote) {
                (Some(a), Some(b)) => !std::sync::Arc::ptr_eq(a, b),
                (None, None) => false,
                _ => true,
            };
            self.ctx = ctx;
            self.mobile_context_changed(cx);
            if remote_changed {
                self.leave_connections(cx);
                self.watch_connections(cx);
            }
            cx.notify();
        }
    }
    pub(crate) fn set_preferences(&mut self, prefs: &Preferences, cx: &mut Context<Self>) {
        if &self.prefs != prefs {
            self.prefs = prefs.clone();
            self.notification_deadline(cx);
            cx.notify();
        }
    }
    pub(crate) fn select_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            if let Some(center) = crate::accounts::get(cx) {
                center.update(cx, |center, _| center.hide_emails());
            }
            self.management = None;
            self.management_generation += 1;
            self.leave_mobile(cx);
            self.leave_connections(cx);
            self.section = section;
            self.watch_mobile(cx);
            self.watch_connections(cx);
            if section == Section::Mcp {
                self.refresh_mcp_health(cx);
            }
            self.select_open = None;
            self.recording = None;
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
            cx.notify();
        }
    }
    /// Consume Escape at the innermost Settings surface before the shell closes Settings.
    pub(crate) fn dismiss_transient(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.dismiss_notification_transient(window, cx) {
            return true;
        }
        if self.dismiss_mobile(cx) {
            return true;
        }
        if self.dismiss_connections(cx) {
            return true;
        }
        if self.recording.take().is_some() {
            self.recording_preview.clear();
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if self.select_open.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.search
                .update(cx, |i, cx| i.set_text(String::new(), cx));
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        if self.management.is_some() {
            self.close_management(window, cx);
            return true;
        }
        if self.skill_preview.take().is_some() {
            window.focus(&self.skill_filter.read(cx).focus_handle(cx), cx);
            cx.notify();
            return true;
        }
        false
    }
    /// Change preferences and tell the shell to keep them.
    fn update(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Preferences)) {
        change(&mut self.prefs);
        self.notification_deadline(cx);
        cx.emit(SettingsEvent::Changed(Box::new(self.prefs.clone())));
        cx.notify();
    }
    fn restore_defaults(&mut self, cx: &mut Context<Self>) {
        self.accent_custom_open = false;
        self.update(cx, |p| p.restore_appearance());
        // onInsetLayout(INSET_LAYOUT_DEFAULT): React's default is the classic layout.
        cx.emit(SettingsEvent::NewLayout(false));
    }
    /// Jump to a search result, on this page or another.
    fn go(&mut self, result: model::SearchResult, cx: &mut Context<Self>) {
        if result.section != self.section {
            self.select_section(result.section, cx);
            cx.emit(SettingsEvent::Section(result.section));
        }
        self.revealed = result.setting.map(str::to_owned);
        self.pending_scroll = self.revealed.is_some();
        self.search_query.clear();
        self.search
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self._reveal = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1800))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.revealed = None;
                this.pending_scroll = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Centre the revealed row in the page once it has been laid out.
    fn scroll_to_revealed(&mut self) {
        let Some(id) = self.revealed.clone() else {
            self.pending_scroll = false;
            return;
        };
        let target = self.targets.borrow().get(id.as_str()).and_then(|s| s.get());
        let viewport = self.scroll.bounds();
        self.pending_scroll = false;
        let Some(target) = target else {
            return;
        };
        let offset = self.scroll.offset();
        let delta = target.center().y - viewport.center().y;
        let max = self.scroll.max_offset().y;
        let y = (offset.y - delta).clamp(-max, px(0.));
        self.scroll.set_offset(gpui::point(offset.x, y));
    }

    /// The breadcrumb, Restore defaults and search: in the inset title bar,
    /// or the page's own 40px header row in the classic layout.
    pub(crate) fn header(&mut self, in_title_bar: bool, cx: &mut Context<Self>) -> AnyElement {
        let results = model::search(&self.search_query, cx.compositor_name());
        let open = !self.search_query.trim().is_empty();
        let weak = cx.weak_entity();
        div()
            .id("settings-header")
            .debug_selector(|| "settings-header".into())
            .h_full()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .child(
                crate::shell::titlebar_drag_area()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(13.))
                    .map(|d| {
                        if in_title_bar {
                            d.pr_3().pl_1()
                        } else {
                            d.px_3()
                        }
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(content(0.45))
                            .child("Settings"),
                    )
                    .child(div().flex_shrink_0().text_color(content(0.25)).child("/"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(content(1.))
                            .child(self.section.label()),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .map(|d| if in_title_bar { d.pr_4() } else { d.pr_2() })
                    .when(self.section == Section::Appearance, |d| {
                        d.child(
                            div()
                                .id("restore-defaults")
                                .role(Role::Button)
                                .aria_label("Restore defaults")
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .gap(px(6.))
                                .rounded_md()
                                .px_2()
                                .py_1()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(content(0.5))
                                .cursor_pointer()
                                .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                                .child(
                                    icon("rotate-ccw", 14.)
                                        .text_color(gpui::transparent_black())
                                        .map(|i| i.text_color(content(0.5))),
                                )
                                .child("Restore defaults")
                                .on_click(cx.listener(|this, _, _, cx| this.restore_defaults(cx))),
                        )
                    })
                    .child(
                        div()
                            .id("settings-search")
                            .relative()
                            .flex_shrink_0()
                            .child(self.search_anchor.probe())
                            .child(
                                div()
                                    .h(px(28.))
                                    .w(px(192.))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .px_2()
                                    .child(icon("search-01", 14.).text_color(content(0.45)))
                                    .child(div().min_w_0().flex_1().child(self.search.clone()))
                                    .when(!self.search_query.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .id("clear-settings-search")
                                                .role(Role::Button)
                                                .aria_label("Clear settings search")
                                                .tip("Clear settings search")
                                                .size_4()
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_sm()
                                                .cursor_pointer()
                                                .child(icon("x", 12.).text_color(content(0.45)))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.search.update(cx, |i, cx| {
                                                        i.set_text(String::new(), cx)
                                                    });
                                                    this.search_query.clear();
                                                    window.focus(&this.search.focus_handle(cx), cx);
                                                    cx.notify();
                                                })),
                                        )
                                    }),
                            )
                            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                                if this.search_query.trim().is_empty() {
                                    return;
                                }
                                let count =
                                    model::search(&this.search_query, cx.compositor_name()).len();
                                match event.keystroke.key.as_str() {
                                    "down" => {
                                        this.search_active =
                                            (this.search_active + 1).min(count.saturating_sub(1))
                                    }
                                    "up" => {
                                        this.search_active = this.search_active.saturating_sub(1)
                                    }
                                    "escape" => {
                                        this.search_query.clear();
                                        this.search
                                            .update(cx, |i, cx| i.set_text(String::new(), cx));
                                    }
                                    _ => return,
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }))
                            .when(open, |d| {
                                let active = self.search_active;
                                d.child(
                                    popover(
                                        "settings-search-results",
                                        Anchor::Slot(self.search_anchor.clone()),
                                    )
                                    .side(Side::Bottom)
                                    .align(Align::End)
                                    .width(300.)
                                    .max_height(320.)
                                    .padding(4.)
                                    .on_dismiss(move |_, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.search_query.clear();
                                            this.search
                                                .update(cx, |i, cx| i.set_text(String::new(), cx));
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        div()
                                            .id("settings-search-list")
                                            .role(Role::ListBox)
                                            .aria_label("Settings search results")
                                            .flex()
                                            .flex_col()
                                            .when(results.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .px_2()
                                                        .py(px(6.))
                                                        .text_size(px(12.))
                                                        .line_height(px(16.))
                                                        .text_color(content(0.45))
                                                        .child("No matching settings"),
                                                )
                                            })
                                            .children(results.into_iter().enumerate().map(
                                                |(ix, result)| {
                                                    let tag = if result.setting.is_some() {
                                                        result.section.label()
                                                    } else {
                                                        "Page"
                                                    };
                                                    let label = result.label;
                                                    div()
                                                        .id(("settings-result", ix))
                                                        .debug_selector(move || {
                                                            format!("settings-result-{ix}")
                                                        })
                                                        .role(Role::ListBoxOption)
                                                        .aria_label(label)
                                                        .aria_selected(ix == active)
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .rounded_lg()
                                                        .px_2()
                                                        .py(px(6.))
                                                        .text_size(px(12.))
                                                        .line_height(px(16.))
                                                        .text_color(content(1.))
                                                        .cursor_pointer()
                                                        .map(|d| {
                                                            if ix == active {
                                                                d.bg(content(0.1))
                                                            } else {
                                                                d.hover(|s| s.bg(content(0.05)))
                                                            }
                                                        })
                                                        .child(
                                                            div()
                                                                .min_w_0()
                                                                .flex_1()
                                                                .truncate()
                                                                .child(label),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_shrink_0()
                                                                .text_size(px(11.))
                                                                .text_color(content(0.4))
                                                                .child(tag),
                                                        )
                                                        .on_mouse_move(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if this.search_active != ix {
                                                                    this.search_active = ix;
                                                                    cx.notify();
                                                                }
                                                            },
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.go(result.clone(), cx)
                                                            },
                                                        ))
                                                },
                                            )),
                                    ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// SettingsNav: the rail body while Settings is open.
pub(crate) fn nav(
    section: Section,
    select: impl Fn(Section, &mut Window, &mut App) + Clone + 'static,
    back: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let row = |id: SharedString, label: &'static str, path: &'static str, active: bool| {
        div()
            .id(id.clone())
            .debug_selector(move || id.to_string())
            .role(Role::Button)
            .aria_label(label)
            .aria_selected(active)
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .px_2()
            .py(px(6.))
            .cursor_pointer()
            .map(|d| {
                if active {
                    d.bg(content(0.1)).text_color(content(1.))
                } else {
                    d.text_color(content(0.5))
                        .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                }
            })
            .child(
                gpui::svg()
                    .path(path)
                    .size(px(16.))
                    .flex_shrink_0()
                    .opacity(0.7)
                    .text_color(if active { content(1.) } else { content(0.5) }),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(17.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(label),
            )
    };
    div()
        .id("settings-nav")
        .debug_selector(|| "settings-nav".into())
        .role(Role::Navigation)
        .aria_label("Settings")
        .size_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id("settings-nav-scroll")
                .min_h_0()
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_5()
                .px_2()
                .py_3()
                .children(model::GROUPS.iter().map(|(group, sections)| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(content(0.35))
                                .child(*group),
                        )
                        .children(sections.iter().map(|&item| {
                            let select = select.clone();
                            row(
                                format!("settings-nav-{}", item.id()).into(),
                                item.label(),
                                item.icon(),
                                item == section,
                            )
                            .on_click(move |_, window, cx| select(item, window, cx))
                        }))
                }))
                .into_any_element(),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_px()
                .p_2()
                .child(
                    row("settings-nav-back".into(), "Back", "arrow-left", false)
                        .on_click(move |_, window, cx| back(window, cx)),
                ),
        )
        .into_any_element()
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_scroll {
            let weak = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.scroll_to_revealed();
                    cx.notify();
                });
            });
        }
        self.notification_window_state(window, cx);
        self.connections_window_state(window, cx);
        let page = self.page(window, cx);
        let narrow = self
            .scroll
            .bounds()
            .size
            .width
            .as_f32()
            .finite_and(|w| w > 0. && w < 560.);
        div()
            .id("settings-view")
            .debug_selector(|| "settings-view".into())
            .role(Role::Region)
            .aria_label("Settings")
            .track_focus(if self.recording.is_some() {
                &self.recorder_focus
            } else {
                &self.focus
            })
            .key_context("SettingsView")
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if let Some(kind) = this.slider_drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.slide(kind, event.position.x, cx);
                    } else {
                        this.slider_drag = None;
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.slider_drag = None),
            )
            .children(if self.section == Section::Skills {
                page
            } else {
                vec![
                    div()
                        .id("settings-scroll")
                        .min_h_0()
                        .flex_1()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            div()
                                .mx_auto()
                                .w_full()
                                .max_w(px(1024.))
                                .map(|d| {
                                    if narrow {
                                        d.px_5().pt_6().pb_16()
                                    } else {
                                        d.px_8().pt_8().pb_16()
                                    }
                                })
                                .child(widgets::page_header(
                                    self.section.label(),
                                    self.section.description(),
                                ))
                                .children(page),
                        )
                        .into_any_element(),
                ]
            })
            .children(self.management_overlay(window, cx))
            .children(self.notification_overlay(cx))
    }
}

trait FiniteAnd {
    fn finite_and(self, f: impl FnOnce(f32) -> bool) -> bool;
}
impl FiniteAnd for f32 {
    fn finite_and(self, f: impl FnOnce(f32) -> bool) -> bool {
        self.is_finite() && f(self)
    }
}
