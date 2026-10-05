//! Port of the pure half of src/features/providers/model/accountUsage.ts:
//! headroom, the Ready / Running low / Exhausted status, and the best
//! alternative account. The `useProviderAccountUsage` hook becomes
//! `RateLimits::load_accounts` and `RateLimits::refresh_accounts`.

use monocode_harness::core::provider_account_identity::identity_key;
use monocode_harness::core::provider_accounts::ProviderAccount;
use serde::{Deserialize, Serialize};

use super::rate_limits::{
    ProviderRateLimits, RateLimitStatus, clamp_used_percent, exhausted_window_reset_at,
    format_reset_duration,
};
use monocode_core::js;

/// `CLOCK_MS`: how often an account list re-reads the clock.
pub const ACCOUNT_USAGE_CLOCK_MS: i64 = 30_000;
/// `LOW_HEADROOM_PERCENT`: at or below this much headroom an account reads
/// as "Running low".
pub const LOW_HEADROOM_PERCENT: f64 = 20.0;

/// `accountUsageKey`: the same `provider:id` key the identity cache uses.
pub fn account_usage_key(account: &ProviderAccount) -> String {
    identity_key(account)
}

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
        if limits.is_none_or(|limits| {
            matches!(
                limits.status,
                RateLimitStatus::Idle | RateLimitStatus::Fetching
            )
        }) {
            return AccountStatus {
                tone: AccountStatusTone::Checking,
                label: "Checking…".into(),
                detail: None,
            };
        }
        let limits = limits.expect("checked above");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::rate_limits::{RateLimitProvider, RateLimitWindow};
    use monocode_core::HarnessId;
    use std::collections::HashMap;

    const NOW: i64 = 1_790_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn window(used_percent: f64, resets_at: Option<i64>) -> RateLimitWindow {
        RateLimitWindow {
            used_percent,
            window_minutes: 300,
            resets_at,
        }
    }

    fn soon(used_percent: f64) -> RateLimitWindow {
        window(used_percent, Some(NOW + HOUR))
    }

    fn limits(
        session: Option<RateLimitWindow>,
        weekly: Option<RateLimitWindow>,
    ) -> ProviderRateLimits {
        ProviderRateLimits {
            provider: RateLimitProvider::Claude,
            session,
            weekly,
            monthly: None,
            reset_credits: None,
            updated_at: NOW,
            error: None,
            status: RateLimitStatus::Ok,
        }
    }

    fn with(status: RateLimitStatus, error: Option<&str>) -> ProviderRateLimits {
        ProviderRateLimits {
            status,
            error: error.map(str::to_string),
            ..limits(None, None)
        }
    }

    fn account(id: &str) -> ProviderAccount {
        ProviderAccount::new(id, HarnessId::Claude, id)
    }

    #[test]
    fn headroom_uses_the_tightest_window() {
        assert_eq!(
            account_headroom(Some(&limits(Some(soon(30.0)), Some(soon(75.0)))), NOW),
            Some(25.0)
        );
    }

    #[test]
    fn headroom_treats_a_window_past_its_reset_time_as_available() {
        let spent = limits(Some(window(100.0, Some(NOW - 1))), Some(soon(40.0)));
        assert_eq!(account_headroom(Some(&spent), NOW), Some(60.0));
    }

    #[test]
    fn headroom_is_none_without_usage_windows() {
        assert_eq!(account_headroom(Some(&limits(None, None)), NOW), None);
        assert_eq!(account_headroom(None, NOW), None);
    }

    #[test]
    fn reads_ready_with_comfortable_headroom() {
        assert_eq!(
            account_status(Some(&limits(Some(soon(10.0)), Some(soon(50.0)))), NOW),
            AccountStatus {
                tone: AccountStatusTone::Ready,
                label: "Ready".into(),
                detail: None,
            }
        );
    }

    #[test]
    fn reads_running_low_at_or_below_20_percent_headroom() {
        assert_eq!(
            account_status(Some(&limits(Some(soon(84.0)), None)), NOW),
            AccountStatus {
                tone: AccountStatusTone::Low,
                label: "Running low".into(),
                detail: Some("16% left".into()),
            }
        );
    }

    #[test]
    fn reads_exhausted_with_the_latest_reset_of_the_used_up_windows() {
        let spent = limits(
            Some(window(100.0, Some(NOW + 31 * 60_000))),
            Some(window(100.0, Some(NOW + 2 * HOUR))),
        );
        assert_eq!(
            account_status(Some(&spent), NOW),
            AccountStatus {
                tone: AccountStatusTone::Exhausted,
                label: "Exhausted".into(),
                detail: Some("back in 2h".into()),
            }
        );
    }

    #[test]
    fn reads_checking_while_the_first_snapshot_loads() {
        assert_eq!(account_status(None, NOW).tone, AccountStatusTone::Checking);
        assert_eq!(
            account_status(Some(&with(RateLimitStatus::Fetching, None)), NOW).tone,
            AccountStatusTone::Checking
        );
    }

    #[test]
    fn surfaces_why_usage_is_missing() {
        assert_eq!(
            account_status(
                Some(&with(
                    RateLimitStatus::Unavailable,
                    Some("Claude not signed in")
                )),
                NOW
            ),
            AccountStatus {
                tone: AccountStatusTone::Unknown,
                label: "Claude not signed in".into(),
                detail: None,
            }
        );
        assert_eq!(
            account_status(
                Some(&with(
                    RateLimitStatus::Error,
                    Some("Claude sign-in expired")
                )),
                NOW
            )
            .label,
            "Claude sign-in expired"
        );
    }

    #[test]
    fn picks_the_account_with_the_most_headroom_above_the_low_threshold() {
        let usage: HashMap<&str, ProviderRateLimits> = [
            ("low", limits(Some(soon(85.0)), None)),
            ("mid", limits(Some(soon(40.0)), None)),
            ("best", limits(Some(soon(5.0)), Some(soon(10.0)))),
        ]
        .into_iter()
        .collect();
        let accounts: Vec<ProviderAccount> =
            ["low", "mid", "best"].into_iter().map(account).collect();
        let best = best_alternative_account(
            &accounts,
            |entry| usage.get(entry.id.as_str()).cloned(),
            NOW,
        );
        assert_eq!(best.map(|account| account.id.as_str()), Some("best"));
    }

    #[test]
    fn suggests_nothing_when_every_other_account_is_low_or_unknown() {
        let usage: HashMap<&str, ProviderRateLimits> = [("low", limits(Some(soon(95.0)), None))]
            .into_iter()
            .collect();
        let accounts: Vec<ProviderAccount> = ["low", "unknown"].into_iter().map(account).collect();
        assert!(
            best_alternative_account(
                &accounts,
                |entry| usage.get(entry.id.as_str()).cloned(),
                NOW
            )
            .is_none()
        );
    }
}
