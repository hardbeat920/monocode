//! Port of src/shared/ui/DateTimePicker.tsx: a month calendar with one
//! keyboard stop, a minimum date, and a 24-hour local time field. The value
//! is a local `YYYY-MM-DDTHH:mm` string.
//!
//! React moved DOM focus between day buttons. Here the grid holds one focus
//! handle and the focused day draws the focus ring while the grid is
//! focused, which keeps the single tab stop.

use std::rc::Rc;

use chrono::{Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use gpui::{
    App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::widgets::plain_input;

/// `toLocalDateTime`: `YYYY-MM-DDTHH:mm`.
pub fn to_local_date_time(date: NaiveDateTime) -> String {
    date.format("%Y-%m-%dT%H:%M").to_string()
}

/// `parseLocalDateTime`: the local date and time, or `None` for a malformed
/// value or one the calendar would normalize (Feb 30, 24:00, a time inside
/// a daylight saving gap).
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

/// `shiftMonth`: the same day `amount` months away, clamped to that
/// month's last day.
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
fn monday_offset(date: NaiveDate) -> i64 {
    i64::from(date.weekday().num_days_from_monday())
}

/// The full date title (`dateStyle: "full"` in en-US).
fn full_date(date: NaiveDate) -> String {
    date.format("%A, %B %-d, %Y").to_string()
}

type ChangeFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

pub struct DateTimePicker {
    value: String,
    min_date: Option<String>,
    today: NaiveDate,
    month: NaiveDate,
    focused_date: NaiveDate,
    grid_focus: FocusHandle,
    time: Entity<InputState>,
    /// The time field's text as last synced, to skip echoes.
    time_text: String,
    on_change: Option<ChangeFn>,
    _subscriptions: Vec<Subscription>,
}

impl DateTimePicker {
    pub fn new(value: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = Local::now().date_naive();
        let time_text = time_part(value).to_string();
        let initial_time = time_text.clone();
        let time = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("HH:mm")
                .default_value(initial_time)
        });
        time.update(cx, |input, cx| {
            input.set_text_align(gpui::TextAlign::Center, cx)
        });
        let events = cx.subscribe_in(&time, window, |this, input, event, window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let text = input.read(cx).value().to_string();
            if text == this.time_text {
                return;
            }
            this.time_text = text.clone();
            let selected = this.selected_date();
            let next = format!("{}T{text}", date_key(selected));
            this.emit(next, window, cx);
        });
        let mut picker = Self {
            value: value.to_string(),
            min_date: None,
            today,
            month: first_of_month(today),
            focused_date: today,
            grid_focus: cx.focus_handle(),
            time,
            time_text,
            on_change: None,
            _subscriptions: vec![events],
        };
        picker.reset_view();
        picker
    }

    /// `minDate`: `YYYY-MM-DD`.
    pub fn min_date(mut self, min_date: Option<&str>) -> Self {
        self.min_date = min_date.map(str::to_string);
        self.reset_view();
        self
    }

    /// `onChange`, with the new `YYYY-MM-DDTHH:mm` value.
    pub fn on_change(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// `autoFocus`: focus the selected day.
    pub fn auto_focus(self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        self.grid_focus.focus(window, cx);
        self
    }

    /// Replace "today", for tests and screenshots.
    pub fn with_today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self.reset_view();
        self
    }

    /// The controlled value changed.
    pub fn set_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.value = value.to_string();
        let time = time_part(value).to_string();
        if time != self.time_text {
            self.time_text = time.clone();
            self.time
                .update(cx, |input, cx| input.set_value(time, window, cx));
        }
        cx.notify();
    }

    /// The minimum date changed. A focused day before it moves to it.
    pub fn set_min_date(&mut self, min_date: Option<&str>, cx: &mut Context<Self>) {
        self.min_date = min_date.map(str::to_string);
        if let Some(minimum) = self.minimum()
            && self.focused_date < minimum
        {
            self.focused_date = minimum;
            self.month = first_of_month(minimum);
        }
        cx.notify();
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn month(&self) -> NaiveDate {
        self.month
    }

    pub fn focused_date(&self) -> NaiveDate {
        self.focused_date
    }

    pub fn grid_focus(&self) -> &FocusHandle {
        &self.grid_focus
    }

    fn minimum(&self) -> Option<NaiveDate> {
        let min = self.min_date.as_deref()?;
        parse_local_date_time(&format!("{min}T12:00")).map(|date| date.date())
    }

    /// `selected`: the value's date, else today.
    fn selected_date(&self) -> NaiveDate {
        let day = self.value.get(..10).unwrap_or("");
        parse_local_date_time(&format!("{day}T12:00"))
            .map(|date| date.date())
            .unwrap_or(self.today)
    }

    /// The initial month and focused day: the selection, or the minimum when
    /// the selection is earlier.
    fn reset_view(&mut self) {
        let selected = self.selected_date();
        let initial = match self.minimum() {
            Some(minimum) if selected < minimum => minimum,
            _ => selected,
        };
        self.focused_date = initial;
        self.month = first_of_month(initial);
    }

    /// Whether `date` comes before the minimum.
    pub fn is_disabled(&self, date: NaiveDate) -> bool {
        self.minimum().is_some_and(|minimum| date < minimum)
    }

    /// `previousDisabled`: the shown month is the minimum's month or earlier.
    pub fn previous_disabled(&self) -> bool {
        self.minimum().is_some_and(|minimum| {
            self.month.year() * 12 + self.month.month0() as i32
                <= minimum.year() * 12 + minimum.month0() as i32
        })
    }

    /// `navigate`: move the focused day, never before the minimum, and show
    /// its month.
    pub fn navigate(&mut self, date: NaiveDate, cx: &mut Context<Self>) {
        let next = match self.minimum() {
            Some(minimum) if date < minimum => minimum,
            _ => date,
        };
        self.focused_date = next;
        self.month = first_of_month(next);
        cx.notify();
    }

    /// The month arrows.
    pub fn shift_shown_month(&mut self, amount: i32, cx: &mut Context<Self>) {
        let next = shift_month(self.focused_date, amount);
        self.navigate(next, cx);
    }

    /// `pick`: choose a day and keep the time.
    pub fn pick(&mut self, date: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_date = date;
        let next = format!("{}T{}", date_key(date), time_part(&self.value));
        self.emit(next, window, cx);
        cx.notify();
    }

    fn emit(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.on_change.clone() {
            window.defer(cx, move |window, cx| f(&value, window, cx));
        }
    }

    /// `onDayKeyDown`: arrows, Home and End within the week, PageUp and
    /// PageDown by month, Enter and Space to pick.
    pub fn day_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let date = self.focused_date;
        if matches!(key, "enter" | "space") {
            self.pick(date, window, cx);
            return true;
        }
        let offset = monday_offset(date);
        let next = match key {
            "pageup" => shift_month(date, -1),
            "pagedown" => shift_month(date, 1),
            "left" => date - chrono::Days::new(1),
            "right" => date + chrono::Days::new(1),
            "up" => date - chrono::Days::new(7),
            "down" => date + chrono::Days::new(7),
            "home" => date - chrono::Days::new(offset as u64),
            "end" => date + chrono::Days::new((6 - offset) as u64),
            _ => return false,
        };
        self.navigate(next, cx);
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        if self.day_key(event.keystroke.key.as_str(), window, cx) {
            cx.stop_propagation();
        }
    }

    /// The month's days, padded to whole weeks starting on Monday.
    pub fn cells(&self) -> Vec<Option<NaiveDate>> {
        let offset = monday_offset(self.month) as usize;
        let count = days_in_month(self.month.year(), self.month.month()) as usize;
        let total = (offset + count).div_ceil(7) * 7;
        (0..total)
            .map(|index| {
                let number = index as i64 - offset as i64 + 1;
                if number > 0 && number <= count as i64 {
                    self.month.with_day(number as u32)
                } else {
                    None
                }
            })
            .collect()
    }

    fn render_day(
        &self,
        date: NaiveDate,
        grid_focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let key = date_key(date);
        let selected = key == self.value.get(..10).unwrap_or("");
        let today = date == self.today;
        let disabled = self.is_disabled(date);
        let focused = grid_focused && date == self.focused_date;
        let ink = theme.colors.content;
        let hover = theme.content(0.05);
        let selector = key.clone();
        let mut day = div()
            .id(ElementId::Name(SharedString::from(format!("day-{key}"))))
            .debug_selector(move || format!("day {selector}"))
            .relative()
            .flex()
            .size(u(32.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.sm))
            .text_px(theme.text.label)
            .tabular()
            .tooltip(monocode_ui::widgets::tooltip(full_date(date)))
            .child(date.day().to_string());
        day = if disabled {
            day.text_color(theme.content(0.20))
        } else if selected {
            day.bg(theme.colors.selection_hover)
                .medium()
                .text_color(ink)
                .border_1()
                .border_color(theme.content(0.20))
        } else {
            day.text_color(theme.content(0.70))
                .hover(move |s| s.bg(hover).text_color(ink))
        };
        if focused {
            day = day.border_2().border_color(theme.colors.accent);
        }
        if today {
            day = day.child(
                div()
                    .absolute()
                    .bottom(u(4.))
                    .left(gpui::relative(0.5))
                    .ml(px(-1.))
                    .size(px(2.))
                    .rounded_full()
                    .bg(if disabled {
                        theme.content(0.20)
                    } else if selected {
                        ink
                    } else {
                        theme.content(0.70)
                    }),
            );
        }
        if !disabled {
            day = day.on_click(cx.listener(move |this, _, window, cx| {
                this.grid_focus.focus(window, cx);
                this.pick(date, window, cx);
            }));
        }
        day
    }
}

/// The time after `T`, or an empty string.
fn time_part(value: &str) -> &str {
    value.find('T').map_or("", |index| &value[index + 1..])
}

impl Focusable for DateTimePicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.grid_focus.clone()
    }
}

impl Render for DateTimePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let grid_focused = self.grid_focus.is_focused(window);
        let previous_disabled = self.previous_disabled();
        let arrow = |id: &'static str, glyph: IconName, disabled: bool| {
            let hover = theme.content(0.05);
            let ink = theme.colors.content;
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .flex()
                .size(u(28.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.sm))
                .group(id)
                .when(disabled, |el| el.opacity(0.3))
                .when(!disabled, |el| el.hover(move |s| s.bg(hover)))
                .child(
                    icon(glyph)
                        .size(u(14.))
                        .text_color(theme.content(0.55))
                        .when(!disabled, |svg| {
                            svg.group_hover(id, move |s| s.text_color(ink))
                        }),
                )
        };
        let header = div()
            .flex()
            .h(u(32.))
            .mb(u(8.))
            .px(u(4.))
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(u(6.))
                    .text_px(theme.text.label)
                    .medium()
                    .text_color(theme.content(0.90))
                    .child(self.month.format("%B").to_string())
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::NORMAL)
                            .tabular()
                            .text_color(theme.content(0.40))
                            .child(self.month.year().to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(u(2.))
                    .child(
                        arrow("previous-month", IconName::ChevronLeft, previous_disabled).when(
                            !previous_disabled,
                            |el| {
                                el.on_click(
                                    cx.listener(|this, _, _, cx| this.shift_shown_month(-1, cx)),
                                )
                            },
                        ),
                    )
                    .child(
                        arrow("next-month", IconName::ChevronRight, false)
                            .on_click(cx.listener(|this, _, _, cx| this.shift_shown_month(1, cx))),
                    ),
            );
        let mut grid = div()
            .id("date-grid")
            .track_focus(&self.grid_focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .children(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|label| {
                        div()
                            .flex_1()
                            .pb(u(6.))
                            .text_center()
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.40))
                            .child(label)
                    })),
            );
        let cells = self.cells();
        for week in cells.chunks(7) {
            let mut row = div().flex().py(u(2.));
            for cell in week {
                let mut slot = div().flex().flex_1().justify_center();
                if let Some(date) = cell {
                    slot = slot.child(self.render_day(*date, grid_focused, &theme, cx));
                }
                row = row.child(slot);
            }
            grid = grid.child(row);
        }
        let time_row = div()
            .mt(u(12.))
            .pt(u(12.))
            .px(u(4.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .flex()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .child(
                div()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .text_px(theme.text.label)
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
                            .text_px(theme.text.micro)
                            .text_color(theme.content(0.40))
                            .child("Local time, 24-hour"),
                    ),
            )
            .child(
                div()
                    .w(u(80.))
                    .h(u(28.))
                    .flex()
                    .items_center()
                    .px(u(8.))
                    .rounded(u(theme.radius.sm))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.05))
                    .font_family(theme.fonts.mono.clone())
                    .text_px(theme.text.label)
                    .tabular()
                    .debug_selector(|| "time-field".into())
                    .child(plain_input(&self.time, Some(theme.content(0.30)), cx)),
            );
        div()
            .flex()
            .flex_col()
            .text_color(theme.colors.content)
            .child(header)
            .child(grid)
            .child(time_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn round_trips_valid_local_dates_and_rejects_malformed_or_normalized_values() {
        let parsed = parse_local_date_time("2028-02-29T23:45").unwrap();
        assert_eq!(parsed, date(2028, 2, 29).and_hms_opt(23, 45, 0).unwrap());
        assert_eq!(to_local_date_time(parsed), "2028-02-29T23:45");
        for invalid in [
            "2027-02-29T12:00",
            "2028-04-31T12:00",
            "2028-01-01T24:00",
            "2028-01-01T12:60",
            "2028-01-01T",
            "2028-01-01T1:00",
            "2028-01-01T12:00Z",
            "",
        ] {
            assert_eq!(parse_local_date_time(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn shifts_months_with_clamping() {
        assert_eq!(shift_month(date(2028, 1, 31), 1), date(2028, 2, 29));
        assert_eq!(shift_month(date(2028, 2, 29), -1), date(2028, 1, 29));
        assert_eq!(shift_month(date(2028, 12, 25), 1), date(2029, 1, 25));
        assert_eq!(shift_month(date(2028, 1, 5), -1), date(2027, 12, 5));
    }

    // Ports of DateTimePicker.test.ts.

    use gpui::TestAppContext;

    use crate::test_support::{Calls, click, draw, keys, mount};

    fn today() -> NaiveDate {
        date(2028, 2, 20)
    }

    #[gpui::test]
    fn selects_a_leap_day_while_keeping_the_chosen_local_time(cx: &mut TestAppContext) {
        let changes = Calls::<String>::new();
        let record = changes.recorder();
        let (picker, cx) = mount(cx, |window, cx| {
            cx.new(|cx| {
                DateTimePicker::new("2028-02-20T18:30", window, cx)
                    .with_today(today())
                    .on_change(move |value, _, _| record(value.to_string()))
            })
        });
        click(cx, "day 2028-02-29");
        assert_eq!(changes.all(), vec!["2028-02-29T18:30".to_string()]);
        assert_eq!(
            picker.read_with(cx, |picker, _| picker.focused_date()),
            date(2028, 2, 29)
        );
    }

    #[gpui::test]
    fn navigates_the_calendar_by_keyboard_with_month_clamping_and_minimum_bounds(
        cx: &mut TestAppContext,
    ) {
        let changes = Calls::<String>::new();
        let record = changes.recorder();
        let (picker, cx) = mount(cx, |window, cx| {
            cx.new(|cx| {
                DateTimePicker::new("2028-01-31T08:15", window, cx)
                    .with_today(today())
                    .min_date(Some("2028-01-30"))
                    .on_change(move |value, _, _| record(value.to_string()))
                    .auto_focus(window, cx)
            })
        });
        let focused = |cx: &mut gpui::VisualTestContext| {
            picker.read_with(cx, |picker, _| picker.focused_date())
        };
        assert!(cx.update(|window, cx| picker.read(cx).grid_focus().is_focused(window)));
        assert_eq!(focused(cx), date(2028, 1, 31));
        keys(cx, "pagedown");
        assert_eq!(focused(cx), date(2028, 2, 29));
        keys(cx, "right");
        assert_eq!(focused(cx), date(2028, 3, 1));
        keys(cx, "up");
        assert_eq!(focused(cx), date(2028, 2, 23));
        keys(cx, "home");
        assert_eq!(focused(cx), date(2028, 2, 21));
        keys(cx, "end");
        assert_eq!(focused(cx), date(2028, 2, 27));
        keys(cx, "pageup");
        assert_eq!(focused(cx), date(2028, 1, 30));
        keys(cx, "left");
        assert_eq!(focused(cx), date(2028, 1, 30));
        assert!(changes.all().is_empty());
        keys(cx, "enter");
        assert_eq!(
            changes.all().last().map(String::as_str),
            Some("2028-01-30T08:15")
        );
        keys(cx, "right space");
        assert_eq!(
            changes.all().last().map(String::as_str),
            Some("2028-01-31T08:15")
        );
    }

    #[gpui::test]
    fn browses_across_year_boundaries_and_disables_days_before_the_minimum(
        cx: &mut TestAppContext,
    ) {
        let changes = Calls::<String>::new();
        let record = changes.recorder();
        let (picker, cx) = mount(cx, |window, cx| {
            cx.new(|cx| {
                DateTimePicker::new("2028-12-25T09:10", window, cx)
                    .with_today(today())
                    .min_date(Some("2028-12-20"))
                    .on_change(move |value, _, _| record(value.to_string()))
            })
        });
        picker.read_with(cx, |picker, _| {
            assert!(picker.is_disabled(date(2028, 12, 19)));
            assert!(!picker.is_disabled(date(2028, 12, 20)));
            assert!(picker.previous_disabled());
        });
        click(cx, "next-month");
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.month(), date(2029, 1, 1));
            assert!(!picker.is_disabled(date(2029, 1, 1)));
            assert!(!picker.previous_disabled());
        });
        assert!(changes.all().is_empty());
        click(cx, "previous-month");
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.month(), date(2028, 12, 1));
            assert!(!picker.is_disabled(date(2028, 12, 25)));
            assert!(picker.previous_disabled());
        });
        // A disabled day does not pick.
        click(cx, "day 2028-12-19");
        assert!(changes.all().is_empty());
    }

    #[gpui::test]
    fn keeps_keyboard_navigation_when_the_minimum_moves_into_another_month(
        cx: &mut TestAppContext,
    ) {
        let changes = Calls::<String>::new();
        let record = changes.recorder();
        let (picker, cx) = mount(cx, |window, cx| {
            cx.new(|cx| {
                DateTimePicker::new("2028-02-29T08:15", window, cx)
                    .with_today(today())
                    .min_date(Some("2028-02-29"))
                    .on_change(move |value, _, _| record(value.to_string()))
                    .auto_focus(window, cx)
            })
        });
        assert_eq!(
            picker.read_with(cx, |picker, _| picker.focused_date()),
            date(2028, 2, 29)
        );
        picker.update(cx, |picker, cx| picker.set_min_date(Some("2028-03-01"), cx));
        draw(cx);
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.focused_date(), date(2028, 3, 1));
            assert_eq!(picker.month(), date(2028, 3, 1));
            assert!(!picker.is_disabled(date(2028, 3, 1)));
        });
        assert!(cx.update(|window, cx| picker.read(cx).grid_focus().is_focused(window)));
        assert!(changes.all().is_empty());
    }

    #[gpui::test]
    fn typing_a_time_keeps_the_selected_day(cx: &mut TestAppContext) {
        let changes = Calls::<String>::new();
        let record = changes.recorder();
        let (_, cx) = mount(cx, |window, cx| {
            cx.new(|cx| {
                DateTimePicker::new("2028-02-20T", window, cx)
                    .with_today(today())
                    .on_change(move |value, _, _| record(value.to_string()))
            })
        });
        click(cx, "time-field");
        crate::test_support::type_text(cx, "07:45");
        assert_eq!(
            changes.all().last().map(String::as_str),
            Some("2028-02-20T07:45")
        );
    }
}
