//! The pure helpers inside src/features/sessions/ui/AgentTranscript.tsx:
//! duration and metric labels, the elapsed clock, subagent row status, and
//! interjection chrome.

use std::sync::LazyLock;

use monocode_core::block::{
    AgentStep, AgentStepKind, BlockTool, InterjectionMeta, InterjectionSeverity, TurnMetrics,
};
use monocode_core::{Block, BlockRole};
use regex::Regex;

use monocode_core::transcript::activity::{
    BlockRef, ToolCallState, is_failed_status, tool_call_state,
};

/// `formatElapsed`: "45s", "2m 39s", "3m".
pub fn format_elapsed(elapsed_ms: Option<i64>) -> Option<String> {
    let elapsed_ms = elapsed_ms?;
    let total_sec = ((elapsed_ms as f64 / 1000.).round() as i64).max(1);
    if total_sec < 60 {
        return Some(format!("{total_sec}s"));
    }
    let minutes = total_sec / 60;
    let seconds = total_sec % 60;
    Some(if seconds > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{minutes}m")
    })
}

/// `formatWorkingDuration`: "Worked for 2m 39s", "Claude Opus 5 working for
/// 12s", or "Working…" before the clock starts.
pub fn format_working_duration(
    elapsed_ms: Option<i64>,
    model_name: Option<&str>,
    done: bool,
) -> String {
    let who = model_name
        .map(monocode_core::js::trim)
        .filter(|who| !who.is_empty());
    let elapsed = format_elapsed(elapsed_ms);
    let verb = match (done, who.is_some()) {
        (true, true) => "worked",
        (true, false) => "Worked",
        (false, true) => "working",
        (false, false) => "Working",
    };
    match (elapsed, who) {
        (None, Some(who)) if done => format!("{who} {verb}"),
        (None, None) if done => verb.to_string(),
        (None, Some(who)) => format!("{who} {verb}\u{2026}"),
        (None, None) => format!("{verb}\u{2026}"),
        (Some(elapsed), Some(who)) => format!("{who} {verb} for {elapsed}"),
        (Some(elapsed), None) => format!("{verb} for {elapsed}"),
    }
}

/// `backgroundLabel`.
pub fn background_label(tasks: &[String]) -> String {
    if tasks.len() == 1 {
        "running in background".into()
    } else {
        format!("{} tasks running in background", tasks.len())
    }
}

/// The text `LiveFoldTitle` shows: what the turn waits on, or its clock.
pub fn live_fold_text(
    elapsed_ms: Option<i64>,
    paused: bool,
    waiting_label: Option<&str>,
    background: &[String],
    model_name: Option<&str>,
) -> String {
    if paused {
        return waiting_label.unwrap_or("Waiting for approval").to_string();
    }
    let clock = format_working_duration(elapsed_ms, model_name, false);
    if background.is_empty() {
        clock
    } else {
        format!("{clock} · {}", background_label(background))
    }
}

/// `useElapsedFrom`: a turn's clock, which stops while the turn waits on the
/// user and resumes without counting the wait.
#[derive(Debug, Clone, Default)]
pub struct ElapsedClock {
    seen: Option<Option<i64>>,
    fallback: Option<i64>,
    paused_ms: i64,
    pause_started: Option<i64>,
}

impl ElapsedClock {
    /// Elapsed ms at `now`. A turn without a start time counts from the first
    /// call.
    pub fn elapsed(&mut self, started_at: Option<i64>, paused: bool, now: i64) -> i64 {
        if self.seen != Some(started_at) {
            self.seen = Some(started_at);
            self.fallback = None;
            self.paused_ms = 0;
            self.pause_started = paused.then_some(now);
        }
        let start = started_at.unwrap_or_else(|| *self.fallback.get_or_insert(now));
        if paused {
            let since = *self.pause_started.get_or_insert(now);
            return (since - start - self.paused_ms).max(0);
        }
        if let Some(since) = self.pause_started.take() {
            self.paused_ms += now - since;
        }
        (now - start - self.paused_ms).max(0)
    }
}

/// `hasTurnMetrics`.
pub fn has_turn_metrics(metrics: &TurnMetrics) -> bool {
    metrics.cache_hit_percent.is_some()
        || metrics.input_tokens.unwrap_or(0) > 0
        || metrics.output_tokens.unwrap_or(0) > 0
        || metrics.cache_read_tokens.unwrap_or(0) > 0
        || metrics.cache_write_tokens.unwrap_or(0) > 0
}

/// `formatMetricCount`: `Intl.NumberFormat` compact notation in English,
/// such as "950", "1.2K", "3.4M".
pub fn format_metric_count(value: f64) -> String {
    let value = value.round().max(0.);
    if value < 1000. {
        return format!("{value:.0}");
    }
    const UNITS: [(f64, &str); 4] = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
    let mut unit = UNITS
        .iter()
        .position(|(size, _)| value >= *size)
        .unwrap_or(3);
    loop {
        let (size, suffix) = UNITS[unit];
        let scaled = (value / size * 10.).round() / 10.;
        // Rounding 999.95K up reads as the next unit, the way Intl does it.
        if scaled >= 1000. && unit > 0 {
            unit -= 1;
            continue;
        }
        let text = if scaled.fract() == 0. {
            format!("{scaled:.0}")
        } else {
            format!("{scaled:.1}")
        };
        return format!("{text}{suffix}");
    }
}

/// What `TurnMetricsBadge` shows: a headline, a detail line, and the label
/// that joins them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnMetricsSummary {
    pub headline: String,
    pub detail: String,
    pub label: String,
}

/// The headline and detail of `TurnMetricsBadge`. `None` without metrics.
pub fn turn_metrics_summary(
    metrics: Option<&TurnMetrics>,
    elapsed_ms: Option<i64>,
) -> Option<TurnMetricsSummary> {
    let metrics = metrics.filter(|metrics| has_turn_metrics(metrics))?;
    let output_rate = match (metrics.output_tokens, elapsed_ms) {
        (Some(tokens), Some(elapsed)) if elapsed > 0 => {
            Some(tokens as f64 / (elapsed as f64 / 1000.))
        }
        _ => None,
    };
    let headline_parts: Vec<String> = [
        metrics
            .cache_hit_percent
            .map(|percent| format!("Cache hit {}%", percent.round() as i64)),
        output_rate.map(|rate| format!("Output {} tok/s", format_metric_count(rate))),
    ]
    .into_iter()
    .flatten()
    .collect();
    let headline = if headline_parts.is_empty() {
        "Turn tokens".to_string()
    } else {
        headline_parts.join(" · ")
    };
    let detail = [
        metrics
            .input_tokens
            .map(|n| format!("{} input", format_metric_count(n as f64))),
        metrics
            .output_tokens
            .map(|n| format!("{} output", format_metric_count(n as f64))),
        metrics
            .cache_read_tokens
            .map(|n| format!("{} cached", format_metric_count(n as f64))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let label = [headline.as_str(), detail.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(". ");
    Some(TurnMetricsSummary {
        headline,
        detail,
        label,
    })
}

/// Local hour and minute for an epoch ms time.
#[cfg(unix)]
fn local_hour_minute(epoch_ms: i64) -> (i64, i64) {
    let secs = epoch_ms.div_euclid(1000) as libc::time_t;
    // SAFETY: `tm` is plain old data that `localtime_r` fills in. Both
    // pointers are valid for the call and `localtime_r` is reentrant.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return utc_hour_minute(epoch_ms);
        }
        tm
    };
    (tm.tm_hour as i64, tm.tm_min as i64)
}

#[cfg(not(unix))]
fn local_hour_minute(epoch_ms: i64) -> (i64, i64) {
    // TODO(port): read the Windows time zone. UTC until then.
    utc_hour_minute(epoch_ms)
}

fn utc_hour_minute(epoch_ms: i64) -> (i64, i64) {
    let day_ms = epoch_ms.rem_euclid(86_400_000);
    (day_ms / 3_600_000, (day_ms / 60_000) % 60)
}

/// `formatClockTime`: "5:43 AM" in local time.
// TODO(port): the TypeScript used the reader's locale; this is the en-US form.
pub fn format_clock_time(epoch_ms: i64) -> String {
    let (hour, minute) = local_hour_minute(epoch_ms);
    format_hour_minute(hour, minute)
}

fn format_hour_minute(hour: i64, minute: i64) -> String {
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let hour12 = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour12}:{minute:02} {suffix}")
}

/// `subagentStatusLine`: how far a run got, "3 steps, 1 failed", or "failed".
pub fn subagent_status_line(block: &Block, steps: &[AgentStep]) -> String {
    if tool_call_state(block) == ToolCallState::Rejected {
        return "failed".into();
    }
    let tools = steps
        .iter()
        .filter(|step| step.kind == AgentStepKind::Tool)
        .count();
    if tools == 0 {
        return String::new();
    }
    let count = if tools == 1 {
        "1 step".to_string()
    } else {
        format!("{tools} steps")
    };
    let failed = steps
        .iter()
        .filter(|step| step.kind == AgentStepKind::Tool && is_failed_status(step.status.as_deref()))
        .count();
    match failed {
        0 => count,
        1 => format!("{count}, 1 failed"),
        n => format!("{count}, {n} failed"),
    }
}

/// `agentStepBlock`: a mirrored step as the transcript block it stands for,
/// so a subagent's trail goes through the same rows as the main agent's.
pub fn agent_step_block(step: &AgentStep) -> Block {
    if step.kind != AgentStepKind::Tool {
        let role = if step.kind == AgentStepKind::Reasoning {
            BlockRole::Reasoning
        } else {
            BlockRole::Assistant
        };
        return Block::new(step.id.clone(), role, step.text.clone());
    }
    let mut block = Block::new(step.id.clone(), BlockRole::Tool, step.text.clone());
    block.tool = Some(BlockTool {
        call_id: Some(step.id.clone()),
        title: Some(step.text.clone()),
        kind: step.tool_kind.clone().filter(|kind| !kind.is_empty()),
        status: step.status.clone().filter(|status| !status.is_empty()),
        detail: step.detail.clone().filter(|detail| !detail.is_empty()),
        preview: step.preview.clone(),
        ..Default::default()
    });
    block
}

static BLANK_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").expect("blank line"));

/// `headlineHasMore`: the line that titled a group has more in it than the
/// header shows.
pub fn headline_has_more(block: Option<&Block>) -> bool {
    let Some(block) = block else {
        return false;
    };
    block.role == BlockRole::Reasoning || BLANK_LINE.is_match(monocode_core::js::trim(&block.text))
}

/// `interjectionChrome`: the label and severity an interjection wears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterjectionChrome {
    pub label: String,
    pub severity_text: Option<&'static str>,
    pub severity: Option<InterjectionSeverity>,
}

pub fn interjection_chrome(meta: &InterjectionMeta) -> InterjectionChrome {
    let label = match meta.custom_type.as_str() {
        "advisor" => "Advisor".to_string(),
        "custom" => "Notice".to_string(),
        other => other.to_string(),
    };
    let severity_text = meta.severity.map(|severity| match severity {
        InterjectionSeverity::Blocker => "Blocker",
        InterjectionSeverity::Concern => "Concern",
        InterjectionSeverity::Nit => "Nit",
    });
    InterjectionChrome {
        label,
        severity_text,
        severity: meta.severity,
    }
}

/// `turnUserBlock`: the turn's user block, skipping app-written turns unless
/// the transcript is a managed worker's.
pub fn turn_user_block(blocks: &[BlockRef], managed: bool) -> Option<&BlockRef> {
    blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::User && (managed || !block.is_internal()))
}

/// `userTurnCount`.
pub fn user_turn_count(blocks: &[BlockRef], managed: bool) -> usize {
    blocks
        .iter()
        .filter(|block| block.role == BlockRole::User && (managed || !block.is_internal()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::transcript::fixtures::*;

    #[test]
    fn formats_working_durations() {
        assert_eq!(
            format_working_duration(Some(2_000), None, true),
            "Worked for 2s"
        );
        assert_eq!(
            format_working_duration(Some(159_000), None, true),
            "Worked for 2m 39s"
        );
        assert_eq!(
            format_working_duration(Some(180_000), None, true),
            "Worked for 3m"
        );
        assert_eq!(
            format_working_duration(Some(400), None, true),
            "Worked for 1s"
        );
        assert_eq!(
            format_working_duration(Some(9_000), Some("Claude Sonnet 5"), true),
            "Claude Sonnet 5 worked for 9s"
        );
        assert_eq!(
            format_working_duration(None, None, false),
            "Working\u{2026}"
        );
        assert_eq!(
            format_working_duration(None, Some("Opus"), false),
            "Opus working\u{2026}"
        );
        assert_eq!(format_working_duration(None, None, true), "Worked");
        assert_eq!(
            format_working_duration(Some(12_000), Some("  "), false),
            "Working for 12s"
        );
    }

    #[test]
    fn says_what_a_live_turn_waits_on() {
        assert_eq!(
            live_fold_text(Some(5_000), true, None, &[], None),
            "Waiting for approval"
        );
        assert_eq!(
            live_fold_text(Some(5_000), true, Some("Waiting for answers"), &[], None),
            "Waiting for answers"
        );
        assert_eq!(
            live_fold_text(Some(5_000), false, None, &["npm run dev".into()], None),
            "Working for 5s · running in background"
        );
        assert_eq!(
            live_fold_text(Some(5_000), false, None, &["a".into(), "b".into()], None),
            "Working for 5s · 2 tasks running in background"
        );
    }

    #[test]
    fn stops_the_clock_while_waiting_on_the_user() {
        let mut clock = ElapsedClock::default();
        assert_eq!(clock.elapsed(Some(1_000), false, 4_000), 3_000);
        assert_eq!(clock.elapsed(Some(1_000), true, 5_000), 4_000);
        assert_eq!(clock.elapsed(Some(1_000), true, 9_000), 4_000);
        assert_eq!(clock.elapsed(Some(1_000), false, 10_000), 4_000);
        assert_eq!(clock.elapsed(Some(1_000), false, 11_000), 5_000);
        // A new turn starts over.
        assert_eq!(clock.elapsed(Some(20_000), false, 21_000), 1_000);
        // Without a start time the clock starts at the first call.
        let mut fallback = ElapsedClock::default();
        assert_eq!(fallback.elapsed(None, false, 50_000), 0);
        assert_eq!(fallback.elapsed(None, false, 53_000), 3_000);
    }

    #[test]
    fn formats_metric_counts_compactly() {
        assert_eq!(format_metric_count(0.), "0");
        assert_eq!(format_metric_count(950.4), "950");
        assert_eq!(format_metric_count(1_000.), "1K");
        assert_eq!(format_metric_count(1_234.), "1.2K");
        assert_eq!(format_metric_count(12_345.), "12.3K");
        assert_eq!(format_metric_count(999_960.), "1M");
        assert_eq!(format_metric_count(3_400_000.), "3.4M");
        assert_eq!(format_metric_count(-5.), "0");
    }

    #[test]
    fn summarises_turn_metrics() {
        let metrics = TurnMetrics {
            input_tokens: Some(12_000),
            output_tokens: Some(2_000),
            cache_read_tokens: Some(9_000),
            cache_hit_percent: Some(74.6),
            ..Default::default()
        };
        let summary = turn_metrics_summary(Some(&metrics), Some(10_000)).unwrap();
        assert_eq!(summary.headline, "Cache hit 75% · Output 200 tok/s");
        assert_eq!(summary.detail, "12K input · 2K output · 9K cached");
        assert_eq!(
            summary.label,
            "Cache hit 75% · Output 200 tok/s. 12K input · 2K output · 9K cached"
        );
        assert_eq!(
            turn_metrics_summary(Some(&TurnMetrics::default()), None),
            None
        );
        let only_input = TurnMetrics {
            input_tokens: Some(10),
            ..Default::default()
        };
        assert_eq!(
            turn_metrics_summary(Some(&only_input), None)
                .unwrap()
                .headline,
            "Turn tokens"
        );
    }

    #[test]
    fn formats_clock_times_in_twelve_hours() {
        assert_eq!(format_hour_minute(0, 5), "12:05 AM");
        assert_eq!(format_hour_minute(5, 43), "5:43 AM");
        assert_eq!(format_hour_minute(12, 0), "12:00 PM");
        assert_eq!(format_hour_minute(23, 59), "11:59 PM");
        assert_eq!(utc_hour_minute(3_600_000 * 5 + 60_000 * 43), (5, 43));
        assert!(format_clock_time(0).ends_with('M'));
    }

    #[test]
    fn counts_a_runs_steps_and_failures() {
        let running = agent("a", "Run tests", "completed");
        assert_eq!(subagent_status_line(&running, &[]), "");
        let steps = vec![
            step("read", AgentStepKind::Tool, "Read package.json", None),
            step("bash", AgentStepKind::Tool, "npm test", Some("failed")),
            step("fix", AgentStepKind::Tool, "Edit src/App.tsx", None),
            step("say", AgentStepKind::Message, "Done", None),
        ];
        assert_eq!(subagent_status_line(&running, &steps), "3 steps, 1 failed");
        assert_eq!(subagent_status_line(&running, &steps[..1]), "1 step");
        assert_eq!(
            subagent_status_line(&agent("b", "x", "failed"), &steps),
            "failed"
        );
    }

    #[test]
    fn mirrors_steps_as_blocks() {
        let mut tool = step("s", AgentStepKind::Tool, "npm test", Some("failed"));
        tool.tool_kind = Some("execute".into());
        tool.detail = Some("assertion error".into());
        let block = agent_step_block(&tool);
        assert_eq!(block.role, BlockRole::Tool);
        let mirrored = block.tool.unwrap();
        assert_eq!(mirrored.kind.as_deref(), Some("execute"));
        assert_eq!(mirrored.detail.as_deref(), Some("assertion error"));
        assert_eq!(mirrored.call_id.as_deref(), Some("s"));
        let message = agent_step_block(&step("m", AgentStepKind::Message, "hi", None));
        assert_eq!(message.role, BlockRole::Assistant);
        let reasoning = agent_step_block(&step("r", AgentStepKind::Reasoning, "hm", None));
        assert_eq!(reasoning.role, BlockRole::Reasoning);
    }

    #[test]
    fn knows_when_a_headline_has_more_to_read() {
        assert!(!headline_has_more(None));
        assert!(headline_has_more(Some(&thought("r", "one line"))));
        assert!(!headline_has_more(Some(&note("n", "one line"))));
        assert!(headline_has_more(Some(&note("n", "first\n\nsecond"))));
    }

    #[test]
    fn names_interjections() {
        let chrome = interjection_chrome(&InterjectionMeta {
            custom_type: "advisor".into(),
            severity: Some(InterjectionSeverity::Concern),
            extra: Default::default(),
        });
        assert_eq!(chrome.label, "Advisor");
        assert_eq!(chrome.severity_text, Some("Concern"));
        let custom = interjection_chrome(&InterjectionMeta {
            custom_type: "custom".into(),
            severity: None,
            extra: Default::default(),
        });
        assert_eq!(custom.label, "Notice");
        assert_eq!(custom.severity_text, None);
    }
}
