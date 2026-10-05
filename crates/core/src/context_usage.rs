//! Port of src/features/sessions/model/contextUsage.ts.

use serde::{Deserialize, Serialize};

use crate::js;

/// How much of the model context window the session occupies.
///
/// This is a level, not a running total: every harness reports the size of
/// the prompt it just sent, so the newest reading replaces the previous one.
/// Once the harness compacts, its next report is smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Tokens in the context window as of the last request.
    pub used: i64,
    /// Context window for the active model, when the harness reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<i64>,
}

/// A fresh reading, where either half may be missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextReading {
    pub used: Option<i64>,
    pub window: Option<i64>,
}

/// `contextRatio`: fraction of the window in use, or `None` when the window
/// is unknown.
pub fn context_ratio(usage: Option<&ContextUsage>) -> Option<f64> {
    let usage = usage?;
    let window = usage.window.filter(|window| *window > 0)?;
    if usage.used < 0 {
        return None;
    }
    Some((usage.used as f64 / window as f64).min(1.0))
}

/// `contextPercent`: whole-percent context used.
pub fn context_percent(usage: Option<&ContextUsage>) -> Option<i64> {
    context_ratio(usage).map(|ratio| js::round(ratio * 100.0) as i64)
}

/// `formatTokens`: compact token count for chrome, such as 980, 176K, 1.2M.
pub fn format_tokens(count: f64) -> String {
    if !count.is_finite() || count < 0.0 {
        return "0".into();
    }
    if count < 1000.0 {
        return js::number_to_string(js::round(count));
    }
    let scaled = |value: f64| {
        if value < 10.0 {
            let fixed = js::to_fixed_1(value);
            fixed
                .strip_suffix(".0")
                .map(str::to_string)
                .unwrap_or(fixed)
        } else {
            js::number_to_string(js::round(value))
        }
    };
    if count < 1_000_000.0 {
        return format!("{}K", scaled(count / 1000.0));
    }
    format!("{}M", scaled(count / 1_000_000.0))
}

/// `contextTooltip`: two-line hover text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTooltip {
    pub headline: String,
    pub detail: String,
}

/// `contextTooltip`: "69% context used" over "176K / 256K tokens".
pub fn context_tooltip(usage: &ContextUsage) -> ContextTooltip {
    let headline = match context_percent(Some(usage)) {
        Some(percent) => format!("{percent}% context used"),
        None => "Context used".into(),
    };
    let detail = match usage.window.filter(|window| *window != 0) {
        Some(window) => format!(
            "{} / {} tokens",
            format_tokens(usage.used as f64),
            format_tokens(window as f64)
        ),
        None => format!("{} tokens", format_tokens(usage.used as f64)),
    };
    ContextTooltip { headline, detail }
}

/// `mergeContextUsage`: merge a fresh reading into what we already know.
///
/// Harnesses split the two halves across messages (Claude reports the window
/// only on the turn result), so a reading without a window keeps the last
/// known one.
pub fn merge_context_usage(previous: Option<&ContextUsage>, next: ContextReading) -> ContextUsage {
    let used = next
        .used
        .or(previous.map(|previous| previous.used))
        .unwrap_or(0);
    let window = next
        .window
        .or(previous.and_then(|previous| previous.window))
        .filter(|window| *window != 0);
    ContextUsage { used, window }
}

/// `dropContextWindow`: forget the window, which belongs to the model, while
/// keeping the level, which describes the transcript.
pub fn drop_context_window(usage: Option<&ContextUsage>) -> Option<ContextUsage> {
    usage.map(|usage| ContextUsage {
        used: usage.used,
        window: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(used: i64, window: Option<i64>) -> ContextUsage {
        ContextUsage { used, window }
    }

    fn reading(used: Option<i64>, window: Option<i64>) -> ContextReading {
        ContextReading { used, window }
    }

    // contextRatio
    #[test]
    fn divides_used_by_the_window() {
        assert_eq!(
            context_ratio(Some(&usage(128_000, Some(256_000)))),
            Some(0.5)
        );
    }

    #[test]
    fn has_no_ratio_without_a_window() {
        assert_eq!(context_ratio(Some(&usage(128_000, None))), None);
        assert_eq!(context_ratio(None), None);
    }

    #[test]
    fn clamps_a_window_overrun_to_full() {
        assert_eq!(
            context_ratio(Some(&usage(300_000, Some(256_000)))),
            Some(1.0)
        );
    }

    #[test]
    fn rejects_nonsense_readings() {
        assert_eq!(context_ratio(Some(&usage(-5, Some(100)))), None);
        assert_eq!(context_ratio(Some(&usage(10, Some(0)))), None);
    }

    // contextPercent
    #[test]
    fn rounds_to_whole_percent() {
        assert_eq!(
            context_percent(Some(&usage(176_000, Some(256_000)))),
            Some(69)
        );
    }

    // formatTokens
    #[test]
    fn keeps_small_counts_exact() {
        assert_eq!(format_tokens(0.0), "0");
        assert_eq!(format_tokens(980.0), "980");
    }

    #[test]
    fn abbreviates_thousands_and_millions() {
        assert_eq!(format_tokens(1_000.0), "1K");
        assert_eq!(format_tokens(1_500.0), "1.5K");
        assert_eq!(format_tokens(176_000.0), "176K");
        assert_eq!(format_tokens(1_000_000.0), "1M");
        assert_eq!(format_tokens(1_200_000.0), "1.2M");
        assert_eq!(format_tokens(1_250.0), "1.3K");
    }

    // contextTooltip
    #[test]
    fn reads_like_the_composer_hover() {
        assert_eq!(
            context_tooltip(&usage(176_000, Some(256_000))),
            ContextTooltip {
                headline: "69% context used".into(),
                detail: "176K / 256K tokens".into(),
            }
        );
    }

    #[test]
    fn omits_the_denominator_when_the_window_is_unknown() {
        assert_eq!(
            context_tooltip(&usage(176_000, None)),
            ContextTooltip {
                headline: "Context used".into(),
                detail: "176K tokens".into(),
            }
        );
    }

    // mergeContextUsage
    #[test]
    fn replaces_the_level_instead_of_accumulating() {
        let first = merge_context_usage(None, reading(Some(30_000), None));
        assert_eq!(
            merge_context_usage(Some(&first), reading(Some(45_000), None)),
            usage(45_000, None)
        );
    }

    #[test]
    fn keeps_a_window_reported_on_an_earlier_message() {
        let seeded = merge_context_usage(None, reading(Some(10_000), Some(200_000)));
        assert_eq!(
            merge_context_usage(Some(&seeded), reading(Some(20_000), None)),
            usage(20_000, Some(200_000))
        );
    }

    #[test]
    fn keeps_the_level_when_only_a_window_arrives() {
        let seeded = merge_context_usage(None, reading(Some(10_000), None));
        assert_eq!(
            merge_context_usage(Some(&seeded), reading(None, Some(200_000))),
            usage(10_000, Some(200_000))
        );
    }

    #[test]
    fn drops_back_down_after_the_harness_compacts() {
        let full = merge_context_usage(None, reading(Some(190_000), Some(200_000)));
        assert_eq!(
            merge_context_usage(Some(&full), reading(Some(40_000), None)),
            usage(40_000, Some(200_000))
        );
    }

    // dropContextWindow
    #[test]
    fn keeps_the_level_but_forgets_the_model_specific_window() {
        assert_eq!(
            drop_context_window(Some(&usage(30_000, Some(1_000_000)))),
            Some(usage(30_000, None))
        );
    }

    #[test]
    fn passes_through_nothing() {
        assert_eq!(drop_context_window(None), None);
    }

    #[test]
    fn leaves_the_ring_hidden_until_the_next_turn_re_reports() {
        assert_eq!(
            context_ratio(drop_context_window(Some(&usage(30_000, Some(200_000)))).as_ref()),
            None
        );
    }
}
