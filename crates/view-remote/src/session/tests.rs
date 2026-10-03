//! Ports of the RemoteSession.test.ts cases that check the pane rather than
//! the data hooks. The hook cases (polling, the outbox, optimistic turns,
//! configuration changes, worktrees, branches) belong to the engine's
//! remote package.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, Entity, TestAppContext, VisualTestContext, Window};
use monocode_core::block::TurnIntent;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::ComposerTurnOptions;
use monocode_core::transcript::fixtures::{note, user};
use monocode_core::{Attachment, HarnessId, Session};
use monocode_view_composer::composer::{ComposerEvent, ComposerHost, RemoteFeatures};
use monocode_view_transcript::transcript::TranscriptEvent;

use super::{
    FailedTurn, NoticeAction, RemoteComposerHost, RemoteMachineState, RemoteSessionEvent,
    RemoteSessionHost, RemoteSessionPane, RemoteSessionProps, RemoteSessionStatus, composer_props,
    transcript_config,
};
use crate::test_support::{click, draw, exists, mount};

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Submit {
        text: String,
        options: ComposerTurnOptions,
    },
    SaveDraft(String),
    Stop,
    Compact,
}

#[derive(Clone, Default)]
struct FakeSessionHost(Rc<RefCell<Vec<Call>>>);

impl RemoteSessionHost for FakeSessionHost {
    fn submit(
        &self,
        text: String,
        _attachments: Vec<Attachment>,
        options: ComposerTurnOptions,
        _: &mut Window,
        _: &mut App,
    ) -> bool {
        self.0.borrow_mut().push(Call::Submit { text, options });
        true
    }

    fn save_draft(&self, text: String, _: Vec<Attachment>, _: &mut Window, _: &mut App) -> bool {
        self.0.borrow_mut().push(Call::SaveDraft(text));
        true
    }

    fn stop(&self, _: &mut Window, _: &mut App) {
        self.0.borrow_mut().push(Call::Stop);
    }

    fn compact(&self, _: &mut Window, _: &mut App) -> bool {
        self.0.borrow_mut().push(Call::Compact);
        true
    }
}

fn host_session(blocks: Vec<monocode_core::Block>) -> Session {
    let mut session = Session::blank(
        "shell",
        HarnessId::Codex,
        "codex:gpt-test",
        "remote://env/home/me/repo",
    );
    session.title = "Remote work".into();
    session.branch = Some("main".into());
    session.blocks = blocks;
    session
}

fn props(session: Session) -> RemoteSessionProps {
    RemoteSessionProps {
        machine_name: "Home server".into(),
        environment_id: "env".into(),
        session: Some(Arc::new(session)),
        execution_cwd: "/home/me/repo".into(),
        online: true,
        features: RemoteFeatures {
            attachments: true,
            plan: true,
            draft: true,
        },
        started: true,
        allowed_model_harnesses: vec![HarnessId::Codex],
        animate: false,
        ..RemoteSessionProps::default()
    }
}

type Events = Rc<RefCell<Vec<RemoteSessionEvent>>>;

fn render(
    cx: &mut TestAppContext,
    props: RemoteSessionProps,
) -> (
    Entity<RemoteSessionPane>,
    &mut VisualTestContext,
    FakeSessionHost,
    Events,
) {
    let host = FakeSessionHost::default();
    let pane_host = host.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        cx.new(|cx| RemoteSessionPane::new(Rc::new(pane_host), props, None, window, cx))
    });
    let events: Events = Rc::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &RemoteSessionEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach()
    });
    (view, cx, host, events)
}

#[gpui::test]
fn asks_to_connect_the_machine_when_it_is_not_set_up_on_this_computer(cx: &mut TestAppContext) {
    let missing = RemoteSessionProps {
        machine: RemoteMachineState::NotConnected,
        ..props(host_session(Vec::new()))
    };
    let (view, cx, _host, events) = render(cx, missing);
    assert_eq!(
        view.read_with(cx, |view, _| view.props().machine.message()),
        Some("The machine for this project isn’t connected on this computer.")
    );
    assert!(exists(cx, "remote-placeholder"));
    assert!(!exists(cx, "composer:docked"));
    click(cx, "button:Manage machines");
    assert_eq!(*events.borrow(), vec![RemoteSessionEvent::ManageMachines]);
}

#[gpui::test]
fn waits_for_the_machine_list_without_offering_to_manage_machines(cx: &mut TestAppContext) {
    for state in [
        RemoteMachineState::Connecting,
        RemoteMachineState::MissingProject,
    ] {
        let waiting = RemoteSessionProps {
            machine: state,
            ..props(host_session(Vec::new()))
        };
        let (_view, cx, _host, _events) = render(cx, waiting);
        assert!(exists(cx, "remote-placeholder"));
        assert!(!exists(cx, "button:Manage machines"));
    }
}

#[gpui::test]
fn uses_the_normal_composer_with_the_host_branch_in_its_top_row(cx: &mut TestAppContext) {
    let (view, cx, _host, _events) = render(
        cx,
        props(host_session(vec![user("u1", "Hello"), note("a1", "Hi")])),
    );
    assert!(exists(cx, "composer:docked"));
    assert!(exists(cx, "remote-transcript"));
    let composer = view.read_with(cx, |view, cx| view.composer().read(cx).props().clone());
    assert_eq!(composer.branch.as_deref(), Some("main"));
    assert!(composer.remote_session);
    assert_eq!(
        composer.remote_features,
        Some(RemoteFeatures {
            attachments: true,
            plan: true,
            draft: true,
        })
    );
    assert_eq!(composer.cwd, "remote://env/home/me/repo");
    assert!(!composer.shell);
}

#[gpui::test]
fn keeps_an_unopened_remote_conversation_docked_while_its_transcript_loads(
    cx: &mut TestAppContext,
) {
    let loading = RemoteSessionProps {
        loading: true,
        ..props(host_session(Vec::new()))
    };
    let (_view, cx, _host, _events) = render(cx, loading);
    assert!(exists(cx, "composer:docked"));
    assert!(!exists(cx, "empty-session-title"));
    assert!(!exists(cx, "remote-transcript"));
}

#[gpui::test]
fn centers_the_composer_in_a_new_conversation(cx: &mut TestAppContext) {
    let fresh = RemoteSessionProps {
        started: false,
        ..props(host_session(Vec::new()))
    };
    assert_eq!(fresh.empty_title(), "What should we work on in repo?");
    let (view, cx, _host, _events) = render(cx, fresh);
    assert!(exists(cx, "composer:centered"));
    assert!(exists(cx, "empty-session-title"));
    assert!(view.read_with(cx, |view, cx| view.composer().read(cx).props().shell));
}

#[gpui::test]
fn keeps_a_new_session_on_its_selected_remote_provider(cx: &mut TestAppContext) {
    let mut session = host_session(Vec::new());
    session.harness = HarnessId::Cursor;
    let cursor = RemoteSessionProps {
        allowed_model_harnesses: vec![HarnessId::Codex, HarnessId::Cursor],
        started: false,
        ..props(session)
    };
    let composer = composer_props(&cursor);
    assert_eq!(composer.harness, HarnessId::Cursor);
    assert_eq!(
        composer.allowed_model_harnesses,
        Some(vec![HarnessId::Codex, HarnessId::Cursor])
    );
    let (view, cx, _host, _events) = render(cx, cursor);
    let shown = view.read_with(cx, |view, cx| view.composer().read(cx).props().harness);
    assert_eq!(shown, HarnessId::Cursor);
}

#[gpui::test]
fn opens_transcript_files_and_diffs_through_the_shared_remote_tabs(cx: &mut TestAppContext) {
    let (view, cx, _host, events) = render(
        cx,
        props(host_session(vec![user("u1", "Hello"), note("a1", "Hi")])),
    );
    let transcript = view.read_with(cx, |view, _| view.transcript().clone());
    transcript.update(cx, |_, cx| {
        cx.emit(TranscriptEvent::OpenFile {
            path: "src/app.ts".into(),
            line: Some(4),
        });
        cx.emit(TranscriptEvent::OpenDiff {
            path: "src/app.ts".into(),
        });
    });
    draw(cx);
    assert_eq!(
        *events.borrow(),
        vec![
            RemoteSessionEvent::OpenFile {
                path: "remote://env/home/me/repo/src/app.ts".into(),
                line: Some(4),
            },
            RemoteSessionEvent::OpenDiff {
                path: Some("remote://env/home/me/repo/src/app.ts".into()),
            },
        ]
    );
}

#[gpui::test]
fn routes_approvals_plans_and_drafts_to_the_host(cx: &mut TestAppContext) {
    let mut draft = user("d1", "Draft for later");
    draft.draft = Some(true);
    let (view, cx, host, events) = render(
        cx,
        props(host_session(vec![
            user("u1", "Hello"),
            note("a1", "Hi"),
            draft,
        ])),
    );
    assert!(view.read_with(cx, |view, cx| {
        view.transcript().read(cx).config().can_send_drafts
    }));
    // A draft hides the composer until it is sent or removed.
    assert!(!exists(cx, "composer:docked"));
    let transcript = view.read_with(cx, |view, _| view.transcript().clone());
    transcript.update(cx, |_, cx| {
        cx.emit(TranscriptEvent::Approval {
            request_id: 7,
            decision: ApprovalDecision::Allow,
        });
        cx.emit(TranscriptEvent::BuildPlan {
            block_id: "plan".into(),
        });
        cx.emit(TranscriptEvent::RemoveDraft {
            block_id: "d1".into(),
        });
        cx.emit(TranscriptEvent::SendDraft {
            block_id: "d1".into(),
        });
        cx.emit(TranscriptEvent::EditLastTurn);
    });
    draw(cx);
    assert_eq!(
        *events.borrow(),
        vec![
            RemoteSessionEvent::Approve {
                request_id: 7,
                decision: ApprovalDecision::Allow,
            },
            RemoteSessionEvent::BuildPlan {
                block_id: "plan".into(),
            },
            RemoteSessionEvent::RemoveDraft {
                block_id: "d1".into(),
            },
        ]
    );
    assert_eq!(
        *host.0.borrow(),
        vec![Call::Submit {
            text: "Draft for later".into(),
            options: ComposerTurnOptions {
                draft_block_id: Some("d1".into()),
                ..ComposerTurnOptions::default()
            },
        }]
    );
}

#[gpui::test]
fn sends_composer_turns_drafts_and_compaction_to_the_host(cx: &mut TestAppContext) {
    let host = FakeSessionHost::default();
    let adapter = RemoteComposerHost(Rc::new(host.clone()));
    let (_view, cx, _host, _events) = render(cx, props(host_session(Vec::new())));
    cx.update(|window, cx| {
        let plan = ComposerTurnOptions {
            intent: Some(TurnIntent::Plan),
            ..ComposerTurnOptions::default()
        };
        assert!(adapter.submit(
            monocode_view_composer::composer::ComposerSubmission {
                text: "Plan the change".into(),
                attachments: Vec::new(),
                options: plan,
                resend: None,
            },
            window,
            cx,
        ));
        assert!(adapter.save_draft("Later".into(), Vec::new(), window, cx));
        assert!(adapter.compact_context(window, cx));
        adapter.stop(window, cx);
        // Host sessions have no local skills or file mentions.
        assert!(adapter.mention_files("/home/me/repo", cx).is_empty());
    });
    let calls = host.0.borrow().clone();
    assert_eq!(calls.len(), 4);
    assert!(matches!(
        &calls[0],
        Call::Submit { text, options } if text == "Plan the change" && options.intent == Some(TurnIntent::Plan)
    ));
    assert_eq!(calls[1], Call::SaveDraft("Later".into()));
    assert_eq!(calls[2], Call::Compact);
    assert_eq!(calls[3], Call::Stop);
}

#[gpui::test]
fn applies_model_effort_and_permission_changes_through_the_owner(cx: &mut TestAppContext) {
    let (view, cx, _host, events) = render(cx, props(host_session(vec![user("u1", "Hello")])));
    let composer = view.read_with(cx, |view, _| view.composer().clone());
    let mut settings = monocode_core::ModelSettings::new();
    settings.insert("reasoningEffort".into(), "high".into());
    composer.update(cx, |_, cx| {
        cx.emit(ComposerEvent::ModelSettingsChange(settings.clone()));
        cx.emit(ComposerEvent::RuntimeModeChange(
            monocode_core::RuntimeMode::default(),
        ));
        cx.emit(ComposerEvent::OpenFile {
            path: "./README.md".into(),
            line: None,
        });
    });
    draw(cx);
    assert_eq!(
        *events.borrow(),
        vec![
            RemoteSessionEvent::ModelSettingsChange(settings),
            RemoteSessionEvent::RuntimeModeChange(monocode_core::RuntimeMode::default()),
            RemoteSessionEvent::OpenFile {
                path: "remote://env/home/me/repo/README.md".into(),
                line: None,
            },
        ]
    );
}

#[gpui::test]
fn shows_the_request_status_over_the_transcript(cx: &mut TestAppContext) {
    let failing = RemoteSessionProps {
        status: RemoteSessionStatus {
            failed_turn: Some(FailedTurn { draft: false }),
            error: "Machine is unreachable".into(),
            ..RemoteSessionStatus::default()
        },
        ..props(host_session(vec![user("u1", "Hello")]))
    };
    let (view, cx, _host, events) = render(cx, failing);
    assert!(exists(cx, "remote-notice:alert"));
    click(cx, "notice:Try again");
    assert_eq!(
        *events.borrow(),
        vec![RemoteSessionEvent::Notice(NoticeAction::TryAgain)]
    );
    // Offline, only Dismiss works.
    let mut offline = view.read_with(cx, |view, _| view.props().clone());
    offline.online = false;
    cx.update(|window, cx| view.update(cx, |view, cx| view.set_props(offline, window, cx)));
    draw(cx);
    events.borrow_mut().clear();
    click(cx, "notice:Try again");
    assert!(events.borrow().is_empty());
    // A pending request that is being sent shows nothing.
    let mut sending = view.read_with(cx, |view, _| view.props().clone());
    sending.status = RemoteSessionStatus {
        pending: true,
        sending: true,
        ..RemoteSessionStatus::default()
    };
    cx.update(|window, cx| view.update(cx, |view, cx| view.set_props(sending, window, cx)));
    draw(cx);
    assert!(!exists(cx, "remote-notice:alert"));
    assert!(!exists(cx, "remote-notice:status"));
}

#[gpui::test]
fn follows_new_host_snapshots(cx: &mut TestAppContext) {
    let (view, cx, _host, _events) = render(cx, props(host_session(vec![user("u1", "Hello")])));
    let mut next = view.read_with(cx, |view, _| view.props().clone());
    let mut session = host_session(vec![user("u1", "Hello"), note("a1", "Done")]);
    session.busy = Some(true);
    next.session = Some(Arc::new(session));
    cx.update(|window, cx| view.update(cx, |view, cx| view.set_props(next, window, cx)));
    draw(cx);
    let (blocks, busy) = view.read_with(cx, |view, cx| {
        (
            view.transcript()
                .read(cx)
                .session()
                .map(|session| session.blocks.len()),
            view.composer().read(cx).props().busy,
        )
    });
    assert_eq!(blocks, Some(2));
    assert!(busy);
    let config = transcript_config(&view.read_with(cx, |view, _| view.props().clone()));
    assert!(!config.can_edit_last_turn);
    assert!(config.approvals);
}
