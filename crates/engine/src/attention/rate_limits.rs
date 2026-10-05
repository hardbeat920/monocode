//! Port of src/features/providers/model/rateLimits.ts: provider usage
//! windows, their parsers, and the labels the usage chips show.
//!
//! `Date.now()` becomes a `now` argument on the constructors that stamp
//! `updated_at`, so tests stay deterministic.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use monocode_core::HarnessId;
use monocode_core::js;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

const WINDOW_DURATION_TOLERANCE_MINUTES: f64 = 1.0;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
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

/// `fetchingRateLimits`.
pub fn fetching_rate_limits(
    provider: RateLimitProvider,
    previous: Option<&ProviderRateLimits>,
) -> ProviderRateLimits {
    if let Some(previous) = previous.filter(|previous| previous.has_snapshot()) {
        return ProviderRateLimits {
            status: RateLimitStatus::Fetching,
            ..previous.clone()
        };
    }
    ProviderRateLimits {
        provider,
        session: previous.and_then(|previous| previous.session),
        weekly: previous.and_then(|previous| previous.weekly),
        monthly: previous.and_then(|previous| previous.monthly),
        reset_credits: previous.and_then(|previous| previous.reset_credits.clone()),
        updated_at: previous.map(|previous| previous.updated_at).unwrap_or(0),
        error: None,
        status: RateLimitStatus::Fetching,
    }
}

/// `unavailableRateLimits`.
pub fn unavailable_rate_limits(
    provider: RateLimitProvider,
    error: &str,
    now: i64,
) -> ProviderRateLimits {
    ProviderRateLimits {
        provider,
        session: None,
        weekly: None,
        monthly: None,
        reset_credits: None,
        updated_at: now,
        error: Some(error.to_string()),
        status: RateLimitStatus::Unavailable,
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
        provider,
        session: None,
        weekly: None,
        monthly: None,
        reset_credits: None,
        updated_at: now,
        error: Some(error.to_string()),
        status: RateLimitStatus::Error,
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

/// `rateLimitWindowTooltip`.
pub fn rate_limit_window_tooltip(window: &RateLimitWindow, now: i64) -> String {
    let used = format!("{} used", format_usage_percent(window.used_percent));
    match window.resets_at {
        None => format!(
            "{used} · {} window",
            format_window_label(window.window_minutes)
        ),
        Some(resets_at) => format!("{used} · {}", format_reset_countdown(resets_at - now)),
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

/// `Date.parse` for the timestamp shapes providers send: RFC 3339, a bare
/// date (UTC), and a date-time without an offset (local time).
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

/// `parseResetTimestamp`.
pub fn parse_reset_timestamp(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => normalize_epoch_ms(number.as_f64()?),
        Value::String(text) => {
            if js::trim(text).is_empty() {
                return None;
            }
            if let Some(numeric) = js::parse_number(text).filter(|numeric| numeric.is_finite()) {
                return normalize_epoch_ms(numeric);
            }
            date_parse(text)
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
        value as i64
    } else {
        (value * 1000.0) as i64
    })
}

/// `asRecord` from codexProtocol.ts.
fn as_record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value?.as_object()
}

/// `numberField`.
fn number_field(rec: &Map<String, Value>, key: &str) -> Option<f64> {
    match rec.get(key)? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) if !js::trim(text).is_empty() => {
            js::parse_number(text).filter(|value| value.is_finite())
        }
        _ => None,
    }
}

/// `stringField`.
fn string_field(rec: &Map<String, Value>, key: &str) -> Option<String> {
    match rec.get(key)? {
        Value::String(text) if !js::trim(text).is_empty() => Some(js::trim(text).to_string()),
        _ => None,
    }
}

/// `mapUsageWindow`.
pub fn map_usage_window(raw: Option<&Value>, window_minutes: i64) -> Option<RateLimitWindow> {
    let rec = as_record(raw)?;
    let used_percent = used_percent_from(rec)?;
    Some(RateLimitWindow {
        used_percent: clamp_used_percent(used_percent),
        window_minutes,
        resets_at: parse_reset_timestamp(rec.get("resets_at"))
            .or_else(|| parse_reset_timestamp(rec.get("resetsAt"))),
    })
}

fn used_percent_from(rec: &Map<String, Value>) -> Option<f64> {
    number_field(rec, "used_percentage")
        .or_else(|| number_field(rec, "usedPercent"))
        .or_else(|| number_field(rec, "utilization"))
}

/// `parseClaudeOAuthUsage`.
pub fn parse_claude_oauth_usage(body: &str, now: i64) -> ProviderRateLimits {
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return error_rate_limits(
            RateLimitProvider::Claude,
            "Claude usage response was not JSON",
            None,
            now,
        );
    };
    let Some(rec) = parsed.as_object() else {
        return error_rate_limits(
            RateLimitProvider::Claude,
            "Claude usage response was empty",
            None,
            now,
        );
    };
    ProviderRateLimits {
        provider: RateLimitProvider::Claude,
        session: map_usage_window(rec.get("five_hour"), SESSION_WINDOW_MINUTES),
        weekly: map_usage_window(rec.get("seven_day"), WEEKLY_WINDOW_MINUTES),
        monthly: None,
        reset_credits: None,
        updated_at: now,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

#[derive(Debug, Clone)]
struct CodexWindowSnapshot {
    used_percent: f64,
    window_duration_mins: Option<f64>,
    resets_at: Option<Value>,
}

/// `parseCodexRateLimits`.
pub fn parse_codex_rate_limits(result: &Value, now: i64) -> ProviderRateLimits {
    let rec = result.as_object();
    let wrapper = rec.and_then(|rec| as_record(rec.get("rateLimits"))).or(rec);
    let primary = snapshot_from(wrapper.and_then(|wrapper| as_record(wrapper.get("primary"))));
    let secondary = snapshot_from(wrapper.and_then(|wrapper| as_record(wrapper.get("secondary"))));
    let classified = classify_codex_windows(primary, secondary);
    let credits = rec.and_then(|rec| {
        rec.get("rateLimitResetCredits")
            .filter(|value| !value.is_null())
            .or_else(|| rec.get("rate_limit_reset_credits"))
    });
    ProviderRateLimits {
        provider: RateLimitProvider::Codex,
        session: map_codex_snapshot(classified.0, SESSION_WINDOW_MINUTES),
        weekly: map_codex_snapshot(classified.1, WEEKLY_WINDOW_MINUTES),
        monthly: map_codex_snapshot(classified.2, MONTHLY_WINDOW_MINUTES),
        reset_credits: parse_reset_credits(credits),
        updated_at: now,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

/// `parseOpencodeGoUsage`: the official OpenCode Go usage payload,
/// `{ usage: { rolling, weekly, monthly } }`, each `{ status, percent,
/// resetsAt }`. `percent` is percent used, matching the dashboard.
pub fn parse_opencode_go_usage(result: &Value, now: i64) -> ProviderRateLimits {
    let rec = result.as_object();
    let usage = rec.and_then(|rec| as_record(rec.get("usage"))).or(rec);
    let field = |key: &str| usage.and_then(|usage| usage.get(key));
    ProviderRateLimits {
        provider: RateLimitProvider::Opencode,
        session: map_opencode_go_window(field("rolling"), SESSION_WINDOW_MINUTES),
        weekly: map_opencode_go_window(field("weekly"), WEEKLY_WINDOW_MINUTES),
        monthly: map_opencode_go_window(field("monthly"), MONTHLY_WINDOW_MINUTES),
        reset_credits: None,
        updated_at: now,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

/// `parseDroidUsage`: Factory's `/api/billing/limits` payload. Only the
/// standard pool is shown; it is the one Droid gates models on.
pub fn parse_droid_usage(result: &Value, now: i64) -> ProviderRateLimits {
    let standard = as_record(Some(result))
        .and_then(|rec| as_record(rec.get("limits")))
        .and_then(|limits| as_record(limits.get("standard")));
    let field = |key: &str| standard.and_then(|standard| standard.get(key));
    ProviderRateLimits {
        provider: RateLimitProvider::Droid,
        session: map_droid_window(field("fiveHour"), SESSION_WINDOW_MINUTES),
        weekly: map_droid_window(field("weekly"), WEEKLY_WINDOW_MINUTES),
        monthly: map_droid_window(field("monthly"), MONTHLY_WINDOW_MINUTES),
        reset_credits: None,
        updated_at: now,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

fn map_droid_window(raw: Option<&Value>, window_minutes: i64) -> Option<RateLimitWindow> {
    let rec = as_record(raw)?;
    let used_percent = number_field(rec, "usedPercent")?;
    Some(RateLimitWindow {
        used_percent: clamp_used_percent(used_percent),
        window_minutes,
        resets_at: parse_reset_timestamp(rec.get("windowEnd")),
    })
}

/// `parseGrokBilling`: Grok Build's `_x.ai/billing` ACP result. Grok reports
/// one credit allowance per billing period, weekly or monthly.
pub fn parse_grok_billing(result: &Value, now: i64) -> ProviderRateLimits {
    let config = as_record(Some(result)).and_then(|rec| as_record(rec.get("config")));
    let used_percent = config.and_then(|config| number_field(config, "creditUsagePercent"));
    let period = config.and_then(|config| as_record(config.get("currentPeriod")));
    let resets_at =
        parse_reset_timestamp(period.and_then(|period| period.get("end"))).or_else(|| {
            parse_reset_timestamp(config.and_then(|config| config.get("billingPeriodEnd")))
        });
    let monthly = grok_period_is_monthly(period, config);
    let window = used_percent.map(|used_percent| RateLimitWindow {
        used_percent: clamp_used_percent(used_percent),
        window_minutes: if monthly {
            MONTHLY_WINDOW_MINUTES
        } else {
            WEEKLY_WINDOW_MINUTES
        },
        resets_at,
    });
    ProviderRateLimits {
        provider: RateLimitProvider::Grok,
        session: None,
        weekly: if monthly { None } else { window },
        monthly: if monthly { window } else { None },
        reset_credits: None,
        updated_at: now,
        error: None,
        status: RateLimitStatus::Ok,
    }
}

fn grok_period_is_monthly(
    period: Option<&Map<String, Value>>,
    config: Option<&Map<String, Value>>,
) -> bool {
    let kind = period
        .and_then(|period| period.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let upper = kind.to_ascii_uppercase();
    if upper.contains("MONTHLY") {
        return true;
    }
    if upper.contains("WEEKLY") {
        return false;
    }
    let start =
        parse_reset_timestamp(period.and_then(|period| period.get("start"))).or_else(|| {
            parse_reset_timestamp(config.and_then(|config| config.get("billingPeriodStart")))
        });
    let end = parse_reset_timestamp(period.and_then(|period| period.get("end"))).or_else(|| {
        parse_reset_timestamp(config.and_then(|config| config.get("billingPeriodEnd")))
    });
    let (Some(start), Some(end)) = (start, end) else {
        return false;
    };
    // Anything longer than two weeks reads as a monthly allowance.
    end - start > 2 * WEEKLY_WINDOW_MINUTES * 60_000
}

fn map_opencode_go_window(raw: Option<&Value>, window_minutes: i64) -> Option<RateLimitWindow> {
    let rec = as_record(raw)?;
    // Require an explicit valid status; unknown shapes are dropped so the
    // caller can treat a fully empty payload as an error, not a snapshot.
    let status = rec.get("status").and_then(Value::as_str);
    if status != Some("ok") && status != Some("rate-limited") {
        return None;
    }
    let used_percent = number_field(rec, "percent").or_else(|| number_field(rec, "usedPercent"))?;
    Some(RateLimitWindow {
        used_percent: clamp_used_percent(used_percent),
        window_minutes,
        resets_at: parse_reset_timestamp(rec.get("resetsAt"))
            .or_else(|| parse_reset_timestamp(rec.get("resets_at"))),
    })
}

fn parse_reset_credits(raw: Option<&Value>) -> Option<RateLimitResetCredits> {
    let rec = as_record(raw)?;
    let count =
        number_field(rec, "availableCount").or_else(|| number_field(rec, "available_count"))?;
    let credits = match rec.get("credits") {
        Some(Value::Array(items)) => Some(items.iter().filter_map(parse_reset_credit).collect()),
        _ => None,
    };
    Some(RateLimitResetCredits {
        available_count: count.floor().max(0.0) as i64,
        credits,
    })
}

/// `rec.a ?? rec.b`: the first key whose value is present and not null.
fn either<'a>(rec: &'a Map<String, Value>, first: &str, second: &str) -> Option<&'a Value> {
    rec.get(first)
        .filter(|value| !value.is_null())
        .or_else(|| rec.get(second))
}

fn parse_reset_credit(raw: &Value) -> Option<RateLimitResetCredit> {
    let rec = raw.as_object()?;
    let id = match rec.get("id") {
        Some(Value::String(id)) if !js::trim(id).is_empty() => id.clone(),
        _ => return None,
    };
    let reset_type = match rec.get("resetType").and_then(Value::as_str) {
        Some("codexRateLimits") => ResetCreditType::CodexRateLimits,
        _ => ResetCreditType::Unknown,
    };
    let status = match rec.get("status").and_then(Value::as_str) {
        Some("available") => ResetCreditStatus::Available,
        Some("redeeming") => ResetCreditStatus::Redeeming,
        Some("redeemed") => ResetCreditStatus::Redeemed,
        _ => ResetCreditStatus::Unknown,
    };
    Some(RateLimitResetCredit {
        id,
        reset_type,
        status,
        granted_at: parse_reset_timestamp(either(rec, "grantedAt", "granted_at")),
        expires_at: parse_reset_timestamp(either(rec, "expiresAt", "expires_at")),
        title: string_field(rec, "title"),
        description: string_field(rec, "description"),
    })
}

fn snapshot_from(rec: Option<&Map<String, Value>>) -> Option<CodexWindowSnapshot> {
    let rec = rec?;
    let used_percent = number_field(rec, "usedPercent")
        .or_else(|| number_field(rec, "used_percent"))
        .or_else(|| number_field(rec, "used_percentage"))?;
    Some(CodexWindowSnapshot {
        used_percent,
        window_duration_mins: number_field(rec, "windowDurationMins")
            .or_else(|| number_field(rec, "window_duration_mins")),
        resets_at: either(rec, "resetsAt", "resets_at").cloned(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    Session,
    Weekly,
    Monthly,
}

type Classified = (
    Option<CodexWindowSnapshot>,
    Option<CodexWindowSnapshot>,
    Option<CodexWindowSnapshot>,
);

fn classify_codex_windows(
    primary: Option<CodexWindowSnapshot>,
    secondary: Option<CodexWindowSnapshot>,
) -> Classified {
    let mut session = None;
    let mut weekly = None;
    let mut monthly = None;
    for window in [&primary, &secondary].into_iter().flatten() {
        match classify_window_duration(window.window_duration_mins) {
            Some(WindowKind::Session) if session.is_none() => session = Some(window.clone()),
            Some(WindowKind::Weekly) if weekly.is_none() => weekly = Some(window.clone()),
            Some(WindowKind::Monthly) if monthly.is_none() => monthly = Some(window.clone()),
            _ => {}
        }
    }
    if session.is_none()
        && let Some(primary) = primary.as_ref()
        && classify_window_duration(primary.window_duration_mins).is_none()
    {
        session = Some(primary.clone());
    }
    if weekly.is_none()
        && let Some(secondary) = secondary.as_ref()
        && classify_window_duration(secondary.window_duration_mins).is_none()
    {
        weekly = Some(secondary.clone());
    }
    (session, weekly, monthly)
}

fn classify_window_duration(duration: Option<f64>) -> Option<WindowKind> {
    let duration = duration.filter(|duration| duration.is_finite())?;
    let near =
        |minutes: i64| (duration - minutes as f64).abs() <= WINDOW_DURATION_TOLERANCE_MINUTES;
    if near(SESSION_WINDOW_MINUTES) {
        return Some(WindowKind::Session);
    }
    if near(WEEKLY_WINDOW_MINUTES) {
        return Some(WindowKind::Weekly);
    }
    // Free plans get a single 30-day window.
    if near(MONTHLY_WINDOW_MINUTES) {
        return Some(WindowKind::Monthly);
    }
    None
}

fn map_codex_snapshot(
    raw: Option<CodexWindowSnapshot>,
    window_minutes: i64,
) -> Option<RateLimitWindow> {
    let raw = raw?;
    Some(RateLimitWindow {
        used_percent: clamp_used_percent(raw.used_percent),
        window_minutes,
        resets_at: parse_reset_timestamp(raw.resets_at.as_ref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_790_000_000_000;

    fn window(used_percent: f64, window_minutes: i64, resets_at: Option<i64>) -> RateLimitWindow {
        RateLimitWindow {
            used_percent,
            window_minutes,
            resets_at,
        }
    }

    fn iso(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn format_window_label_uses_the_compact_labels() {
        assert_eq!(format_window_label(300), "5h");
        assert_eq!(format_window_label(10_080), "wk");
        assert_eq!(format_window_label(60), "1h");
        assert_eq!(format_window_label(45), "45m");
        assert_eq!(format_window_label(1_440), "1d");
    }

    #[test]
    fn format_reset_duration_floors_to_whole_units() {
        assert_eq!(format_reset_duration(47 * 60_000), "47m");
        assert_eq!(format_reset_duration(3 * 3_600_000 + 54 * 60_000), "3h 54m");
        assert_eq!(format_reset_duration(3 * 3_600_000), "3h");
        assert_eq!(
            format_reset_duration(6 * 86_400_000 + 7 * 3_600_000),
            "6d 7h"
        );
        assert_eq!(format_reset_duration(2 * 86_400_000), "2d");
    }

    #[test]
    fn format_reset_duration_reports_an_expired_window_as_now() {
        assert_eq!(format_reset_duration(0), "now");
        assert_eq!(format_reset_duration(-1_000), "now");
    }

    #[test]
    fn format_reset_countdown_prefixes_remaining_time() {
        assert_eq!(
            format_reset_countdown(2 * 3_600_000 + 33 * 60_000),
            "Resets in 2h 33m"
        );
        assert_eq!(format_reset_countdown(0), "Resets now");
    }

    #[test]
    fn chip_label_prefers_remaining_time_when_resets_at_is_known() {
        let now = iso("2026-08-27T08:00:00Z");
        assert_eq!(
            format_rate_limit_window_chip_label(
                &window(42.0, 300, Some(now + 2 * 3_600_000 + 33 * 60_000)),
                now
            ),
            "2h 33m"
        );
    }

    #[test]
    fn chip_label_falls_back_to_the_window_size_without_a_reset() {
        let now = iso("2026-08-27T08:00:00Z");
        assert_eq!(
            format_rate_limit_window_chip_label(&window(42.0, 300, None), now),
            "5h"
        );
        assert_eq!(
            format_rate_limit_window_chip_label(&window(41.0, 10_080, None), now),
            "wk"
        );
    }

    #[test]
    fn format_usage_percent_rounds_to_a_whole_percent() {
        assert_eq!(format_usage_percent(58.4), "58%");
        assert_eq!(format_usage_percent(58.6), "59%");
        assert_eq!(clamp_used_percent(140.0), 100.0);
    }

    #[test]
    fn exhausted_window_reset_at_returns_the_latest_reset_among_spent_windows() {
        let mut limits = idle_rate_limits(RateLimitProvider::Codex);
        limits.session = Some(window(100.0, 300, Some(2_000)));
        limits.weekly = Some(window(100.0, 10_080, Some(9_000)));
        assert_eq!(exhausted_window_reset_at(&limits), Some(9_000));
    }

    #[test]
    fn exhausted_window_reset_at_ignores_windows_with_room_left() {
        let mut limits = idle_rate_limits(RateLimitProvider::Claude);
        limits.session = Some(window(100.0, 300, Some(2_000)));
        limits.weekly = Some(window(40.0, 10_080, Some(9_000)));
        assert_eq!(exhausted_window_reset_at(&limits), Some(2_000));
        assert_eq!(
            exhausted_window_reset_at(&idle_rate_limits(RateLimitProvider::Claude)),
            None
        );
    }

    #[test]
    fn parse_reset_timestamp_treats_small_numbers_as_unix_seconds() {
        assert_eq!(
            parse_reset_timestamp(Some(&json!(1_738_425_600))),
            Some(1_738_425_600_000)
        );
    }

    #[test]
    fn parse_reset_timestamp_keeps_millisecond_epochs() {
        assert_eq!(
            parse_reset_timestamp(Some(&json!(1_738_425_600_000_i64))),
            Some(1_738_425_600_000)
        );
    }

    #[test]
    fn parse_reset_timestamp_parses_iso_strings() {
        assert_eq!(
            parse_reset_timestamp(Some(&json!("2026-08-27T12:00:00.000Z"))),
            Some(iso("2026-08-27T12:00:00.000Z"))
        );
    }

    #[test]
    fn parse_claude_oauth_usage_maps_five_hour_and_seven_day_windows() {
        let body = json!({
            "five_hour": { "used_percentage": 58.2, "resets_at": 1_738_425_600 },
            "seven_day": { "utilization": 41, "resets_at": "2026-09-01T00:00:00.000Z" },
        })
        .to_string();
        let limits = parse_claude_oauth_usage(&body, NOW);
        assert_eq!(limits.status, RateLimitStatus::Ok);
        assert_eq!(
            limits.session,
            Some(window(58.2, 300, Some(1_738_425_600_000)))
        );
        let weekly = limits.weekly.unwrap();
        assert_eq!(weekly.used_percent, 41.0);
        assert_eq!(weekly.window_minutes, 10_080);
        assert_eq!(weekly.resets_at, Some(iso("2026-09-01T00:00:00.000Z")));
    }

    #[test]
    fn parse_claude_oauth_usage_returns_an_error_for_garbage() {
        let limits = parse_claude_oauth_usage("not json", NOW);
        assert_eq!(limits.status, RateLimitStatus::Error);
        assert!(limits.session.is_none());
    }

    #[test]
    fn map_usage_window_accepts_camel_case_codex_shaped_windows() {
        assert_eq!(
            map_usage_window(
                Some(&json!({ "usedPercent": 12, "resetsAt": 1_738_425_600 })),
                300
            ),
            Some(window(12.0, 300, Some(1_738_425_600_000)))
        );
    }

    #[test]
    fn parse_codex_rate_limits_classifies_primary_and_secondary_by_duration() {
        let limits = parse_codex_rate_limits(
            &json!({
                "rateLimits": {
                    "primary": { "usedPercent": 52, "windowDurationMins": 300, "resetsAt": 1_738_425_600 },
                    "secondary": { "used_percent": 37, "window_duration_mins": 10_080, "resets_at": 1_738_900_000 },
                },
            }),
            NOW,
        );
        assert_eq!(limits.session.unwrap().used_percent, 52.0);
        assert_eq!(limits.session.unwrap().window_minutes, 300);
        assert_eq!(limits.weekly.unwrap().used_percent, 37.0);
        assert_eq!(limits.weekly.unwrap().window_minutes, 10_080);
    }

    #[test]
    fn parse_codex_rate_limits_maps_banked_reset_credits_and_their_expiry() {
        let limits = parse_codex_rate_limits(
            &json!({
                "rateLimits": { "primary": { "usedPercent": 52, "windowDurationMins": 300 } },
                "rateLimitResetCredits": {
                    "availableCount": 2,
                    "credits": [{
                        "id": "reset-1",
                        "resetType": "codexRateLimits",
                        "status": "available",
                        "grantedAt": 1_788_768_000,
                        "expiresAt": 1_791_360_000,
                        "title": "Referral reward",
                        "description": "One Codex rate-limit reset",
                    }],
                },
            }),
            NOW,
        );
        assert_eq!(
            limits.reset_credits,
            Some(RateLimitResetCredits {
                available_count: 2,
                credits: Some(vec![RateLimitResetCredit {
                    id: "reset-1".into(),
                    reset_type: ResetCreditType::CodexRateLimits,
                    status: ResetCreditStatus::Available,
                    granted_at: Some(1_788_768_000_000),
                    expires_at: Some(1_791_360_000_000),
                    title: Some("Referral reward".into()),
                    description: Some("One Codex rate-limit reset".into()),
                }]),
            })
        );
    }

    #[test]
    fn parse_codex_rate_limits_keeps_an_aggregate_count_without_detail_rows() {
        let limits = parse_codex_rate_limits(
            &json!({
                "rateLimits": { "primary": { "usedPercent": 12, "windowDurationMins": 300 } },
                "rate_limit_reset_credits": { "available_count": "3", "credits": null },
            }),
            NOW,
        );
        assert_eq!(
            limits.reset_credits,
            Some(RateLimitResetCredits {
                available_count: 3,
                credits: None,
            })
        );
    }

    #[test]
    fn parse_codex_rate_limits_maps_a_free_plans_lone_30_day_window_to_monthly() {
        let limits = parse_codex_rate_limits(
            &json!({
                "rateLimits": {
                    "primary": { "usedPercent": 4, "windowDurationMins": 43_200, "resetsAt": 1_792_550_273 },
                    "secondary": null,
                },
            }),
            NOW,
        );
        assert!(limits.session.is_none());
        assert!(limits.weekly.is_none());
        assert_eq!(
            limits.monthly,
            Some(window(4.0, 43_200, Some(1_792_550_273_000)))
        );
    }

    #[test]
    fn parse_codex_rate_limits_falls_back_to_primary_session_when_durations_are_unknown() {
        let limits = parse_codex_rate_limits(
            &json!({
                "primary": { "usedPercent": 10, "resetsAt": 100 },
                "secondary": { "usedPercent": 20, "resetsAt": 200 },
            }),
            NOW,
        );
        assert_eq!(limits.session.unwrap().used_percent, 10.0);
        assert_eq!(limits.weekly.unwrap().used_percent, 20.0);
    }

    #[test]
    fn parse_droid_usage_maps_the_standard_pool_and_ignores_the_core_pool() {
        let limits = parse_droid_usage(
            &json!({
                "usesTokenRateLimitsBilling": true,
                "limits": {
                    "standard": {
                        "fiveHour": { "usedPercent": 100, "windowEnd": "2026-09-26T03:58:50.537Z", "secondsRemaining": 9651 },
                        "weekly": { "usedPercent": 66, "windowEnd": "2026-09-26T21:31:37.350Z" },
                        "monthly": { "usedPercent": 37, "windowEnd": "2026-10-10T03:36:42.309Z" },
                    },
                    "core": {
                        "fiveHour": { "usedPercent": 0, "windowEnd": null },
                        "weekly": { "usedPercent": 100, "windowEnd": "2026-09-29T02:14:45.325Z" },
                    },
                },
            }),
            NOW,
        );
        assert_eq!(limits.provider, RateLimitProvider::Droid);
        assert_eq!(
            limits.session,
            Some(window(100.0, 300, Some(iso("2026-09-26T03:58:50.537Z"))))
        );
        assert_eq!(limits.weekly.unwrap().used_percent, 66.0);
        assert_eq!(limits.monthly.unwrap().used_percent, 37.0);
        assert_eq!(limits.monthly.unwrap().window_minutes, 43_200);
    }

    #[test]
    fn parse_droid_usage_keeps_a_window_with_no_reset_time_and_drops_missing_ones() {
        let limits = parse_droid_usage(
            &json!({ "limits": { "standard": { "fiveHour": { "usedPercent": 0, "windowEnd": null } } } }),
            NOW,
        );
        assert_eq!(limits.session, Some(window(0.0, 300, None)));
        assert!(limits.weekly.is_none());
        assert!(parse_droid_usage(&json!({}), NOW).session.is_none());
    }

    #[test]
    fn parse_grok_billing_maps_a_weekly_credit_period() {
        let limits = parse_grok_billing(
            &json!({
                "config": {
                    "creditUsagePercent": 14,
                    "currentPeriod": {
                        "type": "USAGE_PERIOD_TYPE_WEEKLY",
                        "start": "2026-09-22T13:17:43.196638+00:00",
                        "end": "2026-09-29T13:17:43.196638+00:00",
                    },
                },
                "subscription_tier": "SuperGrok Heavy",
            }),
            NOW,
        );
        assert_eq!(limits.provider, RateLimitProvider::Grok);
        assert!(limits.session.is_none());
        assert!(limits.monthly.is_none());
        assert_eq!(
            limits.weekly,
            Some(window(
                14.0,
                10_080,
                Some(iso("2026-09-29T13:17:43.196638+00:00"))
            ))
        );
    }

    #[test]
    fn parse_grok_billing_maps_a_monthly_period_by_type_or_by_length() {
        let by_type = parse_grok_billing(
            &json!({ "config": { "creditUsagePercent": 40, "currentPeriod": { "type": "USAGE_PERIOD_TYPE_MONTHLY" } } }),
            NOW,
        );
        assert_eq!(by_type.monthly.unwrap().used_percent, 40.0);
        let by_length = parse_grok_billing(
            &json!({
                "config": {
                    "creditUsagePercent": 5,
                    "billingPeriodStart": "2026-09-01T00:00:00Z",
                    "billingPeriodEnd": "2026-10-01T00:00:00Z",
                },
            }),
            NOW,
        );
        assert!(by_length.weekly.is_none());
        assert_eq!(
            by_length.monthly.unwrap().resets_at,
            Some(iso("2026-10-01T00:00:00Z"))
        );
    }

    #[test]
    fn parse_grok_billing_returns_no_window_without_a_usage_percent() {
        let limits = parse_grok_billing(&json!({ "config": { "currentPeriod": {} } }), NOW);
        assert!(limits.weekly.is_none());
        assert!(limits.monthly.is_none());
    }

    #[test]
    fn parse_opencode_go_usage_maps_rolling_weekly_monthly_windows() {
        let limits = parse_opencode_go_usage(
            &json!({
                "usage": {
                    "rolling": { "status": "ok", "percent": 42, "resetsAt": "2026-09-16T16:27:38.287Z" },
                    "weekly": { "status": "ok", "percent": 30, "resetsAt": "2026-09-23T00:00:00Z" },
                    "monthly": { "status": "ok", "percent": 12, "resetsAt": "2026-10-16T00:00:00Z" },
                },
            }),
            NOW,
        );
        assert_eq!(limits.provider, RateLimitProvider::Opencode);
        assert_eq!(limits.session.unwrap().used_percent, 42.0);
        assert_eq!(limits.session.unwrap().window_minutes, 300);
        assert_eq!(limits.weekly.unwrap().used_percent, 30.0);
        assert_eq!(limits.weekly.unwrap().window_minutes, 10_080);
        assert_eq!(limits.monthly.unwrap().used_percent, 12.0);
        assert_eq!(limits.monthly.unwrap().window_minutes, 43_200);
        assert_eq!(
            limits.session.unwrap().resets_at,
            Some(iso("2026-09-16T16:27:38.287Z"))
        );
    }

    #[test]
    fn parse_opencode_go_usage_drops_non_ok_windows() {
        let limits = parse_opencode_go_usage(
            &json!({
                "usage": {
                    "rolling": { "status": "ok", "percent": 5, "resetsAt": null },
                    "weekly": { "status": "expired", "percent": 50, "resetsAt": null },
                    "monthly": { "percent": 50, "resetsAt": null },
                },
            }),
            NOW,
        );
        assert_eq!(limits.session.unwrap().used_percent, 5.0);
        assert!(limits.weekly.is_none());
        assert!(limits.monthly.is_none());
    }

    #[test]
    fn tooltip_includes_used_percent_and_remaining_time() {
        let now = iso("2026-08-27T08:00:00Z");
        assert_eq!(
            rate_limit_window_tooltip(
                &window(42.4, 300, Some(now + 2 * 3_600_000 + 33 * 60_000)),
                now
            ),
            "42% used · Resets in 2h 33m"
        );
    }

    #[test]
    fn json_shape_matches_the_typescript() {
        let limits = idle_rate_limits(RateLimitProvider::Opencode);
        assert_eq!(
            serde_json::to_value(&limits).unwrap(),
            json!({
                "provider": "opencode",
                "session": null,
                "weekly": null,
                "monthly": null,
                "resetCredits": null,
                "updatedAt": 0,
                "error": null,
                "status": "idle",
            })
        );
    }
}
