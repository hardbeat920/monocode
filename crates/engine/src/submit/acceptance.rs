//! Port of src/app/model/submissionAcceptance.ts and managedSubmission.ts:
//! when a submitted user turn counts as accepted, and the one terminal
//! callback a managed caller (an automation, the orchestrator, the app CLI)
//! gets.
//!
//! `SubmissionAcceptance` was `boolean | Promise<boolean>`. A deferred
//! acceptance here is a receiver: the work runs on its own task, so dropping
//! the acceptance does not cancel the submission, as an unawaited promise
//! did not.

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{App, AsyncApp};
use monocode_core::paths::display_path;
use serde::{Deserialize, Serialize};

/// `ControlOutcome.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlStatus {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// `ControlOutcome`: how a managed turn ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlOutcome {
    pub status: ControlStatus,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ControlOutcome {
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: ControlStatus::Failed,
            text: String::new(),
            error: Some(error.into()),
        }
    }
}

/// `onSettled`.
pub type OnSettled = Rc<dyn Fn(ControlOutcome, &mut App)>;

/// A deferred submission that failed. `project_not_found` marks the
/// permanent `ProjectNotFoundError` that callers deciding whether to retry
/// look for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitError {
    pub message: String,
    pub project_not_found: bool,
}

impl SubmitError {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            project_not_found: false,
        }
    }

    /// `new ProjectNotFoundError(cwd)`.
    pub fn project_not_found(cwd: &str) -> Self {
        Self {
            message: format!(
                "Project folder not found: {}. Reopen the folder to reconnect it.",
                display_path(cwd, None)
            ),
            project_not_found: true,
        }
    }
}

/// `SubmissionAcceptance`: resolves when the user turn is accepted, not when
/// the agent finishes.
pub enum SubmissionAcceptance {
    Ready(bool),
    Deferred(oneshot::Receiver<Result<bool, SubmitError>>),
}

impl std::fmt::Debug for SubmissionAcceptance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmissionAcceptance::Ready(accepted) => write!(f, "Ready({accepted})"),
            SubmissionAcceptance::Deferred(_) => write!(f, "Deferred"),
        }
    }
}

impl SubmissionAcceptance {
    /// A deferred acceptance and the sender that settles it.
    pub fn deferred() -> (oneshot::Sender<Result<bool, SubmitError>>, Self) {
        let (sender, receiver) = oneshot::channel();
        (sender, SubmissionAcceptance::Deferred(receiver))
    }

    /// `onSubmit`'s answer: interactive callers clear their composer when the
    /// turn is accepted or still being checked. Deferred errors have already
    /// been shown in the transcript.
    pub fn accepted_now(&self) -> bool {
        match self {
            SubmissionAcceptance::Ready(accepted) => *accepted,
            SubmissionAcceptance::Deferred(_) => true,
        }
    }

    /// Wait for the answer. A deferred submission whose task went away reads
    /// as not accepted.
    pub async fn resolve(self) -> Result<bool, SubmitError> {
        match self {
            SubmissionAcceptance::Ready(accepted) => Ok(accepted),
            SubmissionAcceptance::Deferred(receiver) => receiver.await.unwrap_or(Ok(false)),
        }
    }
}

/// `ProjectLocationSync`: where the project is now, and whether it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLocationSync {
    pub path: String,
    pub identity: String,
    pub moved: bool,
}

/// `submitAfterProjectSync`: wait for the project folder check, rebase a
/// moved project's state, then submit. Every failure goes to `on_error`.
/// A missing project is returned as an error so callers can tell it from a
/// transient failure; anything else reads as not accepted.
pub async fn submit_after_project_sync<S, A, F>(
    cwd: &str,
    sync: S,
    apply_location_change: impl FnOnce(String, String) -> A,
    submit: impl FnOnce() -> F,
    on_error: impl FnOnce(&SubmitError),
) -> Result<bool, SubmitError>
where
    S: Future<Output = Result<Option<ProjectLocationSync>, String>>,
    A: Future<Output = Result<(), String>>,
    F: Future<Output = Result<bool, SubmitError>>,
{
    let attempt = async {
        let location = sync.await.map_err(SubmitError::message)?;
        let Some(location) = location else {
            return Err(SubmitError::project_not_found(cwd));
        };
        if location.moved {
            apply_location_change(cwd.to_string(), location.path)
                .await
                .map_err(SubmitError::message)?;
        }
        submit().await
    };
    match attempt.await {
        Ok(accepted) => Ok(accepted),
        Err(error) => {
            on_error(&error);
            // Preserve permanent failures for callers that decide whether to retry.
            if error.project_not_found {
                Err(error)
            } else {
                Ok(false)
            }
        }
    }
}

/// `submitWithSettlement`: await acceptance and guarantee one terminal
/// callback, including a rejection before a turn exists. Successful
/// acceptance does not wait for the agent.
pub async fn submit_with_settlement(
    cx: &mut AsyncApp,
    submit: impl FnOnce(OnSettled, &mut App) -> SubmissionAcceptance,
    on_settled: OnSettled,
    rejection_message: &str,
) -> bool {
    let settled = Rc::new(Cell::new(false));
    let settle: OnSettled = {
        let settled = settled.clone();
        Rc::new(move |outcome, cx| {
            if settled.replace(true) {
                return;
            }
            on_settled(outcome, cx);
        })
    };
    let acceptance = cx.update(|cx| submit(settle.clone(), cx));
    match acceptance.resolve().await {
        Err(error) => {
            cx.update(|cx| settle(ControlOutcome::failed(error.message), cx));
            false
        }
        Ok(false) => {
            cx.update(|cx| settle(ControlOutcome::failed(rejection_message), cx));
            false
        }
        Ok(true) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use futures::FutureExt;
    use gpui::TestAppContext;

    fn location(path: &str, moved: bool) -> ProjectLocationSync {
        ProjectLocationSync {
            path: path.into(),
            identity: "repo".into(),
            moved,
        }
    }

    // submissionAcceptance.test.ts
    #[test]
    fn waits_for_a_moved_projects_state_to_be_rebased_before_submitting() {
        let applied: RefCell<Vec<(String, String)>> = RefCell::default();
        let submitted = Cell::new(0);
        let (resolve_move, moved) = oneshot::channel::<()>();
        let accepted = submit_after_project_sync(
            "/old-path",
            async { Ok(Some(location("/new-path", true))) },
            |from, to| {
                applied.borrow_mut().push((from, to));
                moved.map(|_| Ok(()))
            },
            || {
                submitted.set(submitted.get() + 1);
                async { Ok(true) }
            },
            |_| {},
        );
        let mut accepted = Box::pin(accepted);
        assert!(
            futures::executor::block_on(futures::future::poll_immediate(&mut accepted)).is_none()
        );
        assert_eq!(
            *applied.borrow(),
            [("/old-path".to_string(), "/new-path".to_string())]
        );
        assert_eq!(submitted.get(), 0);
        resolve_move.send(()).unwrap();
        assert_eq!(futures::executor::block_on(accepted), Ok(true));
        assert_eq!(submitted.get(), 1);
    }

    #[test]
    fn propagates_an_asynchronous_deferred_rejection_instead_of_treating_its_promise_as_acceptance()
    {
        let accepted = futures::executor::block_on(submit_after_project_sync(
            "/repo",
            async { Ok(Some(location("/repo", false))) },
            |_, _| async { Ok(()) },
            || async { Ok(false) },
            |_| {},
        ));
        assert_eq!(accepted, Ok(false));
    }

    #[test]
    fn rejects_acceptance_if_applying_a_moved_project_fails() {
        let submitted = Cell::new(false);
        let errors: RefCell<Vec<SubmitError>> = RefCell::default();
        let accepted = futures::executor::block_on(submit_after_project_sync(
            "/old-path",
            async { Ok(Some(location("/new-path", true))) },
            |_, _| async { Err("failed to rebase project state".to_string()) },
            || {
                submitted.set(true);
                async { Ok(true) }
            },
            |error| errors.borrow_mut().push(error.clone()),
        ));
        assert_eq!(accepted, Ok(false));
        assert!(!submitted.get());
        assert_eq!(errors.borrow()[0].message, "failed to rebase project state");
    }

    // managedSubmission.test.ts
    fn recorder() -> (Rc<RefCell<Vec<ControlOutcome>>>, OnSettled) {
        let outcomes: Rc<RefCell<Vec<ControlOutcome>>> = Rc::default();
        let sink = outcomes.clone();
        (
            outcomes,
            Rc::new(move |outcome, _| sink.borrow_mut().push(outcome)),
        )
    }

    #[gpui::test]
    async fn settles_a_deferred_false_result_and_releases_the_automation_reservation(
        cx: &mut TestAppContext,
    ) {
        let (outcomes, on_settled) = recorder();
        let (resolve, acceptance) = SubmissionAcceptance::deferred();
        let mut async_cx = cx.to_async();
        let task = cx.foreground_executor().spawn(async move {
            submit_with_settlement(
                &mut async_cx,
                |_, _| acceptance,
                on_settled,
                "Run could not start",
            )
            .await
        });
        cx.run_until_parked();
        assert!(outcomes.borrow().is_empty());
        resolve.send(Ok(false)).unwrap();
        assert!(!task.await);
        assert_eq!(
            *outcomes.borrow(),
            [ControlOutcome::failed("Run could not start")]
        );
    }

    #[gpui::test]
    async fn calls_the_completion_exactly_once_for_a_deferred_false_and_a_late_callback(
        cx: &mut TestAppContext,
    ) {
        let (outcomes, on_settled) = recorder();
        let captured: Rc<RefCell<Option<OnSettled>>> = Rc::default();
        let keep = captured.clone();
        let mut async_cx = cx.to_async();
        let accepted = submit_with_settlement(
            &mut async_cx,
            move |settle, _| {
                *keep.borrow_mut() = Some(settle);
                SubmissionAcceptance::Ready(false)
            },
            on_settled,
            "Turn could not start",
        )
        .await;
        assert!(!accepted);
        let settle = captured.borrow().clone().unwrap();
        cx.update(|cx| {
            settle(ControlOutcome::failed("late failure"), cx);
            settle(
                ControlOutcome {
                    status: ControlStatus::Completed,
                    text: "late response".into(),
                    error: None,
                },
                cx,
            );
        });
        assert_eq!(
            *outcomes.borrow(),
            [ControlOutcome::failed("Turn could not start")]
        );
    }

    #[gpui::test]
    async fn does_not_double_settle_when_a_sync_failure_reports_through_both_paths(
        cx: &mut TestAppContext,
    ) {
        for missing in [false, true] {
            let (outcomes, on_settled) = recorder();
            let submitted = Rc::new(Cell::new(false));
            let mut async_cx = cx.to_async();
            let flag = submitted.clone();
            let accepted = submit_with_settlement(
                &mut async_cx,
                move |settle, cx| {
                    let (sender, acceptance) = SubmissionAcceptance::deferred();
                    cx.spawn(async move |cx| {
                        let result = submit_after_project_sync(
                            "/repo",
                            async move {
                                if missing {
                                    Ok(None)
                                } else {
                                    Err("disk unavailable".to_string())
                                }
                            },
                            |_, _| async { Ok(()) },
                            || {
                                flag.set(true);
                                async { Ok(true) }
                            },
                            |error| {
                                cx.update(|cx| {
                                    settle(ControlOutcome::failed(error.message.clone()), cx)
                                })
                            },
                        )
                        .await;
                        let _ = sender.send(result);
                    })
                    .detach();
                    acceptance
                },
                on_settled,
                "Run could not start",
            )
            .await;
            assert!(!accepted);
            assert!(!submitted.get());
            assert_eq!(outcomes.borrow().len(), 1);
            let expected = if missing {
                SubmitError::project_not_found("/repo").message
            } else {
                "disk unavailable".to_string()
            };
            assert_eq!(
                outcomes.borrow()[0].error.as_deref(),
                Some(expected.as_str())
            );
        }
    }

    #[gpui::test]
    async fn settles_a_submission_that_fails_before_calling_on_settled(cx: &mut TestAppContext) {
        let (outcomes, on_settled) = recorder();
        let mut async_cx = cx.to_async();
        let accepted = submit_with_settlement(
            &mut async_cx,
            |_, _| {
                let (sender, acceptance) = SubmissionAcceptance::deferred();
                sender
                    .send(Err(SubmitError::message("submission failed")))
                    .unwrap();
                acceptance
            },
            on_settled,
            "Turn could not start",
        )
        .await;
        assert!(!accepted);
        assert_eq!(
            *outcomes.borrow(),
            [ControlOutcome::failed("submission failed")]
        );
    }

    #[gpui::test]
    async fn returns_accepted_without_waiting_for_the_agent_then_settles_only_once(
        cx: &mut TestAppContext,
    ) {
        let (outcomes, on_settled) = recorder();
        let captured: Rc<RefCell<Option<OnSettled>>> = Rc::default();
        let keep = captured.clone();
        let mut async_cx = cx.to_async();
        let accepted = submit_with_settlement(
            &mut async_cx,
            move |settle, _| {
                *keep.borrow_mut() = Some(settle);
                SubmissionAcceptance::Ready(true)
            },
            on_settled,
            "Run could not start",
        )
        .await;
        assert!(accepted);
        assert!(outcomes.borrow().is_empty());
        let settle = captured.borrow().clone().unwrap();
        cx.update(|cx| {
            settle(
                ControlOutcome {
                    status: ControlStatus::Completed,
                    text: "done".into(),
                    error: None,
                },
                cx,
            );
            settle(
                ControlOutcome {
                    status: ControlStatus::Cancelled,
                    text: String::new(),
                    error: None,
                },
                cx,
            );
        });
        assert_eq!(outcomes.borrow().len(), 1);
        assert_eq!(outcomes.borrow()[0].text, "done");
    }
}
