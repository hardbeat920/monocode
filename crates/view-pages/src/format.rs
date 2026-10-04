//! Display helpers the pages share: relative times and project paths.
//! Ports of `formatRelativeTime` (src/features/inbox/model/githubTasks.ts),
//! `looksLikeProject` and `isLocalProject`
//! (src/features/projects/model/recents.ts), and `prettyParent`
//! (src/shared/lib/paths.ts).

use std::time::{SystemTime, UNIX_EPOCH};

use monocode_core::paths::slash;
use monocode_layout::paths::{is_remote_project_path, pretty_cwd};

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `formatRelativeTime(new Date(at).toISOString(), now)` with the OS locale.
pub fn format_relative_time(at: i64, now: i64) -> String {
    monocode_engine::inbox::time::format_relative_time_at(at, now, None)
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
    fn matches_intl_secondary_page_default_french_japanese_arabic() {
        let now = 1_787_832_000_000;
        for (locale, past, future) in [
            ("fr", "avant-hier", "dans 2 heures"),
            ("ja", "一昨日", "2 時間後"),
            ("ar", "أول أمس", "خلال ساعتين"),
        ] {
            monocode_locale::with_locale(locale, || {
                assert_eq!(format_relative_time(now - 2 * 86_400_000, now), past);
                assert_eq!(format_relative_time(now + 2 * 3_600_000, now), future);
            })
            .unwrap();
        }
    }

    #[test]
    fn matches_intl_secondary_page_rounding_and_unit_boundaries() {
        let now = 1_787_832_000_000;
        monocode_locale::with_locale("en", || {
            for (milliseconds, expected) in [
                (59_500, "in 1 minute"),
                (-59_500, "59 seconds ago"),
                (31 * 86_400_000, "in 4 weeks"),
                (-31 * 86_400_000, "4 weeks ago"),
                (32 * 86_400_000, "next month"),
                (-32 * 86_400_000, "last month"),
            ] {
                assert_eq!(format_relative_time(now + milliseconds, now), expected);
            }
        })
        .unwrap();
    }

    #[test]
    fn formats_relative_times_in_english() {
        monocode_locale::with_locale("en", || {
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
        })
        .unwrap();
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
