//! Local reference layout for the chat presentation.
use monocode_core::{
    tool::{ToolCall, ToolState},
    transcript::Transcript,
};
pub(crate) fn chat() -> Transcript {
    let mut t = Transcript::default();
    t.begin("assistant");
    t.append("The new regression tests pass. They cover Markdown across storage chunks, repeated tool snapshots, and expanding reasoning while output changes. I also fixed a scroll issue where opening a disclosure could jump to the tail, and limited syntax highlighting to one background job at a time.");
    for (id, name, input, output, state) in [
        (
            "read",
            "Read",
            r#"{"file_path":"docs/native/migration.md"}"#,
            "",
            ToolState::Completed,
        ),
        (
            "edit1",
            "Edit",
            r#"{"file_path":"crates/monocode-core/src/provider.rs"}"#,
            "",
            ToolState::Completed,
        ),
        (
            "cmd1",
            "Bash",
            r#"{"command":"python3 — <<'PY'"}"#,
            "",
            ToolState::Completed,
        ),
        (
            "cmd2",
            "Bash",
            r#"{"command":"cargo fmt --all"}"#,
            "error: let chains are only allowed in Rust 2024 or later",
            ToolState::Failed,
        ),
        (
            "edit2",
            "Edit",
            r#"{"file_path":"crates/monocode-core/src/provider.rs"}"#,
            "",
            ToolState::Completed,
        ),
        (
            "find",
            "Grep",
            r#"{"pattern":"fn simulate_click|fn bounds_for|debug_bounds"}"#,
            "",
            ToolState::Running,
        ),
    ] {
        t.begin("tool");
        t.rows.last_mut().unwrap().text = serde_json::to_string(&ToolCall {
            id: id.into(),
            name: name.into(),
            input: input.into(),
            output: output.into(),
            state,
            panel: None,
            display: None,
        })
        .unwrap();
    }
    t
}

/// Normal-session examples; no provider credentials or live turn are needed.
pub(crate) fn visuals() -> Transcript {
    let mut transcript = Transcript::default();
    transcript.begin("user");
    transcript
        .append("Show interactive examples for exploring data and three-dimensional geometry.");
    for (id, title, source) in [
        (
            "example-dashboard",
            "Throughput explorer",
            include_str!("../../../docs/native/examples/visual-dashboard.js"),
        ),
        (
            "example-geometry",
            "Geometry explorer",
            include_str!("../../../docs/native/examples/visual-scene.js"),
        ),
    ] {
        let document = monocode_core::visual::VisualDocument {
            id: id.into(),
            title: title.into(),
            source: source.into(),
        };
        transcript.begin("visual");
        transcript.append(&serde_json::to_string(&document).unwrap());
    }
    transcript.begin("assistant");
    transcript.append("The examples use a sample dataset. Their controls update locally, so you can explore them without starting an agent turn.");
    transcript
}

/// An answer that alternates prose with the visuals it placed, as an agent
/// writes one after publishing each visual with visual_show.
pub(crate) fn visual_story() -> Transcript {
    let mut transcript = Transcript::default();
    transcript.begin("user");
    transcript.append("How did deploys go this quarter? Anything I should worry about?");
    transcript.begin("assistant");
    transcript.append("I'll pull the deploy history and chart what changed.");
    let tool = |id: &str, name: &str, input: String| {
        let mut call = ToolCall::new(id, name);
        call.input = input;
        call.state = ToolState::Completed;
        serde_json::to_string(&call).unwrap()
    };
    transcript.begin("tool");
    transcript.append(&tool(
        "read-deploys",
        "Read",
        r#"{"file_path":"ops/deploys.json"}"#.into(),
    ));
    transcript.begin("tool");
    transcript.append(&tool(
        "count-deploys",
        "Bash",
        r#"{"command":"jq length ops/deploys.json"}"#.into(),
    ));
    for (id, title, source) in [
        (
            "deploys-weekly",
            "Deploys per week, last 12 weeks",
            r#"return () => visual.chart('bar', [{label: 'Deploys', values: [42, 48, 45, 55, 51, 58, 54, 63, 52, 61, 68, 74]}], {labels: ['W1','W2','W3','W4','W5','W6','W7','W8','W9','W10','W11','W12'], height: 200});"#,
        ),
        (
            "deploy-health",
            "Pipeline health this week",
            r#"return () => visual.grid([
  visual.stat('Success rate', '97.3%', {delta: '+1.2%', detail: 'vs last week', trend: [94, 95, 95.5, 94.8, 96, 96.4, 97.3]}),
  visual.stat('Median build', '6m 12s', {delta: '-48s', better: 'lower', detail: 'after the cache fix', trend: [8.1, 7.9, 7.6, 7.7, 6.9, 6.4, 6.2]}),
  visual.gauge('Runner capacity used', 82, {unit: '%', color: 'warning'}),
], {columns: 3, gap: 16});"#,
        ),
        (
            "failure-causes",
            "Where the 23 failed deploys came from",
            r#"return () => visual.donut([
  {label: 'Flaky e2e tests', value: 11},
  {label: 'Migrations', value: 6},
  {label: 'Config drift', value: 4},
  {label: 'Other', value: 2},
], {label: 'Failures', value: '23'});"#,
        ),
    ] {
        let document = monocode_core::visual::VisualDocument {
            id: id.into(),
            title: title.into(),
            source: source.into(),
        };
        let input = serde_json::json!({"title": title, "source": source}).to_string();
        transcript.begin("tool");
        transcript.append(&tool(
            &format!("check-{id}"),
            "monocode_visual:visual_check",
            input.clone(),
        ));
        transcript.begin("tool");
        transcript.append(&tool(
            &format!("show-{id}"),
            "monocode_visual:visual_show",
            input,
        ));
        transcript.begin("visual");
        transcript.append(&serde_json::to_string(&document).unwrap());
    }
    transcript.begin("assistant");
    transcript.append(
        "Deploys had a strong quarter: volume climbed steadily and finished at a record 74 in the last week.

![Deploys per week, last 12 weeks](visual:deploys-weekly)

The dips in weeks 3, 5 and 9 line up with the release freezes, so they aren't a reliability signal. What changed is the pipeline itself. The CI cache fix landed in week 10, and builds have been faster and steadier since:

![Pipeline health this week](visual:deploy-health)

The one thing to watch is runner capacity. At 82% you'll start queueing builds during the Monday rush, and the trend is still rising.

Failures are low overall, but they're concentrated:

![Where the 23 failed deploys came from](visual:failure-causes)

Nearly half come from flaky end-to-end tests rather than real regressions. Quarantining the five worst tests would remove most of that noise. I'd do that before adding runners.",
    );
    transcript
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_visual_examples_pass_the_real_runtime_and_persist_as_messages() {
        let rows = super::visuals().rows;
        let mut index = 0;
        let mut count = 0;
        while index < rows.len() {
            let end = (index + 1..rows.len())
                .find(|&next| rows[next].first)
                .unwrap_or(rows.len());
            if rows[index].role == "visual" {
                let source = rows[index..end]
                    .iter()
                    .map(|row| row.text.as_str())
                    .collect::<String>();
                let document: monocode_core::visual::VisualDocument =
                    serde_json::from_str(&source).unwrap();
                document.validate().unwrap();
                monocode_core::visual::check(&document.source).unwrap();
                count += 1;
            }
            index = end;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn the_story_answer_places_every_visual_it_publishes() {
        let rows = super::visual_story().rows;
        let answer: String = rows
            .iter()
            .skip_while(|row| row.role != "assistant")
            .map(|row| row.text.as_str())
            .collect();
        let mut placed = std::collections::HashSet::new();
        crate::transcript::placements(&answer, &mut placed);
        assert_eq!(placed.len(), 3);
        let mut index = 0;
        let mut published = 0;
        while index < rows.len() {
            let end = (index + 1..rows.len())
                .find(|&next| rows[next].first)
                .unwrap_or(rows.len());
            if rows[index].role == "visual" {
                let source: String = rows[index..end].iter().map(|r| r.text.as_str()).collect();
                let document: monocode_core::visual::VisualDocument =
                    serde_json::from_str(&source).unwrap();
                monocode_core::visual::check(&document.source).unwrap();
                assert!(placed.contains(&document.id));
                published += 1;
            }
            index = end;
        }
        assert_eq!(published, 3);
    }
}
