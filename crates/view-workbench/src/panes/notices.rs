//! The session notices and empty states: ports of
//! src/features/sessions/ui/UsageLimitNotice.tsx, ReminderNotices.tsx,
//! SessionsEmpty.tsx, and DiscussionEmpty.tsx.
//!
//! Times come formatted from the owner, because the formatters
//! (`formatUsageLimitReset`, `formatReminderTime`) live with the engine's
//! attention and automation code.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, ElementId, EventEmitter, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};
use monocode_ui::color::with_alpha;
use monocode_ui::widgets::{MenuEntry, MenuItem, context_menu, menu, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::pixel_art::sprite;

/// `formatResetDuration` from src/features/providers/model/rateLimits.ts.
pub fn format_reset_duration(ms: i64) -> String {
    if ms <= 0 {
        return "now".into();
    }
    let total_mins = ms / 60_000;
    if total_mins < 60 {
        return format!("{total_mins}m");
    }
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours >= 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        return if rem_hours > 0 {
            format!("{days}d {rem_hours}h")
        } else {
            format!("{days}d")
        };
    }
    if mins > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{hours}h")
    }
}

/// `formatUsageLimitReset`: "3:16 AM · in 4h 42m". The owner passes the
/// engine's formatter, which knows the local time zone.
pub type ResetFormatter = Rc<dyn Fn(i64, i64) -> String>;

/// A formatter without the wall-clock part: "in 4h 42m".
pub fn relative_reset_formatter() -> ResetFormatter {
    Rc::new(|resets_at, now| format!("in {}", format_reset_duration(resets_at - now)))
}

/// `UsageLimit`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageLimit {
    /// Epoch ms when the provider's window resets, once known.
    pub resets_at: Option<i64>,
    /// Send a continue turn once the window resets.
    pub resume_at_reset: bool,
}

/// What the usage limit notice reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageLimitEvent {
    /// `onResume`.
    Resume,
    /// `onResumeAtReset(enabled)`.
    ResumeAtReset(bool),
    /// `onDismiss`.
    Dismiss,
}

/// Epoch milliseconds now.
pub fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// `UsageLimitNotice`: the amber strip above the composer once a provider
/// refuses for usage.
pub struct UsageLimitNotice {
    limit: UsageLimit,
    now: i64,
    clock: Rc<dyn Fn() -> i64>,
    format: ResetFormatter,
    _tick: Option<Task<()>>,
}

impl EventEmitter<UsageLimitEvent> for UsageLimitNotice {}

impl UsageLimitNotice {
    pub fn new(limit: UsageLimit, format: ResetFormatter, cx: &mut Context<Self>) -> Self {
        Self::with_clock(limit, format, Rc::new(epoch_ms), cx)
    }

    /// With a clock of epoch milliseconds, for tests and screenshots.
    pub fn with_clock(
        limit: UsageLimit,
        format: ResetFormatter,
        clock: Rc<dyn Fn() -> i64>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut notice = Self {
            limit,
            now: clock(),
            clock,
            format,
            _tick: None,
        };
        notice.restart_tick(cx);
        notice
    }

    pub fn set_limit(&mut self, limit: UsageLimit, cx: &mut Context<Self>) {
        self.limit = limit;
        self.now = (self.clock)();
        self.restart_tick(cx);
        cx.notify();
    }

    /// The reset is still ahead.
    pub fn waiting(&self) -> bool {
        self.limit.resets_at.is_some_and(|at| at > self.now)
    }

    /// The text after "Usage limit reached".
    pub fn status(&self) -> String {
        match self.limit.resets_at {
            None => String::new(),
            Some(at) if at > self.now => format!("Resets {}", (self.format)(at, self.now)),
            Some(_) => "Limit has reset".into(),
        }
    }

    /// Ticks the countdown every 30s, and flips to Resume once the window
    /// resets.
    fn restart_tick(&mut self, cx: &mut Context<Self>) {
        if !self.waiting() {
            self._tick = None;
            return;
        }
        self._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                let waiting = this
                    .update(cx, |this, cx| {
                        this.now = (this.clock)();
                        cx.notify();
                        this.waiting()
                    })
                    .unwrap_or(false);
                if !waiting {
                    break;
                }
            }
        }));
    }
}

fn strip_button(id: &'static str, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let hover = theme.content(0.10);
    let ink = theme.colors.content;
    div()
        .id(id)
        .group(id)
        .debug_selector(move || id.into())
        .flex()
        .flex_none()
        .h(u(24.))
        .items_center()
        .gap(u(6.))
        .rounded(u(theme.radius.md))
        .px(u(6.))
        .hover(move |s| s.bg(hover).text_color(ink))
}

/// An icon in a strip button: it takes the button's hover color.
fn strip_icon(group: &'static str, name: IconName, color: gpui::Hsla, theme: &Theme) -> gpui::Svg {
    let ink = theme.colors.content;
    icon(name)
        .flex_none()
        .size(u(14.))
        .text_color(color)
        .group_hover(group, move |s| s.text_color(ink))
}

impl Render for UsageLimitNotice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let amber = theme.colors.warning;
        let action: AnyElement = if !self.waiting() {
            strip_button("usage-limit-resume", &theme)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(UsageLimitEvent::Resume)))
                .child(strip_icon(
                    "usage-limit-resume",
                    IconName::Play,
                    theme.content(0.55),
                    &theme,
                ))
                .child("Resume")
                .into_any_element()
        } else if self.limit.resume_at_reset {
            strip_button("usage-limit-armed", &theme)
                .text_color(amber)
                .tooltip(tooltip("Cancel the automatic resume"))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(UsageLimitEvent::ResumeAtReset(false))))
                .child(strip_icon(
                    "usage-limit-armed",
                    IconName::Clock,
                    amber,
                    &theme,
                ))
                .child("Resuming at reset")
                .into_any_element()
        } else {
            strip_button("usage-limit-arm", &theme)
                .tooltip(tooltip("Continue this session once the limit resets"))
                .on_click(cx.listener(|_, _, _, cx| cx.emit(UsageLimitEvent::ResumeAtReset(true))))
                .child(strip_icon(
                    "usage-limit-arm",
                    IconName::Clock,
                    theme.content(0.55),
                    &theme,
                ))
                .child("Resume at reset")
                .into_any_element()
        };
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        div().px(u(8.)).text_color(theme.content(0.55)).child(
            div()
                .debug_selector(|| "usage-limit".into())
                .relative()
                .flex()
                .h(u(32.))
                .items_center()
                .gap(u(8.))
                .rounded_t(u(10.))
                .border_1()
                .border_b_0()
                .border_color(with_alpha(amber, 0.25))
                .bg(with_alpha(amber, 0.10))
                .px(u(8.))
                .text_px(12.)
                .child(
                    icon(IconName::Gauge)
                        .flex_none()
                        .size(u(14.))
                        .text_color(amber),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.content(0.85))
                        .child("Usage limit reached"),
                )
                .child(div().flex_1().min_w_0().truncate().child(self.status()))
                .child(action)
                .child(
                    div()
                        .id("usage-limit-dismiss")
                        .group("usage-limit-dismiss")
                        .flex()
                        .flex_none()
                        .size(u(24.))
                        .items_center()
                        .justify_center()
                        .rounded(u(theme.radius.md))
                        .hover(move |s| s.bg(hover).text_color(ink))
                        .tooltip(tooltip("Dismiss"))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(UsageLimitEvent::Dismiss)))
                        .child(strip_icon(
                            "usage-limit-dismiss",
                            IconName::X,
                            theme.content(0.55),
                            &theme,
                        )),
                ),
        )
    }
}

/// `SessionsEmpty`'s terminal at a prompt, in the mascots' `#` and `.`
/// convention.
pub const TERMINAL: [&str; 12] = [
    "..############..",
    ".##..........##.",
    ".#............#.",
    ".#..#.........#.",
    ".#...#...###..#.",
    ".#..#....###..#.",
    ".#............#.",
    ".#............#.",
    ".##..........##.",
    "..############..",
    "......####......",
    "....########....",
];

/// The terminal's glow: a sparse scatter drawn dimmer than the sprite so the
/// shape dissolves at its edges.
pub const TERMINAL_GLOW: [&str; 12] = [
    "#.#..........#.#",
    "................",
    "#..............#",
    "................",
    "................",
    "................",
    "................",
    "#..............#",
    "................",
    "#.#..........#.#",
    "................",
    "..#..........#..",
];

/// `DiscussionEmpty`'s overlapping speech bubbles.
pub const BUBBLES: [&str; 16] = [
    "..###########.........",
    ".##.........##........",
    ".#...........#........",
    ".#...#.#.#...#........",
    ".#...........#........",
    ".##.........##........",
    "..##########..........",
    "..##.........#######..",
    "..#.........##.....##.",
    "............#.......#.",
    "............#.#.#.#.#.",
    "............#.......#.",
    "............##.....##.",
    ".............#######..",
    "..................##..",
    "...................#..",
];

pub const BUBBLES_GLOW: [&str; 16] = [
    "#.............#.......",
    "......................",
    "......................",
    "#.............#.......",
    "......................",
    "......................",
    "#.....................",
    "...........#.........#",
    "......................",
    "......................",
    "...........#.........#",
    "......................",
    "......................",
    ".....................#",
    "......................",
    "......................",
];

/// An empty state: a message and a pixel sprite with its glow, centered.
#[derive(IntoElement)]
pub struct PixelEmptyState {
    message: SharedString,
    sprite: &'static [&'static str],
    glow: &'static [&'static str],
    sprite_first: bool,
    selector: &'static str,
}

/// `SessionsEmpty`: a project with no sessions yet.
pub fn sessions_empty(message: impl Into<SharedString>) -> PixelEmptyState {
    PixelEmptyState {
        message: message.into(),
        sprite: &TERMINAL,
        glow: &TERMINAL_GLOW,
        sprite_first: false,
        selector: "sessions-empty",
    }
}

/// `DiscussionEmpty`: a discussion with no messages yet.
pub fn discussion_empty(message: impl Into<SharedString>) -> PixelEmptyState {
    PixelEmptyState {
        message: message.into(),
        sprite: &BUBBLES,
        glow: &BUBBLES_GLOW,
        sprite_first: true,
        selector: "discussion-empty",
    }
}

impl RenderOnce for PixelEmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let columns = self.sprite[0].len() as f32;
        let height = 96.0 * self.sprite.len() as f32 / columns;
        let ink = theme.content(0.25);
        let art = div()
            .relative()
            .w(u(96.))
            .h(u(height))
            .child(
                sprite(self.glow.iter().copied(), with_alpha(ink, 0.4))
                    .absolute()
                    .size_full(),
            )
            .child(
                sprite(self.sprite.iter().copied(), ink)
                    .absolute()
                    .size_full(),
            );
        let message = div()
            .text_px(13.)
            .leading(theme.leading.relaxed)
            .text_color(theme.content(0.45))
            .text_center()
            .child(self.message);
        let selector = self.selector;
        let root = div()
            .debug_selector(move || selector.into())
            .flex()
            .flex_col()
            .min_h_full()
            .items_center()
            .justify_center()
            .gap(u(20.))
            .px(u(24.))
            .py(u(40.));
        if self.sprite_first {
            root.child(art).child(message)
        } else {
            root.child(message).child(art)
        }
    }
}

/// One due reminder, formatted by the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderNotice {
    pub session_id: String,
    pub due_at: i64,
    /// `sessionDisplayTitle(title, harness)`.
    pub title: String,
    /// `projectName(cwd)`.
    pub project: String,
    /// `formatReminderTime(dueAt)`.
    pub due_label: String,
}

/// A snooze preset (`sessionReminderPresets`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozePreset {
    pub id: String,
    pub label: String,
    pub disabled: bool,
}

/// What the reminders panel reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReminderNoticesEvent {
    /// `onOpen`.
    Open { session_id: String },
    /// `onSnooze`: the owner turns the preset into a time
    /// (`reminderTime`).
    Snooze {
        session_ids: Vec<String>,
        preset: String,
    },
    /// `onDismiss`.
    Dismiss {
        session_ids: Vec<String>,
        expected_due_at: Option<i64>,
    },
    /// `onRetry`.
    Retry,
    /// `onOpenSettings`.
    OpenSettings,
}

/// `ReminderNotices`: the panel of due reminders in the window's top right
/// corner.
pub struct ReminderNotices {
    reminders: Vec<ReminderNotice>,
    error: Option<String>,
    notifications_enabled: bool,
    presets: Vec<SnoozePreset>,
    snooze: Option<(String, i64, Point<Pixels>)>,
}

impl EventEmitter<ReminderNoticesEvent> for ReminderNotices {}

impl ReminderNotices {
    pub fn new() -> Self {
        Self {
            reminders: Vec::new(),
            error: None,
            notifications_enabled: true,
            presets: Vec::new(),
            snooze: None,
        }
    }

    pub fn set_reminders(
        &mut self,
        reminders: Vec<ReminderNotice>,
        error: Option<String>,
        notifications_enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.reminders = reminders;
        self.error = error;
        self.notifications_enabled = notifications_enabled;
        // The snooze menu closes when its reminder changes or goes away.
        if let Some((id, due_at, _)) = &self.snooze
            && !self
                .reminders
                .iter()
                .any(|r| r.session_id == *id && r.due_at == *due_at)
        {
            self.snooze = None;
        }
        cx.notify();
    }

    /// The presets the snooze menu offers, built by the owner from the
    /// current time.
    pub fn set_snooze_presets(&mut self, presets: Vec<SnoozePreset>, cx: &mut Context<Self>) {
        self.presets = presets;
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        !self.reminders.is_empty() || self.error.is_some()
    }
}

impl Default for ReminderNotices {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for ReminderNotices {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_visible() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let ink = theme.colors.content;
        let hover = theme.content(0.10);
        let mut list = div()
            .id("reminder-list")
            .flex()
            .flex_col()
            .max_h(u(320.))
            .overflow_y_scroll();
        for (index, reminder) in self.reminders.iter().enumerate() {
            let open = reminder.session_id.clone();
            let open_title = reminder.session_id.clone();
            let snooze = (reminder.session_id.clone(), reminder.due_at);
            let dismiss = (reminder.session_id.clone(), reminder.due_at);
            let expanded = self
                .snooze
                .as_ref()
                .is_some_and(|(id, due, _)| *id == reminder.session_id && *due == reminder.due_at);
            let small = |id: String, label: &'static str| {
                div()
                    .id(ElementId::Name(id.into()))
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .py(u(4.))
                    .child(label)
            };
            list = list.child(
                div()
                    .px(u(12.))
                    .py(u(10.))
                    .when(index > 0, |row| {
                        row.border_t_1().border_color(theme.colors.stroke)
                    })
                    .child(
                        div()
                            .id(ElementId::Name(
                                format!("reminder-open:{}", reminder.session_id).into(),
                            ))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(ReminderNoticesEvent::Open {
                                    session_id: open_title.clone(),
                                })
                            }))
                            .child(
                                div()
                                    .truncate()
                                    .text_px(13.)
                                    .medium()
                                    .child(reminder.title.clone()),
                            )
                            .child(
                                div()
                                    .id(ElementId::Name(
                                        format!("reminder-due:{}", reminder.session_id).into(),
                                    ))
                                    .mt(u(4.))
                                    .truncate()
                                    .text_px(11.)
                                    .text_color(theme.content(0.50))
                                    .tooltip(tooltip(reminder.due_label.clone()))
                                    .child(format!(
                                        "{} · {}",
                                        reminder.project, reminder.due_label
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .mt(u(8.))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .text_px(11.)
                            .child(
                                small(
                                    format!("reminder-session:{}", reminder.session_id),
                                    "Open session",
                                )
                                .bg(theme.content(0.10))
                                .hover({
                                    let fill = theme.content(0.15);
                                    move |s| s.bg(fill)
                                })
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        cx.emit(ReminderNoticesEvent::Open {
                                            session_id: open.clone(),
                                        })
                                    },
                                )),
                            )
                            .child(
                                small(format!("reminder-snooze:{}", reminder.session_id), "Snooze")
                                    .text_color(theme.content(0.65))
                                    .hover(move |s| s.bg(hover))
                                    .when(expanded, |button| button.bg(hover))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.snooze = Some((
                                                snooze.0.clone(),
                                                snooze.1,
                                                event.position + gpui::point(px(0.), px(16.)),
                                            ));
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                small(
                                    format!("reminder-dismiss:{}", reminder.session_id),
                                    "Dismiss",
                                )
                                .ml_auto()
                                .text_color(theme.content(0.50))
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(
                                    move |_, _, _, cx| {
                                        cx.emit(ReminderNoticesEvent::Dismiss {
                                            session_ids: vec![dismiss.0.clone()],
                                            expected_due_at: Some(dismiss.1),
                                        })
                                    },
                                )),
                            ),
                    ),
            );
        }
        let mut panel = div()
            .debug_selector(|| "reminder-notices".into())
            .w(u(320.))
            .max_w(window.viewport_size().width - u(24.).to_pixels(window.rem_size()))
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.15))
            .bg(with_alpha(theme.colors.background_base, 0.95))
            .text_color(ink)
            .shadow_xl()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .border_b_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(10.))
                    .child(
                        icon(IconName::Clock)
                            .size(u(14.))
                            .text_color(theme.colors.warning),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_px(12.)
                            .semibold()
                            .child("Due reminders"),
                    )
                    .child(div().text_px(11.).text_color(theme.content(0.50)).child(
                        if self.reminders.is_empty() {
                            String::new()
                        } else {
                            self.reminders.len().to_string()
                        },
                    )),
            );
        if self.error.is_some() {
            panel = panel.child(
                div()
                    .flex()
                    .gap(u(4.))
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.70))
                    .child("Couldn’t load reminders.")
                    .child(
                        div()
                            .id("reminder-retry")
                            .underline()
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(ReminderNoticesEvent::Retry)),
                            )
                            .child("Retry"),
                    ),
            );
        }
        panel = panel.child(list);
        if !self.notifications_enabled && !self.reminders.is_empty() {
            panel = panel.child(
                div()
                    .id("reminder-settings")
                    .w_full()
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink))
                    .on_click(
                        cx.listener(|_, _, _, cx| cx.emit(ReminderNoticesEvent::OpenSettings)),
                    )
                    .child("Desktop alerts are off. Enable in Settings."),
            );
        }
        let viewport = window.viewport_size();
        let inset = u(12.).to_pixels(window.rem_size());
        let mut root = div().child(
            gpui::deferred(
                gpui::anchored()
                    .anchor(gpui::Anchor::TopRight)
                    .position(gpui::point(viewport.width - inset, inset))
                    .child(panel),
            )
            .with_priority(theme.layer.popover.saturating_sub(1)),
        );
        if let Some((id, _, position)) = self.snooze.clone() {
            let entries: Vec<MenuEntry> = self
                .presets
                .iter()
                .map(|preset| {
                    MenuItem::new(preset.id.clone(), preset.label.clone())
                        .disabled(preset.disabled)
                        .into()
                })
                .collect();
            let pick = cx.entity().downgrade();
            let dismiss = cx.entity().downgrade();
            root = root.child(context_menu(
                position,
                menu("snooze-menu", entries).on_pick(move |preset, _, cx| {
                    let preset = preset.to_string();
                    let id = id.clone();
                    pick.update(cx, |this, cx| {
                        this.snooze = None;
                        cx.emit(ReminderNoticesEvent::Snooze {
                            session_ids: vec![id],
                            preset,
                        });
                        cx.notify();
                    })
                    .ok();
                }),
                move |_, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.snooze = None;
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            ));
        }
        root.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use gpui::{Modifiers, TestAppContext};

    use super::*;
    use crate::panes::test_support::{draw, init};

    #[test]
    fn formats_reset_durations_in_whole_units() {
        assert_eq!(format_reset_duration(0), "now");
        assert_eq!(format_reset_duration(59 * 60_000), "59m");
        assert_eq!(format_reset_duration(4 * 3_600_000 + 42 * 60_000), "4h 42m");
        assert_eq!(format_reset_duration(2 * 3_600_000), "2h");
        assert_eq!(format_reset_duration(28 * 3_600_000), "1d 4h");
        assert_eq!(format_reset_duration(48 * 3_600_000), "2d");
    }

    #[gpui::test]
    fn counts_down_then_offers_resume_once_the_limit_resets(cx: &mut TestAppContext) {
        cx.update(init);
        let now = Rc::new(Cell::new(1_000_000i64));
        let clock_now = now.clone();
        let clock: Rc<dyn Fn() -> i64> = Rc::new(move || clock_now.get());
        let limit = UsageLimit {
            resets_at: Some(1_000_000 + 45 * 60_000),
            resume_at_reset: false,
        };
        let (notice, cx) = cx.add_window_view(move |_, cx| {
            UsageLimitNotice::with_clock(limit, relative_reset_formatter(), clock, cx)
        });
        let events: Rc<RefCell<Vec<UsageLimitEvent>>> = Rc::default();
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&notice, move |_, event: &UsageLimitEvent, _| {
                sink.borrow_mut().push(*event)
            })
            .detach();
        });
        draw(cx);
        assert_eq!(
            notice.read_with(cx, |notice, _| notice.status()),
            "Resets in 45m"
        );
        let arm = cx.debug_bounds("usage-limit-arm").unwrap().center();
        cx.simulate_click(arm, Modifiers::none());
        assert_eq!(
            events.borrow().as_slice(),
            [UsageLimitEvent::ResumeAtReset(true)]
        );

        now.set(1_000_000 + 46 * 60_000);
        cx.executor().advance_clock(Duration::from_secs(30));
        draw(cx);
        assert_eq!(
            notice.read_with(cx, |notice, _| notice.status()),
            "Limit has reset"
        );
        assert!(cx.debug_bounds("usage-limit-resume").is_some());
    }

    #[gpui::test]
    fn lists_due_reminders_and_reports_snooze_and_dismiss(cx: &mut TestAppContext) {
        cx.update(init);
        let (notices, cx) = cx.add_window_view(|_, cx| {
            let mut notices = ReminderNotices::new();
            notices.set_reminders(
                vec![ReminderNotice {
                    session_id: "s1".into(),
                    due_at: 5,
                    title: "Ship the arcade".into(),
                    project: "agent-terminal".into(),
                    due_label: "Fri, Oct 3, 9:00 AM".into(),
                }],
                None,
                false,
                cx,
            );
            notices.set_snooze_presets(
                vec![SnoozePreset {
                    id: "reminder:1h".into(),
                    label: "In 1 hour (10:00)".into(),
                    disabled: false,
                }],
                cx,
            );
            notices
        });
        let events: Rc<RefCell<Vec<ReminderNoticesEvent>>> = Rc::default();
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&notices, move |_, event: &ReminderNoticesEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        draw(cx);
        assert!(cx.debug_bounds("reminder-notices").is_some());
        notices.update(cx, |notices, cx| {
            notices.snooze = Some(("s1".into(), 5, gpui::point(px(10.), px(10.))));
            cx.notify();
        });
        draw(cx);
        notices.update(cx, |notices, cx| {
            notices.set_reminders(Vec::new(), None, true, cx)
        });
        draw(cx);
        assert!(notices.read_with(cx, |notices, _| notices.snooze.is_none()
            && !notices.is_visible()));
        assert!(cx.debug_bounds("reminder-notices").is_none());
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn pixel_states_keep_their_grids() {
        assert!(TERMINAL.iter().all(|row| row.len() == 16));
        assert!(BUBBLES.iter().all(|row| row.len() == 22));
        assert_eq!(TERMINAL.len(), TERMINAL_GLOW.len());
        assert_eq!(BUBBLES.len(), BUBBLES_GLOW.len());
    }
}
