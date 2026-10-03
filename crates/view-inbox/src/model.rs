//! Presentation helpers the inbox views call while drawing: labels, refs,
//! relative times, check counts and durations, and URL checks.
//!
//! These are ports of the display functions in
//! src/features/inbox/model/githubTasks.ts, githubPrChecks.ts,
//! inboxFilters.ts, linkedWorkItemActivity.ts,
//! src/features/sessions/model/sessionWorkItem.ts, and
//! src/features/notifications/ui/notificationMuteActions.ts. The engine's
//! inbox package ports the same functions for its own use; this crate keeps
//! its copies so it does not depend on the engine.
// TODO(port): fold these into one shared copy with
// monocode_engine::inbox once the views may depend on the engine.

use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use monocode_core::js;
use regex::Regex;

use crate::data::{
    GithubPrCheck, GithubPrCheckState, InboxItem, InboxKind, InboxProvider, InboxSource,
    LinkedWorkItem, LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus, WorkItemKind,
};

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

/// `new Date(iso).toLocaleString()`, for the Created tooltip.
pub fn local_date_time(iso: &str) -> String {
    date_parse(iso)
        .and_then(|ms| Local.timestamp_millis_opt(ms).earliest())
        .map(|date| date.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string())
        .unwrap_or_default()
}

/// `formatRelativeTime` with `Intl.RelativeTimeFormat("en", { numeric:
/// "auto" })`.
// TODO(port): Intl.RelativeTimeFormat localized the phrase. Only English is
// produced here.
pub fn format_relative_time(iso: &str, now: i64) -> String {
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

/// `inboxItemStatus`.
pub fn inbox_item_status(item: &InboxItem) -> &'static str {
    let state_type = item
        .state_type
        .as_deref()
        .map(|value| js::trim(value).to_lowercase());
    if item.kind == InboxKind::Linear {
        if matches!(state_type.as_deref(), Some("completed" | "canceled")) {
            return "Closed";
        }
        return "Open";
    }
    if item.kind == InboxKind::Jira {
        return if state_type.as_deref() == Some("done") {
            "Closed"
        } else {
            "Open"
        };
    }
    if item.draft {
        return "Draft";
    }
    if item.state == "merged" {
        return "Merged";
    }
    if item.state == "closed" {
        return "Closed";
    }
    "Open"
}

/// `inboxItemRef`: a tracker's identifier, else `#number`.
pub fn inbox_item_ref(item: &InboxItem) -> String {
    if item.is_tracker()
        && let Some(identifier) = item
            .identifier
            .as_deref()
            .map(js::trim)
            .filter(|value| !value.is_empty())
    {
        return identifier.to_string();
    }
    format!("#{}", item.number)
}

/// The kind label a card and the identity row show.
pub fn kind_label(item: &InboxItem) -> &'static str {
    if item.kind == InboxKind::Pr {
        if item.provider == InboxProvider::Gitlab {
            "Merge request"
        } else {
            "Pull request"
        }
    } else {
        "Issue"
    }
}

/// `INBOX_SOURCE_LABELS`.
pub fn inbox_source_label(source: InboxSource) -> &'static str {
    match source {
        InboxProvider::Github => "GitHub",
        InboxProvider::Linear => "Linear",
        InboxProvider::Jira => "Jira",
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
    }
}

/// `isTrackerSource`.
pub fn is_tracker_source(source: InboxSource) -> bool {
    matches!(source, InboxProvider::Linear | InboxProvider::Jira)
}

/// `githubReviewDecisionLabel`.
pub fn github_review_decision_label(decision: &str) -> &'static str {
    match js::trim(decision).to_uppercase().as_str() {
        "APPROVED" => "Approved",
        "CHANGES_REQUESTED" => "Changes requested",
        "REVIEW_REQUIRED" => "Review required",
        _ => "",
    }
}

/// `githubReviewStateLabel`.
pub fn github_review_state_label(state: &str) -> &'static str {
    match js::trim(state).to_uppercase().as_str() {
        "APPROVED" => "Approved",
        "CHANGES_REQUESTED" => "Requested changes",
        "DISMISSED" => "Dismissed",
        "COMMENTED" => "Commented",
        _ => "",
    }
}

/// `gitlabAttentionLabel`.
pub fn gitlab_attention_label(reason: &str) -> String {
    let action = js::trim(reason).to_lowercase();
    let label = match action.as_str() {
        "assigned" => "Assigned to you",
        "mentioned" | "directly_addressed" => "Mentioned you",
        "review_requested" => "Review requested",
        "review_submitted" => "Review submitted",
        "approval_required" => "Approval required",
        "build_failed" => "Pipeline failed",
        "unmergeable" => "Cannot be merged",
        "merge_train_removed" => "Removed from merge train",
        "member_access_requested" => "Access requested",
        "marked" => "Added to your to-dos",
        _ => {
            return action
                .split('_')
                .filter(|word| !word.is_empty())
                .enumerate()
                .map(|(index, word)| {
                    if index == 0 {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().chain(chars).collect(),
                            None => String::new(),
                        }
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    };
    label.to_string()
}

/// The GitLab or ADO attention label of a card, empty for other providers.
pub fn item_attention_label(item: &InboxItem) -> String {
    match item.provider {
        InboxProvider::Gitlab | InboxProvider::AzureDevops => {
            gitlab_attention_label(item.attention_reason.as_deref().unwrap_or(""))
        }
        _ => String::new(),
    }
}

/// `githubAvatarUrl`.
pub fn github_avatar_url(login: &str) -> String {
    let name = js::trim(login);
    if name.is_empty() {
        return String::new();
    }
    format!(
        "https://avatars.githubusercontent.com/{}?s=64",
        js::encode_uri_component(name)
    )
}

/// `inboxPersonAvatarUrl`.
pub fn inbox_person_avatar_url(
    provider: InboxProvider,
    login: &str,
    avatar_url: Option<&str>,
) -> String {
    let explicit = avatar_url.map(js::trim).unwrap_or("");
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    if provider == InboxProvider::Github {
        return github_avatar_url(login);
    }
    String::new()
}

/// The provider name in "Open on GitHub" and "more on GitHub".
pub fn provider_name(provider: InboxProvider) -> &'static str {
    match provider {
        InboxProvider::Linear => "Linear",
        InboxProvider::Jira => "Jira",
        InboxProvider::Gitlab => "GitLab",
        InboxProvider::AzureDevops => "ADO",
        InboxProvider::Github => "GitHub",
    }
}

/// "Open in Linear", "Open on GitHub", and the rest.
pub fn open_on_label(provider: InboxProvider) -> &'static str {
    match provider {
        InboxProvider::Linear => "Open in Linear",
        InboxProvider::Jira => "Open in Jira",
        InboxProvider::Gitlab => "Open on GitLab",
        InboxProvider::AzureDevops => "Open on ADO",
        InboxProvider::Github => "Open on GitHub",
    }
}

/// `labelColor`: `#rrggbb` from a label color, or `None` when it is not a
/// six-digit hex color.
pub fn label_color(value: &str) -> Option<u32> {
    let hex = js::trim(value);
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// `inboxShowsFullFileDiff`: only GitHub pull requests offer full files.
pub fn inbox_shows_full_file_diff(item: &InboxItem) -> bool {
    item.provider == InboxProvider::Github && item.kind == InboxKind::Pr
}

// Checks.

/// `CHECK_STATES`: the display order, worst first.
pub const CHECK_STATES: [GithubPrCheckState; 6] = [
    GithubPrCheckState::Fail,
    GithubPrCheckState::Pending,
    GithubPrCheckState::Cancel,
    GithubPrCheckState::Unknown,
    GithubPrCheckState::Pass,
    GithubPrCheckState::Skipping,
];

fn state_word(state: GithubPrCheckState) -> &'static str {
    match state {
        GithubPrCheckState::Pass => "passed",
        GithubPrCheckState::Fail => "failed",
        GithubPrCheckState::Pending => "in progress",
        GithubPrCheckState::Cancel => "cancelled",
        GithubPrCheckState::Unknown => "unknown",
        GithubPrCheckState::Skipping => "skipped",
    }
}

/// `checkStateLabel`: the capitalized state name.
pub fn check_state_label(state: GithubPrCheckState) -> String {
    capitalize(state_word(state))
}

/// `summary.charAt(0).toUpperCase() + summary.slice(1)`.
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn state_rank(state: GithubPrCheckState) -> usize {
    CHECK_STATES
        .iter()
        .position(|candidate| *candidate == state)
        .unwrap_or(CHECK_STATES.len())
}

/// `sortChecks`: group checks by outcome; each group keeps its order.
pub fn sort_checks(checks: &[GithubPrCheck]) -> Vec<GithubPrCheck> {
    let mut sorted = checks.to_vec();
    sorted.sort_by_key(|check| state_rank(check.state));
    sorted
}

/// `Record<GithubPrCheckState, number>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckCounts {
    pub pass: i64,
    pub fail: i64,
    pub pending: i64,
    pub skipping: i64,
    pub cancel: i64,
    pub unknown: i64,
}

impl CheckCounts {
    pub fn get(&self, state: GithubPrCheckState) -> i64 {
        match state {
            GithubPrCheckState::Pass => self.pass,
            GithubPrCheckState::Fail => self.fail,
            GithubPrCheckState::Pending => self.pending,
            GithubPrCheckState::Skipping => self.skipping,
            GithubPrCheckState::Cancel => self.cancel,
            GithubPrCheckState::Unknown => self.unknown,
        }
    }

    fn bump(&mut self, state: GithubPrCheckState) {
        let slot = match state {
            GithubPrCheckState::Pass => &mut self.pass,
            GithubPrCheckState::Fail => &mut self.fail,
            GithubPrCheckState::Pending => &mut self.pending,
            GithubPrCheckState::Skipping => &mut self.skipping,
            GithubPrCheckState::Cancel => &mut self.cancel,
            GithubPrCheckState::Unknown => &mut self.unknown,
        };
        *slot += 1;
    }
}

/// `countChecks`.
pub fn count_checks<'a>(states: impl IntoIterator<Item = &'a GithubPrCheckState>) -> CheckCounts {
    let mut counts = CheckCounts::default();
    for state in states {
        counts.bump(*state);
    }
    counts
}

/// `countChecks` over checks.
pub fn count_check_states(checks: &[GithubPrCheck]) -> CheckCounts {
    count_checks(checks.iter().map(|check| &check.state))
}

/// `describeCheckCounts`: one clause per non-zero state.
pub fn describe_check_counts(counts: &CheckCounts) -> Option<String> {
    let parts: Vec<String> = CHECK_STATES
        .iter()
        .filter(|state| counts.get(**state) > 0)
        .map(|state| format!("{} {}", counts.get(*state), state_word(*state)))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// `GithubPrChecksOverall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksOverall {
    Loading { description: String },
    Error { description: String },
    Fail { failed: i64, description: String },
    Pending { description: String },
    Neutral { description: String },
    Pass { description: String },
}

impl ChecksOverall {
    pub fn description(&self) -> &str {
        match self {
            Self::Loading { description }
            | Self::Error { description }
            | Self::Fail { description, .. }
            | Self::Pending { description }
            | Self::Neutral { description }
            | Self::Pass { description } => description,
        }
    }
}

/// `summarizePrChecks`.
pub fn summarize_pr_checks(
    loading: bool,
    error: Option<&str>,
    checks: Option<&[GithubPrCheck]>,
) -> ChecksOverall {
    if loading {
        return ChecksOverall::Loading {
            description: "Loading checks".into(),
        };
    }
    if error.is_some_and(|error| !error.is_empty()) {
        let saved = checks.and_then(|checks| describe_check_counts(&count_check_states(checks)));
        return ChecksOverall::Error {
            description: match saved {
                Some(saved) => format!(
                    "Checks failed to load, showing saved results that may be out of date: {saved}"
                ),
                None => "Checks failed to load".into(),
            },
        };
    }
    let counts = count_check_states(checks.unwrap_or(&[]));
    let description = describe_check_counts(&counts).unwrap_or_else(|| "No checks reported".into());
    if counts.fail > 0 {
        return ChecksOverall::Fail {
            failed: counts.fail,
            description,
        };
    }
    if counts.pending > 0 {
        return ChecksOverall::Pending { description };
    }
    if counts.cancel > 0 || counts.unknown > 0 {
        return ChecksOverall::Neutral { description };
    }
    if counts.pass > 0 {
        return ChecksOverall::Pass { description };
    }
    ChecksOverall::Neutral { description }
}

/// `checkDuration`: only when both stamps parse and the end is not before
/// the start.
pub fn check_duration(started_at: Option<&str>, completed_at: Option<&str>) -> Option<String> {
    let start = date_parse(started_at.filter(|value| !value.is_empty())?)?;
    let end = date_parse(completed_at.filter(|value| !value.is_empty())?)?;
    if end < start {
        return None;
    }
    let total_seconds = js::round((end - start) as f64 / 1000.0) as i64;
    if total_seconds < 60 {
        return Some(format!("{total_seconds}s"));
    }
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes < 60 {
        return Some(format!("{minutes}m {seconds:02}s"));
    }
    Some(format!("{}h {:02}m", minutes / 60, minutes % 60))
}

/// `isHttpUrl`: an absolute http or https URL.
pub fn is_http_url(url: Option<&str>) -> bool {
    url.filter(|url| !url.is_empty())
        .and_then(|url| url::Url::parse(url).ok())
        .is_some_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// `githubActionsJobId`: the job id in a GitHub Actions job URL for `repo`.
pub fn github_actions_job_id(url: Option<&str>, repo: &str) -> Option<String> {
    static JOB: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:actions/runs/\d+/job|runs/\d+/jobs)/([1-9]\d*)/?$").expect("valid regex")
    });
    let parsed = url::Url::parse(url?).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || parsed.port().is_some()
    {
        return None;
    }
    let prefix = format!("/{repo}/");
    let pathname = parsed.path();
    if !pathname.to_lowercase().starts_with(&prefix.to_lowercase()) {
        return None;
    }
    let path = pathname.get(prefix.len()..)?;
    JOB.captures(path)
        .and_then(|captures| captures.get(1))
        .map(|id| id.as_str().to_string())
}

// Linked work items.

/// `LinkedWorkItemTerminalState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkedWorkItemTerminalState {
    PrMerged,
    PrClosed,
    IssueClosed,
}

/// `linkedWorkItemTerminalState`.
pub fn linked_work_item_terminal_state(
    card: &LinkedWorkItemUpdateCard,
) -> Option<LinkedWorkItemTerminalState> {
    let state = js::trim(&card.state).to_lowercase();
    if card.kind == WorkItemKind::Issue {
        return (state == "closed").then_some(LinkedWorkItemTerminalState::IssueClosed);
    }
    match state.as_str() {
        "merged" => Some(LinkedWorkItemTerminalState::PrMerged),
        "closed" => Some(LinkedWorkItemTerminalState::PrClosed),
        _ => None,
    }
}

/// `linkedWorkItemUpdateSummary`.
pub fn linked_work_item_update_summary(card: &LinkedWorkItemUpdateCard) -> String {
    let parts: Vec<String> = [
        (card.counts.commits, "new commit"),
        (card.counts.reviews, "new review"),
        (card.counts.comments, "new comment"),
    ]
    .into_iter()
    .filter(|(count, _)| *count != 0)
    .map(|(count, singular)| format!("{count} {singular}{}", if count == 1 { "" } else { "s" }))
    .collect();
    if !parts.is_empty() {
        return parts.join(" · ");
    }
    match card.status {
        LinkedWorkItemUpdateStatus::Loading => "Loading change details…".into(),
        LinkedWorkItemUpdateStatus::Error => "Updated on GitHub · details unavailable".into(),
        LinkedWorkItemUpdateStatus::Ready => "Metadata or status changed".into(),
    }
}

/// `parseGithubWorkItemUrl`: the first GitHub issue or pull request URL in
/// the text.
pub fn parse_github_work_item_url(text: &str) -> Option<LinkedWorkItem> {
    static GITHUB_URL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)https?://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/(pull|issues)/(\d+)(?-u:\b)",
        )
        .expect("valid regex")
    });
    let captures = GITHUB_URL.captures(text)?;
    let number: f64 = captures[4].parse().ok()?;
    if !(1.0..=9_007_199_254_740_991.0).contains(&number) {
        return None;
    }
    let number = number as i64;
    let repo = format!("{}/{}", &captures[1], &captures[2]);
    let kind = if captures[3].eq_ignore_ascii_case("pull") {
        WorkItemKind::Pr
    } else {
        WorkItemKind::Issue
    };
    let path = if kind == WorkItemKind::Pr {
        "pull"
    } else {
        "issues"
    };
    Some(LinkedWorkItem {
        kind,
        url: format!("https://github.com/{repo}/{path}/{number}"),
        repo,
        number,
        extra: Default::default(),
    })
}

// Notification mute presets.

/// `NOTIFICATION_MUTE_HOURS`.
pub const NOTIFICATION_MUTE_HOURS: [i64; 3] = [1, 4, 8];

/// One entry of `notificationMuteActions()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuteAction {
    pub id: String,
    pub label: String,
}

/// `notificationMuteActions(now)`: the presets with their end time, then
/// "Until resumed" and "Choose date and time".
pub fn notification_mute_actions(now_ms: i64) -> Vec<MuteAction> {
    let local = |ms: i64| Local.timestamp_millis_opt(ms).earliest();
    let now = local(now_ms);
    let mut actions: Vec<MuteAction> = NOTIFICATION_MUTE_HOURS
        .iter()
        .map(|hours| {
            let label = format!("{hours} {}", if *hours == 1 { "hour" } else { "hours" });
            let until = local(now_ms + hours * 3_600_000);
            let label = match (now, until) {
                (Some(now), Some(until)) => {
                    let time = until.format("%-H:%M").to_string();
                    let day = if until.date_naive() == now.date_naive() {
                        ""
                    } else {
                        "Tomorrow, "
                    };
                    format!("{label} ({day}{time})")
                }
                _ => label,
            };
            MuteAction {
                id: format!("mute:{hours}"),
                label,
            }
        })
        .collect();
    actions.push(MuteAction {
        id: "mute:indefinite".into(),
        label: "Until resumed".into(),
    });
    actions.push(MuteAction {
        id: "mute:custom".into(),
        label: "Choose date and time".into(),
    });
    actions
}

/// `notificationMuteDeadline(id)`: `Some(Some(ms))` for a timed preset,
/// `Some(None)` for "Until resumed", `None` for anything else.
pub fn notification_mute_deadline(id: &str, now_ms: i64) -> Option<Option<i64>> {
    if id == "mute:indefinite" {
        return Some(None);
    }
    let hours: i64 = id.strip_prefix("mute:")?.parse().ok()?;
    NOTIFICATION_MUTE_HOURS
        .contains(&hours)
        .then_some(Some(now_ms + hours * 3_600_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(state: GithubPrCheckState) -> GithubPrCheck {
        GithubPrCheck {
            name: "ci".into(),
            workflow: "Build".into(),
            state,
            url: None,
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn formats_relative_times() {
        let now = date_parse("2026-08-27T12:00:00Z").unwrap();
        assert_eq!(
            format_relative_time("2026-08-27T10:00:00Z", now),
            "2 hours ago"
        );
        assert_eq!(format_relative_time("2026-08-27T12:00:00Z", now), "now");
        assert_eq!(
            format_relative_time("2026-08-26T12:00:00Z", now),
            "yesterday"
        );
        assert_eq!(format_relative_time("nope", now), "");
    }

    #[test]
    fn summarizes_checks_like_the_tab() {
        let fail = summarize_pr_checks(
            false,
            None,
            Some(&[
                check(GithubPrCheckState::Fail),
                check(GithubPrCheckState::Fail),
                check(GithubPrCheckState::Pass),
            ]),
        );
        assert_eq!(
            fail,
            ChecksOverall::Fail {
                failed: 2,
                description: "2 failed, 1 passed".into()
            }
        );
        assert_eq!(
            summarize_pr_checks(true, None, None).description(),
            "Loading checks"
        );
        assert_eq!(
            summarize_pr_checks(false, None, Some(&[])).description(),
            "No checks reported"
        );
    }

    #[test]
    fn durations_follow_check_duration() {
        assert_eq!(
            check_duration(Some("2030-01-01T10:00:00Z"), Some("2030-01-01T10:01:05Z")),
            Some("1m 05s".into())
        );
        assert_eq!(
            check_duration(Some("2030-01-01T10:00:00Z"), Some("2030-01-01T10:00:12Z")),
            Some("12s".into())
        );
        assert_eq!(check_duration(None, Some("2030-01-01T10:00:12Z")), None);
    }

    #[test]
    fn reads_actions_job_ids() {
        assert_eq!(
            github_actions_job_id(
                Some("https://github.com/acme/web/actions/runs/9/job/123"),
                "acme/web"
            ),
            Some("123".into())
        );
        assert_eq!(
            github_actions_job_id(
                Some("https://github.com/acme/web/actions/runs/9"),
                "acme/web"
            ),
            None
        );
        assert!(!is_http_url(Some("ftp://ci/run/1")));
        assert!(is_http_url(Some("https://ci.example/run/1")));
    }

    #[test]
    fn parses_github_links() {
        let item = parse_github_work_item_url("https://github.com/acme/web/pull/42").unwrap();
        assert_eq!(item.kind, WorkItemKind::Pr);
        assert_eq!(item.repo, "acme/web");
        assert_eq!(item.number, 42);
        assert!(parse_github_work_item_url("https://gitlab.com/acme/web/pull/42").is_none());
    }

    #[test]
    fn mute_actions_name_their_end_time() {
        let now = Local
            .with_ymd_and_hms(2030, 1, 15, 20, 30, 0)
            .unwrap()
            .timestamp_millis();
        let labels: Vec<String> = notification_mute_actions(now)
            .into_iter()
            .map(|action| action.label)
            .collect();
        assert_eq!(
            labels,
            [
                "1 hour (21:30)",
                "4 hours (Tomorrow, 0:30)",
                "8 hours (Tomorrow, 4:30)",
                "Until resumed",
                "Choose date and time",
            ]
        );
        assert_eq!(
            notification_mute_deadline("mute:4", now),
            Some(Some(now + 4 * 3_600_000))
        );
        assert_eq!(
            notification_mute_deadline("mute:indefinite", now),
            Some(None)
        );
        assert_eq!(notification_mute_deadline("mute:custom", now), None);
    }

    #[test]
    fn labels_need_a_six_digit_color() {
        assert_eq!(label_color("#d73a4a"), Some(0xd73a4a));
        assert_eq!(label_color("d73a4a"), Some(0xd73a4a));
        assert_eq!(label_color("red"), None);
    }
}
