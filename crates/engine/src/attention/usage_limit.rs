//! Port of src/features/sessions/model/usageLimit.ts. `usageLimitResumeDue`
//! and the grace period live in `monocode_core::session`; this module adds
//! the reset label, which needs the local time zone.

use chrono::{DateTime, Local, TimeZone};

pub use monocode_core::session::{USAGE_LIMIT_RESUME_GRACE_MS, usage_limit_resume_due};

use super::rate_limits::format_reset_duration;

fn local(ms: i64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(ms).single()
}

/// `formatUsageLimitReset`: "3:16 AM · in 4h 42m" today, "Sep 26, 3:16 AM ·
/// in 1d 4h" later. Uses the local time zone, as the TypeScript did.
pub fn format_usage_limit_reset(resets_at: i64, now: i64) -> String {
    let (Some(reset), Some(today)) = (local(resets_at), local(now)) else {
        return format!("in {}", format_reset_duration(resets_at - now));
    };
    let same_day = reset.date_naive() == today.date_naive();
    format_usage_limit_reset_at(&reset, same_day, resets_at - now)
}

fn format_usage_limit_reset_at<Tz: TimeZone>(
    reset: &DateTime<Tz>,
    same_day: bool,
    left_ms: i64,
) -> String {
    format!(
        "{} · in {}",
        monocode_platform::date_time::format_local(
            reset.timestamp_millis(),
            if same_day {
                monocode_platform::date_time::DateTimeStyle::Time
            } else {
                monocode_platform::date_time::DateTimeStyle::MonthDayTime
            },
        ),
        format_reset_duration(left_ms)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::session::{Session, UsageLimit};

    fn limited() -> Session {
        let mut session = Session::blank("s1", HarnessId::Codex, "gpt", "/tmp/project");
        session.usage_limit = Some(UsageLimit {
            resets_at: Some(10_000),
            resume_at_reset: Some(true),
        });
        session
    }

    #[test]
    fn waits_for_the_reset_plus_a_grace_period() {
        assert!(!usage_limit_resume_due(&limited(), 10_000));
        assert!(usage_limit_resume_due(
            &limited(),
            10_000 + USAGE_LIMIT_RESUME_GRACE_MS
        ));
    }

    #[test]
    fn only_resumes_idle_sessions_the_user_armed() {
        let later = 10_000 + USAGE_LIMIT_RESUME_GRACE_MS;
        let mut busy = limited();
        busy.busy = Some(true);
        assert!(!usage_limit_resume_due(&busy, later));
        let mut unarmed = limited();
        unarmed.usage_limit = Some(UsageLimit {
            resets_at: Some(10_000),
            resume_at_reset: None,
        });
        assert!(!usage_limit_resume_due(&unarmed, later));
        let mut unknown = limited();
        unknown.usage_limit = Some(UsageLimit {
            resets_at: None,
            resume_at_reset: Some(true),
        });
        assert!(!usage_limit_resume_due(&unknown, later));
    }

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn shows_the_time_and_what_is_left() {
        let now = at(2026, 9, 25, 22, 34);
        let today = at(2026, 9, 25, 23, 50);
        let label = format_usage_limit_reset(today, now);
        assert!(label.ends_with(" · in 1h 16m"), "{label}");
        assert_eq!(
            label,
            format!(
                "{} · in 1h 16m",
                monocode_platform::date_time::format_local(
                    today,
                    monocode_platform::date_time::DateTimeStyle::Time,
                )
            )
        );
        let tomorrow = at(2026, 9, 26, 3, 16);
        let label = format_usage_limit_reset(tomorrow, now);
        assert!(
            label.contains("26") && label.ends_with(" · in 4h 42m"),
            "{label}"
        );
        assert_eq!(
            label,
            format!(
                "{} · in 4h 42m",
                monocode_platform::date_time::format_local(
                    tomorrow,
                    monocode_platform::date_time::DateTimeStyle::MonthDayTime,
                )
            )
        );
    }
}
