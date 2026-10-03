//! Small display helpers the shell regions share: the provider logo for a
//! harness, `formatRelative` and `formatGitLabel` from Sidebar.tsx, and the
//! usage percent.

use std::time::{SystemTime, UNIX_EPOCH};

use monocode_core::HarnessId;
use monocode_ui::ProviderLogo;

/// The logo `ProviderIcon` draws for a harness.
pub fn provider_logo(harness: HarnessId) -> ProviderLogo {
    match harness {
        HarnessId::Claude => ProviderLogo::Claude,
        HarnessId::Codex => ProviderLogo::Codex,
        HarnessId::Cursor => ProviderLogo::Cursor,
        HarnessId::Grok => ProviderLogo::Grok,
        HarnessId::Opencode => ProviderLogo::Opencode,
        HarnessId::Pi => ProviderLogo::Pi,
        HarnessId::Omp => ProviderLogo::Omp,
        HarnessId::Fx => ProviderLogo::Fx,
        HarnessId::Hermes => ProviderLogo::Hermes,
        HarnessId::Droid => ProviderLogo::Droid,
        HarnessId::Antigravity => ProviderLogo::Antigravity,
    }
}

/// Epoch milliseconds now.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The local calendar month (0-based) and day of an epoch time.
#[cfg(unix)]
fn local_month_day(epoch_ms: i64) -> Option<(usize, u32)> {
    let seconds = (epoch_ms / 1000) as libc::time_t;
    let mut out: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call; localtime_r is the
    // thread-safe form.
    let result = unsafe { libc::localtime_r(&seconds, &mut out) };
    (!result.is_null()).then_some((out.tm_mon as usize, out.tm_mday as u32))
}

/// The UTC calendar month and day elsewhere (days-from-civil, inverted).
#[cfg(not(unix))]
fn local_month_day(epoch_ms: i64) -> Option<(usize, u32)> {
    let days = epoch_ms.div_euclid(86_400_000) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    Some((month - 1, day))
}

/// `formatRelative` from Sidebar.tsx.
pub fn format_relative(value: i64, now: i64) -> String {
    if value <= 0 {
        return String::new();
    }
    let seconds = ((now - value) as f64 / 1000.0).round().max(0.0) as i64;
    if seconds < 60 {
        return "now".into();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        let rest = minutes % 60;
        return if rest > 0 {
            format!("{hours}h {rest}m")
        } else {
            format!("{hours}h")
        };
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    // `Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" })`
    // in the en-US form.
    local_month_day(value)
        .map(|(month, day)| format!("{} {day}", MONTHS[month.min(11)]))
        .unwrap_or_default()
}

/// `formatGitLabel` from Sidebar.tsx.
pub fn format_git_label(repo: Option<&str>, branch: Option<&str>) -> String {
    let repo = repo.filter(|repo| !repo.is_empty());
    let branch = branch.filter(|branch| !branch.is_empty());
    match (repo, branch) {
        (Some(repo), Some(branch)) => format!("{repo}/{branch}"),
        (None, Some(branch)) => branch.to_string(),
        (Some(repo), None) => repo.to_string(),
        (None, None) => String::new(),
    }
}

/// `formatUsagePercent`: whole percent.
pub fn format_percent(value: f32) -> String {
    format!("{}%", value.round() as i64)
}

/// `NO_BRANCH_LABEL` from worktrees.ts.
pub const NO_BRANCH_LABEL: &str = "No branch selected";

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;

    #[test]
    fn relative_times_match_the_sidebar() {
        let now = 1_800_000_000_000;
        assert_eq!(format_relative(0, now), "");
        assert_eq!(format_relative(now - 10_000, now), "now");
        assert_eq!(format_relative(now - 3 * MINUTE, now), "3m");
        assert_eq!(format_relative(now - 60 * MINUTE, now), "1h");
        assert_eq!(format_relative(now - (7 * 60 + 59) * MINUTE, now), "7h 59m");
        assert_eq!(format_relative(now - 3 * 24 * 60 * MINUTE, now), "3d");
        let old = format_relative(now - 30 * 24 * 60 * MINUTE, now);
        assert!(MONTHS.iter().any(|month| old.starts_with(month)), "{old}");
    }

    #[test]
    fn git_labels_join_repo_and_branch() {
        assert_eq!(format_git_label(Some("repo"), Some("main")), "repo/main");
        assert_eq!(format_git_label(None, Some("main")), "main");
        assert_eq!(format_git_label(Some("repo"), Some("")), "repo");
        assert_eq!(format_git_label(None, None), "");
    }
}
