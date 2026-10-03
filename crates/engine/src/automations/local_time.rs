//! The JavaScript `Date` local-time calls the schedule and reminder code
//! made (`getHours`, `setHours`, `setDate`, `getDay`, `getTimezoneOffset`),
//! over chrono's local time zone.
//!
//! Like `Date`, the setters take local fields that may overflow (hour 24,
//! day 32) and carry them into the next unit. A local time inside a
//! daylight saving gap resolves with the offset in effect before the gap,
//! and an ambiguous one takes the earlier instant, as ECMAScript does.

use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike,
};

/// The local calendar fields of one instant. `month` is 0-based, as in
/// JavaScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalFields {
    pub year: i32,
    pub month: i64,
    pub day: i64,
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub millis: i64,
}

impl LocalFields {
    /// `new Date(at)` read back in local time.
    pub fn of(at: i64) -> Self {
        let local = to_local(at);
        Self {
            year: local.year(),
            month: i64::from(local.month0()),
            day: i64::from(local.day()),
            hours: i64::from(local.hour()),
            minutes: i64::from(local.minute()),
            seconds: i64::from(local.second()),
            millis: i64::from(local.timestamp_subsec_millis()),
        }
    }

    /// `new Date(year, month, day, hours, minutes, seconds, millis).getTime()`.
    pub fn to_ms(self) -> i64 {
        from_local(self)
    }
}

fn to_local(at: i64) -> DateTime<Local> {
    match Local.timestamp_millis_opt(at) {
        LocalResult::Single(local) => local,
        LocalResult::Ambiguous(earlier, _) => earlier,
        LocalResult::None => Local
            .timestamp_millis_opt(0)
            .single()
            .expect("the epoch has a local time"),
    }
}

/// `MakeDate(MakeDay(y, m, d), MakeTime(h, min, s, ms))` interpreted as local
/// time.
fn from_local(fields: LocalFields) -> i64 {
    let year = i64::from(fields.year) + fields.month.div_euclid(12);
    let month = fields.month.rem_euclid(12) as u32 + 1;
    let Some(first) = i32::try_from(year)
        .ok()
        .and_then(|year| NaiveDate::from_ymd_opt(year, month, 1))
    else {
        return 0;
    };
    let midnight = first.and_hms_opt(0, 0, 0).expect("midnight exists");
    let offset_ms = (fields.day - 1) * 86_400_000
        + fields.hours * 3_600_000
        + fields.minutes * 60_000
        + fields.seconds * 1_000
        + fields.millis;
    let naive = midnight + Duration::milliseconds(offset_ms);
    local_naive_to_ms(naive)
}

fn local_naive_to_ms(naive: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(local) => local.timestamp_millis(),
        LocalResult::Ambiguous(earlier, _) => earlier.timestamp_millis(),
        LocalResult::None => {
            // Inside a spring-forward gap: use the offset from before the gap.
            let before = naive - Duration::hours(3);
            let offset = match Local.from_local_datetime(&before) {
                LocalResult::Single(local) | LocalResult::Ambiguous(local, _) => {
                    local.offset().local_minus_utc()
                }
                LocalResult::None => 0,
            };
            (naive - Duration::seconds(i64::from(offset)))
                .and_utc()
                .timestamp_millis()
        }
    }
}

/// `date.getDay()`: 0 is Sunday.
pub fn weekday(at: i64) -> i64 {
    i64::from(to_local(at).weekday().num_days_from_sunday())
}

/// `-date.getTimezoneOffset()`: minutes east of UTC.
pub fn utc_offset_minutes(at: i64) -> i64 {
    i64::from(to_local(at).offset().local_minus_utc()) / 60
}

/// `new Date(year, month, day, hours, minutes).getTime()`, `month` 0-based.
pub fn local_ms(year: i32, month: i64, day: i64, hours: i64, minutes: i64) -> i64 {
    LocalFields {
        year,
        month,
        day,
        hours,
        minutes,
        seconds: 0,
        millis: 0,
    }
    .to_ms()
}

/// `toLocaleDateString(undefined, { month: "short" })` in en-US.
pub fn short_month(month: i64) -> &'static str {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS[month.rem_euclid(12) as usize]
}

/// The en-US short weekday, 0 is Sunday.
pub fn short_weekday(day: i64) -> &'static str {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    DAYS[day.rem_euclid(7) as usize]
}

/// The en-US `hour: "numeric", minute: "2-digit"` clock: `9:05 AM`.
pub fn clock_12h(hours: i64, minutes: i64) -> String {
    let suffix = if hours < 12 { "AM" } else { "PM" };
    let hour = match hours % 12 {
        0 => 12,
        hour => hour,
    };
    format!("{hour}:{minutes:02} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_local_fields_and_carries_overflow() {
        let at = local_ms(2026, 8, 19, 10, 20);
        let fields = LocalFields::of(at);
        assert_eq!(
            (
                fields.year,
                fields.month,
                fields.day,
                fields.hours,
                fields.minutes
            ),
            (2026, 8, 19, 10, 20)
        );
        assert_eq!(local_ms(2026, 8, 31, 9, 0), local_ms(2026, 9, 1, 9, 0));
        assert_eq!(local_ms(2026, 11, 32, 9, 0), local_ms(2027, 0, 1, 9, 0));
        assert_eq!(local_ms(2026, 8, 19, 24, 0), local_ms(2026, 8, 20, 0, 0));
        assert_eq!(weekday(local_ms(2026, 8, 19, 12, 0)), 6);
        assert_eq!(clock_12h(0, 5), "12:05 AM");
        assert_eq!(clock_12h(13, 0), "1:00 PM");
    }
}
