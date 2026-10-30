//! Context-window accounting, matching sessions/model/contextUsage.ts.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub used: u64,
    pub window: Option<u64>,
    /// Prompt-cache lifetime the provider last reported, when it reports one.
    #[serde(default)]
    pub cache_ttl_ms: Option<u64>,
    /// Running cost total last seen from the provider, the baseline for the next turn's cost.
    #[serde(default)]
    pub cost_total_usd: Option<f64>,
}
impl Usage {
    /// Fills in a turn's cost from the provider's running total. A total that
    /// drops means the CLI restarted its counter, so all of it is new spend.
    /// `spent` is what an earlier result in the same turn already accounted for.
    pub fn apply_cost(&mut self, metrics: &mut crate::agent::TurnMetrics, spent: f64) {
        let Some(total) = metrics.total_cost_usd.filter(|t| t.is_finite() && *t >= 0.) else {
            return;
        };
        let baseline = self.cost_total_usd.unwrap_or(0.);
        let delta = if total >= baseline {
            total - baseline
        } else {
            total
        };
        metrics.cost_usd = Some(spent + delta);
        self.cost_total_usd = Some(total);
    }
    pub fn ratio(self) -> Option<f32> {
        self.window
            .filter(|w| *w > 0)
            .map(|w| (self.used as f64 / w as f64).min(1.) as f32)
    }
    pub fn merge_cache_ttl(&mut self, ttl_ms: u64) {
        self.cache_ttl_ms = Some(ttl_ms);
    }
    pub fn merge(&mut self, used: Option<u64>, window: Option<u64>) {
        if let Some(used) = used {
            self.used = used;
        }
        if let Some(window) = window.filter(|w| *w > 0) {
            self.window = Some(window);
        }
    }
}

pub fn claude_used(usage: &Value) -> Option<u64> {
    let used = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .iter()
    .map(|key| usage[key].as_u64().unwrap_or(0))
    .sum();
    (used > 0).then_some(used)
}

pub fn format_tokens(count: u64) -> String {
    let (value, suffix) = if count >= 1_000_000 {
        (count as f64 / 1_000_000., "M")
    } else if count >= 1000 {
        (count as f64 / 1000., "K")
    } else {
        return count.to_string();
    };
    if value < 10. {
        format!(
            "{}{suffix}",
            format!("{value:.1}")
                .trim_end_matches('0')
                .trim_end_matches('.')
        )
    } else {
        format!("{value:.0}{suffix}")
    }
}

/// Claude reports which cache tier it wrote: 1h on a subscription, 5m on an API key.
pub fn claude_cache_ttl_ms(usage: &Value) -> Option<u64> {
    let creation = &usage["cache_creation"];
    if creation["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0) > 0 {
        Some(60 * 60_000)
    } else if creation["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0) > 0 {
        Some(5 * 60_000)
    } else {
        None
    }
}

/// What the next message will cost, from a session's turns (`SessionStatsChip`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SessionStats {
    /// Session spend; absent when no turn reported a cost.
    pub cost_usd: Option<f64>,
    /// Some finished turns reported no cost, so `cost_usd` is a lower bound.
    pub cost_partial: bool,
    pub started_at: Option<u64>,
    pub working_ms: u64,
    pub last_turn_ended_at: Option<u64>,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub session_hit_percent: Option<f64>,
    pub last_hit_percent: Option<f64>,
}

impl SessionStats {
    pub fn from_turns<'a>(turns: impl IntoIterator<Item = &'a crate::agent::TurnInfo>) -> Self {
        let mut turns: Vec<_> = turns.into_iter().collect();
        turns.sort_by_key(|t| t.started_at);
        let mut stats = Self::default();
        let (mut input, mut reported) = (0, false);
        for turn in turns {
            let Some(started) = turn.started_at else {
                continue;
            };
            stats.started_at.get_or_insert(started);
            let done = turn.has_duration();
            if done {
                stats.working_ms += turn.duration_ms;
                stats.last_turn_ended_at =
                    Some(turn.completed_at.unwrap_or(started + turn.duration_ms));
            }
            let metrics = turn.metrics.unwrap_or_default();
            match metrics.cost_usd {
                Some(cost) => *stats.cost_usd.get_or_insert(0.) += cost,
                None if done => stats.cost_partial = true,
                None => {}
            }
            if let Some(hit) = metrics.cache_hit_percent {
                reported = true;
                stats.last_hit_percent = Some(hit);
            }
            stats.cache_read_tokens += metrics.cache_read_tokens;
            stats.cache_write_tokens += metrics.cache_write_tokens;
            input += metrics.input_tokens;
        }
        let cacheable = input + stats.cache_read_tokens + stats.cache_write_tokens;
        if reported && cacheable > 0 {
            stats.session_hit_percent =
                Some(stats.cache_read_tokens as f64 / cacheable as f64 * 100.);
        }
        stats.cost_partial &= stats.cost_usd.is_some();
        stats
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CacheState {
    Live,
    Cold,
    Warm { remaining_ms: u64, warning: bool },
}

/// `None` when the provider never reported a lifetime or no turn has finished.
pub fn cache_state(
    stats: &SessionStats,
    ttl_ms: Option<u64>,
    now_ms: u64,
    busy: bool,
) -> Option<CacheState> {
    if busy {
        return Some(CacheState::Live);
    }
    let (ttl, ended) = (ttl_ms.filter(|t| *t > 0)?, stats.last_turn_ended_at?);
    let expires = ended + ttl;
    if now_ms >= expires {
        return Some(CacheState::Cold);
    }
    let remaining_ms = expires - now_ms;
    Some(CacheState::Warm {
        remaining_ms,
        warning: remaining_ms * 5 <= ttl,
    })
}

pub fn format_cost(usd: f64) -> String {
    if usd > 0. && usd < 0.01 {
        "<$0.01".into()
    } else {
        format!("${usd:.2}")
    }
}

pub fn format_countdown(ms: u64) -> String {
    let total = ms.div_ceil(1000);
    format!("{}:{:02}", total / 60, total % 60)
}

/// 45s, 14m, 2h 5m.
pub fn format_span(ms: u64) -> String {
    let minutes = ms / 60_000;
    match (minutes, minutes % 60) {
        (0, _) => format!("{}s", ms / 1000),
        (m, _) if m < 60 => format!("{m}m"),
        (m, 0) => format!("{}h", m / 60),
        (m, rest) => format!("{}h {rest}m", m / 60),
    }
}

#[cfg(test)]
mod session_stats_tests {
    use super::*;
    use crate::agent::{TurnInfo, TurnMetrics};

    fn turn(started: u64, duration: u64, cost: Option<f64>) -> TurnInfo {
        TurnInfo {
            started_at: Some(started),
            duration_ms: duration,
            duration_known: true,
            metrics: Some(TurnMetrics {
                cost_usd: cost,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn cost_is_partial_when_a_finished_turn_has_none() {
        let turns = [turn(1000, 500, Some(0.5)), turn(3000, 500, None)];
        let stats = SessionStats::from_turns(&turns);
        assert_eq!(stats.cost_usd, Some(0.5));
        assert!(stats.cost_partial);
        assert_eq!(stats.working_ms, 1000);
        assert_eq!(SessionStats::from_turns(&[turn(1, 1, None)]).cost_usd, None);
    }

    #[test]
    fn apply_cost_takes_deltas_and_handles_resets() {
        let mut usage = Usage::default();
        let total = |t| TurnMetrics {
            total_cost_usd: Some(t),
            ..Default::default()
        };
        let (mut a, mut b, mut c) = (total(0.5), total(0.8), total(0.2));
        usage.apply_cost(&mut a, 0.);
        usage.apply_cost(&mut b, 0.);
        usage.apply_cost(&mut c, 0.);
        assert_eq!(a.cost_usd, Some(0.5));
        assert!((b.cost_usd.unwrap() - 0.3).abs() < 1e-9);
        assert_eq!(c.cost_usd, Some(0.2));
        // A second result in the same turn adds to what the turn already spent.
        let mut d = total(0.3);
        usage.apply_cost(&mut d, 0.2);
        assert!((d.cost_usd.unwrap() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn cache_counts_down_warns_then_goes_cold() {
        let stats = SessionStats::from_turns(&[turn(0, 0, None)]);
        let ttl = Some(300_000);
        assert!(matches!(
            cache_state(&stats, ttl, 100_000, false),
            Some(CacheState::Warm { warning: false, .. })
        ));
        assert!(matches!(
            cache_state(&stats, ttl, 250_000, false),
            Some(CacheState::Warm { warning: true, .. })
        ));
        assert_eq!(
            cache_state(&stats, ttl, 300_000, false),
            Some(CacheState::Cold)
        );
        assert_eq!(cache_state(&stats, ttl, 0, true), Some(CacheState::Live));
        assert_eq!(cache_state(&stats, None, 0, false), None);
        assert_eq!(format_countdown(252_000), "4:12");
    }
}
