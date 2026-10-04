//! Port of src/shared/ui/DateTimePicker.tsx: a month calendar with one
//! keyboard stop, a minimum date, and a 24-hour local time field. The value
//! is a local `YYYY-MM-DDTHH:mm` string.
//!
//! React moved DOM focus between day buttons. Here the grid holds one focus
//! handle, and the focused day draws the focus ring while the grid has
//! focus, which keeps the single tab stop.

use chrono::{Datelike as _, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone as _};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::notification_model::local_time;
use super::style::text;
use crate::settings::controls::plain_input;

/// `toLocalDateTime`: `YYYY-MM-DDTHH:mm`.
pub fn to_local_date_time(date: NaiveDateTime) -> String {
    date.format("%Y-%m-%dT%H:%M").to_string()
}

/// The local `YYYY-MM-DDTHH:mm` for a JavaScript timestamp.
pub fn local_date_time_from_ms(ms: i64) -> String {
    local_time(ms)
        .map(|time| to_local_date_time(time.naive_local()))
        .unwrap_or_default()
}

/// `parseLocalDateTime`: the local date and time, or `None` for a malformed
/// value or one the calendar would normalize (Feb 30, 24:00, a time inside a
/// daylight saving gap).
pub fn parse_local_date_time(value: &str) -> Option<NaiveDateTime> {
    let bytes = value.as_bytes();
    let shape = bytes.len() == 16
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 => *byte == b':',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let date = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M").ok()?;
    match Local.from_local_datetime(&date) {
        LocalResult::None => None,
        _ => (to_local_date_time(date) == value).then_some(date),
    }
}

/// The JavaScript timestamp of a local date and time (`date.getTime()`).
pub fn local_ms(date: NaiveDateTime) -> Option<i64> {
    Local
        .from_local_datetime(&date)
        .earliest()
        .map(|time| time.timestamp_millis())
}

/// `dateKey`: `YYYY-MM-DD`.
pub fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|first| first.pred_opt())
        .map_or(28, |last| last.day())
}

/// `shiftMonth`: the same day `amount` months away, clamped to that month's
/// last day.
pub fn shift_month(date: NaiveDate, amount: i32) -> NaiveDate {
    let months = date.year() * 12 + date.month0() as i32 + amount;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or(date)
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// `(getDay() + 6) % 7`: Monday is 0.
fn weekday_offset(date: NaiveDate) -> i64 {
    i64::from(date.weekday().num_days_from_monday())
}

/// The month grid: whole weeks, with `None` outside the month.
pub fn month_cells(month: NaiveDate) -> Vec<Option<NaiveDate>> {
    let first = first_of_month(month);
    let offset = weekday_offset(first) as usize;
    let count = days_in_month(first.year(), first.month()) as usize;
    let weeks = (offset + count).div_ceil(7);
    (0..weeks * 7)
        .map(|index| {
            let number = index as i64 - offset as i64 + 1;
            (number > 0 && number <= count as i64)
                .then(|| first.with_day(number as u32))
                .flatten()
        })
        .collect()
}

/// The new value after a day pick or a time edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTimeChange(pub String);

/// `DateTimePicker`.
pub struct DateTimePicker {
    value: String,
    min_date: Option<NaiveDate>,
    today: NaiveDate,
    month: NaiveDate,
    focused_date: NaiveDate,
    grid_focus: FocusHandle,
    time: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DateTimeChange> for DateTimePicker {}

impl DateTimePicker {
    /// `value` is `YYYY-MM-DDTHH:mm`; `min_date` is `YYYY-MM-DD`; `now` is
    /// `Date.now()`, for today's mark.
    pub fn new(
        value: String,
        min_date: Option<&str>,
        now: i64,
        auto_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let today = local_time(now)
            .map(|time| time.date_naive())
            .unwrap_or_default();
        let selected = parse_local_date_time(&format!("{}T12:00", prefix(&value, 10)))
            .map(|date| date.date())
            .unwrap_or(today);
        let minimum = min_date
            .and_then(|min| parse_local_date_time(&format!("{min}T12:00")))
            .map(|date| date.date());
        let initial = match minimum {
            Some(minimum) if selected < minimum => minimum,
            _ => selected,
        };
        let time_value = time_part(&value).to_string();
        let time = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("HH:mm")
                .default_value(time_value)
        });
        let subscriptions = vec![cx.subscribe(&time, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                // `onChange(`${dateKey(selected)}T${value}`)`.
                let typed = input.read(cx).value().to_string();
                let selected = this.selected_date();
                this.emit_value(format!("{}T{typed}", date_key(selected)), cx);
            }
        })];
        let grid_focus = cx.focus_handle();
        if auto_focus {
            window.focus(&grid_focus, cx);
        }
        Self {
            value,
            min_date: minimum,
            today,
            month: first_of_month(initial),
            focused_date: initial,
            grid_focus,
            time,
            _subscriptions: subscriptions,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// The time field (`placeholder="HH:mm"`).
    pub fn time_input(&self) -> &Entity<InputState> {
        &self.time
    }

    pub fn grid_focus(&self) -> &FocusHandle {
        &self.grid_focus
    }

    pub fn month(&self) -> NaiveDate {
        self.month
    }

    fn selected_date(&self) -> NaiveDate {
        parse_local_date_time(&format!("{}T12:00", prefix(&self.value, 10)))
            .map(|date| date.date())
            .unwrap_or(self.today)
    }

    fn emit_value(&mut self, value: String, cx: &mut Context<Self>) {
        self.value = value.clone();
        cx.emit(DateTimeChange(value));
        cx.notify();
    }

    /// `navigate`: move the focused day (and the month with it), never
    /// before the minimum.
    fn navigate(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        let next = match self.min_date {
            Some(minimum) if date < minimum => minimum,
            _ => date,
        };
        self.focused_date = next;
        self.month = first_of_month(next);
        cx.notify();
    }

    /// `pick`.
    pub fn pick(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        self.focused_date = date;
        let time = time_part(&self.value).to_string();
        self.emit_value(format!("{}T{time}", date_key(date)), cx);
    }

    fn previous_disabled(&self) -> bool {
        self.min_date.is_some_and(|minimum| {
            self.month.year() * 12 + self.month.month0() as i32
                <= minimum.year() * 12 + minimum.month0() as i32
        })
    }

    fn on_grid_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let date = self.focused_date;
        let key = event.keystroke.key.as_str();
        if key == "enter" || key == "space" {
            cx.stop_propagation();
            self.pick(date, cx);
            return;
        }
        let offset = weekday_offset(date);
        let next = match key {
            "pageup" => Some(shift_month(date, -1)),
            "pagedown" => Some(shift_month(date, 1)),
            "left" => date.checked_sub_days(chrono::Days::new(1)),
            "right" => date.checked_add_days(chrono::Days::new(1)),
            "up" => date.checked_sub_days(chrono::Days::new(7)),
            "down" => date.checked_add_days(chrono::Days::new(7)),
            "home" => date.checked_sub_days(chrono::Days::new(offset as u64)),
            "end" => date.checked_add_days(chrono::Days::new((6 - offset) as u64)),
            _ => None,
        };
        if let Some(next) = next {
            cx.stop_propagation();
            self.navigate(next, cx);
        }
    }

    fn render_day(&self, date: NaiveDate, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let key = date_key(date);
        let selected = key == prefix(&self.value, 10);
        let disabled = self.min_date.is_some_and(|minimum| date < minimum);
        let is_today = date == self.today;
        let selector = format!("day:{key}");
        let hover_fill = theme.content(0.05);
        let hover_ink = theme.colors.content;
        let title = local_ms(date.and_hms_opt(12, 0, 0).unwrap())
            .map(|ms| {
                monocode_platform::date_time::format_local(
                    ms,
                    monocode_platform::date_time::DateTimeStyle::FullDate,
                )
            })
            .unwrap_or_default();
        let mut day = div()
            .id(SharedString::from(format!("day-{key}")))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(u(32.))
            .rounded(u(theme.radius.sm))
            .text_px(12.)
            .tabular()
            .debug_selector(move || selector)
            .tooltip(tooltip(title))
            .child(date.day().to_string());
        day = if disabled {
            day.text_color(theme.content(0.20))
        } else if selected {
            day.bg(theme.colors.selection_hover)
                .medium()
                .text_color(theme.colors.content)
                .border_1()
                .border_color(theme.content(0.20))
        } else {
            day.text_color(theme.content(0.70))
                .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
        };
        if focused {
            day = day.border_2().border_color(theme.colors.accent);
        }
        if is_today {
            day = day.child(
                div()
                    .absolute()
                    .bottom(u(4.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(div().size(u(2.)).rounded_full().bg(theme.content(0.70))),
            );
        }
        if !disabled {
            day = day.on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.grid_focus, cx);
                this.pick(date, cx);
            }));
        }
        day.into_any_element()
    }
}

/// `value.slice(0, n)`.
fn prefix(value: &str, n: usize) -> &str {
    value.get(..n).unwrap_or(value)
}

/// The text after `T`, or nothing.
fn time_part(value: &str) -> &str {
    value.split_once('T').map_or("", |(_, time)| time)
}

impl Render for DateTimePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        // `useLayoutEffect`: a focused day before a later minimum moves up.
        if let Some(minimum) = self.min_date
            && self.focused_date < minimum
        {
            self.focused_date = minimum;
            self.month = first_of_month(minimum);
        }
        let grid_focused = self.grid_focus.is_focused(window);
        let nav_hover = theme.content(0.05);
        let nav_ink = theme.colors.content;
        let nav = |id: &'static str, glyph: IconName, disabled: bool| {
            let mut button = div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(u(28.))
                .rounded(u(theme.radius.sm))
                .text_color(theme.content(0.55))
                .debug_selector(move || format!("button:{id}"))
                .child(icon(glyph).size(u(14.)));
            if disabled {
                button = button.opacity(0.3);
            } else {
                button = button.hover(move |s| s.bg(nav_hover).text_color(nav_ink));
            }
            button
        };
        let previous_disabled = self.previous_disabled();
        let mut previous = nav("Previous month", IconName::ChevronLeft, previous_disabled);
        if !previous_disabled {
            previous = previous.on_click(cx.listener(|this, _, _, cx| {
                let next = shift_month(this.focused_date, -1);
                this.navigate(next, cx);
            }));
        }
        let next = nav("Next month", IconName::ChevronRight, false).on_click(cx.listener(
            |this, _, _, cx| {
                let next = shift_month(this.focused_date, 1);
                this.navigate(next, cx);
            },
        ));
        let header = div()
            .mb(u(8.))
            .flex()
            .h(u(32.))
            .items_center()
            .justify_between()
            .px(u(4.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(u(6.))
                    .text_px(12.)
                    .medium()
                    .text_color(theme.content(0.90))
                    .child(text(
                        local_ms(self.month.and_hms_opt(12, 0, 0).unwrap())
                            .map(|ms| {
                                monocode_platform::date_time::format_local(
                                    ms,
                                    monocode_platform::date_time::DateTimeStyle::Month,
                                )
                            })
                            .unwrap_or_default(),
                    ))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::NORMAL)
                            .tabular()
                            .text_color(theme.content(0.40))
                            .child(self.month.year().to_string()),
                    ),
            )
            .child(div().flex().gap(u(2.)).child(previous).child(next));
        let mut grid = div()
            .id("date-grid")
            .track_focus(&self.grid_focus)
            .flex()
            .flex_col()
            .debug_selector(|| "grid".into())
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_grid_key(event, cx)),
            )
            .child(
                div()
                    .flex()
                    .children(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|label| {
                        div()
                            .flex_1()
                            .pb(u(6.))
                            .text_center()
                            .text_px(10.)
                            .text_color(theme.content(0.40))
                            .child(label)
                    })),
            );
        let cells = month_cells(self.month);
        for week in cells.chunks(7) {
            let mut row = div().flex().py(u(2.));
            for cell in week {
                let content = match cell {
                    Some(date) => {
                        let focused = grid_focused && *date == self.focused_date;
                        Some(self.render_day(*date, focused, cx))
                    }
                    None => None,
                };
                row = row.child(div().flex_1().flex().justify_center().children(content));
            }
            grid = grid.child(row);
        }
        let input = plain_input(&self.time, cx);
        let time_focused = self.time.read(cx).focus_handle(cx).is_focused(window);
        let footer = div()
            .mt(u(12.))
            .flex()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .px(u(4.))
            .pt(u(12.))
            .child(
                div()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .text_px(12.)
                            .text_color(theme.content(0.70))
                            .child(
                                icon(IconName::Clock)
                                    .size(u(12.))
                                    .text_color(theme.content(0.40)),
                            )
                            .child("Time"),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .text_px(10.)
                            .text_color(theme.content(0.40))
                            .child("Local time, 24-hour"),
                    ),
            )
            .child(
                div()
                    .w(u(80.))
                    .rounded(u(theme.radius.sm))
                    .border_1()
                    .border_color(if time_focused {
                        theme.content(0.40)
                    } else {
                        theme.content(0.10)
                    })
                    .bg(theme.content(0.05))
                    .px(u(8.))
                    .py(u(6.))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(12.)
                    .tabular()
                    .debug_selector(|| "input:HH:mm".into())
                    .when(time_focused, |el| el.border_color(theme.colors.accent))
                    .child(input),
            );
        div().child(header).child(grid).child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_exact_local_date_times() {
        assert!(parse_local_date_time("2030-01-16T17:00").is_some());
        assert!(parse_local_date_time("2030-02-30T17:00").is_none());
        assert!(parse_local_date_time("2030-01-16T24:00").is_none());
        assert!(parse_local_date_time("2030-01-16 17:00").is_none());
        assert!(parse_local_date_time("2030-1-16T17:00").is_none());
    }

    #[test]
    fn shifts_months_and_clamps_the_day() {
        let date = NaiveDate::from_ymd_opt(2030, 1, 31).unwrap();
        assert_eq!(
            shift_month(date, 1),
            NaiveDate::from_ymd_opt(2030, 2, 28).unwrap()
        );
        assert_eq!(
            shift_month(date, -1),
            NaiveDate::from_ymd_opt(2029, 12, 31).unwrap()
        );
    }

    #[test]
    fn lays_out_whole_weeks_from_monday() {
        // January 2030 starts on a Tuesday.
        let cells = month_cells(NaiveDate::from_ymd_opt(2030, 1, 1).unwrap());
        assert_eq!(cells.len(), 35);
        assert_eq!(cells[0], None);
        assert_eq!(cells[1], NaiveDate::from_ymd_opt(2030, 1, 1));
        assert_eq!(cells[31], NaiveDate::from_ymd_opt(2030, 1, 31));
    }
}
