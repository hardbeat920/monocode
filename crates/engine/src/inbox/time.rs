//! The JavaScript date helpers the inbox models lean on: `Date.parse`,
//! `Date.now`, `timeFilterStart` from src/features/sessions/model/sessionFilters.ts,
//! and `formatRelativeTime` from githubTasks.ts.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use monocode_core::js;
use monocode_locale::RelativeTimeUnit;
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
pub fn format_relative_time(iso: &str, now: i64, locale: Option<&str>) -> String {
    let Some(then) = date_parse(iso) else {
        return String::new();
    };
    format_relative_time_at(then, now, locale)
}

/// The same formatter for pages that already have epoch milliseconds.
pub fn format_relative_time_at(then: i64, now: i64, locale: Option<&str>) -> String {
    let delta = js::round((then - now) as f64 / 1000.0);
    let divisions = [
        (60.0, RelativeTimeUnit::Second),
        (60.0, RelativeTimeUnit::Minute),
        (24.0, RelativeTimeUnit::Hour),
        (7.0, RelativeTimeUnit::Day),
        (4.34524, RelativeTimeUnit::Week),
        (12.0, RelativeTimeUnit::Month),
        (f64::INFINITY, RelativeTimeUnit::Year),
    ];
    let mut value = delta;
    let mut unit = RelativeTimeUnit::Second;
    let mut amount = delta.abs();
    for (step, next) in divisions {
        unit = next;
        if amount < step {
            break;
        }
        value = js::round(value / step);
        amount = value.abs();
    }
    monocode_locale::format_relative_time(value as i32, unit, locale).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_cases(locale: &str, phrases: [&str; 7]) {
        let now = date_parse("2026-08-27T12:00:00Z").unwrap();
        for (seconds, expected) in [0, -86_400, 86_400, -172_800, 172_800, -7_200, 7_200]
            .into_iter()
            .zip(phrases)
        {
            assert_eq!(
                format_relative_time(&to_iso_string(now + seconds * 1_000), now, Some(locale)),
                expected,
                "locale {locale}, offset {seconds} seconds"
            );
        }
    }

    #[test]
    fn matches_intl_relative_french_past_and_future() {
        relative_cases(
            "fr",
            [
                "maintenant",
                "hier",
                "demain",
                "avant-hier",
                "après-demain",
                "il y a 2 heures",
                "dans 2 heures",
            ],
        );
    }

    #[test]
    fn matches_intl_relative_japanese_past_and_future() {
        relative_cases(
            "ja",
            [
                "今",
                "昨日",
                "明日",
                "一昨日",
                "明後日",
                "2 時間前",
                "2 時間後",
            ],
        );
    }

    #[test]
    fn matches_intl_relative_arabic_past_and_future() {
        relative_cases(
            "ar",
            [
                "الآن",
                "أمس",
                "غدًا",
                "أول أمس",
                "بعد الغد",
                "قبل ساعتين",
                "خلال ساعتين",
            ],
        );
    }

    #[test]
    fn matches_intl_relative_rounding_and_unit_boundaries() {
        let now = date_parse("2026-08-27T12:00:00Z").unwrap();
        for (milliseconds, expected) in [
            (59_500, "in 1 minute"),
            (-59_500, "59 seconds ago"),
            (31 * 86_400_000, "in 4 weeks"),
            (-31 * 86_400_000, "4 weeks ago"),
            (32 * 86_400_000, "next month"),
            (-32 * 86_400_000, "last month"),
        ] {
            assert_eq!(
                format_relative_time(&to_iso_string(now + milliseconds), now, Some("en")),
                expected
            );
        }
    }

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
            format_relative_time("2026-08-27T12:00:00Z", now, Some("en")),
            "now"
        );
        assert_eq!(
            format_relative_time("2026-08-26T12:00:00Z", now, Some("en")),
            "yesterday"
        );
        assert_eq!(
            format_relative_time("2026-08-27T12:00:30Z", now, Some("en")),
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
