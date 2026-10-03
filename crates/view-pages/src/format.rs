//! Display helpers the pages share: relative times and project paths.
//! Ports of `formatRelativeTime` (src/features/inbox/model/githubTasks.ts),
//! `looksLikeProject` and `isLocalProject`
//! (src/features/projects/model/recents.ts), and `prettyParent`
//! (src/shared/lib/paths.ts).

use std::time::{SystemTime, UNIX_EPOCH};

use monocode_core::js;
use monocode_core::paths::slash;
use monocode_layout::paths::{is_remote_project_path, pretty_cwd};

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `formatRelativeTime(new Date(at).toISOString(), now)` with
/// `Intl.RelativeTimeFormat("en", { numeric: "auto" })`.
// TODO(port): Intl localized the phrase. Only English is produced here.
pub fn format_relative_time(at: i64, now: i64) -> String {
    let delta = js::round((at - now) as f64 / 1000.0);
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

/// `/^[A-Za-z]:$/`.
fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `looksLikeProject`: a user project, not an app bundle or system root.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    if is_drive(normalized) || normalized == "/" {
        return false;
    }
    if pretty_cwd(path) == "~" {
        return false;
    }
    if path.contains(".app/") || path.contains(".app\\") {
        return false;
    }
    true
}

/// `isLocalProject`.
pub fn is_local_project(path: &str) -> bool {
    looks_like_project(path) && !is_remote_project_path(path)
}

/// `parentPath`.
fn parent_path(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".into(),
        Some(index) => trimmed[..index].to_string(),
        None => trimmed.to_string(),
    }
}

/// `prettyParent`: the home-relative parent folder.
pub fn pretty_parent(path: &str) -> String {
    pretty_cwd(&parent_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_relative_times_in_english() {
        let now = 1_787_832_000_000;
        assert_eq!(
            format_relative_time(now - 2 * 3_600_000, now),
            "2 hours ago"
        );
        assert_eq!(format_relative_time(now, now), "now");
        assert_eq!(format_relative_time(now - 86_400_000, now), "yesterday");
        assert_eq!(format_relative_time(now - 5 * 60_000, now), "5 minutes ago");
        assert_eq!(
            format_relative_time(now - 3 * 86_400_000, now),
            "3 days ago"
        );
    }

    #[test]
    fn recognizes_projects() {
        assert!(looks_like_project("/work/app"));
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("C:"));
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("/Applications/Foo.app/Contents"));
        assert!(!is_local_project("remote://box/work/app"));
        assert_eq!(pretty_parent("/Users/me/code/app"), "~/code");
        assert_eq!(pretty_parent("/work/app"), "/work");
    }
}
