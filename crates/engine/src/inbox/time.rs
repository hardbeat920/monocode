//! The JavaScript date helpers the inbox models lean on: `Date.parse`,
//! `Date.now`, `timeFilterStart` from src/features/sessions/model/sessionFilters.ts,
//! and `formatRelativeTime` from githubTasks.ts.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use monocode_core::js;
use serde::{Deserialize, Serialize};

/// `Date.now()`.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `Date.parse` for the timestamp shapes providers send: RFC 3339, a bare
/// date (UTC), and a date-time without an offset (local time). `None` stands
/// for `NaN`.
pub fn date_parse(value: &str) -> Option<i64> {
    let text = js::trim(value);
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.timestamp_millis());
    }
    if let Ok(parsed) = DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f%#z") {
        return Some(parsed.timestamp_millis());
    }
    if let Ok(parsed) = DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M%#z") {
        return Some(parsed.timestamp_millis());
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|local| local.timestamp_millis());
        }
    }
    None
}

/// `Date.parse(value) || 0`.
pub fn date_parse_or_zero(value: &str) -> i64 {
    date_parse(value).unwrap_or(0)
}

/// `new Date(ms).toISOString()`.
pub fn to_iso_string(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|date| date.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// `SessionTimeFilter`, shared with the session list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum InboxTimeFilter {
    #[default]
    #[serde(rename = "all")]
    All,
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

impl InboxTimeFilter {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "today" => Some(Self::Today),
            "7d" => Some(Self::SevenDays),
            "30d" => Some(Self::ThirtyDays),
            _ => None,
        }
    }
}

/// `timeFilterStart`: local midnight for "today", a rolling window otherwise.
pub fn time_filter_start(time: InboxTimeFilter, now: i64) -> i64 {
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    match time {
        InboxTimeFilter::Today => Local
            .timestamp_millis_opt(now)
            .earliest()
            .and_then(|date| date.date_naive().and_hms_opt(0, 0, 0))
            .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
            .map(|midnight| midnight.timestamp_millis())
            .unwrap_or(0),
        InboxTimeFilter::SevenDays => now - 7 * DAY_MS,
        InboxTimeFilter::ThirtyDays => now - 30 * DAY_MS,
        InboxTimeFilter::All => 0,
    }
}

/// `formatRelativeTime` with `Intl.RelativeTimeFormat(locale, { numeric: "auto" })`.
/// Only English output exists, so `locale` is ignored.
// TODO(port): Intl.RelativeTimeFormat localized the phrase. Only English is
// produced here.
pub fn format_relative_time(iso: &str, now: i64, _locale: Option<&str>) -> String {
    let Some(then) = date_parse(iso) else {
        return String::new();
    };
    let delta = js::round((then - now) as f64 / 1000.0);
    let divisions: [(f64, &str); 7] = [
        (60.0, "second"),
        (60.0, "minute"),
        (24.0, "hour"),
        (7.0, "day"),
        (4.34524, "week"),
        (12.0, "month"),
        (f64::INFINITY, "year"),
    ];
    let mut value = delta;
    let mut unit = "second";
    let mut amount = delta.abs();
    for (step, next) in divisions {
        unit = next;
        if amount < step {
            break;
        }
        value = js::round(value / step);
        amount = value.abs();
    }
    relative_phrase(value, unit)
}

/// The English `numeric: "auto"` phrase for a rounded value and unit.
fn relative_phrase(value: f64, unit: &str) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    let special = match (unit, value as i64) {
        ("second", 0) => Some("now"),
        ("minute", 0) => Some("this minute"),
        ("hour", 0) => Some("this hour"),
        ("day", 0) => Some("today"),
        ("day", -1) => Some("yesterday"),
        ("day", 1) => Some("tomorrow"),
        ("week", 0) => Some("this week"),
        ("week", -1) => Some("last week"),
        ("week", 1) => Some("next week"),
        ("month", 0) => Some("this month"),
        ("month", -1) => Some("last month"),
        ("month", 1) => Some("next month"),
        ("year", 0) => Some("this year"),
        ("year", -1) => Some("last year"),
        ("year", 1) => Some("next year"),
        _ => None,
    };
    if let Some(special) = special {
        return special.to_string();
    }
    let count = value.abs() as i64;
    let plural = if count == 1 { "" } else { "s" };
    if value < 0.0 {
        format!("{count} {unit}{plural} ago")
    } else {
        format!("in {count} {unit}{plural}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_timestamps() {
        assert_eq!(date_parse("2026-08-27T12:00:00Z"), Some(1_787_832_000_000));
        assert_eq!(
            date_parse("2026-08-27T12:00:00.000Z"),
            Some(1_787_832_000_000)
        );
        assert_eq!(
            date_parse("2026-08-27T14:00:00+02:00"),
            Some(1_787_832_000_000)
        );
        assert_eq!(date_parse("not-a-date"), None);
        assert_eq!(date_parse(""), None);
        assert_eq!(to_iso_string(1_787_832_000_000), "2026-08-27T12:00:00.000Z");
    }

    #[test]
    fn formats_hours_ago() {
        let now = date_parse("2026-08-27T12:00:00Z").unwrap();
        assert_eq!(
            format_relative_time("2026-08-27T10:00:00Z", now, Some("en")),
            "2 hours ago"
        );
        assert_eq!(
            format_relative_time("2026-08-27T12:00:00Z", now, None),
            "now"
        );
        assert_eq!(
            format_relative_time("2026-08-26T12:00:00Z", now, None),
            "yesterday"
        );
        assert_eq!(
            format_relative_time("2026-08-27T12:00:30Z", now, None),
            "in 30 seconds"
        );
    }

    #[test]
    fn returns_empty_for_an_unreadable_timestamp() {
        assert_eq!(format_relative_time("not-a-date", now_ms(), None), "");
    }

    #[test]
    fn rolling_windows_count_back_from_now() {
        let now = 10 * 24 * 60 * 60 * 1000;
        assert_eq!(
            time_filter_start(InboxTimeFilter::SevenDays, now),
            3 * 24 * 60 * 60 * 1000
        );
        assert_eq!(time_filter_start(InboxTimeFilter::All, now), 0);
        let today = time_filter_start(InboxTimeFilter::Today, now);
        assert!(today <= now && now - today < 24 * 60 * 60 * 1000);
    }
}
