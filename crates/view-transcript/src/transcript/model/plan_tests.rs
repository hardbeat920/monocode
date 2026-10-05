//! Layout cases from src/features/sessions/ui/AgentTranscript.test.ts, checked
//! on the rows instead of rendered markup.

use super::*;
use monocode_core::block::{
    AgentRunMeta, AgentStepKind, BlockNotice, BlockTool, InterjectionMeta, InterjectionSeverity,
    TurnModel,
};
use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationSettings,
};
use monocode_core::transcript::activity::{
    ToolCallState, activity_phase_title, build_activity_phases, tool_call_state,
};
use monocode_core::transcript::fixtures::*;

/// `tool()` in AgentTranscript.test.ts.
fn tool(id: &str, approval: Option<i64>) -> Block {
    let mut block = Block::new(id, BlockRole::Tool, format!("Inspect hidden-detail-{id}"));
    block.tool = Some(BlockTool {
        kind: Some("shell".into()),
        status: Some(
            if approval.is_some() {
                "pending"
            } else {
                "completed"
            }
            .into(),
        ),
        ..Default::default()
    });
    match approval {
        Some(request) => with_approval(block, request),
        None => block,
    }
}

fn options(busy: bool) -> PlanOptions {
    PlanOptions {
        busy,
        visible: true,
        ..Default::default()
    }
}

fn plan(blocks: Vec<Block>, busy: bool) -> Vec<Row> {
    plan_with(blocks, options(busy))
}

fn plan_with(blocks: Vec<Block>, options: PlanOptions) -> Vec<Row> {
    let blocks = visible_blocks(&refs(blocks), options.harness);
    build_plan(&blocks, &options, &PlanState::default(), None)
}

/// Every block id the rows draw, in order.
fn drawn(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .flat_map(|row| match &row.kind {
            RowKind::Item { item, .. } => {
                item.blocks().iter().map(|block| block.id.clone()).collect()
            }
            RowKind::Proposal(block) => vec![block.id.clone()],
            RowKind::FoldLine(_) => vec!["<fold>".into()],
            RowKind::Accessory => vec!["<accessory>".into()],
            RowKind::Footer(_) => vec!["<footer>".into()],
        })
        .collect()
}

fn fold_line(rows: &[Row]) -> &FoldLine {
    rows.iter()
        .find_map(|row| match &row.kind {
            RowKind::FoldLine(line) => Some(line),
            _ => None,
        })
        .expect("a fold line")
}

fn fold_text(rows: &[Row]) -> String {
    match &fold_line(rows).title {
        FoldTitle::Text(text) => text.clone(),
        FoldTitle::Live { .. } => "<live>".into(),
    }
}

fn footer(rows: &[Row]) -> &TurnFooter {
    rows.iter()
        .find_map(|row| match &row.kind {
            RowKind::Footer(footer) => Some(footer),
            _ => None,
        })
        .expect("a footer")
}

/// The index of the row that draws `id`.
fn position(rows: &[Row], id: &str) -> usize {
    rows.iter()
        .position(|row| {
            drawn(std::slice::from_ref(row))
                .iter()
                .any(|drawn| drawn == id)
        })
        .unwrap_or_else(|| panic!("{id} is not drawn"))
}

fn model(name: &str) -> TurnModel {
    TurnModel {
        harness: HarnessId::Claude,
        id: "claude:sonnet-5".into(),
        name: name.into(),
        extra: Default::default(),
    }
}

#[test]
fn keeps_the_completed_time_beside_actions() {
    let rows = plan(
        vec![
            timed_user("user", "Inspect", 1_000, 2_000),
            note("answer", "Done"),
        ],
        false,
    );
    assert_eq!(fold_text(&rows), "Worked for 2s");
    let footer = footer(&rows);
    assert!(footer.label_hidden);
    assert_eq!(footer.completed_at, Some(3_000));
    assert_eq!(footer.copy_text, "Done");
}

#[test]
fn shows_monocode_cli_actions_as_one_app_group() {
    let command = "/repo/target/debug/MonoCode.app/Contents/MacOS/monocode";
    let mut help = Block::new("help", BlockRole::Tool, format!("{command} app --help"));
    help.tool = Some(BlockTool {
        kind: Some("shell".into()),
        status: Some("completed".into()),
        ..Default::default()
    });
    let mut notes = Block::new(
        "notes",
        BlockRole::Tool,
        format!("{command} app notes.list --json '{{}}'"),
    );
    notes.tool = Some(BlockTool {
        kind: Some("shell".into()),
        status: Some("in_progress".into()),
        ..Default::default()
    });
    let rows = plan(
        vec![user("user", "/monocode list my notes"), help, notes],
        true,
    );
    let activity = rows
        .iter()
        .find_map(|row| match &row.kind {
            RowKind::Item {
                item: TurnItem::Activity(blocks),
                view,
                ..
            } => Some((blocks.clone(), *view)),
            _ => None,
        })
        .expect("an activity row");
    assert_eq!(activity.1, ItemView::Activity { done: false });
    let phases = build_activity_phases(&activity.0);
    assert_eq!(activity_phase_title(&phases[0], true), "Using MonoCode");
}

#[test]
fn hides_provider_authentication_errors_handled_by_the_sign_in_modal() {
    let mut error = Block::new(
        "auth-error",
        BlockRole::System,
        "Authentication required\n\nGrok Build is not signed in.",
    );
    error.notice = Some(BlockNotice::Error);
    let rows = plan_with(
        vec![error.clone()],
        PlanOptions {
            harness: Some(HarnessId::Grok),
            ..options(false)
        },
    );
    assert!(rows.is_empty());
    // A provider without a sign-in flow still shows the error.
    let rows = plan_with(
        vec![error],
        PlanOptions {
            harness: Some(HarnessId::Pi),
            ..options(false)
        },
    );
    assert_eq!(drawn(&rows), ["auth-error"]);
}

fn proposal(status: OrchestrationProposalStatus) -> Block {
    let choice = OrchestrationChoice {
        harness: HarnessId::Claude,
        model: "claude:test".into(),
        name: "Lead".into(),
        extra: Default::default(),
    };
    let mut card = Block::new("proposal", BlockRole::Plan, "Assignment plan");
    card.orchestration = Some(OrchestrationProposal {
        version: 1,
        lead_id: "lead".into(),
        cwd: "/repo".into(),
        checkout_cwd: None,
        request: "Build".into(),
        author: choice.clone(),
        settings: OrchestrationSettings {
            choices: vec![choice],
            max_workers: 2,
            extra: Default::default(),
        },
        status,
        title: "Proposed assignments".into(),
        summary: "Implement and verify".into(),
        tasks: Vec::new(),
        error: None,
        response: None,
        extra: Default::default(),
    });
    card
}

#[test]
fn reveals_an_orchestration_result_after_the_finished_turn_and_before_its_action_row() {
    let blocks = |status| {
        vec![
            timed_user("user", "Build", 1_000, 500),
            // Existing records have the card before the work.
            proposal(status),
            tool("inspection", None),
            note("answer", "The investigation is complete."),
        ]
    };
    let live = plan(blocks(OrchestrationProposalStatus::Ready), true);
    assert!(!drawn(&live).contains(&"proposal".to_string()));
    let finished = plan(blocks(OrchestrationProposalStatus::Ready), false);
    assert!(position(&finished, "answer") < position(&finished, "proposal"));
    assert!(position(&finished, "proposal") < position(&finished, "<footer>"));
    assert_eq!(
        drawn(&finished)
            .iter()
            .filter(|id| *id == "proposal")
            .count(),
        1
    );
    assert_eq!(fold_text(&finished), "Worked for 1s");
    let planning = plan(blocks(OrchestrationProposalStatus::Planning), false);
    assert!(!drawn(&planning).contains(&"proposal".to_string()));
}

#[test]
fn keeps_each_completed_turns_recorded_model_label() {
    let mut prompt = user("user", "Remember this");
    prompt.duration_ms = Some(9_000);
    prompt.turn_model = Some(model("Claude Sonnet 5"));
    let rows = plan_with(
        vec![prompt, note("answer", "Remembered.")],
        PlanOptions {
            harness: Some(HarnessId::Claude),
            current_model_name: Some("Claude Opus 5".into()),
            ..options(false)
        },
    );
    assert_eq!(fold_text(&rows), "Claude Sonnet 5 worked for 9s");
}

#[test]
fn does_not_assign_the_current_model_to_a_legacy_completed_turn() {
    let mut prompt = user("user", "Old prompt");
    prompt.duration_ms = Some(9_000);
    let rows = plan_with(
        vec![prompt, note("answer", "Old answer.")],
        PlanOptions {
            harness: Some(HarnessId::Claude),
            current_model_name: Some("Claude Opus 5".into()),
            ..options(false)
        },
    );
    assert_eq!(fold_text(&rows), "Worked for 9s");
}

#[test]
fn marks_the_edited_message() {
    let rows = plan_with(
        vec![user("user", "Edit this prompt")],
        PlanOptions {
            can_edit_last_turn: true,
            editing_last_turn: true,
            ..options(false)
        },
    );
    assert_eq!(
        rows[0].kind,
        RowKind::Item {
            item: TurnItem::Block(Arc::new(user("user", "Edit this prompt"))),
            index: 0,
            view: ItemView::Block {
                under_line: false,
                can_edit: true,
                editing: true
            },
            placement: Placement::Plain,
        }
    );
}

#[test]
fn renders_the_summary_and_answer_without_a_row_per_completed_tool() {
    let mut blocks = vec![user("user", "Check the project")];
    blocks.extend((0..1357).map(|index| tool(&index.to_string(), None)));
    blocks.push(note("answer", "The project checks passed."));
    let rows = plan(blocks.clone(), false);
    assert_eq!(drawn(&rows), ["user", "<fold>", "answer"]);
    let line = fold_line(&rows);
    assert!(line.expandable && !line.open);
    let short = plan(
        vec![
            blocks[0].clone(),
            tool("one", None),
            tool("two", None),
            blocks.last().unwrap().clone(),
        ],
        false,
    );
    assert_eq!(short.len(), rows.len());
}

#[test]
fn opens_the_fold_body_when_the_reader_asks() {
    let blocks = refs(vec![
        user("user", "Check the project"),
        tool("a", None),
        note("mid", "Halfway."),
        tool("b", None),
        note("answer", "Done."),
    ]);
    let mut state = PlanState::default();
    state.open_work.insert("user".into(), true);
    let rows = build_plan(&blocks, &options(false), &state, None);
    assert_eq!(drawn(&rows), ["user", "<fold>", "a", "mid", "b", "answer"]);
    let placements: Vec<Placement> = rows
        .iter()
        .filter_map(|row| match &row.kind {
            RowKind::Item { placement, .. } => Some(*placement),
            _ => None,
        })
        .collect();
    assert_eq!(
        placements,
        [
            Placement::Plain,
            Placement::FoldBody {
                tail: false,
                prose: false
            },
            Placement::FoldBody {
                tail: false,
                prose: true
            },
            Placement::FoldBody {
                tail: true,
                prose: false
            },
            Placement::Plain,
        ]
    );
    assert!(fold_line(&rows).open);
}

#[test]
fn keeps_live_work_visible_before_the_assistant_answers() {
    let rows = plan(vec![tool("live", None)], true);
    assert!(drawn(&rows).contains(&"live".to_string()));
    assert!(matches!(fold_line(&rows).title, FoldTitle::Live { .. }));
}

#[test]
fn keeps_an_unresolved_approval_visible_even_when_narration_follows_it() {
    let rows = plan(
        vec![
            tool("approval", Some(1)),
            note("answer", "Please approve the command."),
        ],
        true,
    );
    assert!(drawn(&rows).contains(&"approval".to_string()));
    assert!(drawn(&rows).contains(&"answer".to_string()));
    assert!(!fold_line(&rows).expandable);
    match &fold_line(&rows).title {
        FoldTitle::Live { paused, .. } => assert!(*paused),
        other => panic!("expected a live title, got {other:?}"),
    }
}

fn failed_agent() -> Block {
    let mut block = agent("agent", "Inspect auth", "failed");
    let tool = block.tool.as_mut().unwrap();
    tool.call_id = Some("agent-1".into());
    tool.detail = Some("Child process disconnected".into());
    block
}

#[test]
fn keeps_a_failed_subagents_own_row_live_and_settled() {
    for busy in [true, false] {
        let mut prompt = user("user", "Delegate this");
        prompt.started_at = Some(1_000);
        let rows = plan(
            vec![
                prompt,
                failed_agent(),
                note("answer", "I could not finish."),
            ],
            busy,
        );
        let stack = rows
            .iter()
            .find(|row| {
                matches!(
                    &row.kind,
                    RowKind::Item {
                        item: TurnItem::Subagents(_),
                        ..
                    }
                )
            })
            .expect("the run keeps its row");
        assert_eq!(stack.first_block().map(|b| b.id.as_str()), Some("agent"));
        assert_eq!(
            tool_call_state(stack.first_block().unwrap()),
            ToolCallState::Rejected
        );
    }
}

#[test]
fn gives_each_running_subagent_its_own_row_above_the_work_that_folds() {
    let mut prompt = user("user", "Review this");
    prompt.started_at = Some(1_000);
    let mut a1 = agent_run(
        "a1",
        "Correctness review",
        "in_progress",
        vec![step(
            "s1",
            AgentStepKind::Tool,
            "Read src/App.tsx",
            Some("completed"),
        )],
    );
    a1.agent_run.as_mut().unwrap().model = Some("claude-haiku-4-5".into());
    let mut a2 = agent("a2", "Quality review", "in_progress");
    a2.agent_run = Some(AgentRunMeta {
        name: "Quality review".into(),
        model: Some("custom-review-model".into()),
        ..Default::default()
    });
    let rows = plan(
        vec![
            prompt,
            note("lead", "I will run two reviews."),
            a1,
            a2,
            tool("t1", None),
            note("answer", "Both reviewers agree."),
        ],
        true,
    );
    // The stack sits outside the fold body, under the fold line.
    let stack_at = position(&rows, "a1");
    assert_eq!(position(&rows, "a2"), stack_at);
    assert!(position(&rows, "<fold>") < stack_at);
    assert!(stack_at < position(&rows, "answer"));
    assert!(!drawn(&rows).contains(&"t1".to_string()));
    assert!(matches!(
        &rows[stack_at].kind,
        RowKind::Item {
            placement: Placement::FoldSubagents,
            view: ItemView::Subagents { live: true },
            ..
        }
    ));
}

#[test]
fn keeps_the_turns_status_line_at_the_top_of_the_turn_above_a_stack() {
    let mut prompt = user("user", "Review this");
    prompt.started_at = Some(1_000);
    let rows = plan(
        vec![
            prompt,
            note("lead", "I will run two reviews."),
            tool("t0", None),
            note("plan", "Splitting the review in two."),
            agent(
                "a1",
                "Independently review the current repository's recent changes for correctness and regressions. Inspect the uncommitted diff.",
                "in_progress",
            ),
            tool("t1", None),
            note("answer", "Both reviewers agree."),
        ],
        true,
    );
    assert!(position(&rows, "<fold>") < position(&rows, "a1"));
    assert!(fold_line(&rows).expandable);
    let shown = drawn(&rows);
    assert!(!shown.contains(&"t1".to_string()));
    assert!(!shown.contains(&"plan".to_string()));
}

#[test]
fn places_a_session_accessory_after_the_latest_reply_and_before_its_action_row() {
    let rows = plan_with(
        vec![
            timed_user("user", "Change the files", 1_000, 500),
            note("answer", "Done changing files."),
        ],
        PlanOptions {
            has_accessory: true,
            ..options(false)
        },
    );
    assert!(position(&rows, "answer") < position(&rows, "<accessory>"));
    assert!(position(&rows, "<accessory>") < position(&rows, "<footer>"));
}

#[test]
fn renders_an_advisor_interjection_between_answered_work_phases() {
    let mut advisor = Block::new("advisor", BlockRole::System, "Check the fallback.");
    advisor.interjection = Some(InterjectionMeta {
        custom_type: "advisor".into(),
        severity: Some(InterjectionSeverity::Concern),
        extra: Default::default(),
    });
    let rows = plan(
        vec![
            tool("before", None),
            note("answer", "Complete answer."),
            advisor,
            tool("after", None),
            note("ack", "Checked."),
        ],
        true,
    );
    let shown = drawn(&rows);
    for id in ["answer", "advisor", "ack"] {
        assert!(shown.contains(&id.to_string()), "{id} is drawn");
    }
}

#[test]
fn folds_a_settled_turns_interjections_into_the_work_trail() {
    let blocks = vec![
        user("user", "Keep me posted"),
        tool("t1", None),
        irc("i1", "ping from #general"),
        irc("i2", "another ping"),
        tool("t2", None),
        irc("i3", "last ping"),
        note("answer", "The investigation is complete."),
    ];
    let settled = plan(blocks.clone(), false);
    assert_eq!(fold_text(&settled), "Ran 2 commands · 3 notes");
    assert_eq!(drawn(&settled), ["user", "<fold>", "answer"]);

    let live = plan(blocks, true);
    let interjections = live
        .iter()
        .filter(|row| {
            matches!(&row.kind, RowKind::Item { item: TurnItem::Block(block), .. } if block.interjection.is_some())
        })
        .count();
    assert_eq!(interjections, 3);
}

#[test]
fn shows_a_managed_workers_assignment_turn() {
    let mut assignment = user(
        "u1",
        "Review the current branch against main.\n\n<monocode_assignment>\nYou are a worker.\n</monocode_assignment>",
    );
    assignment.internal = Some(true);
    let managed = plan_with(
        vec![assignment.clone(), note("a1", "Looking now")],
        PlanOptions {
            managed: true,
            ..options(false)
        },
    );
    assert_eq!(drawn(&managed), ["u1", "a1"]);
    let lead = plan(vec![assignment, note("a1", "Looking now")], false);
    assert_eq!(drawn(&lead), ["a1"]);
}

#[test]
fn marks_the_search_result_row() {
    let blocks = refs(vec![user("user", "find me"), note("answer", "here")]);
    let state = PlanState {
        search_current: Some("answer".into()),
        ..Default::default()
    };
    let rows = build_plan(&blocks, &options(false), &state, None);
    assert_eq!(
        rows.iter()
            .filter(|row| row.search_current)
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["user/item/answer"]
    );
}

#[test]
fn reuses_rows_of_unchanged_turns() {
    let mut store = BlockStore::default();
    let mut cache = PlanCache::default();
    let first = vec![
        user("u1", "one"),
        note("a1", "first answer"),
        user("u2", "two"),
        note("a2", "partial"),
    ];
    store.update(&first);
    let before = build_plan(
        store.blocks(),
        &options(true),
        &PlanState::default(),
        Some(&mut cache),
    );
    let mut second = first.clone();
    second[3].text.push_str(" and more");
    assert!(store.update(&second));
    let after = build_plan(
        store.blocks(),
        &options(true),
        &PlanState::default(),
        Some(&mut cache),
    );
    // The first turn's rows are the same; the streaming turn's answer changed.
    assert!(before[0].same_as(&after[0]));
    assert!(before[1].same_as(&after[1]));
    let changed = after
        .iter()
        .position(|row| row.key == "u2/item/a2")
        .unwrap();
    assert!(!before[changed].same_as(&after[changed]));
    assert!(!store.update(&second));
}

#[test]
fn walks_handoffs_to_name_each_turns_harness() {
    let rows = plan_with(
        vec![
            timed_user("u1", "go", 0, 1_000),
            note("a1", "working"),
            handoff("h1"),
            timed_user("u2", "continue", 2_000, 1_000),
            note("a2", "done"),
        ],
        PlanOptions {
            harness: Some(HarnessId::Codex),
            ..options(false)
        },
    );
    let harnesses: Vec<Option<HarnessId>> = rows
        .iter()
        .filter_map(|row| match &row.kind {
            RowKind::FoldLine(line) => Some(line.harness),
            _ => None,
        })
        .collect();
    // Before the handoff the session ran on its first handoff's source.
    assert_eq!(
        harnesses,
        [Some(HarnessId::Cursor), Some(HarnessId::Claude)]
    );
}
