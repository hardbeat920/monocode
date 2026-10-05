//! Port of src/features/inbox/ui/InboxNotificationMenu.tsx: the context
//! menu of the rail's Inbox button. It marks every known item read, mutes
//! or resumes every project's notifications, and opens the settings.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, Context, ElementId, EventEmitter, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
};
use monocode_ui::widgets::{popover_at, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::Listener;
use crate::model::{MuteAction, notification_mute_actions, notification_mute_deadline};

/// What the menu shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationMenuState {
    /// Rail and known inbox projects.
    pub project_count: usize,
    pub muted_count: usize,
    /// `inboxHasUnseenItems(knownInboxEntries(projectPaths))`.
    pub has_unread: bool,
    /// `onOpenSettings` is wired.
    pub can_open_settings: bool,
}

/// The notification side the menu reads and changes. The attention
/// package and the inbox's seen state implement it in the app.
pub trait InboxNotificationData {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    fn state(&self, cx: &App) -> NotificationMenuState;
    fn now_ms(&self) -> i64;
    /// `markInboxItemsSeen(entries)`: false when the write failed.
    fn mark_all_read(&self, cx: &mut App) -> bool;
    /// Mutes every project until `until` (`None`: until resumed).
    fn mute_all(&self, until: Option<i64>, cx: &mut App) -> Result<(), String>;
    /// Resumes the muted projects only.
    fn resume_muted(&self, cx: &mut App) -> Result<(), String>;
    fn open_settings(&self, cx: &mut App);
    /// `NotificationMuteDatePicker` for every project.
    fn mute_date_picker(&self, _window: &mut Window, _cx: &mut App) -> Option<AnyView> {
        None
    }
}

/// The menu closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseNotificationMenu;

pub struct InboxNotificationMenu {
    data: Rc<dyn InboxNotificationData>,
    position: Point<Pixels>,
    error: Option<String>,
    mute_open: bool,
    custom: Option<Option<AnyView>>,
    animate: bool,
    _subscription: Subscription,
}

impl EventEmitter<CloseNotificationMenu> for InboxNotificationMenu {}

/// The rows: id, label, enabled.
pub fn notification_menu_rows(
    state: &NotificationMenuState,
) -> Vec<(&'static str, &'static str, bool)> {
    let mut rows = vec![
        ("read-all", "Mark all as read", state.has_unread),
        ("sep", "", false),
        ("mute", "Mute all projects", state.project_count > 0),
        ("resume", "Resume muted projects", state.muted_count > 0),
    ];
    if state.can_open_settings {
        rows.push(("settings", "Notification settings…", true));
    }
    rows
}

/// The status line under "Inbox".
pub fn notification_menu_status(state: &NotificationMenuState) -> String {
    format!(
        "{} {} · {} muted",
        state.project_count,
        if state.project_count == 1 {
            "project"
        } else {
            "projects"
        },
        state.muted_count
    )
}

impl InboxNotificationMenu {
    pub fn new(
        data: Rc<dyn InboxNotificationData>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        let subscription = data.subscribe(
            Box::new(move |cx| {
                if let Some(menu) = weak.upgrade() {
                    menu.update(cx, |_, cx| cx.notify());
                }
            }),
            cx,
        );
        Self {
            data,
            position,
            error: None,
            mute_open: false,
            custom: None,
            animate: true,
            _subscription: subscription,
        }
    }

    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn mute_open(&self) -> bool {
        self.mute_open
    }

    pub fn custom_open(&self) -> bool {
        self.custom.is_some()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(CloseNotificationMenu);
    }

    /// `onPick`.
    pub fn pick(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.data.state(cx);
        match id {
            "read-all" => {
                if !state.has_unread {
                    return;
                }
                if !self.data.mark_all_read(cx) {
                    self.error = Some("Could not save read status. Please try again.".into());
                    cx.notify();
                    return;
                }
                self.close(cx);
            }
            "settings" => {
                self.close(cx);
                self.data.open_settings(cx);
            }
            "mute" => {
                if state.project_count > 0 {
                    self.mute_open = !self.mute_open;
                    cx.notify();
                }
            }
            "resume" => {
                if state.muted_count == 0 {
                    return;
                }
                match self.data.resume_muted(cx) {
                    Ok(()) => self.close(cx),
                    Err(_) => {
                        self.error = Some(
                            "Could not save notification preferences. Please try again.".into(),
                        );
                        cx.notify();
                    }
                }
            }
            "mute:custom" => {
                if state.project_count == 0 {
                    return;
                }
                let picker = self.data.mute_date_picker(window, cx);
                self.custom = Some(picker);
                cx.notify();
            }
            other => {
                if state.project_count == 0 {
                    return;
                }
                let Some(until) = notification_mute_deadline(other, self.data.now_ms()) else {
                    return;
                };
                match self.data.mute_all(until, cx) {
                    Ok(()) => self.close(cx),
                    Err(_) => {
                        self.error = Some(
                            "Could not save notification preferences. Please try again.".into(),
                        );
                        cx.notify();
                    }
                }
            }
        }
    }

    fn row(
        &self,
        id: &'static str,
        label: SharedString,
        enabled: bool,
        submenu: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::of(cx);
        let ink = if enabled {
            theme.colors.content
        } else {
            theme.content(0.30)
        };
        let hover = theme.content(0.05);
        let mut row = div()
            .id(SharedString::from(format!("notification-{id}")))
            .flex()
            .h(u(28.))
            .w_full()
            .items_center()
            .gap(u(12.))
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .text_px(theme.text.body)
            .leading(theme.leading.none)
            .text_color(ink)
            .child(div().flex_1().min_w_0().truncate().child(label));
        if submenu && self.mute_open {
            row = row.bg(theme.colors.selection);
        }
        if submenu {
            row = row.child(
                icon(IconName::ChevronRight)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            );
        }
        if enabled {
            row = row
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(id, window, cx)));
        }
        row
    }

    fn submenu(&self, actions: Vec<MuteAction>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut list = div().flex().flex_col().p(u(4.));
        for (index, action) in actions.into_iter().enumerate() {
            let hover = theme.content(0.05);
            let id = action.id.clone();
            list = list.child(
                div()
                    .id(ElementId::NamedInteger("mute-action".into(), index as u64))
                    .flex()
                    .h(u(28.))
                    .w_full()
                    .items_center()
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(theme.text.body)
                    .leading(theme.leading.none)
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(&id, window, cx)))
                    .child(action.label),
            );
        }
        popover_frame("notification-mute-menu")
            .width(228.)
            .animate(self.animate)
            .child(list)
            .into_any_element()
    }
}

impl Render for InboxNotificationMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let state = self.data.state(cx);
        if let Some(picker) = self.custom.clone() {
            let content = div()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close(cx)))
                .flex()
                .flex_col()
                .gap(u(4.))
                .p(u(12.))
                .child(
                    div()
                        .px(u(4.))
                        .text_px(theme.text.label)
                        .medium()
                        .text_color(theme.content(0.85))
                        .child("Mute all projects"),
                )
                .children(picker);
            return popover_at(
                self.position,
                gpui::Anchor::TopLeft,
                popover_frame("notification-custom")
                    .width(280.)
                    .animate(self.animate)
                    .child(content),
                cx,
            )
            .into_any_element();
        }
        let mut list = div().flex().flex_col().p(u(4.)).child(
            div()
                .flex()
                .flex_col()
                .gap(u(4.))
                .px(u(8.))
                .py(u(6.))
                .text_px(theme.text.label)
                .child(
                    div()
                        .medium()
                        .text_color(theme.colors.content)
                        .child("Inbox"),
                )
                .child(
                    div()
                        .text_color(theme.content(0.50))
                        .child(notification_menu_status(&state)),
                )
                .children(
                    self.error
                        .clone()
                        .map(|error| div().text_color(theme.colors.danger).child(error)),
                ),
        );
        for (id, label, enabled) in notification_menu_rows(&state) {
            if id == "sep" {
                list = list.child(div().my(u(4.)).h(gpui::px(1.)).bg(theme.content(0.10)));
                continue;
            }
            let row = self.row(id, label.into(), enabled, id == "mute", cx);
            if id == "mute" && self.mute_open {
                let submenu = self.submenu(notification_mute_actions(self.data.now_ms()), cx);
                list = list.child(
                    div().relative().child(row).child(
                        div().absolute().top(u(-4.)).left_full().child(
                            gpui::deferred(gpui::anchored().child(submenu))
                                .with_priority(theme.layer.submenu),
                        ),
                    ),
                );
            } else {
                list = list.child(row);
            }
        }
        popover_at(
            self.position,
            gpui::Anchor::TopLeft,
            popover_frame("inbox-notification-menu")
                .width(272.)
                .animate(self.animate)
                .child(
                    div()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            if !this.mute_open {
                                this.close(cx)
                            }
                        }))
                        .child(list),
                ),
            cx,
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disables_rows_that_have_nothing_to_do() {
        let state = NotificationMenuState {
            project_count: 2,
            muted_count: 0,
            has_unread: false,
            can_open_settings: true,
        };
        let rows = notification_menu_rows(&state);
        assert_eq!(rows[0], ("read-all", "Mark all as read", false));
        assert_eq!(rows[2], ("mute", "Mute all projects", true));
        assert_eq!(rows[3], ("resume", "Resume muted projects", false));
        assert_eq!(rows[4], ("settings", "Notification settings…", true));
        assert_eq!(notification_menu_status(&state), "2 projects · 0 muted");
    }
}
