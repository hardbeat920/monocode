//! Port of src/features/sessions/model/btw.test.ts. The `consumeBtwCommand`
//! and `consumeBtwPrefix` cases live with the composer's port of those
//! functions (view-composer `composer::model::commands`).

use monocode_core::block::{Block, BlockRole, BtwMessage, BtwMessageRole, BtwThread};
use monocode_core::btw::{
    BTW_MAX_BLOCK_CHARS, BTW_MAX_SNAPSHOT_CHARS, session_has_btw_threads, supports_btw_harness,
};
use monocode_core::models::ModelCatalog;
use monocode_core::{HarnessEvent, HarnessId, js};
use serde_json::{Value, json};

use super::*;

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn block(id: &str, role: BlockRole) -> Block {
    Block::new(id, role, id)
}

fn text_block(id: &str, role: BlockRole, text: &str) -> Block {
    Block::new(id, role, text)
}

fn thread(overrides: Value) -> BtwThread {
    let mut base = json!({
        "id": "btw-1",
        "sourceEndBlockId": "user-1",
        "createdAt": 1,
        "updatedAt": 2,
        "status": "ready",
        "messages": [
            { "id": "question", "role": "user", "text": "Why?", "createdAt": 1 },
            { "id": "answer", "role": "assistant", "text": "Because.", "createdAt": 2 }
        ]
    });
    for (key, value) in overrides.as_object().cloned().unwrap_or_default() {
        base[key] = value;
    }
    from_json(base)
}

fn message(id: &str, role: BtwMessageRole, text: &str, created_at: i64) -> BtwMessage {
    from_json(json!({ "id": id, "role": role, "text": text, "createdAt": created_at }))
}

fn ids(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|block| block.id.as_str()).collect()
}

fn handoff_session_blocks(second_done: bool) -> Vec<Block> {
    let mut blocks = vec![
        json!({ "id": "u1", "role": "user", "text": "first", "durationMs": 1000 }),
        json!({ "id": "a1", "role": "assistant", "text": "done" }),
        json!({
            "id": "h", "role": "handoff", "text": "",
            "handoff": { "from": "claude", "to": "fx", "status": "ready" }
        }),
        json!({ "id": "u2", "role": "user", "text": "second", "durationMs": 900 }),
    ];
    if second_done {
        blocks.push(json!({ "id": "a2", "role": "assistant", "text": "done" }));
    }
    blocks.into_iter().map(from_json).collect()
}

// supportsBtwHarness

#[test]
fn allows_harnesses_with_an_isolated_text_runner() {
    assert!(supports_btw_harness(Some(HarnessId::Claude)));
    assert!(supports_btw_harness(Some(HarnessId::Codex)));
    assert!(supports_btw_harness(Some(HarnessId::Opencode)));
    assert!(supports_btw_harness(Some(HarnessId::Pi)));
}

#[test]
fn rejects_unsupported_and_missing_harnesses() {
    assert!(!supports_btw_harness(Some(HarnessId::Fx)));
    assert!(!supports_btw_harness(Some(HarnessId::Hermes)));
    assert!(!supports_btw_harness(Some(HarnessId::Antigravity)));
    assert!(!supports_btw_harness(None));
}

// btwOpenTargetTurnId

#[test]
fn targets_the_latest_completed_turn_while_the_current_turn_is_still_running() {
    let blocks: Vec<Block> = vec![
        from_json(json!({ "id": "u1", "role": "user", "text": "first", "durationMs": 1000 })),
        from_json(json!({ "id": "a1", "role": "assistant", "text": "done" })),
        from_json(json!({ "id": "u2", "role": "user", "text": "second" })),
        from_json(json!({ "id": "a2", "role": "assistant", "text": "working", "streaming": true })),
    ];
    let turns = vec![
        vec![blocks[0].clone(), blocks[1].clone()],
        vec![blocks[2].clone(), blocks[3].clone()],
    ];
    assert_eq!(
        btw_open_target_turn_id(&turns, &blocks, HarnessId::Claude, false).as_deref(),
        Some("u1")
    );
}

#[test]
fn skips_the_latest_completed_turn_when_its_provider_cannot_run_btw() {
    let blocks = handoff_session_blocks(true);
    let turns = vec![
        vec![blocks[0].clone(), blocks[1].clone()],
        vec![blocks[3].clone(), blocks[4].clone()],
    ];
    assert_eq!(
        btw_open_target_turn_id(&turns, &blocks, HarnessId::Fx, false).as_deref(),
        Some("u1")
    );
}

// btwTurnHarness

#[test]
fn keeps_btw_available_on_pre_handoff_turns_after_the_session_moves_on() {
    let blocks = handoff_session_blocks(false);
    let first = blocks[..2].to_vec();
    assert_eq!(
        btw_turn_harness(&blocks, &first, HarnessId::Fx),
        Some(HarnessId::Claude)
    );
}

// sessionHasBtwEligibleTurn

#[test]
fn returns_true_when_only_an_earlier_turn_can_accept_btw() {
    let blocks = handoff_session_blocks(true);
    assert!(session_has_btw_eligible_turn(&blocks, HarnessId::Fx, false));
}

// resolveBtwHarness

#[test]
fn falls_back_to_a_stored_thread_harness_after_a_handoff() {
    let threads = [thread(json!({ "harness": "claude" }))];
    assert_eq!(
        resolve_btw_harness(Some(HarnessId::Fx), Some(&threads)),
        Some(HarnessId::Claude)
    );
}

// sessionHasBtwThreads

#[test]
fn detects_persisted_side_threads() {
    let mut user = text_block("u1", BlockRole::User, "hi");
    user.btw_threads = Some(vec![thread(json!({}))]);
    assert!(session_has_btw_threads(&[user]));
}

// sessionBtwThreads

#[test]
fn lists_threads_from_every_turn_oldest_first_with_their_turn() {
    let mut u1 = text_block("u1", BlockRole::User, "first");
    u1.duration_ms = Some(1);
    u1.btw_threads = Some(vec![thread(json!({ "id": "late", "createdAt": 30 }))]);
    let mut u2 = text_block("u2", BlockRole::User, "second");
    u2.duration_ms = Some(1);
    u2.btw_threads = Some(vec![thread(json!({ "id": "early", "createdAt": 10 }))]);
    let blocks = vec![
        u1,
        block("a1", BlockRole::Assistant),
        u2,
        block("a2", BlockRole::Assistant),
    ];
    let entries = session_btw_threads(&blocks, false);
    let thread_ids: Vec<&str> = entries
        .iter()
        .map(|entry| entry.thread.id.as_str())
        .collect();
    assert_eq!(thread_ids, ["early", "late"]);
    assert_eq!(entries[0].turn[0].id, "u2");
    assert_eq!(entries[1].turn[0].id, "u1");
}

#[test]
fn returns_nothing_when_the_session_has_no_side_threads() {
    assert!(session_btw_threads(&[block("u1", BlockRole::User)], false).is_empty());
}

// btwThreadBlocks

#[test]
fn turns_answered_questions_into_settled_transcript_turns() {
    let messages = [
        message("q1", BtwMessageRole::User, "Why?", 100),
        message("a1", BtwMessageRole::Assistant, "Because.", 350),
    ];
    let blocks = btw_thread_blocks(
        BtwThreadBlocksInput {
            messages: &messages,
            running: false,
            harness: Some(HarnessId::Claude),
            model: Some("sonnet"),
            ..Default::default()
        },
        &ModelCatalog::new(),
    );
    assert_eq!(blocks.len(), 2);
    let question = &blocks[0];
    assert_eq!(question.id, "q1");
    assert_eq!(question.role, BlockRole::User);
    assert_eq!(question.started_at, Some(100));
    assert_eq!(question.duration_ms, Some(250));
    let turn_model = question.turn_model.as_ref().unwrap();
    assert_eq!(turn_model.harness, HarnessId::Claude);
    assert_eq!(turn_model.id, "sonnet");
    assert_eq!(
        blocks[1],
        Block::new("a1", BlockRole::Assistant, "Because.")
    );
}

#[test]
fn uses_a_replys_own_activity_blocks_when_it_has_them() {
    let mut answer = message("a1", BtwMessageRole::Assistant, "", 2);
    answer.blocks = Some(vec![block("tool-1", BlockRole::Tool)]);
    let messages = [message("q1", BtwMessageRole::User, "Look", 1), answer];
    let blocks = btw_thread_blocks(
        BtwThreadBlocksInput {
            messages: &messages,
            ..Default::default()
        },
        &ModelCatalog::new(),
    );
    assert_eq!(ids(&blocks), ["q1", "tool-1"]);
}

#[test]
fn leaves_the_streaming_question_open_with_its_live_blocks() {
    let messages = [message("q1", BtwMessageRole::User, "Now?", 1)];
    let pending = [block("live", BlockRole::Assistant)];
    let blocks = btw_thread_blocks(
        BtwThreadBlocksInput {
            messages: &messages,
            pending_blocks: Some(&pending),
            running: true,
            ..Default::default()
        },
        &ModelCatalog::new(),
    );
    assert_eq!(blocks[0].duration_ms, None);
    assert_eq!(ids(&blocks), ["q1", "live"]);
}

#[test]
fn closes_a_question_that_failed_without_an_answer() {
    let messages = [message("q1", BtwMessageRole::User, "Now?", 10)];
    let blocks = btw_thread_blocks(
        BtwThreadBlocksInput {
            messages: &messages,
            running: false,
            updated_at: Some(40),
            ..Default::default()
        },
        &ModelCatalog::new(),
    );
    assert_eq!(blocks[0].duration_ms, Some(30));
}

// btwVisibleBlocks

#[test]
fn returns_only_visible_blocks_through_the_anchored_turn() {
    let mut hidden_user = block("hidden-user", BlockRole::User);
    hidden_user.internal = Some(true);
    let mut hidden_tool = block("hidden-tool", BlockRole::Tool);
    hidden_tool.orchestration = Some(from_json(json!({
        "version": 1,
        "leadId": "lead",
        "cwd": "/repo",
        "request": "Do it",
        "author": { "harness": "claude", "model": "claude:opus-5", "name": "Opus 5" },
        "settings": { "maxWorkers": 2, "choices": [] },
        "status": "ready",
        "title": "Ship",
        "summary": "",
        "tasks": []
    })));
    let blocks = vec![
        block("before", BlockRole::User),
        hidden_user,
        block("assistant", BlockRole::Assistant),
        block("reasoning", BlockRole::Reasoning),
        hidden_tool,
        block("tool", BlockRole::Tool),
        block("tasks", BlockRole::Tasks),
        block("plan", BlockRole::Plan),
        block("user-1", BlockRole::User),
        block("after", BlockRole::Assistant),
    ];
    assert_eq!(
        ids(&btw_visible_blocks(&blocks, "user-1")),
        ["before", "assistant", "tool", "tasks", "plan", "user-1"]
    );
}

#[test]
fn returns_no_blocks_when_the_anchor_is_no_longer_present() {
    assert!(btw_visible_blocks(&[block("user-1", BlockRole::User)], "missing").is_empty());
}

// serializeBtwBlock

#[test]
fn serializes_generated_image_blocks_by_name_and_description() {
    let image: Block = from_json(json!({
        "id": "image-1",
        "role": "image",
        "text": "",
        "image": {
            "path": "/app-data/generated-images/image.png",
            "name": "generated-image",
            "mimeType": "image/png",
            "size": 8,
            "alt": "A clean product photo"
        }
    }));
    assert_eq!(
        serialize_btw_block(&image, None),
        "Image: generated-image — A clean product photo"
    );
}

#[test]
fn serializes_regular_blocks_and_attachments_with_normalized_text() {
    let user: Block = from_json(json!({
        "id": "u",
        "role": "user",
        "text": "  first line\r\nsecond line  ",
        "attachments": [
            { "id": "file-1", "name": " screenshot.png ", "mimeType": " image/png ", "kind": "image", "size": 10 },
            { "id": "file-2", "name": "", "mimeType": "text/plain", "kind": "file", "size": 20 }
        ]
    }));
    assert_eq!(
        serialize_btw_block(&user, None),
        "User: first line\nsecond line\nAttachment: screenshot.png (image/png)\nAttachment: unnamed file (text/plain)"
    );
}

#[test]
fn includes_tool_metadata_and_displays_preview_paths_relative_to_cwd() {
    let tool: Block = from_json(json!({
        "id": "tool",
        "role": "tool",
        "text": "ignored tool text",
        "tool": {
            "title": "Read file",
            "kind": "read",
            "status": "completed",
            "detail": "  loaded\r\n  successfully ",
            "preview": {
                "kind": "read",
                "path": "/repo/src/App.tsx",
                "query": "  TODO  ",
                "startLine": 12,
                "additions": 2,
                "deletions": 1,
                "output": "  result\r\n  ",
                "lines": [
                    { "number": 12, "kind": "context", "text": "const app = true;" },
                    { "kind": "add", "text": "new line" }
                ]
            }
        }
    }));
    assert_eq!(
        serialize_btw_block(&tool, Some("/repo")),
        "Tool: Read file\nKind: read\nStatus: completed\nDetail: loaded\n  successfully\nFile: src/App.tsx\nQuery: TODO\nStart line: 12\nAdditions: 2\nDeletions: 1\nOutput:\nresult\nLines:\n12: context: const app = true;\nadd: new line"
    );
}

// serializeBtwSnapshot

#[test]
fn throws_when_the_completed_turn_is_unavailable() {
    assert_eq!(
        serialize_btw_snapshot(&[], "missing", None),
        Err("The completed turn is no longer available.".to_string())
    );
}

#[test]
fn keeps_the_newest_context_and_bounds_both_block_and_snapshot_size() {
    let mut blocks = vec![text_block("old", BlockRole::User, "old context")];
    for index in 0..9 {
        blocks.push(text_block(
            &format!("assistant-{index}"),
            BlockRole::Assistant,
            &"m".repeat(BTW_MAX_BLOCK_CHARS + 100),
        ));
    }
    blocks.push(text_block("user-1", BlockRole::User, "latest context"));

    let snapshot = serialize_btw_snapshot(&blocks, "user-1", None).unwrap();

    assert!(js::len(&snapshot) <= BTW_MAX_SNAPSHOT_CHARS);
    assert!(snapshot.contains("User: latest context"));
    assert!(!snapshot.contains("User: old context"));
    assert!(snapshot.contains('…'));
    assert!(!snapshot.contains(&"m".repeat(BTW_MAX_BLOCK_CHARS)));
}

// buildBtwPrompt

#[test]
fn combines_safety_instructions_transcript_context_and_side_messages() {
    let blocks = vec![
        text_block("user-1", BlockRole::User, "  Explain this\r\n  change. "),
        text_block(
            "assistant-1",
            BlockRole::Assistant,
            "It changes the parser.",
        ),
    ];
    let side = thread(json!({
        "sourceEndBlockId": "assistant-1",
        "messages": [
            { "id": "q", "role": "user", "text": "  What does it affect? ", "createdAt": 1 },
            { "id": "a", "role": "assistant", "text": " The parser. ", "createdAt": 2 }
        ]
    }));
    let prompt = build_btw_prompt(&blocks, &side, None).unwrap();

    assert!(prompt.contains("You are answering an isolated, read-only"));
    assert!(prompt.contains("The main conversation snapshot below is reference context only"));
    assert!(prompt.contains("User: Explain this\n  change."));
    assert!(prompt.contains("Assistant: It changes the parser."));
    assert!(prompt.contains("User: What does it affect?\n\nAssistant: The parser."));
}

#[test]
fn uses_a_placeholder_when_a_thread_has_no_non_empty_messages() {
    let blocks = vec![text_block("user-1", BlockRole::User, "Context")];
    let side = thread(json!({
        "messages": [{ "id": "empty", "role": "user", "text": " \r\n ", "createdAt": 1 }]
    }));
    let prompt = build_btw_prompt(&blocks, &side, None).unwrap();

    assert!(prompt.contains("## By-the-way conversation\n(no side question yet)"));
}

// applyBtwHarnessEvent

#[test]
fn records_harness_activity_blocks_for_a_btw_reply() {
    let blocks = apply_btw_harness_event(
        &[],
        &HarnessEvent::ReasoningDelta {
            text: "Checking docs".into(),
        },
        HarnessId::Codex,
        "codex:gpt-5.4",
        "user-1",
    );
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].role, BlockRole::Reasoning);
    assert_eq!(blocks[0].text, "Checking docs");
    assert_eq!(blocks[0].streaming, Some(true));
}

#[test]
fn seals_streaming_reply_blocks() {
    let live = apply_btw_harness_event(
        &[],
        &HarnessEvent::MessageDelta {
            text: "Because.".into(),
        },
        HarnessId::Claude,
        "claude:sonnet",
        "user-1",
    );
    assert!(live[0].is_streaming());
    let sealed = seal_btw_response_blocks(&live, HarnessId::Claude, "claude:sonnet", "user-1");
    assert_eq!(sealed.len(), 1);
    assert!(!sealed[0].is_streaming());
    assert_eq!(sealed[0].text, "Because.");
}

// replaceBtwThread

#[test]
fn replaces_an_existing_thread_without_mutating_the_block() {
    let original = thread(json!({}));
    let mut with_thread = text_block("user-1", BlockRole::User, "Question");
    with_thread.btw_threads = Some(vec![original.clone()]);
    let updated = thread(json!({ "status": "error", "error": "Failed" }));

    let result = replace_btw_thread(&with_thread, updated.clone()).unwrap();

    assert_eq!(result.btw_threads, Some(vec![updated]));
    assert_eq!(with_thread.btw_threads, Some(vec![original]));
}

#[test]
fn returns_the_same_block_when_the_thread_is_not_present() {
    let mut source = block("user-1", BlockRole::User);
    source.btw_threads = Some(vec![thread(json!({}))]);

    assert_eq!(
        replace_btw_thread(&source, thread(json!({ "id": "missing" }))),
        None
    );
}
