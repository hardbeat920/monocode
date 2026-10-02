//! Port of src/integrations/harness/core/apply.test.ts and applyBatch.test.ts.

use serde_json::{Value, json};

use super::*;
use crate::context_usage::ContextUsage;
use crate::harness::HarnessId;
use crate::models::{HarnessAvailability, ModelEnv, ModelPrefs};
use crate::plan::plan_turn_key;
use crate::project_providers::ProjectProviders;
use crate::reducer::preview::tests::preview_from_tool;
use crate::session::new_session;

/// Counter ids and a clock the test sets, like the `Date.now` spy.
struct TestEnv {
    ids: u64,
    now: i64,
}

impl ReducerEnv for TestEnv {
    fn new_id(&mut self) -> String {
        self.ids += 1;
        format!("block-{}", self.ids)
    }

    fn now_ms(&mut self) -> i64 {
        self.now
    }
}

struct T {
    catalog: ModelCatalog,
    prefs: ModelPrefs,
    availability: HarnessAvailability,
    projects: ProjectProviders,
    env: TestEnv,
}

impl T {
    fn new() -> Self {
        Self {
            catalog: ModelCatalog::new(),
            prefs: ModelPrefs::default(),
            availability: HarnessAvailability::default(),
            projects: ProjectProviders::default(),
            env: TestEnv { ids: 0, now: 0 },
        }
    }

    /// `newSession(harness, cwd, model)`.
    fn session_with(&self, harness: HarnessId, cwd: &str, model: Option<&str>) -> Session {
        let env = ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        };
        new_session(&env, "session", harness, cwd, model, None, None)
    }

    fn session(&self, harness: HarnessId, cwd: &str) -> Session {
        self.session_with(harness, cwd, None)
    }

    fn apply(&mut self, session: &Session, event: Value) -> Session {
        let mut next = session.clone();
        apply_harness_event_mut(&mut self.env, &mut next, &ev(event));
        next
    }

    fn apply_all(&mut self, session: &Session, events: &[Value]) -> Session {
        events.iter().fold(session.clone(), |current, event| {
            self.apply(&current, event.clone())
        })
    }

    fn user(&mut self, session: &Session, text: &str) -> Session {
        let mut next = session.clone();
        append_user_mut(&mut self.env, &self.catalog, &mut next, text, &[], None);
        next
    }

    fn steer(&mut self, session: &Session, text: &str, extra: Option<&UserTurnExtra>) -> Session {
        let mut next = session.clone();
        append_steer_user_mut(&mut self.env, &self.catalog, &mut next, text, &[], extra);
        next
    }

    fn stop(&mut self, session: &Session) -> Session {
        stop_streaming(session, self.env.now)
    }
}

fn ev(value: Value) -> HarnessEvent {
    serde_json::from_value(value).expect("valid harness event")
}

fn texts(session: &Session) -> Vec<&str> {
    session
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect()
}

fn roles(session: &Session) -> Vec<BlockRole> {
    session.blocks.iter().map(|block| block.role).collect()
}

fn by_call<'a>(session: &'a Session, call_id: &str) -> &'a Block {
    session
        .blocks
        .iter()
        .find(|block| tool_call_id(block) == Some(call_id))
        .expect("tool block")
}

fn tool(block: &Block) -> &BlockTool {
    block.tool.as_ref().expect("tool")
}

fn delta(text: &str) -> Value {
    json!({ "type": "message.delta", "text": text })
}

// background work
#[test]
fn tracks_what_a_yielded_turn_waits_on_and_drops_it_when_the_turn_ends() {
    let mut t = T::new();
    let mut session = t.user(&t.session(HarnessId::Claude, "/tmp"), "hi");
    session = t.apply(
        &session,
        json!({ "type": "background.updated", "tasks": ["npm test"] }),
    );
    assert_eq!(session.background_tasks, Some(vec!["npm test".to_string()]));
    session = t.apply(
        &session,
        json!({ "type": "background.updated", "tasks": [] }),
    );
    assert_eq!(session.background_tasks, None);
    session = t.apply(
        &session,
        json!({ "type": "background.updated", "tasks": ["npm run dev"] }),
    );
    session = t.stop(&session);
    assert_eq!(session.background_tasks, None);
}

// turn duration
#[test]
fn records_the_selected_provider_and_model_on_a_user_turn() {
    let mut t = T::new();
    let session = t.user(
        &t.session_with(HarnessId::Claude, "/tmp", Some("claude:opus-5")),
        "hi",
    );
    let model = session.blocks[0].turn_model.as_ref().unwrap();
    assert_eq!(model.harness, HarnessId::Claude);
    assert_eq!(model.id, "claude:opus-5");
    assert_eq!(model.name, "Claude Opus 5");
}

#[test]
fn stamps_how_long_the_agent_worked_when_the_turn_ends() {
    let mut t = T::new();
    t.env.now = 1_000;
    let mut session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "hi");
    assert_eq!(session.busy, Some(true));
    assert_eq!(session.blocks[0].started_at, Some(1_000));
    assert_eq!(session.blocks[0].duration_ms, None);

    t.env.now = 26_000;
    session = t.stop(&session);
    assert_eq!(session.busy, Some(false));
    assert_eq!(session.blocks[0].duration_ms, Some(25_000));
}

#[test]
fn does_not_overwrite_a_duration_already_recorded() {
    let mut t = T::new();
    t.env.now = 1_000;
    let mut session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "hi");
    t.env.now = 5_000;
    session = t.stop(&session);
    t.env.now = 90_000;
    session = t.stop(&session);
    assert_eq!(session.blocks[0].duration_ms, Some(4_000));
}

#[test]
fn records_duration_when_the_turn_errors() {
    let mut t = T::new();
    t.env.now = 1_000;
    let mut session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "hi");
    t.env.now = 8_000;
    session = t.apply(
        &session,
        json!({ "type": "session.error", "message": "boom" }),
    );
    assert_eq!(session.busy, Some(false));
    assert_eq!(session.blocks[0].duration_ms, Some(7_000));
}

#[test]
fn marks_orphaned_subagent_work_failed_when_the_provider_dies() {
    let mut t = T::new();
    let mut session = t.user(&t.session(HarnessId::Codex, "/tmp"), "delegate it");
    session = t.apply(
        &session,
        json!({ "type": "tool.started", "callId": "agent-1", "title": "Inspect auth", "kind": "agent", "status": "in_progress" }),
    );
    session = t.apply(
        &session,
        json!({ "type": "session.error", "message": "Codex app-server exited" }),
    );

    assert_eq!(session.busy, Some(false));
    let agent = by_call(&session, "agent-1");
    assert_eq!(agent.streaming, Some(false));
    assert_eq!(tool(agent).kind.as_deref(), Some("agent"));
    assert_eq!(tool(agent).status.as_deref(), Some("failed"));
    let last = session.blocks.last().unwrap();
    assert_eq!(last.role, BlockRole::System);
    assert_eq!(last.text, "Codex app-server exited");
    assert_eq!(last.notice, Some(BlockNotice::Error));
}

// approval lifetime
fn waiting_for_approval(t: &mut T) -> Session {
    let session = t.user(&t.session(HarnessId::Codex, "/tmp"), "check it");
    let session = t.apply(
        &session,
        json!({ "type": "tool.started", "callId": "shell-1", "title": "Run npm test", "kind": "execute", "status": "pending" }),
    );
    t.apply(
        &session,
        json!({ "type": "approval.requested", "requestId": 7, "callId": "shell-1", "title": "Run npm test", "kind": "execute" }),
    )
}

fn cancelled(request_id: i64) -> BlockApproval {
    BlockApproval {
        request_id,
        decided: Some(ApprovalDecided::Cancelled),
        extra: Extra::new(),
    }
}

#[test]
fn cancels_an_unresolved_request_when_its_turn_stops() {
    let mut t = T::new();
    let waiting = waiting_for_approval(&mut t);
    let session = t.stop(&waiting);
    let block = by_call(&session, "shell-1");

    assert_eq!(session.busy, Some(false));
    assert_eq!(block.streaming, Some(false));
    assert_eq!(tool(block).status.as_deref(), Some("cancelled"));
    assert_eq!(block.approval, Some(cancelled(7)));
}

#[test]
fn cancels_a_stale_request_before_a_later_turn_is_appended() {
    let mut t = T::new();
    let mut stale = waiting_for_approval(&mut t);
    stale.busy = Some(false);
    let session = t.user(&stale, "continue");

    assert_eq!(by_call(&session, "shell-1").approval, Some(cancelled(7)));
    let last = session.blocks.last().unwrap();
    assert_eq!(
        (last.role, last.text.as_str()),
        (BlockRole::User, "continue")
    );
}

// streamed markdown
#[test]
fn keeps_heading_breaks_tables_and_doubled_letters() {
    let mut t = T::new();
    let chunks = [
        "# Result",
        "\n",
        "\n",
        "book",
        "keeper..\n",
        "\n",
        "| a | b |\n",
        "| --- | --- |\n",
        "| 1 | 2 |",
    ];
    let events: Vec<Value> = chunks.iter().map(|text| delta(text)).collect();
    let session = t.apply_all(&t.session(HarnessId::Pi, "/tmp"), &events);
    assert_eq!(session.blocks[0].text, chunks.concat());
}

#[test]
fn stores_generated_images_as_standalone_blocks_without_assistant_text() {
    let mut t = T::new();
    let session = t.apply(
        &t.session(HarnessId::Codex, "/tmp"),
        json!({
            "type": "image.generated",
            "itemId": "image_1",
            "path": "/app-data/generated-images/image.png",
            "name": "generated-image",
            "mimeType": "image/png",
            "size": 8,
            "alt": "A clean product photo",
        }),
    );
    assert_eq!(session.blocks.len(), 1);
    let block = &session.blocks[0];
    assert_eq!((block.role, block.text.as_str()), (BlockRole::Image, ""));
    assert_eq!(
        serde_json::to_value(block.image.as_ref().unwrap()).unwrap(),
        json!({
            "path": "/app-data/generated-images/image.png",
            "name": "generated-image",
            "mimeType": "image/png",
            "size": 8,
            "alt": "A clean product photo",
        })
    );
}

#[test]
fn does_not_double_an_assistant_block_when_a_completed_snapshot_repeats_it() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/tmp"),
        &[delta("I'll read the file"), delta("I'll read the file")],
    );
    assert_eq!(texts(&session), ["I'll read the file"]);
}

#[test]
fn continues_open_prose_through_status_rows_then_completes_it() {
    let mut t = T::new();
    let mut session = t.apply(&t.session(HarnessId::Omp, "/tmp"), delta("contributor（"));
    let id = session.blocks[0].id.clone();
    session = t.apply(
        &session,
        json!({ "type": "status", "text": "Advisor reviewed this turn" }),
    );
    session = t.apply(&session, delta("邮箱归属链已验证）"));
    assert_eq!(
        texts(&session),
        [
            "contributor（邮箱归属链已验证）",
            "Advisor reviewed this turn"
        ]
    );
    assert_eq!(session.blocks[0].id, id);
    assert_eq!(session.blocks[0].streaming, Some(true));
    session = t.apply(&session, json!({ "type": "message.completed" }));
    session = t.apply(&session, delta("Next message."));
    assert_eq!(session.blocks[0].streaming, Some(false));
    assert_eq!(session.blocks[2].role, BlockRole::Assistant);
    assert_eq!(session.blocks[2].text, "Next message.");
}

#[test]
fn keeps_adjacent_completed_assistant_messages_in_separate_blocks() {
    let mut t = T::new();
    let completed = json!({ "type": "message.completed" });
    let session = t.apply_all(
        &t.session(HarnessId::Codex, "/tmp"),
        &[
            delta("- update the notes and commit"),
            completed.clone(),
            delta("Connect returned an empty file for one image."),
            completed,
        ],
    );
    assert_eq!(
        texts(&session),
        [
            "- update the notes and commit",
            "Connect returned an empty file for one image."
        ]
    );
    assert!(
        session
            .blocks
            .iter()
            .all(|block| { block.role == BlockRole::Assistant && block.streaming == Some(false) })
    );
    assert_ne!(session.blocks[0].id, session.blocks[1].id);
}

#[test]
fn seals_open_prose_at_an_interjection_with_and_without_preceding_status() {
    for status in [false, true] {
        let mut t = T::new();
        let mut session = t.apply(&t.session(HarnessId::Omp, "/tmp"), delta("contributor（"));
        let id = session.blocks[0].id.clone();
        if status {
            session = t.apply(&session, json!({ "type": "status", "text": "Reviewed" }));
        }
        session = t.apply(
            &session,
            json!({ "type": "interjection", "text": "Review", "customType": "advisor" }),
        );
        assert_eq!(session.blocks[0].id, id);
        assert_eq!(session.blocks[0].streaming, Some(false));
        session = t.apply(&session, delta("邮箱归属链已验证）"));
        let mut expected = vec!["contributor（"];
        if status {
            expected.push("Reviewed");
        }
        expected.extend(["Review", "邮箱归属链已验证）"]);
        assert_eq!(texts(&session), expected);
        let last = session.blocks.last().unwrap();
        assert_eq!(
            (last.role, last.streaming),
            (BlockRole::Assistant, Some(true))
        );
        assert_ne!(last.id, id);
    }
}

#[test]
fn does_not_resume_a_sealed_assistant_through_status_after_an_interjection() {
    let mut t = T::new();
    let mut session = t.apply(&t.session(HarnessId::Omp, "/tmp"), delta("First."));
    let id = session.blocks[0].id.clone();
    session = t.apply_all(
        &session,
        &[
            json!({ "type": "interjection", "text": "Review", "customType": "advisor" }),
            json!({ "type": "status", "text": "Advisor reviewed this turn" }),
            delta("Second."),
        ],
    );
    assert_eq!(
        texts(&session),
        ["First.", "Review", "Advisor reviewed this turn", "Second."]
    );
    assert_eq!(session.blocks[0].id, id);
    assert_eq!(session.blocks[0].streaming, Some(false));
    assert_ne!(session.blocks.last().unwrap().id, id);
}

#[test]
fn keeps_stacked_interjections_as_hard_boundaries() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Omp, "/tmp"),
        &[
            delta("First."),
            json!({ "type": "interjection", "text": "One", "customType": "advisor" }),
            json!({ "type": "interjection", "text": "Two", "customType": "advisor" }),
            delta("Second."),
        ],
    );
    let pairs: Vec<_> = session
        .blocks
        .iter()
        .map(|block| (block.role, block.text.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            (BlockRole::Assistant, "First."),
            (BlockRole::System, "One"),
            (BlockRole::System, "Two"),
            (BlockRole::Assistant, "Second."),
        ]
    );
}

#[test]
fn seals_open_reasoning_at_an_interjection() {
    let mut t = T::new();
    let mut session = t.apply(
        &t.session(HarnessId::Omp, "/tmp"),
        json!({ "type": "reasoning.delta", "text": "Think" }),
    );
    let id = session.blocks[0].id.clone();
    session = t.apply_all(
        &session,
        &[
            json!({ "type": "interjection", "text": "Review", "customType": "advisor" }),
            json!({ "type": "reasoning.delta", "text": " more" }),
        ],
    );
    let first = &session.blocks[0];
    assert_eq!(
        (first.id.as_str(), first.text.as_str(), first.streaming),
        (id.as_str(), "Think", Some(false))
    );
    let last = session.blocks.last().unwrap();
    assert_eq!(
        (last.role, last.text.as_str(), last.streaming),
        (BlockRole::Reasoning, " more", Some(true))
    );
    assert_ne!(last.id, id);
}

#[test]
fn continues_open_prose_through_many_status_rows() {
    let mut t = T::new();
    let mut session = t.apply(&t.session(HarnessId::Omp, "/tmp"), delta("Hel"));
    let id = session.blocks[0].id.clone();
    for text in ["A", "B", "C", "D", "E"] {
        session = t.apply(&session, json!({ "type": "status", "text": text }));
    }
    session = t.apply(&session, delta("lo"));
    let first = &session.blocks[0];
    assert_eq!(
        (first.id.as_str(), first.text.as_str(), first.streaming),
        (id.as_str(), "Hello", Some(true))
    );
    assert_eq!(
        session
            .blocks
            .iter()
            .filter(|block| block.role == BlockRole::System)
            .count(),
        5
    );
}

#[test]
fn continues_reasoning_across_status_but_seals_it_when_a_tool_starts() {
    let mut t = T::new();
    let mut session = t.apply_all(
        &t.session(HarnessId::Omp, "/tmp"),
        &[
            json!({ "type": "reasoning.delta", "text": "Think" }),
            json!({ "type": "status", "text": "Reviewing" }),
            json!({ "type": "reasoning.delta", "text": " more" }),
        ],
    );
    assert_eq!(session.blocks[0].text, "Think more");
    assert_eq!(session.blocks[0].streaming, Some(true));
    session = t.apply(
        &session,
        json!({ "type": "tool.started", "callId": "call", "title": "Read" }),
    );
    assert_eq!(session.blocks[0].streaming, Some(false));
    session = t.apply_all(
        &session,
        &[
            json!({ "type": "status", "text": "Waiting" }),
            json!({ "type": "tool.updated", "callId": "call", "status": "completed" }),
        ],
    );
    let tools: Vec<&Block> = session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Tool)
        .collect();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].streaming, Some(false));
    assert_eq!(tool(tools[0]).call_id.as_deref(), Some("call"));
    assert_eq!(tool(tools[0]).status.as_deref(), Some("completed"));
    session = t.apply(
        &session,
        json!({ "type": "reasoning.delta", "text": "New thought" }),
    );
    let last = session.blocks.last().unwrap();
    assert_eq!(
        (last.role, last.text.as_str()),
        (BlockRole::Reasoning, "New thought")
    );
}

// appendSteerUser
#[test]
fn appends_a_user_message_without_sealing_an_in_flight_assistant_block() {
    let mut t = T::new();
    let mut session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "build it");
    session = t.apply(&session, delta("Working on it"));
    assert_eq!(session.blocks[1].streaming, Some(true));

    session = t.steer(&session, "focus on tests", None);
    assert_eq!(session.blocks.len(), 3);
    assert_eq!(session.blocks[1].streaming, Some(true));
    let steer = &session.blocks[2];
    assert_eq!(
        (steer.role, steer.text.as_str()),
        (BlockRole::User, "focus on tests")
    );
    let model = steer.turn_model.as_ref().unwrap();
    assert_eq!(
        (model.harness, model.id.as_str()),
        (HarnessId::Cursor, session.model.as_str())
    );
    assert_eq!(steer.started_at, None);
    assert_eq!(session.busy, Some(true));
}

#[test]
fn keeps_a_note_card_on_a_steered_user_turn() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "build it");
    let note = NoteCardMeta {
        id: "n1".into(),
        slug: "overview".into(),
        title: "Overview".into(),
        ..NoteCardMeta::default()
    };
    let session = t.steer(
        &session,
        "hi",
        Some(&UserTurnExtra {
            note_card: Some(note.clone()),
            ..UserTurnExtra::default()
        }),
    );
    let steer = &session.blocks[1];
    assert_eq!((steer.role, steer.text.as_str()), (BlockRole::User, "hi"));
    assert_eq!(steer.note_card, Some(note));
}

// usage limits
#[test]
fn records_when_a_limited_turn_can_resume() {
    let mut t = T::new();
    let limited = t.apply(
        &t.session(HarnessId::Codex, "/tmp"),
        json!({ "type": "usage.limited", "resetsAt": 5_000 }),
    );
    assert_eq!(
        limited.usage_limit,
        Some(UsageLimit {
            resets_at: Some(5_000),
            resume_at_reset: None
        })
    );
    let unknown = t.apply(
        &t.session(HarnessId::Codex, "/tmp"),
        json!({ "type": "usage.limited" }),
    );
    assert_eq!(
        serde_json::to_value(unknown.usage_limit.unwrap()).unwrap(),
        json!({})
    );
}

// status blocks
fn system_blocks(session: &Session) -> Vec<&Block> {
    session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::System)
        .collect()
}

#[test]
fn keeps_one_row_when_the_same_status_repeats() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Claude, "/tmp"), "go");
    let status = json!({ "type": "status", "text": "Retrying in 3s" });
    let session = t.apply_all(&session, &[status.clone(), status]);
    let system = system_blocks(&session);
    assert_eq!(system.len(), 1);
    assert_eq!(system[0].text, "Retrying in 3s");
}

#[test]
fn still_appends_a_status_that_differs_from_the_last_one() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Claude, "/tmp"), "go");
    let session = t.apply_all(
        &session,
        &[
            json!({ "type": "status", "text": "Retrying" }),
            json!({ "type": "status", "text": "Compacting" }),
        ],
    );
    assert_eq!(system_blocks(&session).len(), 2);
}

#[test]
fn ignores_blank_status_text() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Claude, "/tmp"), "go");
    let mut next = session.clone();
    let changed = apply_harness_event_mut(
        &mut t.env,
        &mut next,
        &ev(json!({ "type": "status", "text": "  " })),
    );
    assert!(!changed);
    assert!(system_blocks(&next).is_empty());
}

// interjection blocks
#[test]
fn keeps_a_boundary_between_completed_assistant_messages() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Pi, "/tmp"), "go");
    let session = t.apply_all(
        &session,
        &[
            delta("Complete answer."),
            json!({ "type": "message.completed" }),
            json!({ "type": "interjection", "text": "Check the fallback.", "customType": "advisor" }),
            delta("Checked."),
        ],
    );
    let rest = &session.blocks[1..];
    assert_eq!(rest.len(), 3);
    assert_eq!(
        (rest[0].role, rest[0].text.as_str(), rest[0].streaming),
        (BlockRole::Assistant, "Complete answer.", Some(false))
    );
    assert_eq!(rest[1].role, BlockRole::System);
    assert_eq!(
        rest[1].interjection.as_ref().unwrap().custom_type,
        "advisor"
    );
    assert_eq!(
        (rest[2].role, rest[2].text.as_str(), rest[2].streaming),
        (BlockRole::Assistant, "Checked.", Some(true))
    );
}

#[test]
fn appends_every_interjection_as_a_distinct_persisted_boundary() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Pi, "/tmp"), "go");
    let note = json!({
        "type": "interjection", "text": "Check the fallback.",
        "customType": "advisor", "severity": "concern",
    });
    let session = t.apply_all(&session, &[note.clone(), note]);
    let interjections: Vec<&Block> = session
        .blocks
        .iter()
        .filter(|block| block.interjection.is_some())
        .collect();
    assert_eq!(interjections.len(), 2);
    assert_eq!(interjections[0].role, BlockRole::System);
    assert_eq!(interjections[0].text, "Check the fallback.");
    assert_eq!(
        serde_json::to_value(interjections[0].interjection.as_ref().unwrap()).unwrap(),
        json!({ "customType": "advisor", "severity": "concern" })
    );
}

// task list updates
fn task_blocks(session: &Session) -> Vec<&Block> {
    session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Tasks)
        .collect()
}

fn task_items(session: &Session) -> Value {
    serde_json::to_value(&task_blocks(session)[0].task_list.as_ref().unwrap().items).unwrap()
}

#[test]
fn updates_one_structured_checklist_instead_of_appending_plan_cards() {
    let mut t = T::new();
    let mut session = t.user(&t.session(HarnessId::Codex, "/tmp"), "fix it");
    session = t.apply(
        &session,
        json!({
            "type": "tasks.updated", "key": "turn_1",
            "explanation": "Starting with the regression.",
            "items": [
                { "text": "Inspect", "status": "in_progress" },
                { "text": "Verify", "status": "pending" },
            ],
        }),
    );
    let task_id = task_blocks(&session)[0].id.clone();
    session = t.apply_all(
        &session,
        &[
            json!({ "type": "tool.started", "callId": "call_1", "title": "Read src/App.tsx" }),
            json!({
                "type": "tasks.updated", "key": "turn_1",
                "items": [
                    { "text": "Inspect", "status": "completed" },
                    { "text": "Verify", "status": "in_progress" },
                ],
            }),
        ],
    );

    let tasks = task_blocks(&session);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task_id);
    assert_eq!(tasks[0].text, "[x] Inspect\n[~] Verify");
    assert_eq!(
        serde_json::to_value(tasks[0].task_list.as_ref().unwrap()).unwrap(),
        json!({
            "key": "turn_1",
            "items": [
                { "text": "Inspect", "status": "completed" },
                { "text": "Verify", "status": "in_progress" },
            ],
        })
    );
    assert!(!roles(&session).contains(&BlockRole::Plan));
}

#[test]
fn merges_partial_status_updates_without_removing_or_renaming_tasks() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "fix it");
    let session = t.apply_all(
        &session,
        &[
            json!({
                "type": "tasks.updated",
                "items": [
                    { "id": "1", "text": "Inspect", "status": "completed" },
                    { "id": "2", "text": "Implement", "status": "in_progress" },
                    { "id": "3", "text": "Verify", "status": "pending" },
                ],
            }),
            json!({
                "type": "tasks.updated", "merge": true,
                "items": [{ "id": "2", "text": "Implementing the fix", "status": "completed" }],
            }),
        ],
    );
    assert_eq!(
        task_items(&session),
        json!([
            { "id": "1", "text": "Inspect", "status": "completed" },
            { "id": "2", "text": "Implement", "status": "completed" },
            { "id": "3", "text": "Verify", "status": "pending" },
        ])
    );
}

#[test]
fn keeps_known_labels_stable_when_a_full_snapshot_changes_membership() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "fix it");
    let session = t.apply_all(
        &session,
        &[
            json!({
                "type": "tasks.updated",
                "items": [
                    { "id": "1", "text": "Inspect", "status": "completed" },
                    { "id": "2", "text": "Implement", "status": "in_progress" },
                ],
            }),
            json!({
                "type": "tasks.updated",
                "items": [
                    { "id": "2", "text": "Implementing the fix", "status": "completed" },
                    { "id": "3", "text": "Verify", "status": "in_progress" },
                ],
            }),
        ],
    );
    assert_eq!(
        task_items(&session),
        json!([
            { "id": "2", "text": "Implement", "status": "completed" },
            { "id": "3", "text": "Verify", "status": "in_progress" },
        ])
    );
}

#[test]
fn keeps_a_keyed_task_list_from_another_provider_conversation() {
    let mut t = T::new();
    let mut session = t.user(&t.session(HarnessId::Claude, "/tmp"), "first");
    session = t.apply(
        &session,
        json!({
            "type": "tasks.updated", "key": "claude-tasks", "providerSessionId": "sess_1",
            "authoritative": true, "items": [{ "id": "1", "text": "Old task", "status": "completed" }],
        }),
    );
    session = t.user(&session, "second");
    for status in ["pending", "completed"] {
        session = t.apply(
            &session,
            json!({
                "type": "tasks.updated", "key": "claude-tasks", "providerSessionId": "sess_2",
                "authoritative": true, "items": [{ "id": "1", "text": "New task", "status": status }],
            }),
        );
    }

    let lists: Vec<_> = task_blocks(&session)
        .iter()
        .map(|block| block.task_list.clone())
        .collect();
    assert_eq!(
        serde_json::to_value(&lists).unwrap(),
        json!([
            {
                "key": "claude-tasks", "providerSessionId": "sess_1",
                "items": [{ "id": "1", "text": "Old task", "status": "completed" }],
            },
            {
                "key": "claude-tasks", "providerSessionId": "sess_2",
                "items": [{ "id": "1", "text": "New task", "status": "completed" }],
            },
        ])
    );
}

#[test]
fn resets_an_in_progress_task_to_pending_when_the_turn_stops() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "fix it");
    let session = t.apply(
        &session,
        json!({
            "type": "tasks.updated",
            "items": [
                { "text": "Inspect", "status": "completed" },
                { "text": "Implement", "status": "in_progress" },
            ],
        }),
    );
    let session = t.stop(&session);
    assert_eq!(task_blocks(&session)[0].text, "[x] Inspect\n[ ] Implement");
    assert_eq!(
        task_items(&session),
        json!([
            { "text": "Inspect", "status": "completed" },
            { "text": "Implement", "status": "pending" },
        ])
    );
}

#[test]
fn keeps_authored_plans_as_separate_document_blocks() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Codex, "/tmp"), "plan it");
    let session = t.apply_all(
        &session,
        &[
            json!({ "type": "tasks.updated", "items": [{ "text": "Inspect", "status": "pending" }] }),
            json!({ "type": "plan", "text": "# Proposed approach\n\nUse two layers." }),
        ],
    );
    assert_eq!(
        roles(&session),
        [BlockRole::User, BlockRole::Tasks, BlockRole::Plan]
    );
}

#[test]
fn streams_one_plan_block_and_marks_the_final_snapshot_ready() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Codex, "/tmp"), "plan it");
    let session = t.apply_all(
        &session,
        &[
            json!({ "type": "plan", "key": "plan_1", "text": "# Approach", "append": true, "streaming": true }),
            json!({ "type": "plan", "key": "plan_1", "text": "\n\nDo the work.", "append": true, "streaming": true }),
            json!({ "type": "plan", "key": "plan_1", "text": "# Approach\n\nDo the work.", "streaming": false }),
        ],
    );
    let plans: Vec<&Block> = session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Plan)
        .collect();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].text, "# Approach\n\nDo the work.");
    assert_eq!(plans[0].streaming, Some(false));
    assert_eq!(
        serde_json::to_value(plans[0].plan.as_ref().unwrap()).unwrap(),
        json!({
            "key": "plan_1", "status": "ready",
            "originalText": "# Approach\n\nDo the work.", "edited": false,
        })
    );
}

#[test]
fn promotes_only_the_final_assistant_message_when_no_native_plan_exists() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Pi, "/tmp"), "plan it");
    let plan_text = "# Implementation plan\n\n1. Make the change.\n2. Test it.";
    let session = t.apply_all(
        &session,
        &[
            delta("I'll inspect the relevant files first."),
            json!({ "type": "message.completed" }),
            json!({ "type": "tool.started", "callId": "read_1", "title": "Read src/App.tsx", "kind": "read" }),
            json!({ "type": "tool.updated", "callId": "read_1", "title": "Read src/App.tsx", "kind": "read", "status": "completed" }),
            delta(plan_text),
            json!({ "type": "message.completed" }),
        ],
    );
    let session = promote_last_assistant_to_plan(&t.stop(&session), Some("turn:1"));

    assert_eq!(
        roles(&session),
        [
            BlockRole::User,
            BlockRole::Assistant,
            BlockRole::Tool,
            BlockRole::Plan
        ]
    );
    assert_eq!(
        session.blocks[1].text,
        "I'll inspect the relevant files first."
    );
    let plan = &session.blocks[3];
    assert_eq!(
        (plan.text.as_str(), plan.streaming),
        (plan_text, Some(false))
    );
    assert_eq!(
        serde_json::to_value(plan.plan.as_ref().unwrap()).unwrap(),
        json!({ "key": "turn:1", "status": "ready", "originalText": plan_text, "edited": false })
    );
}

#[test]
fn does_not_replace_assistant_text_when_a_native_plan_already_exists() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Codex, "/tmp"), "plan it");
    let session = t.apply_all(
        &session,
        &[
            delta("Planning complete."),
            json!({ "type": "message.completed" }),
            json!({ "type": "plan", "text": "# Native plan\n\nUse the provider artifact.", "key": "turn:1" }),
        ],
    );

    let mut promoted = session.clone();
    assert!(!promote_last_assistant_to_plan_mut(
        &mut promoted,
        Some("turn:1")
    ));
    assert_eq!(promoted, session);
    assert_eq!(
        roles(&promoted),
        [BlockRole::User, BlockRole::Assistant, BlockRole::Plan]
    );
}

#[test]
fn does_not_promote_a_provider_billing_message_into_a_plan() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Cursor, "/tmp"), "plan it");
    let session = t.apply_all(
        &session,
        &[
            delta("Upgrade your plan to continue"),
            json!({ "type": "message.completed" }),
        ],
    );
    let promoted = promote_last_assistant_to_plan(&t.stop(&session), Some("turn:1"));
    assert_eq!(roles(&promoted), [BlockRole::User, BlockRole::Assistant]);
}

// plan keys
#[test]
fn reaches_this_turns_plan_block_past_a_mid_turn_follow_up() {
    let mut t = T::new();
    let key = plan_turn_key(1, "8a0d2c1e-0000-4000-8000-000000000001");
    let session = t.user(&t.session(HarnessId::Claude, "/repo"), "plan it");
    let session = t.apply(
        &session,
        json!({ "type": "plan", "key": key, "text": "# Approach", "streaming": true }),
    );
    let session = t.steer(&session, "also cover the tests", None);
    let session = t.apply(
        &session,
        json!({ "type": "plan", "key": key, "text": "# Approach\n\nCover the tests too." }),
    );

    let plans: Vec<&Block> = session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Plan)
        .collect();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].text, "# Approach\n\nCover the tests too.");
}

#[test]
fn does_not_adopt_a_saved_plan_block_when_the_turn_counter_starts_over() {
    let mut t = T::new();
    // First run of the app: this is the session's first turn, so gen is 1.
    let session = t.user(&t.session(HarnessId::Claude, "/repo"), "plan the refactor");
    let session = t.apply(
        &session,
        json!({
            "type": "plan", "text": "# Old plan",
            "key": plan_turn_key(1, "8a0d2c1e-0000-4000-8000-000000000001"),
        }),
    );

    // The key is saved with the transcript, so it survives the restart.
    let saved: Vec<Block> =
        serde_json::from_value(serde_json::to_value(&session.blocks).unwrap()).unwrap();
    let plan_key = |blocks: &[Block]| {
        blocks
            .iter()
            .find(|block| block.role == BlockRole::Plan)
            .and_then(|block| block.plan.as_ref())
            .and_then(|plan| plan.key.clone())
    };
    assert_eq!(plan_key(&saved), plan_key(&session.blocks));

    // Second run: the counter is back to 1 and the user plans again.
    let mut reopened = session.clone();
    reopened.blocks = saved;
    let reopened = t.user(&reopened, "plan the follow-up");
    let reopened = t.apply(
        &reopened,
        json!({
            "type": "plan", "text": "# New plan",
            "key": plan_turn_key(1, "8a0d2c1e-0000-4000-8000-000000000002"),
        }),
    );

    let plans: Vec<&str> = reopened
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::Plan)
        .map(|block| block.text.as_str())
        .collect();
    assert_eq!(plans, ["# Old plan", "# New plan"]);
    // The new plan belongs to the turn that produced it, not to the old one.
    assert_eq!(reopened.blocks.last().unwrap().text, "# New plan");
}

// applyHarnessEvent context
#[test]
fn tracks_the_newest_level_instead_of_summing_turns() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/repo"),
        &[
            json!({ "type": "context", "used": 30_000, "window": 200_000 }),
            json!({ "type": "context", "used": 55_000 }),
        ],
    );
    assert_eq!(
        session.context,
        Some(ContextUsage {
            used: 55_000,
            window: Some(200_000)
        })
    );
}

#[test]
fn keeps_the_level_when_only_a_window_arrives() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/repo"),
        &[
            json!({ "type": "context", "used": 12_000 }),
            json!({ "type": "context", "window": 400_000 }),
        ],
    );
    assert_eq!(
        session.context,
        Some(ContextUsage {
            used: 12_000,
            window: Some(400_000)
        })
    );
}

#[test]
fn leaves_blocks_alone() {
    let mut t = T::new();
    let session = t.apply(
        &t.session(HarnessId::Codex, "/repo"),
        json!({ "type": "context", "used": 1_000, "window": 200_000 }),
    );
    assert!(session.blocks.is_empty());
}

// applyHarnessEvent turn metrics
#[test]
fn attaches_provider_metrics_to_the_latest_user_turn() {
    let mut t = T::new();
    let session = t.user(&t.session(HarnessId::Claude, "/repo"), "Explain this");
    let session = t.apply(
        &session,
        json!({
            "type": "turn.metrics", "inputTokens": 1_000, "outputTokens": 250,
            "cacheReadTokens": 800, "cacheHitPercent": 44.4,
        }),
    );
    assert_eq!(
        serde_json::to_value(session.blocks[0].turn_metrics.as_ref().unwrap()).unwrap(),
        json!({ "inputTokens": 1_000, "outputTokens": 250, "cacheReadTokens": 800, "cacheHitPercent": 44.4 })
    );
}

// tool enrichment
#[test]
fn retains_edit_and_write_previews_when_a_tool_completes_without_repeating_its_input() {
    for (name, input) in [
        (
            "Edit",
            json!({ "file_path": "/notes.md", "old_string": "old", "new_string": "new" }),
        ),
        (
            "Write",
            json!({ "file_path": "/notes.md", "content": "  content\n" }),
        ),
        ("Write", json!({ "file_path": "/notes.md", "content": "" })),
    ] {
        let mut t = T::new();
        let preview = preview_from_tool(name, input, None).unwrap();
        let session = t.apply_all(
            &t.session(HarnessId::Claude, "/repo"),
            &[
                json!({
                    "type": "tool.started", "callId": "edit", "title": name,
                    "kind": "edit", "status": "pending", "preview": preview,
                }),
                json!({ "type": "tool.updated", "callId": "edit", "status": "completed" }),
            ],
        );
        assert_eq!(
            tool(&session.blocks[0]).preview.as_ref(),
            Some(&preview),
            "{name}"
        );
    }
}

#[test]
fn fills_in_a_bare_read_row_when_approval_carries_the_path() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Cursor, "/repo"),
        &[
            json!({ "type": "tool.updated", "callId": "call_1", "title": "Read", "kind": "read", "status": "pending" }),
            json!({
                "type": "approval.requested", "requestId": 1, "title": "Read src/App.tsx",
                "kind": "read", "callId": "call_1",
                "preview": { "kind": "read", "path": "src/App.tsx", "fileName": "App.tsx" },
            }),
        ],
    );
    let block = by_call(&session, "call_1");
    assert_eq!(block.text, "Read src/App.tsx");
    assert_eq!(
        tool(block).preview.as_ref().unwrap().path.as_deref(),
        Some("src/App.tsx")
    );
}

#[test]
fn replaces_a_bare_bash_label_with_the_command_once_input_arrives() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/repo"),
        &[
            json!({ "type": "tool.started", "callId": "call_1", "title": "Bash", "kind": "execute", "status": "pending" }),
            json!({ "type": "tool.updated", "callId": "call_1", "title": "ls", "kind": "execute", "status": "pending" }),
        ],
    );
    assert_eq!(by_call(&session, "call_1").text, "ls");
}

#[test]
fn keeps_a_long_shell_command_instead_of_the_earlier_shell_placeholder() {
    let mut t = T::new();
    let command = format!(
        "npm run check:web 2>&1 | grep -E \"{}\"",
        "test output".repeat(28)
    );
    assert!(command.len() > 240);
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/repo"),
        &[
            json!({ "type": "tool.started", "callId": "call_1", "title": "Shell", "kind": "execute", "status": "pending" }),
            json!({ "type": "tool.updated", "callId": "call_1", "title": command, "kind": "execute", "status": "pending" }),
            json!({ "type": "tool.updated", "callId": "call_1", "status": "completed" }),
        ],
    );
    assert_eq!(session.blocks[0].text, command);
    assert_eq!(
        tool(&session.blocks[0]).status.as_deref(),
        Some("completed")
    );
}

// clarifying questions
fn questions() -> Value {
    json!([{
        "id": "Which file?", "prompt": "Which file?", "multiSelect": false, "allowCustom": true,
        "options": [{ "id": "a.ts", "label": "a.ts" }, { "id": "b.ts", "label": "b.ts" }],
    }])
}

#[test]
fn parks_the_prompt_on_the_session_instead_of_an_allow_deny_row() {
    let mut t = T::new();
    let session = t.apply(
        &t.session(HarnessId::Claude, "/repo"),
        json!({ "type": "question.asked", "requestId": 3, "title": "Which file?", "questions": questions() }),
    );
    assert_eq!(
        serde_json::to_value(session.pending_question.as_ref().unwrap()).unwrap(),
        json!({ "requestId": 3, "title": "Which file?", "questions": questions() })
    );
    assert!(session.blocks.is_empty());
}

#[test]
fn clears_the_prompt_when_the_user_answers_or_skips() {
    let mut t = T::new();
    let session = t.apply_all(
        &t.session(HarnessId::Claude, "/repo"),
        &[
            json!({ "type": "question.asked", "requestId": 3, "questions": questions() }),
            json!({ "type": "question.resolved", "requestId": 3, "decision": "answered" }),
        ],
    );
    assert_eq!(session.pending_question, None);
}

#[test]
fn drops_a_parked_prompt_when_the_turn_stops() {
    let mut t = T::new();
    let session = t.apply(
        &t.session(HarnessId::Claude, "/repo"),
        json!({ "type": "question.asked", "requestId": 3, "questions": questions() }),
    );
    let session = t.stop(&session);
    assert_eq!(session.pending_question, None);
}

// subagent steps
fn run(block: &Block) -> &AgentRunMeta {
    block.agent_run.as_ref().expect("agent run")
}

#[test]
fn keeps_model_metadata_before_steps_arrive_and_preserves_it_through_later_updates() {
    let mut t = T::new();
    let session = t.apply(
        &t.session(HarnessId::Codex, "/tmp"),
        json!({ "type": "tool.started", "callId": "spawn", "kind": "agent", "title": "Review", "agentModel": "gpt-5.6-sol" }),
    );
    assert_eq!(
        serde_json::to_value(run(&session.blocks[0])).unwrap(),
        json!({ "name": "Review", "model": "gpt-5.6-sol", "steps": [] })
    );
    let session = t.apply_all(
        &session,
        &[
            json!({ "type": "tool.updated", "callId": "spawn", "title": "Review auth" }),
            json!({ "type": "agent.step", "callId": "spawn", "stepId": "s1", "kind": "message", "text": "Checking auth" }),
            json!({ "type": "tool.updated", "callId": "spawn", "agentModel": "gpt-5.6-terra" }),
            json!({ "type": "tool.updated", "callId": "spawn", "status": "completed" }),
        ],
    );
    let agent = run(&session.blocks[0]);
    assert_eq!(agent.name, "Review auth");
    assert_eq!(agent.model.as_deref(), Some("gpt-5.6-terra"));
    assert_eq!(agent.steps.len(), 1);
    assert_eq!(agent.steps[0].text, "Checking auth");
    assert_eq!(
        tool(&session.blocks[0]).status.as_deref(),
        Some("completed")
    );
}

fn spawn(t: &mut T) -> Session {
    t.apply(
        &t.session(HarnessId::Claude, "/tmp"),
        json!({ "type": "tool.started", "callId": "agent-1", "title": "Correctness review", "kind": "agent", "status": "in_progress" }),
    )
}

#[test]
fn mirrors_a_subagents_work_onto_the_call_that_spawned_it() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let session = t.apply_all(
        &session,
        &[
            json!({
                "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
                "text": "Read src/App.tsx", "toolKind": "read", "status": "in_progress",
            }),
            json!({
                "type": "agent.step", "callId": "agent-1", "stepId": "m1", "kind": "message",
                "text": "Two regressions stand out.",
            }),
        ],
    );
    let agent = run(&session.blocks[0]);
    assert_eq!(agent.name, "Correctness review");
    assert_eq!(agent.steps.len(), 2);
    let first = &agent.steps[0];
    assert_eq!(
        (first.kind, first.text.as_str(), first.status.as_deref()),
        (AgentStepKind::Tool, "Read src/App.tsx", Some("in_progress"))
    );
    assert_eq!(agent.steps[1].kind, AgentStepKind::Message);
}

#[test]
fn settles_a_step_in_place_instead_of_repeating_it() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let session = t.apply_all(
        &session,
        &[
            json!({
                "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
                "text": "Read src/App.tsx", "status": "in_progress",
            }),
            json!({
                "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
                "text": "", "status": "completed",
            }),
        ],
    );
    let steps = &run(&session.blocks[0]).steps;
    assert_eq!(steps.len(), 1);
    // The result renames nothing: the row keeps the label the call announced.
    assert_eq!(
        (steps[0].text.as_str(), steps[0].status.as_deref()),
        ("Read src/App.tsx", Some("completed"))
    );
}

#[test]
fn drops_a_step_with_no_parent_call_to_hang_it_on() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let mut next = session.clone();
    let changed = apply_harness_event_mut(
        &mut t.env,
        &mut next,
        &ev(json!({
            "type": "agent.step", "callId": "agent-missing", "stepId": "t1", "kind": "tool",
            "text": "Read src/App.tsx",
        })),
    );
    assert!(!changed);
    assert_eq!(next, session);
}

#[test]
fn caps_a_failed_steps_error_output_like_the_parents_own() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let session = t.apply(
        &session,
        json!({
            "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
            "text": "npm test", "status": "failed", "detail": "boom ".repeat(4_000),
        }),
    );
    let detail = run(&session.blocks[0]).steps[0]
        .detail
        .clone()
        .unwrap_or_default();
    assert!(js::len(&detail) <= 8_002);
    assert!(detail.ends_with('…'));
}

#[test]
fn drops_a_blank_error_output_rather_than_carrying_it_around() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let session = t.apply(
        &session,
        json!({
            "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
            "text": "npm test", "status": "failed", "detail": "   ",
        }),
    );
    assert_eq!(run(&session.blocks[0]).steps[0].detail, None);
}

#[test]
fn keeps_the_parent_tool_blocks_own_identity() {
    let mut t = T::new();
    let session = spawn(&mut t);
    let session = t.apply_all(
        &session,
        &[
            json!({
                "type": "agent.step", "callId": "agent-1", "stepId": "t1", "kind": "tool",
                "text": "Read src/App.tsx",
            }),
            json!({
                "type": "tool.updated", "callId": "agent-1", "title": "Correctness review",
                "kind": "agent", "status": "completed", "detail": "No regressions found.",
            }),
        ],
    );
    assert_eq!(
        tool(&session.blocks[0]).status.as_deref(),
        Some("completed")
    );
    assert_eq!(run(&session.blocks[0]).steps.len(), 1);
}

// batched harness events (applyBatch.test.ts)
fn content(session: &Session) -> Session {
    let mut stripped = session.clone();
    for block in &mut stripped.blocks {
        block.id.clear();
    }
    stripped
}

fn conversation(t: &T) -> Session {
    let mut session = t.session(HarnessId::Codex, "/repo");
    session.blocks = vec![
        Block::new("user", BlockRole::User, "Help"),
        Block {
            streaming: Some(true),
            ..Block::new("reply", BlockRole::Assistant, "Hello")
        },
    ];
    session
}

fn events(values: Vec<Value>) -> Vec<HarnessEvent> {
    values.into_iter().map(ev).collect()
}

fn folded(t: &mut T, session: &Session, events: &[HarnessEvent]) -> Session {
    let mut next = session.clone();
    for event in events {
        apply_harness_event_mut(&mut t.env, &mut next, event);
    }
    next
}

fn batched(t: &mut T, session: &Session, events: &[HarnessEvent]) -> (Session, bool) {
    let mut next = session.clone();
    let changed = apply_harness_events_mut(&mut t.env, &mut next, events);
    (next, changed)
}

#[test]
fn preserves_mixed_snapshots_repeated_tokens_whitespace_and_message_boundaries() {
    let mut t = T::new();
    let events = events(vec![
        delta(" "),
        delta("Hello world"),
        delta("Hello world"),
        delta("\n"),
        delta("\n"),
        json!({ "type": "status", "text": "Working" }),
        delta("Next paragraph"),
        delta("."),
        json!({ "type": "message.completed" }),
        delta("New message"),
        delta("!"),
        json!({ "type": "reasoning.delta", "text": "" }),
        json!({ "type": "reasoning.delta", "text": "Think" }),
        json!({ "type": "reasoning.delta", "text": "Think carefully" }),
        json!({ "type": "reasoning.completed" }),
        json!({ "type": "tool.started", "callId": "read", "title": "Read" }),
        json!({ "type": "approval.requested", "callId": "read", "requestId": 1, "title": "Read" }),
        json!({ "type": "approval.resolved", "requestId": 1, "decision": "allow" }),
        json!({ "type": "tool.updated", "callId": "read", "status": "completed" }),
        delta("Done"),
        delta("."),
    ]);
    let session = conversation(&t);
    let (batch, _) = batched(&mut t, &session, &events);
    let one_by_one = folded(&mut t, &session, &events);
    assert_eq!(content(&batch), content(&one_by_one));
    assert_eq!(session.blocks[1].text, "Hello");
}

#[test]
fn does_not_treat_combined_token_fragments_as_a_snapshot_of_existing_text() {
    let mut t = T::new();
    let mut session = conversation(&t);
    session.blocks[1].text = "abc".into();
    let (next, _) = batched(&mut t, &session, &events(vec![delta("a"), delta("bc")]));
    assert_eq!(next.blocks[1].text, "abcabc");
}

#[test]
fn retains_identity_for_empty_reasoning_and_repeated_full_snapshots() {
    let mut t = T::new();
    let session = conversation(&t);
    for values in [
        vec![],
        vec![
            json!({ "type": "reasoning.delta", "text": "" }),
            json!({ "type": "reasoning.delta", "text": "" }),
        ],
        vec![delta("Hello"), delta("Hello")],
    ] {
        let (next, changed) = batched(&mut t, &session, &events(values));
        assert!(!changed);
        assert_eq!(next, session);
    }
}

#[test]
fn handles_ten_simultaneous_long_histories_without_cloning_history_blocks() {
    let mut t = T::new();
    let events: Vec<HarnessEvent> = (0..64)
        .map(|index| ev(delta(&format!(" token-{index}"))))
        .collect();
    for _thread in 0..10 {
        let mut session = conversation(&t);
        let history = (0..2_000).map(|index| {
            Block::new(
                format!("history-{index}"),
                BlockRole::User,
                format!("History {index}"),
            )
        });
        session.blocks.splice(0..0, history);
        let expected = folded(&mut t, &session, &events);
        let before: Vec<*const u8> = session.blocks[..2_001]
            .iter()
            .map(|block| block.text.as_ptr())
            .collect();
        assert!(apply_harness_events_mut(&mut t.env, &mut session, &events));
        assert_eq!(session, expected);
        // Folding in place leaves every history block's text where it was.
        let after: Vec<*const u8> = session.blocks[..2_001]
            .iter()
            .map(|block| block.text.as_ptr())
            .collect();
        assert_eq!(before, after);
    }
}

#[test]
fn reports_no_change_where_the_typescript_kept_the_session() {
    let mut t = T::new();
    let session = t.session(HarnessId::Claude, "/repo");
    for value in [
        json!({ "type": "session.started" }),
        json!({ "type": "session.ended", "code": 0 }),
        json!({ "type": "image.generated", "itemId": "i", "data": "AAAA", "name": "x.png" }),
        json!({ "type": "question.updated", "requestId": 9 }),
        json!({ "type": "question.resolved", "requestId": 9, "decision": "skipped" }),
        json!({ "type": "turn.started", "providerTurnId": "t1" }),
        json!({ "type": "background.updated", "tasks": [] }),
        json!({ "type": "plan", "text": "" }),
        json!({ "type": "tasks.updated", "items": [] }),
    ] {
        let mut next = session.clone();
        assert!(
            !apply_harness_event_mut(&mut t.env, &mut next, &ev(value.clone())),
            "{value}"
        );
        assert_eq!(next, session);
    }
}
