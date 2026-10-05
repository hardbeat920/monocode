//! Port of the parts of src/features/providers/model/rateLimits.ts the Codex
//! adapter needs to report when a spent usage limit resets:
//! `parseCodexRateLimits` reduced to its windows, and `exhaustedWindowResetAt`.
//!
//! The full rate-limit model belongs to the app, but the harness may not
//! depend on it, so the adapter carries this small copy.

use serde_json::Value;

use monocode_core::js;

use super::json::{Record, as_record};

const SESSION_WINDOW_MINUTES: f64 = 300.0;
const WEEKLY_WINDOW_MINUTES: f64 = 10_080.0;
const MONTHLY_WINDOW_MINUTES: f64 = 43_200.0;
const WINDOW_DURATION_TOLERANCE_MINUTES: f64 = 1.0;

/// `RateLimitWindow`, without the window size the adapter never reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub resets_at: Option<i64>,
}

/// The `session`, `weekly`, and `monthly` windows of `ProviderRateLimits`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CodexRateLimits {
    pub session: Option<RateLimitWindow>,
    pub weekly: Option<RateLimitWindow>,
    pub monthly: Option<RateLimitWindow>,
}

#[derive(Debug, Clone)]
struct Snapshot<'a> {
    used_percent: f64,
    window_duration_mins: Option<f64>,
    resets_at: Option<&'a Value>,
}

/// `parseCodexRateLimits`, windows only.
pub fn parse_codex_rate_limits(result: &Value) -> CodexRateLimits {
    let rec = as_record(Some(result));
    let wrapper = as_record(rec.and_then(|rec| rec.get("rateLimits"))).or(rec);
    let primary = snapshot_from(as_record(wrapper.and_then(|w| w.get("primary"))));
    let secondary = snapshot_from(as_record(wrapper.and_then(|w| w.get("secondary"))));

    let mut session: Option<&Snapshot> = None;
    let mut weekly: Option<&Snapshot> = None;
    let mut monthly: Option<&Snapshot> = None;
    for window in [primary.as_ref(), secondary.as_ref()].into_iter().flatten() {
        match classify_window_duration(window.window_duration_mins) {
            Some(WindowKind::Session) if session.is_none() => session = Some(window),
            Some(WindowKind::Weekly) if weekly.is_none() => weekly = Some(window),
            Some(WindowKind::Monthly) if monthly.is_none() => monthly = Some(window),
            _ => {}
        }
    }
    if session.is_none()
        && let Some(primary) = primary.as_ref()
        && classify_window_duration(primary.window_duration_mins).is_none()
    {
        session = Some(primary);
    }
    if weekly.is_none()
        && let Some(secondary) = secondary.as_ref()
        && classify_window_duration(secondary.window_duration_mins).is_none()
    {
        weekly = Some(secondary);
    }
    CodexRateLimits {
        session: session.map(map_snapshot),
        weekly: weekly.map(map_snapshot),
        monthly: monthly.map(map_snapshot),
    }
}

/// `exhaustedWindowResetAt`: when a used-up window resets, the later one when
/// several are spent.
pub fn exhausted_window_reset_at(limits: &CodexRateLimits) -> Option<i64> {
    let mut latest: Option<i64> = None;
    for window in [limits.session, limits.weekly, limits.monthly]
        .into_iter()
        .flatten()
    {
        let Some(resets_at) = window.resets_at else {
            continue;
        };
        if window.used_percent < 100.0 {
            continue;
        }
        latest = Some(latest.unwrap_or(0).max(resets_at));
    }
    latest
}

enum WindowKind {
    Session,
    Weekly,
    Monthly,
}

fn classify_window_duration(duration: Option<f64>) -> Option<WindowKind> {
    let duration = duration.filter(|value| value.is_finite())?;
    if (duration - SESSION_WINDOW_MINUTES).abs() <= WINDOW_DURATION_TOLERANCE_MINUTES {
        return Some(WindowKind::Session);
    }
    if (duration - WEEKLY_WINDOW_MINUTES).abs() <= WINDOW_DURATION_TOLERANCE_MINUTES {
        return Some(WindowKind::Weekly);
    }
    // Free plans get a single 30-day window.
    if (duration - MONTHLY_WINDOW_MINUTES).abs() <= WINDOW_DURATION_TOLERANCE_MINUTES {
        return Some(WindowKind::Monthly);
    }
    None
}

fn snapshot_from(rec: Option<&Record>) -> Option<Snapshot<'_>> {
    let rec = rec?;
    let used_percent = number_field(rec, "usedPercent")
        .or_else(|| number_field(rec, "used_percent"))
        .or_else(|| number_field(rec, "used_percentage"))?;
    Some(Snapshot {
        used_percent,
        window_duration_mins: number_field(rec, "windowDurationMins")
            .or_else(|| number_field(rec, "window_duration_mins")),
        resets_at: rec
            .get("resetsAt")
            .filter(|value| !value.is_null())
            .or_else(|| rec.get("resets_at")),
    })
}

fn map_snapshot(raw: &Snapshot) -> RateLimitWindow {
    RateLimitWindow {
        used_percent: clamp_used_percent(raw.used_percent),
        resets_at: raw.resets_at.and_then(parse_reset_timestamp),
    }
}

fn clamp_used_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

/// `numberField` from rateLimits.ts: a finite number, or a numeric string.
fn number_field(rec: &Record, key: &str) -> Option<f64> {
    match rec.get(key) {
        Some(Value::Number(n)) => n.as_f64().filter(|value| value.is_finite()),
        Some(Value::String(text)) if !js::trim(text).is_empty() => {
            js::parse_number(text).filter(|value| value.is_finite())
        }
        _ => None,
    }
}

/// `parseResetTimestamp`: epoch seconds or milliseconds, or a date string.
fn parse_reset_timestamp(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => normalize_epoch_ms(n.as_f64()?),
        Value::String(text) if !js::trim(text).is_empty() => {
            if let Some(numeric) = js::parse_number(text).filter(|value| value.is_finite()) {
                return normalize_epoch_ms(numeric);
            }
            // TODO(port): `Date.parse` accepts more formats than RFC 3339.
            time::OffsetDateTime::parse(
                js::trim(text),
                &time::format_description::well_known::Rfc3339,
            )
            .ok()
            .map(|date| (date.unix_timestamp_nanos() / 1_000_000) as i64)
        }
        _ => None,
    }
}

fn normalize_epoch_ms(value: f64) -> Option<i64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    // 1e10 sits between seconds-epoch (<2286) and millisecond-epoch (>2001).
    Some(if value > 10_000_000_000.0 {
        value
    } else {
        value * 1000.0
    } as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reports_the_latest_spent_window_reset() {
        let limits = parse_codex_rate_limits(&json!({
            "primary": { "usedPercent": 100, "windowDurationMins": 300, "resetsAt": 1_900 },
            "secondary": { "usedPercent": 100, "windowDurationMins": 10_080, "resetsAt": 2_000 },
        }));
        assert_eq!(exhausted_window_reset_at(&limits), Some(2_000_000));
    }

    #[test]
    fn ignores_windows_with_room_left() {
        let limits = parse_codex_rate_limits(&json!({
            "primary": { "usedPercent": 40, "windowDurationMins": 300, "resetsAt": 1_900 },
            "secondary": null,
        }));
        assert_eq!(exhausted_window_reset_at(&limits), None);
    }

    #[test]
    fn reads_iso_reset_times() {
        let limits = parse_codex_rate_limits(&json!({
            "primary": { "usedPercent": "100", "resetsAt": "2026-01-01T00:00:00Z" },
        }));
        assert_eq!(exhausted_window_reset_at(&limits), Some(1_767_225_600_000));
    }
}
