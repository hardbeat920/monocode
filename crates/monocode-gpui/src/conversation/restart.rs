//! Continue a turn that was running when MonoCode last stopped. The launch
//! settled its rows and noted the interruption; this decides whether to go on.
use super::*;
use monocode_core::store::{InFlight, Interrupted};

pub(crate) const CONTINUE_PROMPT: &str = "Continue from where you left off.";

impl Conversation {
    pub(crate) fn resume_after_restart(
        &mut self,
        turn: InFlight,
        interrupted: Interrupted,
        cx: &mut Context<Self>,
    ) {
        if self.needs_history_restore() {
            self.defer_history_action(
                move |this, cx| this.resume_after_restart(turn, interrupted, cx),
                cx,
            );
            return;
        }
        // These states have their own recovery or need the user to decide.
        if self.busy
            || self.stopping
            || self.account_switching
            || self.managed
            || self.is_remote()
            || turn.orchestrator
            || self.settings.mono.is_some()
            || self.settings.usage_limit.is_some()
            || self.settings.pending_provider_switch.is_some()
            || self.settings.worktree_removed
            || (self.settings.new_worktree && self.settings.worktree_cwd.is_none())
            || !Path::new(self.execution_cwd()).is_dir()
        {
            return;
        }
        let resumable = self.summary.provider_id.is_some();
        let prompt = if resumable {
            CONTINUE_PROMPT.to_owned()
        } else if let Some(prompt) = interrupted.prompt.filter(|_| !interrupted.tool_activity) {
            // The provider never opened a session, so nothing ran. Send the
            // request again rather than ask a fresh session to "continue".
            prompt
        } else {
            return;
        };
        let draft = (
            self.settings.plan,
            self.settings.operator,
            self.settings.orchestrator,
            std::mem::take(&mut self.attachments),
        );
        self.settings.plan = turn.plan;
        self.settings.operator = turn.operator;
        self.settings.orchestrator = false;
        self.settings.access = turn.access;
        self.settings.restart_continuation = resumable;
        if !resumable {
            self.attachments = interrupted.attachments;
        }
        self.launch_prompt(prompt, cx);
        // The continuation owns the interrupted turn's modes; the composer
        // keeps the draft the user had.
        self.settings.plan = draft.0;
        self.settings.operator = draft.1;
        self.settings.orchestrator = draft.2;
        self.settings.restart_continuation = false;
        self.attachments.extend(draft.3);
        cx.emit(SessionChanged);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_core::{agent::AccessMode, store::Store};
    use std::sync::Mutex;

    fn conversation(cwd: &Path, cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", cwd.to_str().unwrap()).unwrap();
        // Fails after saving the turn, without launching a real provider.
        summary.provider = "fixture".into();
        summary.provider_id = Some("saved-thread".into());
        cx.update(|cx| {
            crate::input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        })
    }

    fn plan_turn() -> InFlight {
        InFlight {
            plan: true,
            access: AccessMode::ReadOnly,
            resume: true,
            ..Default::default()
        }
    }

    #[gpui::test]
    fn continues_under_the_interrupted_turns_policy_and_keeps_the_draft_mode(
        cx: &mut TestAppContext,
    ) {
        let dir = std::env::temp_dir();
        let window = conversation(&dir, cx);
        window
            .update(cx, |view, _, cx| {
                view.settings.operator = true;
                view.resume_after_restart(plan_turn(), Interrupted::default(), cx);
                assert!(view.busy, "the continuation launched");
                assert!(
                    view.app_read_only,
                    "it runs in the interrupted turn's plan mode"
                );
                assert_eq!(view.settings.access, AccessMode::ReadOnly);
                assert!(view.settings.operator, "the composer keeps its own mode");
                assert!(!view.settings.plan);
                assert!(!view.settings.restart_continuation);
                let rows = view.transcript.read(cx).source_rows().to_vec();
                assert!(rows.iter().any(|row| row.text == CONTINUE_PROMPT));
            })
            .unwrap();
    }

    #[gpui::test]
    fn chats_with_their_own_recovery_only_keep_the_notice(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir();
        for case in 0..4 {
            let window = conversation(&dir, cx);
            window
                .update(cx, |view, _, cx| {
                    let mut turn = plan_turn();
                    match case {
                        0 => view.managed = true,
                        1 => turn.orchestrator = true,
                        2 => {
                            view.settings.usage_limit = Some(Default::default());
                        }
                        _ => {
                            // No provider session and tools already ran.
                            view.summary.provider_id = None;
                        }
                    }
                    let interrupted = Interrupted {
                        prompt: Some("Fix it".into()),
                        attachments: Vec::new(),
                        tool_activity: true,
                    };
                    view.resume_after_restart(turn, interrupted, cx);
                    assert!(!view.busy, "case {case} must not launch");
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn a_turn_without_a_provider_session_resends_its_request(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir();
        let window = conversation(&dir, cx);
        window
            .update(cx, |view, _, cx| {
                view.summary.provider_id = None;
                let interrupted = Interrupted {
                    prompt: Some("Fix the build".into()),
                    attachments: Vec::new(),
                    tool_activity: false,
                };
                view.resume_after_restart(plan_turn(), interrupted, cx);
                assert!(view.busy);
                let rows = view.transcript.read(cx).source_rows().to_vec();
                assert!(rows.iter().any(|row| row.text == "Fix the build"));
                assert!(!rows.iter().any(|row| row.text == CONTINUE_PROMPT));
            })
            .unwrap();
    }
}
