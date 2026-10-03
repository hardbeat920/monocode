//! Port of src/features/sessions/model/sessionRemoval.test.ts. `FakeBackend`
//! stands in for the mocked `invoke`.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::TestAppContext;
use monocode_core::block::Block;
use monocode_core::session::QueuedMessage;
use monocode_layout::layout::{
    OpenEditorTabOptions, leaf_ids, new_file_tab, new_tab, new_terminal_file, open_editor_tab,
    open_terminal_tab,
};

use super::*;
use crate::runtime::testing::{FakeBackend, init_test_engine};

const CWD: &str = "/tmp/project";

fn session(id: &str) -> Session {
    Session::blank(id, HarnessId::Cursor, "cursor:auto", CWD)
}

type ConfirmFn = Box<dyn FnMut(&Rc<RefCell<RemovalWorkspace>>, &mut App) -> Task<bool>>;
type StopFn = Box<dyn FnMut(&Rc<RefCell<RemovalWorkspace>>)>;

/// The TypeScript fixture's state and adapters.
struct Adapter {
    state: Rc<RefCell<RemovalWorkspace>>,
    confirm: RefCell<ConfirmFn>,
    on_stop: RefCell<StopFn>,
    stops: Cell<usize>,
    commits: RefCell<Vec<SessionWorkspaceRemoval>>,
    created: Cell<usize>,
}

impl RemovalAdapter for Adapter {
    fn snapshot(&self, _cx: &App) -> RemovalWorkspace {
        self.state.borrow().clone()
    }

    fn apply(&self, change: WorkspaceChange, _cx: &mut App) {
        match change {
            WorkspaceChange::OrchestrationReleased { .. } => {}
            WorkspaceChange::Removed { removal, .. } => {
                let mut state = self.state.borrow_mut();
                state.tabs = removal.tabs.clone();
                state.sessions = removal.sessions.clone();
                state.active_tab_id = removal.active_tab_id.clone();
                self.commits.borrow_mut().push(removal);
            }
            WorkspaceChange::Stopped(stopped) => {
                let mut state = self.state.borrow_mut();
                for session in &mut state.sessions {
                    if session.id == stopped.id {
                        *session = stopped.clone();
                    }
                }
            }
        }
    }

    fn confirm(
        &self,
        _tabs: Vec<WorkspaceTab>,
        _mode: SessionRemovalMode,
        cx: &mut App,
    ) -> Task<bool> {
        (self.confirm.borrow_mut())(&self.state, cx)
    }

    fn stop(&self, _session_id: &str, _cx: &mut App) -> Task<()> {
        self.stops.set(self.stops.get() + 1);
        (self.on_stop.borrow_mut())(&self.state);
        Task::ready(())
    }

    fn create_session(&self, seed: &ReplacementSeed, _cx: &App) -> Session {
        self.created.set(self.created.get() + 1);
        let mut session = Session::blank(
            uuid::Uuid::new_v4().to_string(),
            seed.harness.unwrap_or(HarnessId::Cursor),
            seed.model.clone().unwrap_or_default(),
            &seed.cwd,
        );
        if let Some(mode) = seed.runtime_mode {
            session.runtime_mode = mode;
        }
        session
    }
}

struct Fixture {
    closing: Session,
    other: Session,
    adapter: Rc<Adapter>,
    backend: Arc<FakeBackend>,
    mode: SessionRemovalMode,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, mode: SessionRemovalMode) -> Self {
        let backend = init_test_engine(cx);
        let mut closing = session("closing");
        closing.busy = Some(true);
        closing.blocks = vec![
            Block::new("user", BlockRole::User, "hello"),
            Block {
                streaming: Some(true),
                ..Block::new("answer", BlockRole::Assistant, "partial")
            },
        ];
        closing.queued_messages = Some(vec![QueuedMessage {
            id: "queued".into(),
            text: "next".into(),
            attachments: Vec::new(),
            note_card: None,
            handoff_card: None,
            intent: None,
        }]);
        closing.queue_status = Some(MessageQueueStatus::Active);
        let other = session("other");
        let tabs = vec![new_tab(&closing.id), new_tab(&other.id)];
        let active_tab_id = tabs[0].id.clone();
        let state = RemovalWorkspace {
            tabs,
            sessions: vec![closing.clone(), other.clone()],
            active_tab_id,
            dirty_files: HashSet::new(),
        };
        let adapter = Rc::new(Adapter {
            state: Rc::new(RefCell::new(state)),
            confirm: RefCell::new(Box::new(|_, _| Task::ready(true))),
            on_stop: RefCell::new(Box::new(|_| {})),
            stops: Cell::new(0),
            commits: RefCell::new(Vec::new()),
            created: Cell::new(0),
        });
        Self {
            closing,
            other,
            adapter,
            backend,
            mode,
        }
    }

    fn read(&self) -> RemovalWorkspace {
        self.adapter.state.borrow().clone()
    }

    fn write(&self, update: impl FnOnce(&mut RemovalWorkspace)) {
        update(&mut self.adapter.state.borrow_mut());
    }

    fn start(&self, cx: &mut TestAppContext) -> Task<Result<bool, String>> {
        let remover = create_session_remover(SessionRemovalOptions {
            mode: self.mode,
            scope: WorkspaceTabCloseScope::Project,
            replacement: ReplacementSeed {
                cwd: CWD.into(),
                harness: Some(HarnessId::Cursor),
                ..Default::default()
            },
            adapter: self.adapter.clone(),
        });
        let id = self.closing.id.clone();
        cx.update(|cx| remover.remove(&id, cx))
    }

    fn run(&self, cx: &mut TestAppContext) -> Result<bool, String> {
        let task = self.start(cx);
        cx.run_until_parked();
        task.now_or_never().expect("removal finished")
    }

    /// The store command that writes this mode's removal.
    fn storage_command(&self) -> &'static str {
        match self.mode {
            SessionRemovalMode::Archive => "session_upsert",
            SessionRemovalMode::Delete => "session_delete",
        }
    }
}

const MODES: [SessionRemovalMode; 2] = [SessionRemovalMode::Archive, SessionRemovalMode::Delete];

#[gpui::test]
fn creates_the_replacement_session_behind_the_removal_interface(cx: &mut TestAppContext) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        f.write(|state| {
            state.tabs.truncate(1);
            state.sessions = vec![f.closing.clone()];
            state.active_tab_id = state.tabs[0].id.clone();
        });
        assert_eq!(f.run(cx), Ok(true), "{mode:?}");
        let state = f.read();
        assert_eq!(state.sessions.len(), 1);
        let replacement = &state.sessions[0];
        assert_eq!(replacement.cwd, CWD);
        assert_eq!(replacement.harness, HarnessId::Cursor);
        assert!(replacement.blocks.is_empty());
        assert_ne!(replacement.id, f.closing.id);
        assert_eq!(
            leaf_ids(&state.tabs[0].layout),
            vec![replacement.id.clone()]
        );
    }
}

#[gpui::test]
fn preserves_another_agents_completion_new_tabs_and_focus_across_both_waits(
    cx: &mut TestAppContext,
) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        let (confirmed, confirm) = oneshot::channel::<()>();
        let confirm = RefCell::new(Some(confirm));
        *f.adapter.confirm.borrow_mut() = Box::new(move |_, cx| {
            let wait = confirm.borrow_mut().take().unwrap();
            cx.foreground_executor().spawn(async move {
                let _ = wait.await;
                true
            })
        });
        let saving = f.backend.hold_next(f.storage_command());
        let pending = f.start(cx);
        cx.run_until_parked();
        let mut updated = f.other.clone();
        updated.busy = Some(false);
        updated.blocks = vec![Block::new("done", BlockRole::Assistant, "finished")];
        f.write(|state| state.sessions = vec![f.closing.clone(), updated.clone()]);
        confirmed.send(()).unwrap();
        cx.run_until_parked();
        assert!(
            f.backend
                .commands()
                .contains(&f.storage_command().to_string())
        );
        let added = session("added");
        let added_tab = new_tab(&added.id);
        f.write(|state| {
            state.tabs.push(added_tab.clone());
            state.sessions.push(added.clone());
            state.active_tab_id = added_tab.id.clone();
        });
        saving.release();
        cx.run_until_parked();
        assert_eq!(pending.now_or_never(), Some(Ok(true)), "{mode:?}");
        let state = f.read();
        let ids: Vec<&str> = state.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["other", "added"], "{mode:?}");
        assert_eq!(state.sessions[0], updated);
        assert_eq!(state.active_tab_id, added_tab.id);
        assert!(state.tabs.iter().any(|tab| tab.id == added_tab.id));
    }
}

#[gpui::test]
fn retains_a_stopped_session_and_paused_queue_when_storage_fails(cx: &mut TestAppContext) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        for command in ["session_upsert", "session_set_archived", "session_delete"] {
            f.backend.set_failing(command, true);
        }
        assert!(f.run(cx).is_err(), "{mode:?}");
        assert_eq!(f.adapter.stops.get(), 1);
        assert!(f.adapter.commits.borrow().is_empty());
        let state = f.read();
        let stopped = &state.sessions[0];
        assert_eq!(stopped.busy, Some(false));
        assert_eq!(stopped.queue_status, Some(MessageQueueStatus::Paused));
        assert_ne!(stopped.blocks[1].streaming, Some(true));
        assert_eq!(state.tabs.len(), 2);
    }
}

#[gpui::test]
fn does_not_stop_or_write_anything_if_confirmation_is_declined(cx: &mut TestAppContext) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        *f.adapter.confirm.borrow_mut() = Box::new(|_, _| Task::ready(false));
        assert_eq!(f.run(cx), Ok(false));
        assert_eq!(f.adapter.stops.get(), 0);
        assert!(
            f.backend.commands().is_empty(),
            "{:?}",
            f.backend.commands()
        );
        assert_eq!(f.read().sessions[0].busy, Some(true));
    }
}

#[gpui::test]
fn keeps_files_that_become_dirty_while_storage_is_pending(cx: &mut TestAppContext) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        let file = new_file_tab("/tmp/project/new.ts", CWD, false, None, None);
        f.write(|state| {
            state.tabs[0] = open_editor_tab(
                &state.tabs[0],
                &file,
                &OpenEditorTabOptions {
                    pin: true,
                    ..Default::default()
                },
            );
        });
        let saving = f.backend.hold_next(f.storage_command());
        let pending = f.start(cx);
        cx.run_until_parked();
        f.write(|state| {
            state.dirty_files.insert(file.id.clone());
        });
        saving.release();
        cx.run_until_parked();
        assert_eq!(pending.now_or_never(), Some(Ok(true)), "{mode:?}");
        let state = f.read();
        assert!(!state.sessions.iter().any(|s| s.id == f.closing.id));
        assert!(
            state.tabs[0].editor_panes[0]
                .files
                .iter()
                .any(|f| f.id == file.id)
        );
        assert!(!leaf_ids(&state.tabs[0].layout).contains(&f.closing.id));
        assert!(f.adapter.commits.borrow()[0].closed_tabs.is_empty());
    }
}

#[gpui::test]
fn keeps_a_terminal_opened_while_confirmation_is_pending(cx: &mut TestAppContext) {
    for mode in MODES {
        let f = Fixture::new(cx, mode);
        let terminal = new_terminal_file(CWD, None, None);
        let opened = terminal.clone();
        *f.adapter.confirm.borrow_mut() = Box::new(move |state, _| {
            let mut state = state.borrow_mut();
            state.tabs[0] = open_terminal_tab(&state.tabs[0], &opened, None);
            Task::ready(true)
        });
        assert_eq!(f.run(cx), Ok(true));
        let state = f.read();
        assert!(
            state.tabs[0].terminal_panes[0]
                .files
                .iter()
                .any(|f| f.id == terminal.id)
        );
        assert!(f.adapter.commits.borrow()[0].closed_tabs.is_empty());
    }
}

#[gpui::test]
fn archives_the_flushed_transcript_after_cancellation_with_streaming_stopped(
    cx: &mut TestAppContext,
) {
    let f = Fixture::new(cx, SessionRemovalMode::Archive);
    let closing_id = f.closing.id.clone();
    *f.adapter.on_stop.borrow_mut() = Box::new(move |state| {
        for session in &mut state.borrow_mut().sessions {
            if session.id == closing_id {
                session.blocks.push(Block {
                    streaming: Some(true),
                    ..Block::new("buffered", BlockRole::Assistant, "last buffered output")
                });
            }
        }
    });
    assert_eq!(f.run(cx), Ok(true));
    let upserts = f.backend.calls("session_upsert");
    let last = upserts[0]["session"]["blocks"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["text"], "last buffered output");
    assert_ne!(last.get("streaming"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        f.backend.commands(),
        vec!["session_upsert", "session_set_archived"]
    );
}
