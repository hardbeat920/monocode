//! The plain data the usage and account views draw.
//!
//! These types mirror the attention package in monocode-engine
//! (`rate_limits.rs`, `account_usage.rs`, `pi_usage.rs`, `harness_updates.rs`,
//! `rate_limits_fetch.rs`) and the provider account types in
//! monocode-harness. This crate may not depend on the engine, so the app
//! converts. The serde shapes are the same, so a `serde_json` round trip
//! converts any of them.
//!
//! Ports of the pure helpers the views call: src/features/providers/model/
//! rateLimits.ts (labels and states), accountUsage.ts (status and the best
//! alternative), providerAccounts.ts and providerAccountIdentity.ts (keys and
//! the organization tag), piUsage.ts (`piUsageProvider`), and the version
//! check in opencodeProtocol.ts that HarnessUpdateNotice uses.

use monocode_core::HarnessId;
use monocode_core::js;
use serde::{Deserialize, Serialize};

// Rate limits (rateLimits.ts).

/// `RateLimitProvider`: the providers with a usage feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RateLimitProvider {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "droid")]
    Droid,
    #[serde(rename = "grok")]
    Grok,
}

impl RateLimitProvider {
    pub const ALL: [RateLimitProvider; 5] = [
        RateLimitProvider::Claude,
        RateLimitProvider::Codex,
        RateLimitProvider::Opencode,
        RateLimitProvider::Droid,
        RateLimitProvider::Grok,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            RateLimitProvider::Claude => "claude",
            RateLimitProvider::Codex => "codex",
            RateLimitProvider::Opencode => "opencode",
            RateLimitProvider::Droid => "droid",
            RateLimitProvider::Grok => "grok",
        }
    }

    pub const fn harness(self) -> HarnessId {
        match self {
            RateLimitProvider::Claude => HarnessId::Claude,
            RateLimitProvider::Codex => HarnessId::Codex,
            RateLimitProvider::Opencode => HarnessId::Opencode,
            RateLimitProvider::Droid => HarnessId::Droid,
            RateLimitProvider::Grok => HarnessId::Grok,
        }
    }

    /// The usage provider behind a harness, when it has one.
    pub fn from_harness(harness: HarnessId) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.harness() == harness)
    }
}

/// `RateLimitStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RateLimitStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "fetching")]
    Fetching,
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "unavailable")]
    Unavailable,
}

/// `RateLimitWindow`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    /// Percentage of the window consumed (0 to 100).
    pub used_percent: f64,
    /// Window duration in minutes: 300 (5h) or 10080 (7d).
    pub window_minutes: i64,
    /// Unix ms timestamp when the window resets, if known.
    pub resets_at: Option<i64>,
}

/// `RateLimitResetCredit["resetType"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetCreditType {
    #[serde(rename = "codexRateLimits")]
    CodexRateLimits,
    #[serde(rename = "unknown")]
    Unknown,
}

/// `RateLimitResetCredit["status"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetCreditStatus {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "redeeming")]
    Redeeming,
    #[serde(rename = "redeemed")]
    Redeemed,
    #[serde(rename = "unknown")]
    Unknown,
}

/// `RateLimitResetCredit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredit {
    pub id: String,
    pub reset_type: ResetCreditType,
    pub status: ResetCreditStatus,
    pub granted_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// `RateLimitResetCredits`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredits {
    pub available_count: i64,
    /// Optional detail rows; the backend can report only the aggregate count.
    pub credits: Option<Vec<RateLimitResetCredit>>,
}

/// `ProviderRateLimits`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRateLimits {
    pub provider: RateLimitProvider,
    pub session: Option<RateLimitWindow>,
    pub weekly: Option<RateLimitWindow>,
    pub monthly: Option<RateLimitWindow>,
    /// Codex-only banked rate-limit reset rewards, when supplied by app-server.
    pub reset_credits: Option<RateLimitResetCredits>,
    pub updated_at: i64,
    pub error: Option<String>,
    pub status: RateLimitStatus,
}

impl ProviderRateLimits {
    /// `session || weekly || monthly || resetCredits`.
    fn has_snapshot(&self) -> bool {
        self.session.is_some()
            || self.weekly.is_some()
            || self.monthly.is_some()
            || self.reset_credits.is_some()
    }

    /// `session || weekly || monthly`.
    pub fn has_window(&self) -> bool {
        self.session.is_some() || self.weekly.is_some() || self.monthly.is_some()
    }
}

pub const SESSION_WINDOW_MINUTES: i64 = 300;
pub const WEEKLY_WINDOW_MINUTES: i64 = 10_080;
pub const MONTHLY_WINDOW_MINUTES: i64 = 43_200;
pub const RATE_LIMIT_POLL_MS: i64 = 15 * 60_000;
pub const RATE_LIMIT_MIN_REFETCH_MS: i64 = 5 * 60_000;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `idleRateLimits`.
pub fn idle_rate_limits(provider: RateLimitProvider) -> ProviderRateLimits {
    ProviderRateLimits {
        provider,
        session: None,
        weekly: None,
        monthly: None,
        reset_credits: None,
        updated_at: 0,
        error: None,
        status: RateLimitStatus::Idle,
    }
}

/// `unavailableRateLimits`.
pub fn unavailable_rate_limits(
    provider: RateLimitProvider,
    error: &str,
    now: i64,
) -> ProviderRateLimits {
    ProviderRateLimits {
        updated_at: now,
        error: Some(error.to_string()),
        status: RateLimitStatus::Unavailable,
        ..idle_rate_limits(provider)
    }
}

/// `errorRateLimits`.
pub fn error_rate_limits(
    provider: RateLimitProvider,
    error: &str,
    previous: Option<&ProviderRateLimits>,
    now: i64,
) -> ProviderRateLimits {
    if let Some(previous) = previous.filter(|previous| previous.has_snapshot()) {
        return ProviderRateLimits {
            error: Some(error.to_string()),
            status: RateLimitStatus::Error,
            updated_at: now,
            ..previous.clone()
        };
    }
    ProviderRateLimits {
        updated_at: now,
        error: Some(error.to_string()),
        status: RateLimitStatus::Error,
        ..idle_rate_limits(provider)
    }
}

/// `clampUsedPercent`.
pub fn clamp_used_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

/// `formatUsagePercent`.
pub fn format_usage_percent(used_percent: f64) -> String {
    format!("{}%", js::round(clamp_used_percent(used_percent)) as i64)
}

/// `formatWindowLabel`: compact window-size label. 10080 minutes stays "wk"
/// to match the original status-bar copy.
pub fn format_window_label(window_minutes: i64) -> String {
    if window_minutes == WEEKLY_WINDOW_MINUTES {
        return "wk".into();
    }
    if window_minutes == MONTHLY_WINDOW_MINUTES {
        return "mo".into();
    }
    if window_minutes == SESSION_WINDOW_MINUTES {
        return "5h".into();
    }
    if window_minutes == 60 {
        return "1h".into();
    }
    if window_minutes < 60 {
        return format!("{window_minutes}m");
    }
    if window_minutes % (60 * 24 * 7) == 0 {
        return format!("{}wk", window_minutes / (60 * 24 * 7));
    }
    if window_minutes % (60 * 24) == 0 {
        return format!("{}d", window_minutes / (60 * 24));
    }
    if window_minutes % 60 == 0 {
        return format!("{}h", window_minutes / 60);
    }
    format!("{window_minutes}m")
}

/// `formatResetDuration`: compact remaining duration, flooring to whole
/// units ("47m", "3h 54m", "6d 7h"). "now" once the window has reset.
pub fn format_reset_duration(ms: i64) -> String {
    if ms <= 0 {
        return "now".into();
    }
    let total_mins = ms / 60_000;
    if total_mins < 60 {
        return format!("{total_mins}m");
    }
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours >= 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        return if rem_hours > 0 {
            format!("{days}d {rem_hours}h")
        } else {
            format!("{days}d")
        };
    }
    if mins > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{hours}h")
    }
}

/// `formatResetCountdown`.
pub fn format_reset_countdown(ms: i64) -> String {
    let duration = format_reset_duration(ms);
    if duration == "now" {
        "Resets now".into()
    } else {
        format!("Resets in {duration}")
    }
}

/// `formatRateLimitWindowChipLabel`: remaining time when the reset is known,
/// the fixed window size otherwise.
pub fn format_rate_limit_window_chip_label(window: &RateLimitWindow, now: i64) -> String {
    match window.resets_at {
        Some(resets_at) => format_reset_duration(resets_at - now),
        None => format_window_label(window.window_minutes),
    }
}

/// `rateLimitWindowTooltip`: the used percent, or the remaining percent
/// when `show_remaining` is on, then the reset or the window size.
pub fn rate_limit_window_tooltip(
    window: &RateLimitWindow,
    now: i64,
    show_remaining: bool,
) -> String {
    let pct = clamp_used_percent(window.used_percent);
    let usage = if show_remaining {
        format!("{} remaining", format_usage_percent(100.0 - pct))
    } else {
        format!("{} used", format_usage_percent(pct))
    };
    match window.resets_at {
        None => format!(
            "{usage} · {} window",
            format_window_label(window.window_minutes)
        ),
        Some(resets_at) => format!("{usage} · {}", format_reset_countdown(resets_at - now)),
    }
}

/// `exhaustedWindowResetAt`: when a used-up window resets; the later one
/// when several are spent.
pub fn exhausted_window_reset_at(limits: &ProviderRateLimits) -> Option<i64> {
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

/// `CodexRateLimitResetOutcome`, from rateLimitsFetch.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexRateLimitResetOutcome {
    #[serde(rename = "reset")]
    Reset,
    #[serde(rename = "nothingToReset")]
    NothingToReset,
    #[serde(rename = "noCredit")]
    NoCredit,
    #[serde(rename = "alreadyRedeemed")]
    AlreadyRedeemed,
}

// Provider accounts (providerAccounts.ts, providerAccountIdentity.ts).

/// `DEFAULT_PROVIDER_ACCOUNT_ID`.
pub const DEFAULT_PROVIDER_ACCOUNT_ID: &str = "default";

/// `PROVIDER_ACCOUNT_PROVIDERS`: providers whose CLIs support isolated,
/// locally named account profiles.
pub const PROVIDER_ACCOUNT_PROVIDERS: [HarnessId; 2] = [HarnessId::Claude, HarnessId::Codex];

/// `supportsProviderAccounts`.
pub fn supports_provider_accounts(provider: HarnessId) -> bool {
    PROVIDER_ACCOUNT_PROVIDERS.contains(&provider)
}

/// `ProviderAccount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    pub id: String,
    pub provider: HarnessId,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

impl ProviderAccount {
    pub fn new(id: &str, provider: HarnessId, label: &str) -> Self {
        Self {
            id: id.to_string(),
            provider,
            label: label.to_string(),
            is_default: None,
        }
    }

    /// The default profile, which cannot be removed.
    pub fn is_default(&self) -> bool {
        self.is_default == Some(true)
    }
}

/// `ProviderAccountIdentity`: what a provider CLI cached after sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderAccountIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

/// `identityKey`.
pub fn identity_key(account: &ProviderAccount) -> String {
    format!("{}:{}", account.provider, account.id)
}

/// `accountUsageKey`: the same `provider:id` key the identity cache uses.
pub fn account_usage_key(account: &ProviderAccount) -> String {
    identity_key(account)
}

/// `identityOrganizationTag`: "Personal" for Claude's default
/// "<name>'s Organization".
pub fn identity_organization_tag(identity: Option<&ProviderAccountIdentity>) -> Option<String> {
    let name = js::trim(identity?.organization.as_deref()?);
    if name.is_empty() {
        return None;
    }
    // `/['’]s Organization$/`.
    let personal = ["'s Organization", "’s Organization"]
        .iter()
        .any(|suffix| name.ends_with(suffix));
    Some(if personal {
        "Personal".into()
    } else {
        name.to_string()
    })
}

// Account status (accountUsage.ts).

/// `CLOCK_MS`: how often an account list re-reads the clock.
pub const ACCOUNT_USAGE_CLOCK_MS: i64 = 30_000;
/// `LOW_HEADROOM_PERCENT`: at or below this much headroom an account reads
/// as "Running low".
pub const LOW_HEADROOM_PERCENT: f64 = 20.0;

/// `AccountStatusTone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountStatusTone {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "exhausted")]
    Exhausted,
    #[serde(rename = "checking")]
    Checking,
    #[serde(rename = "unknown")]
    Unknown,
}

/// `AccountStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountStatus {
    pub tone: AccountStatusTone,
    pub label: String,
    /// Extra context, such as "back in 31m" for an exhausted account.
    pub detail: Option<String>,
}

/// `accountHeadroom`: remaining percent of the tightest window, or `None`
/// without usage data. A window whose reset time has passed counts as fully
/// available.
pub fn account_headroom(limits: Option<&ProviderRateLimits>, now: i64) -> Option<f64> {
    let limits = limits?;
    [limits.session, limits.weekly, limits.monthly]
        .into_iter()
        .flatten()
        .map(|window| {
            if window.resets_at.is_some_and(|resets_at| resets_at <= now) {
                100.0
            } else {
                100.0 - clamp_used_percent(window.used_percent)
            }
        })
        .reduce(f64::min)
}

/// `accountStatus`: Ready, Running low, or Exhausted, shared by every view.
pub fn account_status(limits: Option<&ProviderRateLimits>, now: i64) -> AccountStatus {
    let headroom = account_headroom(limits, now);
    let (Some(limits), Some(headroom)) = (limits, headroom) else {
        let Some(limits) = limits.filter(|limits| {
            !matches!(
                limits.status,
                RateLimitStatus::Idle | RateLimitStatus::Fetching
            )
        }) else {
            return AccountStatus {
                tone: AccountStatusTone::Checking,
                label: "Checking…".into(),
                detail: None,
            };
        };
        let error = limits.error.clone().filter(|error| !error.is_empty());
        return AccountStatus {
            tone: AccountStatusTone::Unknown,
            label: if limits.status == RateLimitStatus::Unavailable {
                error.unwrap_or_else(|| "Not signed in".into())
            } else {
                error.unwrap_or_else(|| "Usage unavailable".into())
            },
            detail: None,
        };
    };
    if headroom <= 0.0 {
        return AccountStatus {
            tone: AccountStatusTone::Exhausted,
            label: "Exhausted".into(),
            detail: back_in(limits, now),
        };
    }
    if headroom <= LOW_HEADROOM_PERCENT {
        return AccountStatus {
            tone: AccountStatusTone::Low,
            label: "Running low".into(),
            detail: Some(format!("{}% left", js::round(headroom) as i64)),
        };
    }
    AccountStatus {
        tone: AccountStatusTone::Ready,
        label: "Ready".into(),
        detail: None,
    }
}

/// "back in 31m" for the used-up window that stays blocked longest.
fn back_in(limits: &ProviderRateLimits, now: i64) -> Option<String> {
    let reset_at = exhausted_window_reset_at(limits)?;
    if reset_at <= now {
        return None;
    }
    Some(format!("back in {}", format_reset_duration(reset_at - now)))
}

/// `bestAlternativeAccount`: the account with the most headroom, if it is
/// comfortably above "low".
pub fn best_alternative_account(
    accounts: &[ProviderAccount],
    usage_for: impl Fn(&ProviderAccount) -> Option<ProviderRateLimits>,
    now: i64,
) -> Option<&ProviderAccount> {
    let mut best: Option<(&ProviderAccount, f64)> = None;
    for account in accounts {
        let Some(headroom) = account_headroom(usage_for(account).as_ref(), now) else {
            continue;
        };
        if headroom <= LOW_HEADROOM_PERCENT {
            continue;
        }
        if best.is_none_or(|(_, best_headroom)| headroom > best_headroom) {
            best = Some((account, headroom));
        }
    }
    best.map(|(account, _)| account)
}

// Pi usage (piUsage.ts).

/// `PiUsageProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiUsageProvider {
    Anthropic,
    OpenaiCodex,
}

impl PiUsageProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            PiUsageProvider::Anthropic => "anthropic",
            PiUsageProvider::OpenaiCodex => "openai-codex",
        }
    }
}

/// `piUsageProvider`: only concrete Pi models from supported billing
/// providers. The model is `pi:<provider>/<model>` (`parsePiModelRef`).
pub fn pi_usage_provider(model: Option<&str>) -> Option<PiUsageProvider> {
    let rest = js::trim(model?.strip_prefix("pi:")?);
    let separator = rest.find('/')?;
    if separator == 0 || separator == rest.len() - 1 {
        return None;
    }
    match &rest[..separator] {
        "anthropic" => Some(PiUsageProvider::Anthropic),
        "openai-codex" => Some(PiUsageProvider::OpenaiCodex),
        _ => None,
    }
}

/// `piBillingProvider`.
pub fn pi_billing_provider(provider: PiUsageProvider) -> RateLimitProvider {
    match provider {
        PiUsageProvider::Anthropic => RateLimitProvider::Claude,
        PiUsageProvider::OpenaiCodex => RateLimitProvider::Codex,
    }
}

// Harness updates (harnessUpdates.ts).

/// `HarnessUpdate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessUpdate {
    pub harness: HarnessId,
    pub installed: String,
    pub latest: String,
}

/// `parseOpenCodeVersion`: the first `N.N.N` in a version line
/// (`/[0-9]+\.[0-9]+\.[0-9]+/`).
pub fn parse_version(output: &str) -> Option<String> {
    let bytes = output.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }
        let mut end = start;
        let mut ok = true;
        for part in 0..3 {
            let run = digits(end);
            if run == 0 {
                ok = false;
                break;
            }
            end += run;
            if part < 2 {
                if bytes.get(end) != Some(&b'.') {
                    ok = false;
                    break;
                }
                end += 1;
            }
        }
        if ok {
            return Some(output[start..end].to_string());
        }
        start += 1;
    }
    None
}

/// `compareSemver`: negative, zero, or positive, over the first three parts.
pub fn compare_semver(left: &str, right: &str) -> i64 {
    // `Number.parseInt(part, 10)`, with `NaN` read as 0.
    let parse = |part: &str| -> i64 {
        let text = part.trim_start_matches(js::is_space);
        let (sign, digits) = match text.as_bytes().first() {
            Some(b'-') => (-1, &text[1..]),
            Some(b'+') => (1, &text[1..]),
            _ => (1, text),
        };
        let end = digits
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(digits.len());
        digits[..end].parse::<i64>().map(|n| sign * n).unwrap_or(0)
    };
    let a: Vec<i64> = left.split('.').map(parse).collect();
    let b: Vec<i64> = right.split('.').map(parse).collect();
    for index in 0..3 {
        let delta = a.get(index).copied().unwrap_or(0) - b.get(index).copied().unwrap_or(0);
        if delta != 0 {
            return delta;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn window(used_percent: f64, resets_at: Option<i64>) -> RateLimitWindow {
        RateLimitWindow {
            used_percent,
            window_minutes: 300,
            resets_at,
        }
    }

    fn limits(session: Option<RateLimitWindow>) -> ProviderRateLimits {
        ProviderRateLimits {
            session,
            status: RateLimitStatus::Ok,
            updated_at: NOW,
            ..idle_rate_limits(RateLimitProvider::Claude)
        }
    }

    #[test]
    fn formats_windows_and_resets() {
        assert_eq!(format_usage_percent(41.6), "42%");
        assert_eq!(format_usage_percent(f64::NAN), "0%");
        assert_eq!(format_window_label(300), "5h");
        assert_eq!(format_window_label(10_080), "wk");
        assert_eq!(format_window_label(2_880), "2d");
        assert_eq!(format_reset_duration(47 * 60_000), "47m");
        assert_eq!(format_reset_duration(3 * HOUR + 54 * 60_000), "3h 54m");
        assert_eq!(format_reset_duration(12 * 24 * HOUR), "12d");
        assert_eq!(format_reset_countdown(0), "Resets now");
        assert_eq!(
            rate_limit_window_tooltip(&window(42.0, Some(NOW + 2 * HOUR)), NOW, false),
            "42% used · Resets in 2h"
        );
        assert_eq!(
            rate_limit_window_tooltip(&window(42.0, None), NOW, false),
            "42% used · 5h window"
        );
    }

    #[test]
    fn shows_remaining_percent_with_a_reset_countdown() {
        assert_eq!(
            rate_limit_window_tooltip(&window(42.4, Some(NOW + 2 * HOUR)), NOW, true),
            "58% remaining · Resets in 2h"
        );
    }

    #[test]
    fn clamps_used_percent_before_showing_remaining_usage() {
        for (used, remaining) in [(0.0, "100%"), (100.0, "0%"), (-10.0, "100%"), (110.0, "0%")] {
            let weekly = RateLimitWindow {
                used_percent: used,
                window_minutes: 10_080,
                resets_at: None,
            };
            assert_eq!(
                rate_limit_window_tooltip(&weekly, 0, true),
                format!("{remaining} remaining · wk window"),
                "{used}"
            );
        }
    }

    #[test]
    fn reads_account_status_like_the_footer() {
        assert_eq!(account_status(None, NOW).label, "Checking…");
        let ready = limits(Some(window(42.0, Some(NOW + HOUR))));
        assert_eq!(
            account_status(Some(&ready), NOW).tone,
            AccountStatusTone::Ready
        );
        let low = limits(Some(window(85.0, Some(NOW + HOUR))));
        let status = account_status(Some(&low), NOW);
        assert_eq!(
            (status.label.as_str(), status.detail.as_deref()),
            ("Running low", Some("15% left"))
        );
        let out = limits(Some(window(100.0, Some(NOW + 31 * 60_000))));
        let status = account_status(Some(&out), NOW);
        assert_eq!(
            (status.label.as_str(), status.detail.as_deref()),
            ("Exhausted", Some("back in 31m"))
        );
        let gone = unavailable_rate_limits(RateLimitProvider::Codex, "", NOW);
        assert_eq!(account_status(Some(&gone), NOW).label, "Not signed in");
    }

    #[test]
    fn picks_the_account_with_the_most_headroom() {
        let accounts = vec![
            ProviderAccount::new("a", HarnessId::Codex, "A"),
            ProviderAccount::new("b", HarnessId::Codex, "B"),
            ProviderAccount::new("c", HarnessId::Codex, "C"),
        ];
        let usage = |account: &ProviderAccount| match account.id.as_str() {
            "a" => Some(limits(Some(window(90.0, Some(NOW + HOUR))))),
            "b" => Some(limits(Some(window(30.0, Some(NOW + HOUR))))),
            "c" => Some(limits(Some(window(10.0, Some(NOW + HOUR))))),
            _ => None,
        };
        assert_eq!(
            best_alternative_account(&accounts, usage, NOW).map(|a| a.id.as_str()),
            Some("c")
        );
    }

    #[test]
    fn tags_the_default_claude_organization_as_personal() {
        let org = |name: &str| ProviderAccountIdentity {
            organization: Some(name.into()),
            ..Default::default()
        };
        assert_eq!(
            identity_organization_tag(Some(&org("Alice's Organization"))).as_deref(),
            Some("Personal")
        );
        assert_eq!(
            identity_organization_tag(Some(&org("Alice’s Organization"))).as_deref(),
            Some("Personal")
        );
        assert_eq!(
            identity_organization_tag(Some(&org(" Acme "))).as_deref(),
            Some("Acme")
        );
        assert_eq!(identity_organization_tag(Some(&org("  "))), None);
    }

    #[test]
    fn reads_pi_usage_providers() {
        assert_eq!(
            pi_usage_provider(Some("pi:anthropic/claude-sonnet-4-6")),
            Some(PiUsageProvider::Anthropic)
        );
        assert_eq!(
            pi_usage_provider(Some("pi:openai-codex/gpt-5.4")),
            Some(PiUsageProvider::OpenaiCodex)
        );
        for model in [
            "pi:default",
            "pi:openai/gpt-5.4",
            "pi:openrouter/anthropic/claude",
            "claude:opus",
        ] {
            assert_eq!(pi_usage_provider(Some(model)), None, "{model}");
        }
        assert_eq!(pi_usage_provider(None), None);
    }

    #[test]
    fn parses_and_compares_versions() {
        assert_eq!(
            parse_version("2.1.285 (Claude Code)").as_deref(),
            Some("2.1.285")
        );
        assert_eq!(
            parse_version("codex-cli 0.156.1").as_deref(),
            Some("0.156.1")
        );
        assert_eq!(parse_version("v1.2").as_deref(), None);
        assert!(compare_semver("2.1.285", "2.1.284") > 0);
        assert_eq!(compare_semver("2.1.285", "2.1.285"), 0);
        assert!(compare_semver("1.9.0", "1.10.0") < 0);
    }

    #[test]
    fn round_trips_the_attention_json_shape() {
        let value = ProviderRateLimits {
            reset_credits: Some(RateLimitResetCredits {
                available_count: 1,
                credits: None,
            }),
            ..limits(Some(window(42.0, None)))
        };
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["session"]["usedPercent"], 42.0);
        assert_eq!(json["resetCredits"]["availableCount"], 1);
        assert_eq!(json["status"], "ok");
        let back: ProviderRateLimits = serde_json::from_value(json).unwrap();
        assert_eq!(back, value);
    }
}
