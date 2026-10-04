//! Port of src/features/sessions/ui/BtwSheet.test.ts. A recording host
//! stands in for the engine; its rules follow btw.ts closely enough for
//! these sessions.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Task, TestAppContext, VisualTestContext, Window, div,
};
use monocode_core::block::{
    Block, BlockRole, BtwMessage, BtwMessageRole, BtwThread, BtwThreadStatus, ModelSettings,
};
use monocode_core::btw::{BtwSessionThread, session_has_btw_threads, supports_btw_harness};
use monocode_core::models::ModelCatalog;
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::activity::group_turns;
use monocode_core::{Attachment, Extra, HarnessId};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{ComposerHost, ComposerSubmission, SkillContext};

use super::*;

#[derive(Debug, Clone, PartialEq)]
struct Submitted {
    turn: Vec<String>,
    thread_id: String,
    text: String,
    model: Option<String>,
    settings: ModelSettings,
}

#[derive(Default)]
struct Calls {
    submits: Vec<Submitted>,
    retries: Vec<String>,
    deletes: Vec<(Vec<String>, String)>,
    stops: Vec<(Vec<String>, String)>,
}

struct TestHost {
    calls: RefCell<Calls>,
    accept: Cell<bool>,
}

impl TestHost {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            calls: RefCell::default(),
            accept: Cell::new(true),
        })
    }
}

fn ids(turn: &[Block]) -> Vec<String> {
    turn.iter().map(|block| block.id.clone()).collect()
}

fn turns_of(blocks: &[Block], managed: bool) -> Vec<Vec<Block>> {
    let refs: Vec<BlockRef> = blocks.iter().cloned().map(Arc::new).collect();
    group_turns(&refs, managed)
        .into_iter()
        .map(|turn| turn.into_iter().map(Arc::unwrap_or_clone).collect())
        .collect()
}

fn turn_harness(turn: &[Block], session_harness: HarnessId) -> HarnessId {
    turn.iter()
        .find(|block| block.role == BlockRole::User)
        .and_then(|block| block.turn_model.as_ref())
        .map_or(session_harness, |model| model.harness)
}

impl BtwHost for TestHost {
    fn session_threads(&self, blocks: &[Block], managed: bool) -> Vec<BtwSessionThread> {
        if !session_has_btw_threads(blocks) {
            return Vec::new();
        }
        let mut entries = Vec::new();
        for turn in turns_of(blocks, managed) {
            let threads = turn
                .iter()
                .find(|block| block.role == BlockRole::User)
                .and_then(|block| block.btw_threads.clone())
                .unwrap_or_default();
            for thread in threads {
                entries.push(BtwSessionThread {
                    thread,
                    turn: turn.clone(),
                });
            }
        }
        entries.sort_by_key(|entry| entry.thread.created_at);
        entries
    }

    fn open_target_turn_id(
        &self,
        turns: &[Vec<Block>],
        _: &[Block],
        session_harness: HarnessId,
        _: bool,
    ) -> Option<String> {
        turns.iter().rev().find_map(|turn| {
            let user = turn
                .iter()
                .rev()
                .find(|block| block.role == BlockRole::User)?;
            let supported = supports_btw_harness(Some(turn_harness(turn, session_harness)));
            (user.duration_ms.is_some() && supported).then(|| turn[0].id.clone())
        })
    }

    fn surface_harness(
        &self,
        _: &[Block],
        turn: &[Block],
        session_harness: HarnessId,
        threads: Option<&[BtwThread]>,
    ) -> Option<HarnessId> {
        let harness = turn_harness(turn, session_harness);
        if supports_btw_harness(Some(harness)) {
            return Some(harness);
        }
        threads?
            .iter()
            .find_map(|thread| thread.harness.filter(|h| supports_btw_harness(Some(*h))))
    }

    fn thread_blocks(&self, input: BtwThreadBlocksInput<'_>) -> Vec<Block> {
        let mut blocks = Vec::new();
        for (index, message) in input.messages.iter().enumerate() {
            let role = match message.role {
                BtwMessageRole::User => BlockRole::User,
                BtwMessageRole::Assistant => BlockRole::Assistant,
            };
            let mut block = Block::new(message.id.clone(), role, message.text.clone());
            if role == BlockRole::User {
                block.started_at = Some(message.created_at);
                let answered = input
                    .messages
                    .get(index + 1)
                    .is_some_and(|next| next.role == BtwMessageRole::Assistant);
                if answered || !input.running {
                    block.duration_ms = Some(0);
                }
            }
            blocks.push(block);
        }
        blocks
    }

    fn preferred_model_settings(
        &self,
        harness: HarnessId,
        model: &str,
        current: &ModelSettings,
    ) -> ModelSettings {
        let catalog = ModelCatalog::new();
        let resolved = catalog.resolve_model(harness, Some(model));
        catalog.preferred_model_settings(&resolved, Some(current), &ModelSettings::new())
    }

    fn submit(&self, request: BtwRequest<'_>, _: &mut App) -> bool {
        self.calls.borrow_mut().submits.push(Submitted {
            turn: ids(request.turn),
            thread_id: request.thread_id.to_string(),
            text: request.text.to_string(),
            model: request.model.map(str::to_string),
            settings: request.model_settings.clone(),
        });
        self.accept.get()
    }

    fn retry(&self, _: &[Block], thread_id: &str, _: &mut App) {
        self.calls.borrow_mut().retries.push(thread_id.to_string());
    }

    fn delete(&self, turn: &[Block], thread_id: &str, _: &mut App) {
        self.calls
            .borrow_mut()
            .deletes
            .push((ids(turn), thread_id.to_string()));
    }

    fn stop(&self, turn: &[Block], thread_id: &str, _: &mut App) {
        self.calls
            .borrow_mut()
            .stops
            .push((ids(turn), thread_id.to_string()));
    }

    fn composer_host(&self) -> Rc<dyn ComposerHost> {
        Rc::new(NullComposerHost)
    }
}

/// The session composer's host with nothing behind it.
struct NullComposerHost;

impl ComposerHost for NullComposerHost {
    fn submit(&self, _: ComposerSubmission, _: &mut Window, _: &mut App) -> bool {
        false
    }
    fn attachments_from_paths(&self, _: Vec<String>, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn attachments_from_files(&self, _: Vec<ClipboardFile>, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn skills(&self, _: &SkillContext, _: &mut App) -> Vec<Skill> {
        Vec::new()
    }
    fn mention_files(&self, _: &str, _: &mut App) -> Vec<ProjectFile> {
        Vec::new()
    }
    fn rank_mentions(&self, _: &str, _: &str, _: &mut App) -> Vec<RankedFile> {
        Vec::new()
    }
}

fn thread(id: &str, question: &str, created_at: i64) -> BtwThread {
    let message = |suffix: &str, role, text: String| BtwMessage {
        id: format!("{id}-{suffix}"),
        role,
        text,
        created_at,
        blocks: None,
        extra: Extra::new(),
    };
    BtwThread {
        id: id.into(),
        source_end_block_id: "a1".into(),
        created_at,
        updated_at: created_at,
        status: BtwThreadStatus::Ready,
        messages: vec![
            message("q", BtwMessageRole::User, question.into()),
            message(
                "a",
                BtwMessageRole::Assistant,
                format!("{question} answered"),
            ),
        ],
        harness: Some(HarnessId::Claude),
        model: None,
        model_settings: None,
        provider_thread_id: None,
        error: None,
        pending_blocks: None,
        extra: Extra::new(),
    }
}

fn session(threads: Option<Vec<BtwThread>>) -> Arc<Vec<Block>> {
    let mut user = Block::new("u1", BlockRole::User, "first");
    user.duration_ms = Some(1000);
    user.btw_threads = threads;
    Arc::new(vec![user, Block::new("a1", BlockRole::Assistant, "done")])
}

struct Pane {
    sheet: Entity<BtwSheet>,
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().relative().size_full().child(self.sheet.clone())
    }
}

struct Fixture<'a> {
    sheet: Entity<BtwSheet>,
    host: Rc<TestHost>,
    cx: &'a mut VisualTestContext,
}

fn props(blocks: Arc<Vec<Block>>, harness: HarnessId, available: bool) -> BtwSheetProps {
    BtwSheetProps {
        conversation: BtwConversationProps {
            available,
            blocks,
            harness,
            ..BtwConversationProps::default()
        },
        composer: ComposerProps {
            animate: false,
            runner_enabled: false,
            ..ComposerProps::default()
        },
        ..BtwSheetProps::default()
    }
}

fn mount(cx: &mut TestAppContext, blocks: Arc<Vec<Block>>, harness: HarnessId) -> Fixture<'_> {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        crate::transcript::init(cx);
        monocode_view_composer::composer::init(cx);
        monocode_view_composer::pickers::init(cx);
    });
    let host = TestHost::new();
    let sheet_host: Rc<dyn BtwHost> = host.clone();
    let (pane, cx) = cx.add_window_view(|_, cx| {
        let sheet = cx.new(|cx| {
            let mut sheet = BtwSheet::new(sheet_host, cx);
            sheet.set_props(props(blocks, harness, true), cx);
            sheet
        });
        Pane { sheet }
    });
    let sheet = cx.update(|_, cx| pane.read(cx).sheet.clone());
    let mut fixture = Fixture { sheet, host, cx };
    fixture.draw();
    fixture
}

impl Fixture<'_> {
    fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.update(|window, cx| {
                window.draw(cx).clear();
            });
            self.cx.run_until_parked();
        }
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut BtwSheet, &mut Context<BtwSheet>) -> R) -> R {
        let sheet = self.sheet.clone();
        let result = self.cx.update(|_, cx| sheet.update(cx, f));
        self.draw();
        result
    }

    fn read<R>(&mut self, f: impl FnOnce(&BtwSheet, &App) -> R) -> R {
        let sheet = self.sheet.clone();
        self.cx.update(|_, cx| f(sheet.read(cx), cx))
    }

    fn set_props(&mut self, props: BtwSheetProps) {
        self.update(|sheet, cx| sheet.set_props(props, cx));
    }

    fn labels(&mut self) -> Vec<String> {
        self.read(|sheet, _| {
            sheet
                .tabs()
                .iter()
                .map(|tab| compact_question(tab.question.as_deref()))
                .collect()
        })
    }

    fn tab_id(&mut self, index: usize) -> String {
        self.read(|sheet, _| sheet.tabs()[index].id.clone())
    }

    fn select(&mut self, index: usize) {
        let id = self.tab_id(index);
        self.update(|sheet, cx| sheet.select_tab(&id, cx));
    }

    fn selected(&mut self) -> Option<usize> {
        self.read(|sheet, _| {
            let active = sheet.active_tab_id()?;
            sheet.tabs().iter().position(|tab| tab.id == active)
        })
    }

    /// The side composer's text.
    fn composer_text(&mut self) -> String {
        self.read(|sheet, cx| {
            sheet
                .composer()
                .map(|composer| composer.read(cx).draft().to_string())
                .unwrap_or_default()
        })
    }

    fn thread_text(&mut self) -> String {
        self.read(|sheet, cx| {
            let blocks = sheet
                .transcript()
                .and_then(|transcript| transcript.read(cx).session().cloned())
                .map(|session| session.blocks.clone())
                .unwrap_or_default();
            blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    fn submits(&self) -> Vec<Submitted> {
        self.host.calls.borrow().submits.clone()
    }
}

#[gpui::test]
fn stays_hidden_until_opened(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    assert!(!f.read(|sheet, _| sheet.is_rendered()));
}

#[gpui::test]
fn opens_an_empty_tab_for_a_bare_btw_without_sending(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    assert!(f.read(|sheet, _| sheet.is_open()));
    assert_eq!(f.labels(), ["New question"]);
    assert!(f.submits().is_empty());
}

#[gpui::test]
fn carries_typed_text_into_the_side_composer_without_sending(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("half a thought", true, cx));
    assert_eq!(f.composer_text(), "half a thought");
    assert!(f.submits().is_empty());
}

#[gpui::test]
fn keeps_an_unsent_question_in_its_own_tab_while_switching(cx: &mut TestAppContext) {
    let mut f = mount(
        cx,
        session(Some(vec![thread("t1", "Earlier", 1)])),
        HarnessId::Claude,
    );
    f.update(|sheet, cx| sheet.open_with("half a thought", true, cx));
    assert_eq!(f.composer_text(), "half a thought");
    f.select(0);
    assert_eq!(f.composer_text(), "");
    f.select(1);
    assert_eq!(f.composer_text(), "half a thought");
}

#[gpui::test]
fn keeps_a_pending_question_visible_when_another_draft_opens(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("First question", false, cx));
    f.update(|sheet, cx| sheet.start_draft(cx));
    assert_eq!(f.labels(), ["First question", "New question"]);
    assert_eq!(f.selected(), Some(1));
}

#[gpui::test]
fn closes_on_escape_from_the_side_composer(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    let composer = f.read(|sheet, _| sheet.composer()).expect("side composer");
    f.cx.update(|window, cx| {
        let handle = composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    f.cx.simulate_keystrokes("escape");
    f.draw();
    assert!(!f.read(|sheet, _| sheet.is_open()));
    assert!(!f.read(|sheet, _| sheet.is_rendered()));
}

#[gpui::test]
fn closes_on_escape_when_nothing_is_focused(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    // The pane forwards an Escape that reached it with nothing focused.
    assert!(f.update(|sheet, cx| sheet.escape(cx)));
    assert!(!f.read(|sheet, _| sheet.is_open()));
}

#[gpui::test]
fn sends_btw_text_against_the_latest_finished_turn(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("something here...", false, cx));
    let submits = f.submits();
    assert_eq!(submits.len(), 1);
    assert_eq!(submits[0].turn, ["u1", "a1"]);
    assert_eq!(submits[0].text, "something here...");
    assert_eq!(submits[0].model, None);
    assert_eq!(submits[0].settings, ModelSettings::new());
    assert_eq!(f.labels()[0], "something here...");
    assert!(f.read(|sheet, _| sheet.running()));
}

#[gpui::test]
fn refuses_to_open_when_btw_is_unavailable(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.set_props(props(session(None), HarnessId::Claude, false));
    assert!(!f.update(|sheet, cx| sheet.open_with("hi", false, cx)));
    assert!(!f.read(|sheet, _| sheet.is_rendered()));
}

#[gpui::test]
fn stays_closed_when_availability_returns_after_forcing_the_sheet_closed(cx: &mut TestAppContext) {
    let blocks = session(None);
    let mut f = mount(cx, blocks.clone(), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    assert!(f.read(|sheet, _| sheet.is_open()));

    f.set_props(props(blocks.clone(), HarnessId::Claude, false));
    assert!(!f.read(|sheet, _| sheet.is_open()));
    assert!(!f.read(|sheet, _| sheet.is_rendered()));

    f.set_props(props(blocks, HarnessId::Claude, true));
    assert!(!f.read(|sheet, _| sheet.is_open()));
    assert!(!f.read(|sheet, _| sheet.is_rendered()));
}

#[gpui::test]
fn opens_saved_threads_when_there_is_no_eligible_turn_for_a_new_draft(cx: &mut TestAppContext) {
    let mut f = mount(
        cx,
        session(Some(vec![thread("t1", "Saved question", 1)])),
        HarnessId::Fx,
    );
    assert!(f.update(|sheet, cx| sheet.open_with("", false, cx)));
    assert!(f.read(|sheet, _| sheet.is_open()));
    assert_eq!(f.labels(), ["Saved question"]);
    assert!(f.thread_text().contains("Saved question answered"));
    assert!(f.submits().is_empty());
}

#[gpui::test]
fn asks_the_active_tab_from_the_composer_and_keeps_text_while_it_runs(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    // Through the side composer: typing and Enter reach the sheet's submit.
    let composer = f.read(|sheet, _| sheet.composer()).expect("side composer");
    f.cx.update(|window, cx| {
        let handle = composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    f.cx.simulate_input("first question");
    f.cx.simulate_keystrokes("enter");
    f.draw();
    assert_eq!(f.submits().len(), 1);
    assert_eq!(f.submits()[0].text, "first question");

    assert!(!f.update(|sheet, cx| sheet.submit("too soon", cx)));
    assert_eq!(f.submits().len(), 1);
}

#[gpui::test]
fn does_not_become_optimistic_when_the_app_rejects_a_question(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.host.accept.set(false);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    assert!(!f.update(|sheet, cx| sheet.submit("question that was not accepted", cx)));
    assert!(!f.read(|sheet, _| sheet.running()));
    assert_eq!(f.submits().len(), 1);
}

#[gpui::test]
fn does_not_open_or_clear_a_submitted_command_when_the_app_rejects_it(cx: &mut TestAppContext) {
    let mut f = mount(cx, session(None), HarnessId::Claude);
    f.host.accept.set(false);
    assert!(!f.update(|sheet, cx| sheet.open_with("question that was not accepted", false, cx)));
    assert!(!f.read(|sheet, _| sheet.is_open()));
    assert!(!f.read(|sheet, _| sheet.running()));
    assert!(!f.read(|sheet, _| sheet.is_rendered()));
}

#[gpui::test]
fn stops_a_streaming_answer_from_the_side_composer(cx: &mut TestAppContext) {
    let mut running = thread("t1", "Still going", 1);
    running.status = BtwThreadStatus::Running;
    running.messages.truncate(1);
    let mut f = mount(cx, session(Some(vec![running])), HarnessId::Claude);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    f.select(0);
    assert!(f.read(|sheet, _| sheet.running()));
    // The side composer shows Stop instead of Send while the answer runs.
    let props = f.read(|sheet, _| sheet.composer_props());
    assert!(props.busy && !props.allow_busy_submit);
    f.update(|sheet, cx| sheet.stop(cx));
    let stops = f.host.calls.borrow().stops.clone();
    assert_eq!(stops.len(), 1);
    assert!(stops[0].0.contains(&"u1".to_string()));
    assert_eq!(stops[0].1, "t1");
}

#[gpui::test]
fn shows_one_tab_per_side_thread_in_the_session_and_switches_between_them(cx: &mut TestAppContext) {
    let mut f = mount(
        cx,
        session(Some(vec![
            thread("t1", "First question", 1),
            thread("t2", "Second question", 2),
        ])),
        HarnessId::Claude,
    );
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    assert_eq!(
        f.labels(),
        ["First question", "Second question", "New question"]
    );
    assert_eq!(f.selected(), Some(2));
    f.select(0);
    assert_eq!(f.selected(), Some(0));
    let text = f.thread_text();
    assert!(text.contains("First question answered"));
    assert!(!text.contains("Second question answered"));
}

#[gpui::test]
fn deletes_a_thread_from_its_tab_and_closes_when_no_tabs_remain(cx: &mut TestAppContext) {
    let mut f = mount(
        cx,
        session(Some(vec![thread("t1", "Only question", 1)])),
        HarnessId::Claude,
    );
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    // Drop the new empty tab first, then the saved thread.
    let draft = f.tab_id(1);
    f.update(|sheet, cx| sheet.close_tab(&draft, cx));
    let saved = f.tab_id(0);
    f.update(|sheet, cx| sheet.close_tab(&saved, cx));
    let deletes = f.host.calls.borrow().deletes.clone();
    assert_eq!(deletes.len(), 1);
    assert!(deletes[0].0.contains(&"u1".to_string()));
    assert_eq!(deletes[0].1, "t1");
    assert!(!f.read(|sheet, _| sheet.is_open()));
}

#[gpui::test]
fn shows_the_error_and_retries_a_failed_thread(cx: &mut TestAppContext) {
    let mut failed = thread("t1", "Broken", 1);
    failed.status = BtwThreadStatus::Error;
    failed.error = Some("The runner crashed.".into());
    let mut f = mount(cx, session(Some(vec![failed])), HarnessId::Fx);
    f.update(|sheet, cx| sheet.open_with("", false, cx));
    f.update(|sheet, cx| sheet.retry(cx));
    assert_eq!(*f.host.calls.borrow().retries, ["t1"]);
}
