//! Port of src/features/inbox/model/githubPrChecks.ts: the PR checks and job
//! details fetches, the job id in an Actions URL, and the labels, counts,
//! overall summary, and durations the checks panel shows.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use super::client::{InboxClient, Pending};
use super::time::date_parse;
use super::types::{GithubCheckDetails, GithubPrCheck, GithubPrCheckState, GithubPrChecks};

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

/// `CHECK_STATES`: the display order, worst first.
pub const CHECK_STATES: [GithubPrCheckState; 6] = [
    GithubPrCheckState::Fail,
    GithubPrCheckState::Pending,
    GithubPrCheckState::Cancel,
    GithubPrCheckState::Unknown,
    GithubPrCheckState::Pass,
    GithubPrCheckState::Skipping,
];

fn state_label(state: GithubPrCheckState) -> &'static str {
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
    let label = state_label(state);
    let mut chars = label.chars();
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

/// `sortChecks`: group checks by outcome; each group keeps its arrival order.
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

/// `describeCheckCounts`: one clause per non-zero state, so color never
/// carries the counts alone.
pub fn describe_check_counts(counts: &CheckCounts) -> Option<String> {
    let parts: Vec<String> = CHECK_STATES
        .iter()
        .filter(|state| counts.get(**state) > 0)
        .map(|state| format!("{} {}", counts.get(*state), state_label(*state)))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// `GithubPrChecksOverall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubPrChecksOverall {
    Loading { description: String },
    Error { description: String },
    Fail { failed: i64, description: String },
    Pending { description: String },
    Neutral { description: String },
    Pass { description: String },
}

impl GithubPrChecksOverall {
    /// The `kind` string.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Loading { .. } => "loading",
            Self::Error { .. } => "error",
            Self::Fail { .. } => "fail",
            Self::Pending { .. } => "pending",
            Self::Neutral { .. } => "neutral",
            Self::Pass { .. } => "pass",
        }
    }

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

/// `summarizePrChecks`: load error, then fail, pending, cancel or unknown,
/// and success only when a pass sits beside passing or skipped checks. An
/// empty or skipped-only list stays neutral, and the first load never reads
/// as "no checks".
pub fn summarize_pr_checks(
    loading: bool,
    error: Option<&str>,
    checks: Option<&[GithubPrCheck]>,
) -> GithubPrChecksOverall {
    if loading {
        return GithubPrChecksOverall::Loading {
            description: "Loading checks".into(),
        };
    }
    if error.is_some_and(|error| !error.is_empty()) {
        let saved = checks.and_then(|checks| describe_check_counts(&count_check_states(checks)));
        return GithubPrChecksOverall::Error {
            description: match saved {
                Some(saved) => {
                    format!(
                        "Checks failed to load, showing saved results that may be out of date: {saved}"
                    )
                }
                None => "Checks failed to load".into(),
            },
        };
    }
    let counts = count_check_states(checks.unwrap_or(&[]));
    let description = describe_check_counts(&counts).unwrap_or_else(|| "No checks reported".into());
    if counts.fail > 0 {
        return GithubPrChecksOverall::Fail {
            failed: counts.fail,
            description,
        };
    }
    if counts.pending > 0 {
        return GithubPrChecksOverall::Pending { description };
    }
    if counts.cancel > 0 || counts.unknown > 0 {
        return GithubPrChecksOverall::Neutral { description };
    }
    if counts.pass > 0 {
        return GithubPrChecksOverall::Pass { description };
    }
    // Empty or skipped-only: neutral, but the skipped count still gets said.
    GithubPrChecksOverall::Neutral { description }
}

/// `checkDuration`: only when both stamps parse and the end is not before
/// the start.
pub fn check_duration(started_at: Option<&str>, completed_at: Option<&str>) -> Option<String> {
    let start = date_parse(started_at.filter(|value| !value.is_empty())?)?;
    let end = date_parse(completed_at.filter(|value| !value.is_empty())?)?;
    if end < start {
        return None;
    }
    let total_seconds = monocode_core::js::round((end - start) as f64 / 1000.0) as i64;
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

impl InboxClient {
    /// `fetchGithubCheckDetails`.
    pub fn fetch_github_check_details(
        &self,
        cwd: &str,
        repo: &str,
        job_id: &str,
    ) -> Pending<GithubCheckDetails> {
        self.request(
            "git_github_check_details",
            json!({ "cwd": cwd, "repo": repo, "jobId": job_id }),
        )
    }

    /// `fetchGithubPrChecks`. The backend maps GitHub's run and conclusion
    /// to a state; this only carries it.
    pub fn fetch_github_pr_checks(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
    ) -> Pending<GithubPrChecks> {
        self.request(
            "git_github_pr_checks",
            json!({ "cwd": cwd, "repo": repo, "number": number }),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::inbox::client::test_support::{client, settle};

    fn check(name: &str, state: GithubPrCheckState) -> GithubPrCheck {
        GithubPrCheck {
            name: name.into(),
            workflow: "Build".into(),
            state,
            url: None,
            started_at: None,
            completed_at: None,
        }
    }

    use GithubPrCheckState::*;

    #[gpui::test]
    fn fetch_calls_git_github_pr_checks_with_cwd_repo_and_number(cx: &mut TestAppContext) {
        let (client, backend) = client(cx, |_, _| {
            Ok(
                json!({ "headOid": "abc123", "checks": [{ "name": "ci/build", "workflow": "Build", "state": "pass", "url": null, "startedAt": null, "completedAt": null }] }),
            )
        });
        let checks = settle(cx, client.fetch_github_pr_checks("/tmp/web", "acme/web", 7)).unwrap();
        assert_eq!(
            checks,
            GithubPrChecks {
                head_oid: "abc123".into(),
                checks: vec![check("ci/build", Pass)]
            }
        );
        assert_eq!(
            backend.calls(),
            vec![(
                "git_github_pr_checks".to_string(),
                json!({ "cwd": "/tmp/web", "repo": "acme/web", "number": 7 })
            )]
        );
    }

    #[test]
    fn sort_groups_by_outcome_and_stays_stable_inside_a_group() {
        let sorted = sort_checks(&[
            check("skip-1", Skipping),
            check("pass-1", Pass),
            check("fail-1", Fail),
            check("pass-2", Pass),
            check("pending-1", Pending),
            check("fail-2", Fail),
            check("cancel-1", Cancel),
            check("unknown-1", Unknown),
        ]);
        let names: Vec<&str> = sorted.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "fail-1",
                "fail-2",
                "pending-1",
                "cancel-1",
                "unknown-1",
                "pass-1",
                "pass-2",
                "skip-1"
            ]
        );
    }

    #[test]
    fn describe_names_every_non_zero_state() {
        let counts = count_checks(&[Pass, Pass, Fail, Pending, Cancel, Unknown, Skipping]);
        assert_eq!(
            describe_check_counts(&counts).as_deref(),
            Some("1 failed, 1 in progress, 1 cancelled, 1 unknown, 2 passed, 1 skipped")
        );
        assert_eq!(
            describe_check_counts(&count_checks(&[] as &[GithubPrCheckState])),
            None
        );
    }

    #[test]
    fn summary_keeps_initial_loading_distinct_from_an_empty_result() {
        assert_eq!(summarize_pr_checks(true, None, None).kind(), "loading");
        let empty = summarize_pr_checks(false, None, Some(&[]));
        assert_eq!(empty.kind(), "neutral");
        assert_eq!(empty.description(), "No checks reported");
    }

    #[test]
    fn summary_ranks_load_error_fail_pending_and_cancel_above_success() {
        assert_eq!(
            summarize_pr_checks(false, Some("offline"), Some(&[check("a", Fail)])).kind(),
            "error"
        );
        assert_eq!(
            summarize_pr_checks(false, None, Some(&[check("a", Fail), check("b", Pending)])).kind(),
            "fail"
        );
        assert_eq!(
            summarize_pr_checks(
                false,
                None,
                Some(&[check("a", Pending), check("b", Cancel)])
            )
            .kind(),
            "pending"
        );
        assert_eq!(
            summarize_pr_checks(
                false,
                None,
                Some(&[check("a", Cancel), check("b", Unknown)])
            )
            .kind(),
            "neutral"
        );
        assert_eq!(
            summarize_pr_checks(false, None, Some(&[check("a", Pass), check("b", Skipping)]))
                .kind(),
            "pass"
        );
    }

    #[test]
    fn summary_carries_the_fail_count() {
        let summary = summarize_pr_checks(
            false,
            None,
            Some(&[check("a", Fail), check("b", Fail), check("c", Pass)]),
        );
        assert_eq!(
            summary,
            GithubPrChecksOverall::Fail {
                failed: 2,
                description: "2 failed, 1 passed".into()
            }
        );
    }

    #[test]
    fn summary_stays_neutral_for_a_skipping_only_list_while_still_saying_the_count() {
        let summary = summarize_pr_checks(
            false,
            None,
            Some(&[check("a", Skipping), check("b", Skipping)]),
        );
        assert_eq!(summary.kind(), "neutral");
        assert_eq!(summary.description(), "2 skipped");
    }

    #[test]
    fn summary_describes_saved_results_when_a_refresh_fails_on_top_of_them() {
        let summary = summarize_pr_checks(false, Some("boom"), Some(&[check("a", Pass)]));
        assert_eq!(summary.kind(), "error");
        assert_eq!(
            summary.description(),
            "Checks failed to load, showing saved results that may be out of date: 1 passed"
        );
        assert_eq!(
            summarize_pr_checks(false, Some("boom"), None).description(),
            "Checks failed to load"
        );
    }

    #[test]
    fn duration_formats_only_valid_non_negative_spans() {
        let d = |a: Option<&str>, b: Option<&str>| check_duration(a, b);
        assert_eq!(
            d(Some("2030-01-01T10:00:00Z"), Some("2030-01-01T10:00:42Z")).as_deref(),
            Some("42s")
        );
        assert_eq!(
            d(Some("2030-01-01T10:00:00Z"), Some("2030-01-01T10:05:05Z")).as_deref(),
            Some("5m 05s")
        );
        assert_eq!(
            d(Some("2030-01-01T10:00:00Z"), Some("2030-01-01T12:03:00Z")).as_deref(),
            Some("2h 03m")
        );
        assert_eq!(d(None, Some("2030-01-01T10:00:42Z")), None);
        assert_eq!(d(Some("2030-01-01T10:00:00Z"), None), None);
        assert_eq!(d(Some("not-a-date"), Some("2030-01-01T10:00:42Z")), None);
        assert_eq!(
            d(Some("2030-01-01T10:00:42Z"), Some("2030-01-01T10:00:00Z")),
            None
        );
    }

    #[test]
    fn http_url_accepts_only_absolute_http_urls() {
        assert!(is_http_url(Some(
            "https://github.com/acme/web/actions/runs/1"
        )));
        assert!(is_http_url(Some("http://ci.local/run/1")));
        assert!(!is_http_url(Some("ftp://ci/run/1")));
        assert!(!is_http_url(Some("javascript:alert(1)")));
        assert!(!is_http_url(Some("not a url")));
        assert!(!is_http_url(Some("")));
        assert!(!is_http_url(None));
    }

    #[test]
    fn job_id_reads_actions_job_urls_for_the_repository() {
        let url = Some("https://github.com/acme/web/actions/runs/1/job/2");
        assert_eq!(github_actions_job_id(url, "acme/web").as_deref(), Some("2"));
        assert_eq!(github_actions_job_id(url, "ACME/Web").as_deref(), Some("2"));
        assert_eq!(
            github_actions_job_id(
                Some("https://github.com/acme/web/runs/5/jobs/77/"),
                "acme/web"
            )
            .as_deref(),
            Some("77")
        );
        assert_eq!(github_actions_job_id(url, "acme/other"), None);
        assert_eq!(
            github_actions_job_id(
                Some("https://example.com/acme/web/actions/runs/1/job/2"),
                "acme/web"
            ),
            None
        );
        assert_eq!(
            github_actions_job_id(
                Some("https://github.com/acme/web/actions/runs/1/job/0"),
                "acme/web"
            ),
            None
        );
        assert_eq!(github_actions_job_id(None, "acme/web"), None);
        assert_eq!(check_state_label(Pending), "In progress");
    }
}
