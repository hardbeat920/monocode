//! Port of `SettingsView` in SettingsView.tsx: the header with the section
//! breadcrumb, Restore defaults, and search, then the open section in a
//! scrolling column.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div, point,
    prelude::FluentBuilder as _,
};
use monocode_core::Platform;
use monocode_core::settings::{
    SettingsSectionId, settings_section_description, settings_section_label,
};
use monocode_settings::Kv;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::appearance_page::AppearanceSection;
use super::appearance_state::AppearanceState;
use super::archive::ArchiveSection;
use super::chat::ChatSection;
use super::chrome::{Anchors, NARROW_WIDTH, Reveal, card_note, group, page_header};
use super::controls::{HostsGlobal, css_px};
use super::general::GeneralSection;
use super::host::{SettingsCallbacks, SettingsHosts, SettingsProps, SlotContext, ViewSlot};
use super::inbox::InboxSection;
use super::keybindings::KeybindingsSection;
use super::providers::ProvidersSection;
use super::search::SettingsSearch;
use super::section::{RevealState, SectionContext};

/// How long a revealed row stays highlighted.
pub const REVEAL_DURATION: Duration = Duration::from_millis(1800);

/// The open section's view.
#[derive(Clone)]
pub enum SectionBody {
    General(Entity<GeneralSection>),
    Chat(Entity<ChatSection>),
    Appearance(Entity<AppearanceSection>),
    Keybindings(Entity<KeybindingsSection>),
    Providers(Entity<ProvidersSection>),
    Inbox(Entity<InboxSection>),
    Archive(Entity<ArchiveSection>),
    /// A page another crate draws (connections, MCP, worktrees, skills).
    Slot(AnyView),
    /// A slot nobody filled.
    Missing(SettingsSectionId),
}

impl SectionBody {
    fn view(&self) -> Option<AnyView> {
        Some(match self {
            SectionBody::General(view) => view.clone().into(),
            SectionBody::Chat(view) => view.clone().into(),
            SectionBody::Appearance(view) => view.clone().into(),
            SectionBody::Keybindings(view) => view.clone().into(),
            SectionBody::Providers(view) => view.clone().into(),
            SectionBody::Inbox(view) => view.clone().into(),
            SectionBody::Archive(view) => view.clone().into(),
            SectionBody::Slot(view) => view.clone(),
            SectionBody::Missing(_) => return None,
        })
    }
}

pub struct SettingsPage {
    kv: Kv,
    platform: Platform,
    hosts: SettingsHosts,
    props: SettingsProps,
    callbacks: SettingsCallbacks,
    section: SettingsSectionId,
    anchor: Option<SharedString>,
    reveal: Entity<RevealState>,
    anchors: Anchors,
    reveal_timer: Option<Task<()>>,
    pending_scroll: Rc<RefCell<Option<SharedString>>>,
    appearance: Entity<AppearanceState>,
    search: Entity<SettingsSearch>,
    body: SectionBody,
    scroll: ScrollHandle,
    focus: FocusHandle,
}

impl SettingsPage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kv: Kv,
        platform: Platform,
        hosts: SettingsHosts,
        section: SettingsSectionId,
        props: SettingsProps,
        callbacks: SettingsCallbacks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.set_global(HostsGlobal(hosts.clone()));
        let anchors = Anchors::default();
        let reveal = cx.new(|_| {
            RevealState(Reveal {
                anchors: anchors.clone(),
                ..Default::default()
            })
        });
        let appearance_host = hosts.appearance.clone();
        let appearance_kv = kv.clone();
        let appearance = cx.new(|cx| {
            let mut state = AppearanceState::new(appearance_kv, platform, appearance_host, cx);
            state.set_controlled_rail_mode(
                props.collapsed_project_rail_mode,
                callbacks.on_collapsed_project_rail_mode_change.clone(),
            );
            state
        });
        let page = cx.entity().downgrade();
        let search = cx.new(|cx| {
            SettingsSearch::new(platform, window, cx).on_reveal(Rc::new(
                move |section, setting_id, window, cx| {
                    page.update(cx, |page, cx| {
                        page.on_reveal(section, setting_id, window, cx)
                    })
                    .ok();
                },
            ))
        });
        let mut this = Self {
            kv,
            platform,
            hosts,
            props,
            callbacks,
            section,
            anchor: None,
            reveal,
            anchors,
            reveal_timer: None,
            pending_scroll: Rc::new(RefCell::new(None)),
            appearance,
            search,
            body: SectionBody::Missing(section),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
        };
        this.body = this.build_body(window, cx);
        this
    }

    pub fn section(&self) -> SettingsSectionId {
        self.section
    }

    pub fn body(&self) -> &SectionBody {
        &self.body
    }

    pub fn appearance(&self) -> &Entity<AppearanceState> {
        &self.appearance
    }

    pub fn search(&self) -> &Entity<SettingsSearch> {
        &self.search
    }

    pub fn anchors(&self) -> &Anchors {
        &self.anchors
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn kv(&self) -> &Kv {
        &self.kv
    }

    /// The row or group currently highlighted.
    pub fn revealed(&self, cx: &App) -> Option<SharedString> {
        self.reveal.read(cx).0.revealed.clone()
    }

    fn section_context(&self) -> SectionContext {
        SectionContext {
            kv: self.kv.clone(),
            platform: self.platform,
            hosts: self.hosts.clone(),
            reveal: self.reveal.clone(),
        }
    }

    fn slot_context(&self, cx: &App) -> SlotContext {
        SlotContext {
            section: self.section,
            cwd: self.props.cwd.clone(),
            recents: self.props.recents.clone(),
            notification_project_path: self.props.notification_project_path.clone(),
            notification_settings_request: self.props.notification_settings_request,
            highlighted: self.revealed(cx).as_deref() == Some("project-notifications"),
        }
    }

    fn slot(&self, slot: Option<&ViewSlot>, window: &mut Window, cx: &mut App) -> SectionBody {
        match slot {
            Some(build) => SectionBody::Slot(build(&self.slot_context(cx), window, cx)),
            None => SectionBody::Missing(self.section),
        }
    }

    fn build_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> SectionBody {
        let ctx = self.section_context();
        let slot = self.slot_context(cx);
        match self.section {
            SettingsSectionId::General => {
                let open = self.callbacks.on_open_whats_new.clone();
                SectionBody::General(cx.new(|cx| GeneralSection::new(ctx, open, window, cx)))
            }
            SettingsSectionId::Chat => SectionBody::Chat(cx.new(|cx| ChatSection::new(ctx, cx))),
            SettingsSectionId::Appearance => {
                let appearance = self.appearance.clone();
                SectionBody::Appearance(
                    cx.new(|cx| AppearanceSection::new(ctx, appearance, window, cx)),
                )
            }
            SettingsSectionId::Keybindings => {
                SectionBody::Keybindings(cx.new(|cx| KeybindingsSection::new(ctx, window, cx)))
            }
            SettingsSectionId::Providers => {
                SectionBody::Providers(cx.new(|cx| ProvidersSection::new(ctx, &slot, window, cx)))
            }
            SettingsSectionId::Inbox => {
                SectionBody::Inbox(cx.new(|cx| InboxSection::new(ctx, &slot, window, cx)))
            }
            SettingsSectionId::Archive => {
                let (cwd, sessions, callbacks) = (
                    self.props.cwd.clone(),
                    self.props.sessions.clone(),
                    self.callbacks.clone(),
                );
                SectionBody::Archive(
                    cx.new(|cx| ArchiveSection::new(ctx, cwd, sessions, callbacks, cx)),
                )
            }
            SettingsSectionId::Connections => {
                let slot = self.hosts.connections.clone();
                self.slot(slot.as_ref(), window, cx)
            }
            SettingsSectionId::Mcp => {
                let slot = self.hosts.mcp.clone();
                self.slot(slot.as_ref(), window, cx)
            }
            SettingsSectionId::Skills => {
                let slot = self.hosts.skills.clone();
                self.slot(slot.as_ref(), window, cx)
            }
            SettingsSectionId::Worktrees => {
                let slot = self.hosts.worktrees.clone();
                self.slot(slot.as_ref(), window, cx)
            }
        }
    }

    /// The `section` prop changed: mount the new section.
    pub fn set_section(
        &mut self,
        section: SettingsSectionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if section == self.section {
            return;
        }
        self.section = section;
        self.anchors.clear();
        self.scroll.set_offset(point(gpui::px(0.), gpui::px(0.)));
        self.body = self.build_body(window, cx);
        // The reveal effect re-runs for the new page, so a search result on
        // another page scrolls once that page has mounted its row.
        let revealed = self.revealed(cx);
        if revealed.is_some() {
            self.schedule_scroll(revealed, cx);
        }
        cx.notify();
    }

    /// The `anchor` prop: the row Settings should reveal when it opens.
    pub fn set_anchor(&mut self, anchor: Option<impl Into<SharedString>>, cx: &mut Context<Self>) {
        self.anchor = anchor.map(Into::into);
        self.reveal(self.anchor.clone(), cx);
    }

    /// The other props. A new notification request reveals the anchor again.
    pub fn set_props(&mut self, props: SettingsProps, cx: &mut Context<Self>) {
        let request_changed =
            props.notification_settings_request != self.props.notification_settings_request;
        let sessions_changed = props.sessions != self.props.sessions;
        self.appearance.update(cx, |state, _| {
            state.set_controlled_rail_mode(
                props.collapsed_project_rail_mode,
                self.callbacks.on_collapsed_project_rail_mode_change.clone(),
            )
        });
        self.props = props;
        if sessions_changed && let SectionBody::Archive(archive) = &self.body {
            let sessions = self.props.sessions.clone();
            archive.update(cx, |archive, cx| archive.set_sessions(sessions, cx));
        }
        if request_changed {
            self.reveal(self.anchor.clone(), cx);
        }
        cx.notify();
    }

    /// Re-reads host data (catalogs, availability, archived projects).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let SectionBody::Providers(providers) = &self.body {
            providers.update(cx, |providers, cx| providers.refresh(cx));
        }
        cx.notify();
    }

    fn schedule_scroll(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        let Some(id) = id else {
            return;
        };
        // A project quick action lets the project card focus itself.
        let skip = id.as_ref() == "project-notifications"
            && self.props.notification_project_path.is_some();
        if !skip {
            *self.pending_scroll.borrow_mut() = Some(id);
            cx.notify();
        }
    }

    /// Scrolls a row or card into the middle of the page without the
    /// highlight, once it has been laid out.
    pub fn scroll_to(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        *self.pending_scroll.borrow_mut() = Some(id.into());
        cx.notify();
    }

    /// `setRevealed` and its effect: scroll the row into view, then clear the
    /// highlight after 1.8 seconds.
    pub fn reveal(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        self.reveal.update(cx, |state, cx| {
            state.0.revealed = id.clone();
            cx.notify();
        });
        self.reveal_timer = None;
        if id.is_none() {
            return;
        }
        self.schedule_scroll(id, cx);
        self.reveal_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_DURATION).await;
            this.update(cx, |this, cx| {
                this.reveal.update(cx, |state, cx| {
                    state.0.revealed = None;
                    cx.notify();
                });
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `onReveal`: switch pages when the result is elsewhere, then reveal.
    pub fn on_reveal(
        &mut self,
        section: SettingsSectionId,
        setting_id: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if section != self.section {
            match self.callbacks.on_select_section.clone() {
                Some(select) => select(section, window, cx),
                // A page without a controlling owner switches itself.
                None => self.set_section(section, window, cx),
            }
        }
        self.reveal(setting_id.map(SharedString::from), cx);
    }

    fn header(&mut self, cx: &mut Context<Self>, window: &mut Window) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mac = self.platform.is_mac();
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let restore = (self.section == SettingsSectionId::Appearance).then(|| {
            div()
                .id("restore-defaults")
                .flex()
                .flex_none()
                .items_center()
                .gap(u(6.))
                .px(u(8.))
                .py(u(4.))
                .rounded(u(theme.radius.md))
                .text_px(theme.text.label)
                .text_color(theme.content(0.50))
                .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                .debug_selector(|| "button:restore-defaults".into())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.appearance
                        .update(cx, |state, cx| state.restore_defaults(window, cx))
                }))
                .child(
                    icon(IconName::RotateCcw)
                        .size(u(14.))
                        .text_color(theme.content(0.50)),
                )
                .child("Restore defaults")
        });
        let window_controls = (!mac)
            .then(|| self.hosts.window_controls.clone())
            .flatten()
            .map(|build| build(&self.slot_context(cx), window, cx));
        div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.title_bar_height))
            .items_center()
            .border_b_1()
            .border_color(theme.colors.stroke)
            .when(mac && !self.props.beside_rail, |el| {
                el.child(div().flex_none().w(u(theme.metrics.traffic_light_inset)))
            })
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .flex_1()
                    .items_center()
                    .gap(u(8.))
                    .px(u(12.))
                    .text_px(theme.text.body)
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.content(0.45))
                            .child("Settings"),
                    )
                    .child(div().flex_none().text_color(theme.content(0.25)).child("/"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.colors.content)
                            .child(settings_section_label(self.section)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(6.))
                    .pr(u(8.))
                    .children(restore)
                    .child(self.search.clone()),
            )
            .children(window_controls)
            .into_any_element()
    }

    /// The placeholder for a page another crate has not plugged in yet.
    fn missing(&self, section: SettingsSectionId, cx: &mut Context<Self>) -> AnyElement {
        let reveal = self.reveal.read(cx).0.clone();
        let mut card = group(&reveal, settings_section_label(section)).first(true);
        if section == SettingsSectionId::Worktrees {
            card = card.id("project-worktrees");
        }
        if section == SettingsSectionId::Mcp {
            card = card.id("mcp-servers");
        }
        if section == SettingsSectionId::Connections {
            card = card.id("remote-machines");
        }
        card.child(card_note("This page is not available in this build.", cx))
            .into_any_element()
    }

    /// Measures the content width (the `@container/settings` query) and
    /// performs a pending scroll once the target row has bounds.
    fn layout_probe(&self) -> impl IntoElement + use<> {
        let reveal = self.reveal.clone();
        let pending = self.pending_scroll.clone();
        let anchors = self.anchors.clone();
        let scroll = self.scroll.clone();
        canvas(
            move |bounds, window, cx| {
                let narrow = css_px(bounds.size.width, window) < NARROW_WIDTH;
                if reveal.read(cx).0.narrow != narrow {
                    reveal.update(cx, |state, cx| {
                        state.0.narrow = narrow;
                        cx.notify();
                    });
                }
            },
            move |_, _, window, _| {
                let Some(id) = pending.borrow().clone() else {
                    return;
                };
                let Some(target) = anchors.get(&id) else {
                    return;
                };
                let container = scroll.bounds();
                let offset = scroll.offset();
                let max = scroll.max_offset();
                let delta = target.center().y - container.center().y;
                let y = (offset.y - delta).clamp(-max.y, gpui::px(0.));
                scroll.set_offset(point(offset.x, y));
                *pending.borrow_mut() = None;
                window.refresh();
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

impl Focusable for SettingsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let header = self.header(cx, window);
        let narrow = self.reveal.read(cx).0.narrow;
        let title = settings_section_label(self.section);
        let description = settings_section_description(self.section);
        let body: AnyElement = match (&self.body, self.section) {
            (SectionBody::Slot(view), SettingsSectionId::Skills) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(view.clone())
                .into_any_element(),
            (body, _) => {
                let content = match body.view() {
                    Some(view) => view.into_any_element(),
                    None => self.missing(self.section, cx),
                };
                let column = div()
                    .w_full()
                    .max_w(u(1024.))
                    .mx_auto()
                    .px(u(if narrow { 20. } else { 32. }))
                    .pt(u(if narrow { 24. } else { 32. }))
                    .pb(u(64.))
                    .child(page_header(title, description, cx))
                    .child(content);
                div()
                    .id("settings-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .debug_selector(|| "settings-scroll".into())
                    .child(self.layout_probe())
                    .child(column)
                    .into_any_element()
            }
        };
        div()
            .id("settings-page")
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .min_h_0()
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key != "escape" || event.keystroke.modifiers.modified() {
                    return;
                }
                if let Some(close) = this.callbacks.on_close.clone() {
                    cx.stop_propagation();
                    close((), window, cx);
                }
            }))
            .child(header)
            .child(body)
    }
}
