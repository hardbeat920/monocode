//! Port of src/features/orchestration/model/orchestrationSummary.ts. The
//! runtime already ports `summarizeOrchestration` over its `LiveRun` shape;
//! this module builds that shape from a run and adds the task labels.

use monocode_core::Session;

use super::state::OrchestrationRun;
use crate::runtime::session_history::{self, LiveRun, LiveRunTask};
use crate::runtime::session_store::{OrchestrationSummary, OrchestrationSummaryTask};

/// The parts of a run history reads.
pub fn live_run(run: &OrchestrationRun) -> LiveRun {
    LiveRun {
        lead_id: run.lead_id.clone(),
        status: run.status.as_str().to_string(),
        tasks: run
            .tasks
            .iter()
            .map(|task| LiveRunTask {
                session_id: task.session_id.clone(),
                title: task.title.clone(),
                harness: task.harness,
                model: task.model.clone(),
                status: task.status.as_str().to_string(),
            })
            .collect(),
    }
}

/// `summarizeOrchestration`: a small history projection that never includes
/// prompts, results, or credentials.
pub fn summarize_orchestration(
    run: &OrchestrationRun,
    sessions: &[Session],
) -> OrchestrationSummary {
    session_history::summarize_orchestration(&live_run(run), sessions)
}

/// `orchestrationTaskLabel`.
pub fn orchestration_task_label(
    task: &OrchestrationSummaryTask,
    summary: &OrchestrationSummary,
) -> &'static str {
    if task.needs_input == Some(true) {
        return "Needs input";
    }
    if summary.live != Some(true)
        && matches!(task.status.as_str(), "running" | "cancelling" | "queued")
    {
        return "Saved";
    }
    if summary.status == "paused" && task.status == "queued" {
        return "Paused";
    }
    match task.status.as_str() {
        "queued" => "Queued",
        "running" => "Working",
        "cancelling" => "Stopping",
        "completed" => "Done",
        "failed" => "Failed",
        "blocked" => "Needs review",
        "interrupted" => "Interrupted",
        "cancelled" => "Cancelled",
        // TODO(port): the TypeScript table returned undefined for an unknown
        // status; an empty label keeps that.
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::state::tests::{run, task};
    use crate::orchestration::state::{RunStatus, TaskStatus};
    use monocode_core::block::{BlockApproval, BlockRole};
    use monocode_core::{Block, Extra};

    #[test]
    fn labels_tasks_from_live_state_and_saved_history() {
        let mut queued = task("q");
        queued.status = TaskStatus::Queued;
        let mut paused = run(vec![task("a"), queued]);
        paused.status = RunStatus::Paused;
        let mut worker = Session::blank("a", monocode_core::HarnessId::Claude, "m", "/repo");
        worker.blocks.push(Block {
            approval: Some(BlockApproval {
                request_id: 1,
                decided: None,
                extra: Extra::new(),
            }),
            ..Block::new("ask", BlockRole::Approval, "rm")
        });
        let summary = summarize_orchestration(&paused, &[worker]);
        assert_eq!(summary.status, "paused");
        assert_eq!(
            orchestration_task_label(&summary.tasks[0], &summary),
            "Needs input"
        );
        assert_eq!(
            orchestration_task_label(&summary.tasks[1], &summary),
            "Paused"
        );
        let saved = OrchestrationSummary {
            live: None,
            ..summary.clone()
        };
        assert_eq!(orchestration_task_label(&saved.tasks[1], &saved), "Saved");
        let mut done = summary.tasks[1].clone();
        done.status = "completed".into();
        assert_eq!(orchestration_task_label(&done, &summary), "Done");
    }
}
