//! Port of src/features/notifications/ui/NotificationMuteControl.tsx and
//! NotificationMuteDatePicker.tsx: the "Mute" button with its duration menu
//! and the custom date and time step.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::InputEvent;
use monocode_ui::widgets::{popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::date_time_picker::{
    DateTimeChange, DateTimePicker, date_key, local_date_time_from_ms, local_ms,
    parse_local_date_time,
};
use super::host::NotificationsHost;
use super::notification_model::{
    MUTE_CUSTOM, Mute, PreferencePatch, is_project_muted, local_time, next_mute_deadline,
    notification_mute_actions, notification_mute_deadline, notification_mute_status,
};
use super::popover::{Side, anchored_to_trigger, dismiss_outside};
use super::style::text;
use crate::settings::controls::TriggerBounds;

/// The message a refused write shows.
pub const SAVE_ERROR: &str = "Could not save notification preferences. Please try again.";

/// What the control tells its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteControlEvent {
    /// `onChanged`: a mute or resume was saved.
    Changed,
}

/// Which popup is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteMenu {
    Menu,
    Custom,
}

/// Redraws a view when the earliest timed mute ends, as
/// `subscribeNotificationPreferences` notified on expiry.
pub(crate) struct MuteExpiry {
    deadline: Option<i64>,
    _timer: Option<Task<()>>,
}

impl MuteExpiry {
    pub(crate) fn new() -> Self {
        Self {
            deadline: None,
            _timer: None,
        }
    }

    pub(crate) fn schedule<V: 'static>(
        &mut self,
        host: &Rc<dyn NotificationsHost>,
        cx: &mut Context<V>,
    ) {
        let now = host.now();
        let next = next_mute_deadline(&host.preferences(cx), now);
        if next == self.deadline {
            return;
        }
        self.deadline = next;
        self._timer = next.map(|deadline| {
            let wait = Duration::from_millis((deadline - now).max(0) as u64);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
        });
    }

    /// Forgets the scheduled deadline, so the next render schedules again.
    pub(crate) fn reset(&mut self) {
        self.deadline = None;
        self._timer = None;
    }
}

/// `NotificationMuteControl`.
pub struct NotificationMuteControl {
    host: Rc<dyn NotificationsHost>,
    project_ids: Vec<String>,
    /// Prefixes the test selectors, so several controls on one page stay
    /// apart.
    scope: SharedString,
    error: Option<String>,
    open: Option<MuteMenu>,
    picker: Option<Entity<NotificationMuteDatePicker>>,
    picker_events: Option<Subscription>,
    trigger: TriggerBounds,
    trigger_focus: FocusHandle,
    menu_focus: FocusHandle,
    expiry: MuteExpiry,
    animate: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<MuteControlEvent> for NotificationMuteControl {}

impl NotificationMuteControl {
    pub fn new(
        host: Rc<dyn NotificationsHost>,
        project_ids: Vec<String>,
        scope: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        let observe = host.observe(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.expiry.reset();
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        Self {
            host,
            project_ids,
            scope: scope.into(),
            error: None,
            open: None,
            picker: None,
            picker_events: None,
            trigger: TriggerBounds::default(),
            trigger_focus: cx.focus_handle(),
            menu_focus: cx.focus_handle(),
            expiry: MuteExpiry::new(),
            animate: true,
            _subscriptions: observe.into_iter().collect(),
        }
    }

    pub fn set_project_ids(&mut self, project_ids: Vec<String>, cx: &mut Context<Self>) {
        if self.project_ids != project_ids {
            self.project_ids = project_ids;
            cx.notify();
        }
    }

    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn open(&self) -> Option<MuteMenu> {
        self.open
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn picker(&self) -> Option<&Entity<NotificationMuteDatePicker>> {
        self.picker.as_ref()
    }

    pub fn trigger_focus(&self) -> &FocusHandle {
        &self.trigger_focus
    }

    fn selector(&self, name: &str) -> String {
        format!("{}/{name}", self.scope)
    }

    /// `close`.
    fn close(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = None;
        self.picker = None;
        if restore_focus {
            window.focus(&self.trigger_focus, cx);
        }
        cx.notify();
    }

    /// `change`: save a mute (or a resume, with `None`).
    pub fn change(&mut self, mute: Option<Mute>, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_ids.is_empty() {
            return;
        }
        match self
            .host
            .update_preferences(&self.project_ids, &PreferencePatch::mute(mute), cx)
        {
            Ok(()) => {
                self.error = None;
                self.expiry.reset();
                self.close(true, window, cx);
                cx.emit(MuteControlEvent::Changed);
            }
            Err(_) => {
                self.error = Some(SAVE_ERROR.into());
                cx.notify();
            }
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.open = None;
            self.picker = None;
        } else {
            self.open = Some(MuteMenu::Menu);
            window.focus(&self.menu_focus, cx);
        }
        cx.notify();
    }

    /// `onPick`.
    pub fn pick(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == MUTE_CUSTOM {
            let host = self.host.clone();
            let ids = self.project_ids.clone();
            let picker = cx.new(|cx| NotificationMuteDatePicker::new(host, ids, window, cx));
            self.picker_events =
                Some(
                    cx.subscribe_in(&picker, window, |this, _, event, window, cx| match event {
                        DatePickerEvent::Cancel => this.close(true, window, cx),
                        DatePickerEvent::Changed => {
                            this.expiry.reset();
                            this.close(true, window, cx);
                            cx.emit(MuteControlEvent::Changed);
                        }
                    }),
                );
            self.picker = Some(picker);
            self.open = Some(MuteMenu::Custom);
            cx.notify();
            return;
        }
        if let Some(mute) = notification_mute_deadline(id, self.host.now()) {
            self.change(Some(mute), window, cx);
        }
    }

    fn render_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let hover = theme.content(0.05);
        let mut list = div().flex().flex_col().p(u(4.)).child(
            div()
                .px(u(8.))
                .py(u(6.))
                .text_px(11.)
                .text_color(theme.content(0.45))
                .child("Mute all notifications for"),
        );
        for action in notification_mute_actions(self.host.now()) {
            let id = action.id;
            let selector = self.selector(&format!("menuitem:{id}"));
            list = list.child(
                div()
                    .id(SharedString::from(id))
                    .flex()
                    .items_center()
                    .h(u(28.))
                    .w_full()
                    .px(u(8.))
                    .rounded(u(theme.radius.lg))
                    .text_px(theme.text.body)
                    .leading(theme.leading.none)
                    .text_color(theme.colors.content)
                    .hover(move |s| s.bg(hover))
                    .debug_selector(move || selector)
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(id, window, cx)))
                    .child(text(action.label).truncate()),
            );
        }
        let menu_selector = self.selector("menu:Mute notifications");
        let content = div()
            .track_focus(&self.menu_focus)
            .debug_selector(move || menu_selector)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close(true, window, cx);
                }
            }))
            .child(
                popover_frame("mute-menu")
                    .width(244.)
                    .animate(self.animate)
                    .child(list),
            );
        let weak = cx.entity().downgrade();
        let content = dismiss_outside(
            &self.trigger,
            Rc::new(move |window, cx| {
                weak.update(cx, |this, cx| this.close(false, window, cx))
                    .ok();
            }),
            content,
        );
        anchored_to_trigger(Side::Bottom, true, 4., window, cx, content)
    }

    fn render_custom(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.picker.clone()?;
        let dialog_selector = self.selector("dialog:Mute project notifications");
        let content = div()
            .debug_selector(move || dialog_selector)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close(true, window, cx);
                }
            }))
            .child(
                popover_frame("mute-custom")
                    .width(280.)
                    .animate(self.animate)
                    .child(div().p(u(12.)).child(picker)),
            );
        let weak = cx.entity().downgrade();
        let content = dismiss_outside(
            &self.trigger,
            Rc::new(move |window, cx| {
                weak.update(cx, |this, cx| this.close(false, window, cx))
                    .ok();
            }),
            content,
        );
        Some(anchored_to_trigger(
            Side::Bottom,
            true,
            6.,
            window,
            cx,
            content,
        ))
    }
}

impl Render for NotificationMuteControl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.expiry.schedule(&self.host, cx);
        let theme = Theme::of(cx).clone();
        let now = self.host.now();
        let preferences = self.host.preferences(cx);
        let muted: Vec<&String> = self
            .project_ids
            .iter()
            .filter(|id| {
                preferences
                    .get(*id)
                    .is_some_and(|preference| is_project_muted(preference, now))
            })
            .collect();
        let status = if muted.is_empty() {
            None
        } else if self.project_ids.len() > 1 {
            Some(format!(
                "{} of {} projects muted",
                muted.len(),
                self.project_ids.len()
            ))
        } else {
            notification_mute_status(preferences.get(&self.project_ids[0]), now)
        };
        let any_muted = !muted.is_empty();
        let hover = theme.content(0.05);
        let hover_ink = theme.colors.content;
        let mut row = div()
            .flex()
            .max_w_full()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap_x(u(12.))
            .gap_y(u(8.));
        if let Some(status) = status {
            let selector = self.selector(&format!("status:{status}"));
            row = row.child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .debug_selector(move || selector)
                    .child(text(status)),
            );
        }
        if any_muted {
            let selector = self.selector("button:Resume notifications");
            row = row.child(
                div()
                    .id("resume")
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .py(u(6.))
                    .text_px(12.)
                    .text_color(theme.content(0.70))
                    .hover(move |s| s.bg(hover).text_color(hover_ink))
                    .debug_selector(move || selector)
                    .on_click(cx.listener(|this, _, window, cx| this.change(None, window, cx)))
                    .child("Resume notifications"),
            );
        }
        let label = if any_muted {
            "Change mute duration"
        } else {
            "Mute notifications"
        };
        let disabled = self.project_ids.is_empty();
        let selector = self.selector(&format!("button:{label}"));
        let ink = theme.content(0.70);
        let fill = theme.content(0.10);
        let mut trigger = div()
            .id("mute-trigger")
            .track_focus(&self.trigger_focus)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .px(u(10.))
            .py(u(4.))
            .text_px(12.)
            .leading(theme.leading.label)
            .whitespace_nowrap()
            .text_color(ink)
            .tooltip(tooltip(
                "Mute pauses all project notifications without changing your category choices.",
            ))
            .debug_selector(move || selector)
            .child(self.trigger.probe())
            .child(icon(IconName::BellOff).size(u(14.)))
            .child(if any_muted { "Muted" } else { "Mute" })
            .child(
                icon(IconName::ChevronDown)
                    .size(u(12.))
                    .text_color(theme.content(0.40)),
            );
        if disabled {
            trigger = trigger.opacity(0.4);
        } else {
            trigger = trigger
                .hover(move |s| s.bg(fill).text_color(hover_ink))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.toggle(window, cx)));
        }
        let popup = match self.open {
            Some(MuteMenu::Menu) => Some(self.render_menu(window, cx)),
            Some(MuteMenu::Custom) => self.render_custom(window, cx),
            None => None,
        };
        row = row.child(div().relative().flex_none().child(trigger).children(popup));
        if let Some(error) = self.error.clone() {
            let selector = self.selector("alert");
            row = row.child(
                div()
                    .w_full()
                    .text_px(12.)
                    .text_color(theme.colors.danger)
                    .debug_selector(move || selector)
                    .child(text(error)),
            );
        }
        row
    }
}

/// What the date step tells the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePickerEvent {
    Cancel,
    Changed,
}

/// `NotificationMuteDatePicker`: custom timing is a complete step, separate
/// from the duration presets.
pub struct NotificationMuteDatePicker {
    host: Rc<dyn NotificationsHost>,
    project_ids: Vec<String>,
    value: String,
    error: Option<String>,
    picker: Entity<DateTimePicker>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DatePickerEvent> for NotificationMuteDatePicker {}

impl NotificationMuteDatePicker {
    pub fn new(
        host: Rc<dyn NotificationsHost>,
        project_ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let now = host.now();
        let until = if project_ids.len() == 1 {
            host.preferences(cx)
                .get(&project_ids[0])
                .and_then(|preference| match preference.muted_until {
                    Some(Mute::Until(until)) => Some(until),
                    _ => None,
                })
        } else {
            None
        };
        let initial = match until {
            Some(until) if until > now => until,
            _ => now + 3_600_000,
        };
        // Up to the next whole minute.
        let initial = (initial as f64 / 60_000.0).ceil() as i64 * 60_000;
        let value = local_date_time_from_ms(initial);
        let min_date = local_time(now).map(|time| date_key(time.date_naive()));
        let picker = cx.new(|cx| {
            DateTimePicker::new(value.clone(), min_date.as_deref(), now, true, window, cx)
        });
        let time = picker.read(cx).time_input().clone();
        let subscriptions = vec![
            cx.subscribe(&picker, |this, _, DateTimeChange(value), cx| {
                this.value = value.clone();
                this.error = None;
                cx.notify();
            }),
            cx.subscribe_in(&time, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(window, cx);
                }
            }),
        ];
        Self {
            host,
            project_ids,
            value,
            error: None,
            picker,
            _subscriptions: subscriptions,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn picker(&self) -> &Entity<DateTimePicker> {
        &self.picker
    }

    /// The form's submit.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.project_ids.is_empty() {
            return;
        }
        let Some(date) = parse_local_date_time(&self.value) else {
            self.error = Some("Choose a valid date and time.".into());
            cx.notify();
            return;
        };
        let Some(ms) = local_ms(date) else {
            self.error = Some("Choose a valid date and time.".into());
            cx.notify();
            return;
        };
        if ms <= self.host.now() {
            self.error = Some("Choose a date and time in the future.".into());
            cx.notify();
            return;
        }
        match self.host.update_preferences(
            &self.project_ids,
            &PreferencePatch::mute(Some(Mute::Until(ms))),
            cx,
        ) {
            Ok(()) => {
                self.error = None;
                cx.emit(DatePickerEvent::Changed);
            }
            Err(_) => self.error = Some(SAVE_ERROR.into()),
        }
        cx.notify();
    }
}

impl Render for NotificationMuteDatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let hover = theme.content(0.05);
        let hover_ink = theme.colors.content;
        let c = theme.colors;
        let disabled = self.project_ids.is_empty();
        let mut submit = div()
            .id("mute-until")
            .flex()
            .flex_none()
            .items_center()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(gpui::transparent_black())
            .px(u(10.))
            .py(u(4.))
            .text_px(12.)
            .debug_selector(|| "button:Mute until then".into())
            .child("Mute until then");
        // `.primary-action`.
        if disabled {
            submit = submit
                .bg(c.primary_disabled)
                .text_color(c.primary_disabled_foreground);
        } else {
            let hover_fill = c.primary_hover;
            submit = submit
                .bg(c.primary)
                .text_color(c.primary_foreground)
                .hover(move |s| s.bg(hover_fill))
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));
        }
        div()
            .w_full()
            .child(
                div()
                    .mb(u(12.))
                    .px(u(4.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .child("Mute all notifications until"),
            )
            .child(self.picker.clone())
            .when_some(self.error.clone(), |el, error| {
                el.child(
                    div()
                        .mt(u(12.))
                        .px(u(4.))
                        .text_px(12.)
                        .text_color(theme.colors.danger)
                        .debug_selector(|| "alert".into())
                        .child(text(error)),
                )
            })
            .child(
                div()
                    .mt(u(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(8.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .pt(u(10.))
                    .child(
                        div()
                            .id("mute-cancel")
                            .rounded(u(theme.radius.sm))
                            .px(u(8.))
                            .py(u(6.))
                            .text_px(12.)
                            .text_color(theme.content(0.50))
                            .hover(move |s| s.bg(hover).text_color(hover_ink))
                            .debug_selector(|| "button:Cancel".into())
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DatePickerEvent::Cancel)))
                            .child("Cancel"),
                    )
                    .child(submit),
            )
    }
}
