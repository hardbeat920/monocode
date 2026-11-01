//! A lead's Orchestrator cards as a paired phone shows them
//! (mobile/src/features/workspace/types.ts `OrchestrationRun`). The phone
//! mirrors the desktop card: the same phases, buttons and assignment tones, so
//! what it may do is decided here, not on the phone.
use crate::transcript::orchestration::{Action, Phase, RunView, TaskView, actions, headline};
use monocode_core::orchestration::State;
use serde_json::{Value, json};

/// The phone's prompt, as the desktop card's open row shows it.
const PROMPT_CHARS: usize = 2_400;

fn phase_id(phase: &Phase) -> &'static str {
    match phase {
        Phase::Review { .. } => "review",
        Phase::Running => "running",
        Phase::Paused => "paused",
        Phase::Stopped => "stopped",
        Phase::Finished { .. } => "finished",
        Phase::Inactive => "inactive",
    }
}

fn action_id(action: &Action) -> &'static str {
    match action {
        Action::Start => "start",
        Action::Dismiss => "dismiss",
        Action::Remove(_) => "remove",
        Action::Team(_) => "team",
        Action::Stop => "stop",
        Action::Resume => "resume",
        Action::Retry(_) => "retry",
        Action::SendReport => "sendReport",
    }
}

/// A phone's button press, with the assignment it names for remove and retry.
pub(crate) fn action(name: &str, task: Option<&str>, index: Option<usize>) -> Option<Action> {
    Some(match name {
        "start" => Action::Start,
        "dismiss" => Action::Dismiss,
        "remove" => Action::Remove(task?.to_owned()),
        "stop" => Action::Stop,
        "resume" => Action::Resume,
        "retry" => Action::Retry(index?),
        "sendReport" => Action::SendReport,
        _ => return None,
    })
}

/// The tone's key on the phone, which picks the colour; the label is the desktop's.
fn tone(task: &TaskView) -> &'static str {
    if task.needs_input {
        return "input";
    }
    match task.state {
        State::Pending => "queued",
        State::Starting => "starting",
        State::Running => "working",
        State::Done => "done",
        State::Failed => "failed",
        State::Stopped => "stopped",
        State::Blocked => "blocked",
        State::Interrupted => "interrupted",
    }
}

/// Only the latest run and a plan under review can still be acted on.
pub(crate) fn current(views: &[(Option<String>, RunView)]) -> Vec<&(Option<String>, RunView)> {
    views
        .iter()
        .filter(|(_, run)| run.phase != Phase::Inactive)
        .collect()
}

/// One card. `session` names a worker session as the phone knows it.
pub(crate) fn run(
    call: Option<&str>,
    run: &RunView,
    session: impl Fn(i64) -> Option<String>,
) -> Value {
    let reviewing = matches!(run.phase, Phase::Review { .. });
    let live = run.phase != Phase::Inactive;
    let (buttons, reason) = actions(&run.phase);
    let tasks: Vec<Value> = run
        .plan
        .tasks
        .iter()
        .zip(&run.tasks)
        .enumerate()
        .map(|(ix, (task, state))| {
            let (_, label) = crate::transcript::orchestration::tone_of(state);
            let mut value = json!({
                "id": task.id,
                "title": task.title,
                "prompt": monocode_core::response::limit_middle(&task.prompt, PROMPT_CHARS),
                "files": task.files,
                "dependsOn": task.depends_on,
                "tone": tone(state),
                "label": label,
                "harness": state.harness,
                "model": state.model,
                "steps": state.steps,
                "outsideScope": state.outside_scope,
                "report": state.report,
                "canRetry": live && !reviewing && matches!(state.state, State::Failed | State::Stopped | State::Interrupted),
                "canRemove": reviewing && run.plan.tasks.len() > 1,
                "index": ix,
            });
            if let Some(note) = &state.note {
                value["note"] = json!(note);
            }
            if let Some(id) = state.session.and_then(&session) {
                value["sessionId"] = json!(id);
            }
            value
        })
        .collect();
    let mut value = json!({
        "call": call.unwrap_or_default(),
        "title": run.plan.title,
        "summary": monocode_core::response::limit(&run.plan.summary, 600),
        "phase": phase_id(&run.phase),
        "headline": headline(run),
        "actions": buttons
            .into_iter()
            .map(|(_, label, primary, disabled, action)| json!({
                "action": action_id(&action),
                "label": label,
                "primary": primary,
                "disabled": disabled,
            }))
            .collect::<Vec<_>>(),
        "tasks": tasks,
    });
    if let Some(reason) = reason {
        value["reason"] = json!(reason);
    }
    value
}

/// What a lead's catalog entry carries of its newest card: enough for its row
/// and the session's strip without the prompts and reports.
pub(crate) fn summary(run: &RunView) -> Value {
    json!({
        "title": run.plan.title,
        "phase": phase_id(&run.phase),
        "headline": headline(run),
        "tones": run.tasks.iter().map(tone).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::orchestration::{Plan, Task};

    fn plan() -> Plan {
        let task = |id: &str, depends_on: Vec<String>| Task {
            id: id.into(),
            title: format!("Assignment {id}"),
            prompt: "Do it and report.".into(),
            files: vec!["src".into()],
            depends_on,
            provider: None,
            model: None,
            difficulty: None,
        };
        Plan {
            title: "Split settings".into(),
            summary: "Two moves, then tests.".into(),
            tasks: vec![task("a", Vec::new()), task("b", vec!["a".into()])],
        }
    }

    #[test]
    fn a_plan_under_review_offers_start_and_removal_but_no_retry() {
        let view = RunView::review(
            plan(),
            Phase::Review { blocked: None },
            "claude",
            "Opus",
            None,
        );
        let value = run(Some("call-1"), &view, |_| None);
        assert_eq!(value["phase"], "review");
        assert_eq!(value["call"], "call-1");
        assert_eq!(value["actions"][0]["action"], "start");
        assert_eq!(value["actions"][0]["disabled"], false);
        assert_eq!(value["tasks"][0]["canRemove"], true);
        assert_eq!(value["tasks"][0]["canRetry"], false);
        assert_eq!(value["tasks"][1]["note"], "after a");
        assert_eq!(value["tasks"][1]["dependsOn"], json!(["a"]));
    }

    #[test]
    fn a_running_card_names_its_workers_and_who_needs_input() {
        let mut view = RunView::review(plan(), Phase::Running, "claude", "Opus", None);
        view.tasks[0].state = State::Running;
        view.tasks[0].needs_input = true;
        view.tasks[0].session = Some(7);
        view.tasks[1].state = State::Failed;
        let value = run(Some("call-1"), &view, |id| Some(format!("s{id}")));
        assert_eq!(value["tasks"][0]["tone"], "input");
        assert_eq!(value["tasks"][0]["label"], "Needs input");
        assert_eq!(value["tasks"][0]["sessionId"], "s7");
        assert_eq!(value["tasks"][1]["canRetry"], true);
        assert_eq!(value["actions"][0]["action"], "stop");
        assert_eq!(summary(&view)["tones"], json!(["input", "failed"]));
    }

    #[test]
    fn phone_actions_need_the_assignment_they_name() {
        assert_eq!(
            action("remove", Some("a"), None),
            Some(Action::Remove("a".into()))
        );
        assert_eq!(action("remove", None, None), None);
        assert_eq!(action("retry", None, Some(1)), Some(Action::Retry(1)));
        assert_eq!(action("retry", None, None), None);
        assert_eq!(action("launch", None, None), None);
    }
}
