//! ReminderNotices.tsx. Persistence and opening sessions belong to the host.
use crate::{
    popover::{Anchor, AnchorSlot, Menu, MenuAction, MenuEvent, menu_popover},
    theme::*,
};
use chrono::{Local, TimeZone};
use gpui::{Context, Entity, EventEmitter, Role, Subscription, Window, div, prelude::*, px};
use monocode_core::session_groups::{reminder_at, reminder_presets};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub session_id: i64,
    pub due_at: u64,
    pub title: String,
    pub project: String,
    pub time_label: String,
}
#[derive(Clone)]
pub(crate) enum Action {
    Open {
        session_id: i64,
        due_at: u64,
    },
    Dismiss {
        session_id: i64,
        due_at: u64,
    },
    Snooze {
        session_id: i64,
        expected_due_at: u64,
        due_at: u64,
    },
    Retry,
    OpenSettings,
}
struct Snooze {
    target: (i64, u64),
    anchor: AnchorSlot,
    menu: Entity<Menu>,
    _events: Subscription,
}
#[derive(Default)]
pub(crate) struct View {
    reminders: Vec<Snapshot>,
    error: Option<String>,
    desktop_alerts: bool,
    snooze: Option<Snooze>,
}
impl EventEmitter<Action> for View {}
impl View {
    pub(crate) fn sync(
        &mut self,
        reminders: Vec<Snapshot>,
        error: Option<String>,
        desktop_alerts: bool,
        cx: &mut Context<Self>,
    ) {
        let mut unchanged = self.reminders == reminders
            && self.error == error
            && self.desktop_alerts == desktop_alerts;
        if self.snooze.as_ref().is_some_and(|s| {
            !reminders
                .iter()
                .any(|r| (r.session_id, r.due_at) == s.target)
        }) {
            self.snooze = None;
            unchanged = false;
        }
        self.reminders = reminders;
        self.error = error;
        self.desktop_alerts = desktop_alerts;
        // Shell syncs on every render; only a change may redraw.
        if !unchanged {
            cx.notify();
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.reminders.is_empty() && self.error.is_none()
    }
    fn snooze(
        &mut self,
        target: (i64, u64),
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = reminder_presets(Local::now().naive_local())
            .into_iter()
            .map(|(id, label, enabled)| {
                let mut action = MenuAction::new(id, label);
                action.disabled = !enabled;
                action.into()
            })
            .collect();
        let menu = cx.new(|cx| Menu::new("Snooze reminder", items, window, cx));
        let events = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(id, _) = event
                && this
                    .reminders
                    .iter()
                    .any(|r| (r.session_id, r.due_at) == target)
                && let Some(at) = reminder_at(id.as_ref(), Local::now().naive_local())
                    .and_then(|at| Local.from_local_datetime(&at).earliest())
            {
                cx.emit(Action::Snooze {
                    session_id: target.0,
                    expected_due_at: target.1,
                    due_at: at.timestamp_millis().max(0) as u64,
                });
            }
            this.snooze = None;
            cx.notify();
        });
        self.snooze = Some(Snooze {
            target,
            anchor,
            menu,
            _events: events,
        });
        cx.notify();
    }
}
impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let max_height = (f32::from(window.viewport_size().height) * 0.5).min(320.);
        let rows = self
            .reminders
            .iter()
            .enumerate()
            .map(|(ix, reminder)| {
                let target = (reminder.session_id, reminder.due_at);
                let anchor = AnchorSlot::default();
                let trigger = anchor.clone();
                div()
                    .id(("due-reminder", ix))
                    .px_3()
                    .py(px(10.))
                    .when(ix > 0, |d| d.border_t_1().border_color(content(0.1)))
                    .child(
                        div()
                            .id(("due-reminder-title", ix))
                            .role(Role::Button)
                            .cursor_pointer()
                            .w_full()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(Action::Open {
                                    session_id: target.0,
                                    due_at: target.1,
                                })
                            }))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(reminder.title.clone()),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.5))
                                    .child(format!(
                                        "{} · {}",
                                        reminder.project, reminder.time_label
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(11.))
                            .child(
                                div()
                                    .id(("due-reminder-open", ix))
                                    .role(Role::Button)
                                    .cursor_pointer()
                                    .rounded_md()
                                    .bg(content(0.1))
                                    .hover(|s| s.bg(content(0.15)))
                                    .px_2()
                                    .py_1()
                                    .child("Open session")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(Action::Open {
                                            session_id: target.0,
                                            due_at: target.1,
                                        })
                                    })),
                            )
                            .child(
                                div()
                                    .id(("due-reminder-snooze", ix))
                                    .role(Role::Button)
                                    .aria_label("Snooze")
                                    .cursor_pointer()
                                    .relative()
                                    .rounded_md()
                                    .text_color(content(0.65))
                                    .hover(|s| s.bg(content(0.1)))
                                    .px_2()
                                    .py_1()
                                    .child("Snooze")
                                    .child(anchor.probe())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.snooze(target, trigger.clone(), window, cx)
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id(("due-reminder-dismiss", ix))
                                    .role(Role::Button)
                                    .cursor_pointer()
                                    .rounded_md()
                                    .text_color(content(0.5))
                                    .hover(|s| s.bg(content(0.1)))
                                    .px_2()
                                    .py_1()
                                    .child("Dismiss")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(Action::Dismiss {
                                            session_id: target.0,
                                            due_at: target.1,
                                        })
                                    })),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        let mut fill = pane_fill();
        fill.a = 0.95;
        div()
            .relative()
            .w_full()
            .child(
                div()
                    .id("reminder-accessible-1")
                    .relative()
                    .role(Role::Region)
                    .aria_label("Due reminders")
                    .overflow_hidden()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(content(0.15))
                    .bg(fill)
                    .text_color(content(1.))
                    .shadow_xl()
                    .child(super::surface::backdrop(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .border_b_1()
                            .border_color(content(0.1))
                            .px_3()
                            .py(px(10.))
                            .child(icon("clock", 14.).text_color(gpui::rgb(0xffb900)))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Due reminders"),
                            )
                            .child(
                                div()
                                    .id("reminder-accessible-2")
                                    .role(Role::Status)
                                    .text_size(px(11.))
                                    .text_color(content(0.5))
                                    .child(if self.reminders.is_empty() {
                                        String::new()
                                    } else {
                                        self.reminders.len().to_string()
                                    }),
                            ),
                    )
                    .when(self.error.is_some(), |d| {
                        d.child(
                            div()
                                .id("reminder-accessible-3")
                                .role(Role::Alert)
                                .px_3()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .flex()
                                .gap_1()
                                .child("Couldn’t load reminders.")
                                .child(
                                    div()
                                        .id("reminder-retry")
                                        .role(Role::Button)
                                        .cursor_pointer()
                                        .underline()
                                        .child("Retry")
                                        .on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(Action::Retry)),
                                        ),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("due-reminder-list")
                            .max_h(px(max_height))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .when(!self.desktop_alerts && !self.reminders.is_empty(), |d| {
                        d.child(
                            div()
                                .id("reminder-settings")
                                .role(Role::Button)
                                .cursor_pointer()
                                .w_full()
                                .border_t_1()
                                .border_color(content(0.1))
                                .px_3()
                                .py_2()
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .hover(|s| s.text_color(content(1.)))
                                .child("Desktop alerts are off. Enable in Settings.")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Action::OpenSettings))),
                        )
                    }),
            )
            .children(
                self.snooze
                    .as_ref()
                    .map(|s| menu_popover(&s.menu, Anchor::Slot(s.anchor.clone()), cx).gap(4.)),
            )
    }
}
