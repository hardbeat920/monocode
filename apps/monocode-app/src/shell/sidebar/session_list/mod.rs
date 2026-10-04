//! The Sessions tab of the sidebar: the search row, the session list, the
//! cards, and the card menu. Port of the sessions half of
//! src/app/shell/Sidebar.tsx (`SessionCard`, `FolderRow`, the session and
//! folder menus) and src/features/sessions/ui/SessionFiltersMenu.tsx.

mod actions;
mod groups;
mod model;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Point, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::HarnessId;
use monocode_core::session::session_display_title;
use monocode_engine::attention::Attention;
use monocode_engine::runtime::Engine;
use monocode_ui::ProviderLogo;
use monocode_ui::widgets::{
    MenuEntry, MenuItem, context_menu, diff_stat, icon_button, menu, spinner, text_field,
};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use crate::format::{self, NO_BRANCH_LABEL, format_git_label, format_relative, now_ms};
use crate::shell::Shell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Busy,
    Done,
    NeedsApproval,
    Draft,
}

/// One `SessionCard`.
#[derive(Clone, Debug)]
pub struct SessionCard {
    pub id: String,
    pub provider: ProviderLogo,
    pub model: String,
    pub title: String,
    pub git: String,
    pub additions: i64,
    pub deletions: i64,
    pub updated_at: i64,
    pub status: SessionStatus,
    pub pinned: bool,
}

/// What the list draws for one frame.
#[derive(Clone, Debug, Default)]
pub struct ListData {
    pub sessions: Vec<SessionCard>,
    pub sessions_loading: bool,
    pub active_session_id: Option<String>,
    pub selected_ids: Vec<String>,
    pub listed: Vec<monocode_engine::runtime::session_store::SessionSummary>,
    pub entries: Vec<monocode_engine::history::session_folders::SessionListEntry>,
    pub navigation_ids: Vec<String>,
    pub harnesses: Vec<HarnessId>,
    pub filters_active: bool,
    pub search_narrowed: bool,
    pub has_more: bool,
}

/// The model's display name (`resolveModel(harness, model).name`).
fn model_name(harness: HarnessId, model: &str, cx: &App) -> String {
    monocode_app::boot::AppServices::try_global(cx)
        .map(|services| {
            services
                .catalog
                .read()
                .resolve_model(harness, Some(model))
                .name
        })
        .unwrap_or_else(|| model.to_string())
}

/// The Sessions tab of one window.
pub struct SessionList {
    shell: WeakEntity<Shell>,
    session_search: Entity<InputState>,
    session_menu: Option<Point<Pixels>>,
    /// The card under the context menu.
    menu_session: Option<String>,
    filters_menu: Option<Point<Pixels>>,
    folder_menu: Option<(String, Point<Pixels>)>,
    rename_input: Entity<InputState>,
    editing: Option<(String, bool)>,
    link_dialog: Option<Entity<monocode_view_inbox::pr::link_dialog::LinkSessionWorkItemDialog>>,
    link_subscription: Option<Subscription>,
    history_observation: Option<(gpui::EntityId, Subscription)>,
    remote_watch: Option<(String, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl SessionList {
    pub(super) fn has_open_overlay(&self) -> bool {
        self.session_menu.is_some()
            || self.filters_menu.is_some()
            || self.folder_menu.is_some()
            || self.link_dialog.is_some()
    }

    pub fn new(
        shell: WeakEntity<Shell>,
        demo_menu: Option<(f32, f32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions"));
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let search = cx.subscribe(&session_search, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                if let Some(history) = this.history(cx) {
                    history.update(cx, |history, cx| history.set_search_query(&value, cx));
                }
                cx.notify();
            }
        });
        let rename = cx.subscribe_in(&rename_input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_rename(cx);
            }
            if matches!(event, InputEvent::Blur) {
                this.commit_rename(cx);
            }
        });
        let mut subscriptions = vec![search, rename];
        if let Some(engine) = Engine::try_global(cx) {
            let sessions = engine.sessions.clone();
            subscriptions.push(cx.observe(&sessions, |_, _, cx| cx.notify()));
        }
        if let Some(attention) = Attention::try_global(cx) {
            let approvals = attention.approvals.clone();
            let notifier = attention.notifier.clone();
            subscriptions.push(cx.observe(&approvals, |_, _, cx| cx.notify()));
            subscriptions.push(cx.observe(&notifier, |_, _, cx| cx.notify()));
        }
        if let Some(remote) = monocode_engine::remote::RemoteGlobal::try_global(cx) {
            let connections = remote.connections.clone();
            subscriptions.push(cx.observe(&connections, |_, _, cx| cx.notify()));
        }
        Self {
            shell,
            session_search,
            rename_input,
            session_menu: demo_menu.map(|(x, y)| gpui::point(gpui::px(x), gpui::px(y))),
            menu_session: None,
            filters_menu: None,
            folder_menu: None,
            editing: None,
            link_dialog: None,
            link_subscription: None,
            history_observation: None,
            remote_watch: None,
            _subscriptions: subscriptions,
        }
    }

    fn with_shell(&self, cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
        self.shell.update(cx, f).ok();
    }
}

impl Render for SessionList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_observations(window, cx);
        let theme = Theme::of(cx).clone();
        let data = self.data(cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(self.render_session_list(&data, &theme, cx))
            .children(self.render_session_menu(&data, cx))
            .children(self.render_filters_menu(&data, cx))
            .children(self.render_folder_menu(cx))
            .children(self.link_dialog.clone())
    }
}

impl SessionList {
    fn render_session_list(
        &self,
        data: &ListData,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let search_row = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(4.))
            .px(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .child(text_field(&self.session_search).icon(IconName::Search))
            .child(
                icon_button("sessions-filter", IconName::ListFilter)
                    .size(24.)
                    .icon_size(12.)
                    .tooltip("Filter sessions")
                    .active(data.filters_active)
                    .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                        this.filters_menu = Some(event.position());
                        cx.notify();
                    })),
            );
        let mut list = div().flex().flex_col().gap(u(2.)).p(u(6.));
        let now = now_ms();
        let mut card_index = 0;
        for entry in &data.entries {
            list = list.child(self.render_list_entry(entry, data, &mut card_index, now, theme, cx));
        }
        if data.has_more {
            list = list.child(
                icon_button("sessions-load-more", IconName::ChevronDown)
                    .tooltip("Show more sessions")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(history) = this.history(cx) {
                            history.update(cx, |history, cx| history.load_more_sessions(cx));
                        }
                    })),
            );
        }
        if data.sessions.is_empty() && !data.sessions_loading {
            list = list.child(
                div()
                    .px(u(6.))
                    .py(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("No sessions yet"),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(search_row)
            .child(
                div()
                    .id("session-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
    }

    /// `SessionCard`: provider and model, title, branch, diff stats, and a
    /// status or relative time.
    fn render_session_card(
        &self,
        index: usize,
        session: &SessionCard,
        is_active: bool,
        now: i64,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let is_selected = self.menu_session.as_deref() == Some(session.id.as_str())
            && self.session_menu.is_some();
        let needs_approval = session.status == SessionStatus::NeedsApproval;
        let draft = session.status == SessionStatus::Draft;

        let status: AnyElement = {
            let row = div()
                .flex()
                .flex_none()
                .items_center()
                .gap(u(4.))
                .text_px(theme.text.caption)
                .tabular();
            match session.status {
                SessionStatus::NeedsApproval => row
                    .text_color(c.warning)
                    .child(
                        icon(IconName::CircleAlert)
                            .size(u(12.))
                            .text_color(c.warning),
                    )
                    .child("Need approval")
                    .into_any_element(),
                SessionStatus::Busy => row
                    .text_color(c.accent)
                    .child(spinner(("session-spinner", index)))
                    .child("Working...")
                    .into_any_element(),
                SessionStatus::Done => row
                    .text_color(c.success)
                    .child(icon(IconName::Check).size(u(12.)).text_color(c.success))
                    .child("Done")
                    .into_any_element(),
                SessionStatus::Draft => row
                    .text_color(theme.content(0.55))
                    .child(
                        icon(IconName::CircleDashed)
                            .size(u(12.))
                            .text_color(theme.content(0.55)),
                    )
                    .child("Draft")
                    .into_any_element(),
                SessionStatus::Idle => row
                    .text_color(theme.content(0.45))
                    .child(format_relative(session.updated_at, now))
                    .into_any_element(),
            }
        };

        let header = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .child(provider_logo(session.provider).size(14.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.50))
                            .child(session.model.clone()),
                    ),
            )
            .child(status);
        let title = div()
            .mt(u(4.))
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(6.))
            .when(session.pinned, |el| {
                el.child(
                    icon(IconName::Pin)
                        .size(u(12.))
                        .text_color(theme.content(0.45)),
                )
            })
            .child(
                if self
                    .editing
                    .as_ref()
                    .is_some_and(|(id, folder)| !folder && id == &session.id)
                {
                    text_field(&self.rename_input).into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_px(theme.text.body)
                        .semibold()
                        .leading(theme.leading.snug)
                        .text_color(c.content)
                        .child(session.title.clone())
                        .into_any_element()
                },
            );
        let branch = session.git.clone();
        let footer = div()
            .mt(u(4.))
            .flex()
            .items_center()
            .gap(u(8.))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(u(4.))
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.45))
                    .child(
                        icon(IconName::GitBranch)
                            .size(u(12.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(div().min_w_0().truncate().child(branch)),
            )
            .child(diff_stat(session.additions, session.deletions));

        let mut card = div()
            .id(("session", index))
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .px(u(10.))
            .py(u(8.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(gpui::transparent_black())
            .child(header)
            .child(title)
            .child(footer)
            .on_drag(
                monocode_view_workbench::panes::pane_tree::PaneDragSource::Session(
                    session.id.clone(),
                ),
                {
                    let label = session.title.clone();
                    move |_, _, _, cx| {
                        cx.new(|_| crate::panes::drag::WorkspaceDragPreview {
                            label: label.clone(),
                        })
                    }
                },
            )
            .drag_over::<monocode_view_workbench::panes::pane_tree::PaneDragSource>({ let color = theme.accent(0.15); move |style, _, _, _| style.bg(color) })
            .on_drop({
                let target = session.id.clone();
                cx.listener(move |this, source: &monocode_view_workbench::panes::pane_tree::PaneDragSource, window, cx| {
                    if let monocode_view_workbench::panes::pane_tree::PaneDragSource::Session(id) = source {
                        this.folder_drop(id, monocode_engine::history::session_folders::SessionListDropTarget::Session { id: target.clone() }, window, cx);
                    }
                })
            })
            .on_click({
                let id = session.id.clone();
                cx.listener(move |this, event: &ClickEvent, _, cx| {
                    let data = this.data(cx);
                    let modifiers = event.modifiers();
                    let selected = this.history(cx).and_then(|history| {
                        history.update(cx, |history, cx| {
                            history.select_card(
                                &id,
                                monocode_engine::history::sidebar::CardClick {
                                    shift: modifiers.shift,
                                    toggle: if cfg!(target_os = "macos") {
                                        modifiers.platform
                                    } else {
                                        modifiers.control
                                    },
                                },
                                &data.navigation_ids,
                                data.active_session_id.as_deref(),
                                cx,
                            )
                        })
                    });
                    if let Some(id) = selected {
                        this.with_shell(cx, |shell, cx| shell.open_session(&id, cx));
                    }
                    cx.notify();
                })
            })
            .on_mouse_down(MouseButton::Right, {
                let id = session.id.clone();
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.session_menu = Some(event.position);
                    this.menu_session = Some(id.clone());
                    if let Some(history) = this.history(cx) {
                        history.update(cx, |history, cx| history.open_session_menu(&id, cx));
                    }
                    cx.notify();
                })
            });
        if is_selected {
            card = card.bg(theme.accent(0.15)).text_color(c.content);
        } else if needs_approval {
            card = card
                .bg(theme.content(0.20))
                .border_dashed()
                .border_color(theme.content(0.30));
        } else if is_active {
            card = card.bg(c.selection);
        } else if draft {
            card = card.border_dashed().border_color(theme.content(0.25));
        } else {
            let hover = theme.content(0.05);
            card = card.hover(move |s| s.bg(hover));
        }
        card
    }
}

use gpui::prelude::FluentBuilder as _;
