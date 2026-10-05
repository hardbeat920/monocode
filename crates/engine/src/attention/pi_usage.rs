//! Port of src/features/providers/model/piUsage.ts: which Pi models report
//! subscription usage, and the checks on what the usage fetch returned.

use monocode_harness::providers::pi::protocol::parse_pi_model_ref;
use serde_json::Value;

use super::rate_limits::{
    ProviderRateLimits, RateLimitProvider, RateLimitStatus, RateLimitWindow, error_rate_limits,
    idle_rate_limits, unavailable_rate_limits,
};

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

    fn integration(self) -> monocode_integrations::pi_usage::PiUsageProvider {
        match self {
            PiUsageProvider::Anthropic => {
                monocode_integrations::pi_usage::PiUsageProvider::Anthropic
            }
            PiUsageProvider::OpenaiCodex => {
                monocode_integrations::pi_usage::PiUsageProvider::OpenaiCodex
            }
        }
    }
}

/// `piUsageProvider`: only concrete Pi models from supported billing
/// providers.
pub fn pi_usage_provider(model: Option<&str>) -> Option<PiUsageProvider> {
    let rest = model?.strip_prefix("pi:")?;
    match parse_pi_model_ref(Some(rest))?.provider.as_str() {
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

/// `fetchPiUsage`. Blocks on the network; run it off the UI thread.
pub fn fetch_pi_usage(provider: PiUsageProvider, now: i64) -> ProviderRateLimits {
    let result = monocode_integrations::pi_usage::fetch_pi_usage(provider.integration());
    match serde_json::to_value(&result) {
        Ok(value) => pi_usage_from_value(provider, Some(&value), now),
        Err(_) => pi_usage_from_value(provider, None, now),
    }
}

/// The checks `fetchPiUsage` ran on the IPC result. `None` stands for a
/// rejected call, whose detail never reaches the UI.
pub fn pi_usage_from_value(
    provider: PiUsageProvider,
    result: Option<&Value>,
    now: i64,
) -> ProviderRateLimits {
    let billing = pi_billing_provider(provider);
    let Some(result) = result else {
        return error_rate_limits(
            billing,
            "Could not fetch Pi usage. Try refreshing.",
            None,
            now,
        );
    };
    let rec = result.as_object();
    let field = |key: &str| rec.and_then(|rec| rec.get(key));
    let status = field("status").and_then(Value::as_str);
    if let (Some("unavailable"), Some(Value::String(message))) = (status, field("message")) {
        return unavailable_rate_limits(billing, message, now);
    }
    if let (Some("error"), Some(Value::String(message))) = (status, field("message")) {
        return error_rate_limits(billing, message, None, now);
    }
    let windows = field("windows").and_then(Value::as_object);
    let session = parse_window(windows.and_then(|windows| windows.get("session")));
    let weekly = parse_window(windows.and_then(|windows| windows.get("weekly")));
    match (status, session, weekly) {
        (Some("ok"), Some(session), Some(weekly)) if session.is_some() || weekly.is_some() => {
            ProviderRateLimits {
                status: RateLimitStatus::Ok,
                session,
                weekly,
                updated_at: now,
                ..idle_rate_limits(billing)
            }
        }
        _ => error_rate_limits(
            billing,
            "Pi usage response was unexpected. Try refreshing.",
            None,
            now,
        ),
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn safe_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
}

/// `parseWindow`: `Some(None)` for an explicit `null`, `None` for a value
/// that does not have the window shape.
fn parse_window(value: Option<&Value>) -> Option<Option<RateLimitWindow>> {
    let value = value?;
    if value.is_null() {
        return Some(None);
    }
    let rec = value.as_object()?;
    let used_percent = rec.get("usedPercent")?.as_f64()?;
    let window_minutes = rec.get("windowMinutes")?.as_f64()?;
    let resets_at = match rec.get("resetsAt") {
        Some(Value::Null) => None,
        Some(Value::Number(number)) => {
            let value = number.as_f64()?;
            if !safe_integer(value) || value < 0.0 {
                return None;
            }
            Some(value as i64)
        }
        _ => return None,
    };
    if !(0.0..=100.0).contains(&used_percent)
        || !safe_integer(window_minutes)
        || window_minutes <= 0.0
    {
        return None;
    }
    Some(Some(RateLimitWindow {
        used_percent,
        window_minutes: window_minutes as i64,
        resets_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_790_000_000_000;

    #[test]
    fn matches_only_concrete_pi_models_from_supported_billing_providers() {
        assert_eq!(
            pi_usage_provider(Some("pi:anthropic/claude-sonnet-4-6")),
            Some(PiUsageProvider::Anthropic)
        );
        assert_eq!(
            pi_usage_provider(Some("pi:openai-codex/gpt-5.4")),
            Some(PiUsageProvider::OpenaiCodex)
        );
        for model in [
            None,
            Some("pi:default"),
            Some("pi:anthropic/"),
            Some("omp:anthropic/claude"),
            Some("pi:openai/gpt-5.4"),
            Some("pi:openrouter/anthropic/claude"),
        ] {
            assert_eq!(pi_usage_provider(model), None, "{model:?}");
        }
    }

    #[test]
    fn keeps_weekly_only_quotas_and_zero_usage_without_inventing_windows() {
        let result = pi_usage_from_value(
            PiUsageProvider::OpenaiCodex,
            Some(&json!({
                "status": "ok",
                "windows": {
                    "session": null,
                    "weekly": { "usedPercent": 0, "windowMinutes": 10080, "resetsAt": 1_790_700_198_000_i64 },
                },
            })),
            NOW,
        );
        assert_eq!(result.provider, RateLimitProvider::Codex);
        assert_eq!(result.status, RateLimitStatus::Ok);
        assert!(result.session.is_none());
        assert_eq!(
            result.weekly,
            Some(RateLimitWindow {
                used_percent: 0.0,
                window_minutes: 10080,
                resets_at: Some(1_790_700_198_000),
            })
        );
        assert!(result.reset_credits.is_none());
    }

    #[test]
    fn rejects_malformed_values_instead_of_displaying_fabricated_quota_data() {
        for response in [
            json!(null),
            json!({}),
            json!({ "status": "ok", "windows": { "session": null, "weekly": null } }),
            json!({ "status": "ok", "windows": { "session": { "usedPercent": "NaN", "windowMinutes": 300, "resetsAt": null }, "weekly": null } }),
            json!({ "status": "ok", "windows": { "session": { "usedPercent": 20, "windowMinutes": -1, "resetsAt": null }, "weekly": null } }),
            json!({ "status": "ok", "windows": { "session": { "usedPercent": 20, "windowMinutes": 300, "resetsAt": "bad" }, "weekly": null } }),
        ] {
            let result = pi_usage_from_value(PiUsageProvider::Anthropic, Some(&response), NOW);
            assert_eq!(result.status, RateLimitStatus::Error, "{response}");
            assert!(result.session.is_none());
            assert!(result.weekly.is_none());
        }
    }

    #[test]
    fn does_not_expose_transport_errors_in_the_ui() {
        let result = pi_usage_from_value(PiUsageProvider::Anthropic, None, NOW);
        assert_eq!(result.status, RateLimitStatus::Error);
        assert_eq!(
            result.error.as_deref(),
            Some("Could not fetch Pi usage. Try refreshing.")
        );
    }

    #[test]
    fn reads_the_integration_result_shape() {
        let unavailable = pi_usage_from_value(
            PiUsageProvider::Anthropic,
            Some(&json!({ "status": "unavailable", "message": "Sign in to Pi" })),
            NOW,
        );
        assert_eq!(unavailable.status, RateLimitStatus::Unavailable);
        assert_eq!(unavailable.error.as_deref(), Some("Sign in to Pi"));
    }
}
