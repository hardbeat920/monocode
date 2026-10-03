//! Port of src/features/sessions/model/transcriptActivity.test.ts.

use super::*;
use crate::block::{BlockApproval, BlockTool, InterjectionMeta, InterjectionSeverity};
use crate::transcript::fixtures::*;

fn ids(blocks: &[BlockRef]) -> Vec<&str> {
    blocks.iter().map(|block| block.id.as_str()).collect()
}

fn kinds(items: &[TurnItem]) -> Vec<&'static str> {
    items
        .iter()
        .map(|item| match item {
            TurnItem::Block(_) => "block",
            TurnItem::Activity(_) => "activity",
            TurnItem::Subagents(_) => "subagents",
        })
        .collect()
}

fn activity_ids(item: &TurnItem) -> Vec<&str> {
    match item {
        TurnItem::Activity(blocks) => ids(blocks),
        other => panic!("expected activity, got {other:?}"),
    }
}

fn block_id(item: &TurnItem) -> &str {
    match item {
        TurnItem::Block(block) => &block.id,
        other => panic!("expected block, got {other:?}"),
    }
}

fn items(blocks: Vec<Block>) -> Vec<TurnItem> {
    group_turn_items(&refs(blocks), false)
}

fn settled_items(blocks: Vec<Block>) -> Vec<TurnItem> {
    group_turn_items(&refs(blocks), true)
}

fn phases(blocks: Vec<Block>) -> Vec<ActivityPhase> {
    build_activity_phases(&refs(blocks))
}

mod group_turn_items {
    use super::*;

    #[test]
    fn keeps_consecutive_shell_calls_in_one_activity_stack() {
        let items = items(vec![shell("a"), shell("b"), shell_status("c", "pending")]);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "b", "c"]);
    }

    #[test]
    fn does_not_split_a_stack_when_tools_are_waiting_for_approval() {
        let items = items(vec![
            shell("a"),
            with_approval(shell_status("b", "pending"), 1),
            with_approval(shell_status("c", "pending"), 2),
        ]);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "b", "c"]);
    }

    #[test]
    fn does_not_split_a_stack_across_empty_assistant_placeholders() {
        let mut ghost = Block::new("ghost", BlockRole::Assistant, "");
        ghost.streaming = Some(true);
        let items = items(vec![
            shell("a"),
            ghost,
            with_approval(shell_status("b", "pending"), 1),
            with_approval(shell_status("c", "pending"), 2),
        ]);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "b", "c"]);
    }

    #[test]
    fn hides_provider_todo_calls_in_favor_of_the_shared_tasks_block() {
        let tasks = tasks_block("tasks");
        let mut todo = Block::new("todo-tool", BlockRole::Tool, "Update TODOs");
        todo.tool = Some(BlockTool {
            kind: Some("tasks".into()),
            title: Some("Update TODOs".into()),
            status: Some("completed".into()),
            ..Default::default()
        });
        let items = items(vec![todo, tasks.clone()]);
        assert_eq!(items, vec![TurnItem::Block(Arc::new(tasks))]);

        let mut live = Block::new("todo-tool", BlockRole::Tool, "Update TODOs");
        live.streaming = Some(true);
        live.tool = Some(BlockTool {
            kind: Some("tasks".into()),
            status: Some("pending".into()),
            ..Default::default()
        });
        assert!(!activity_still_running(&refs(vec![live])));
    }

    #[test]
    fn folds_edits_into_the_activity_stack() {
        let items = items(vec![shell("a"), edit("b", "src/App.tsx"), shell("c")]);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "b", "c"]);
    }

    #[test]
    fn keeps_an_edit_awaiting_approval_out_of_the_stack() {
        let pending = with_approval(edit("b", "src/App.tsx"), 1);
        let items = items(vec![shell("a"), pending]);
        assert_eq!(kinds(&items), ["activity", "block"]);
    }

    #[test]
    fn keeps_all_assistant_prose_outside_reasoning_and_tool_activity() {
        let items = items(vec![
            user("u", "cut the release"),
            note("a1", "Running the checks first."),
            shell("a"),
            note("a2", "Checks pass. Bumping:"),
            edit("b", "src/App.tsx"),
            note("a3", "Released."),
        ]);
        assert_eq!(
            kinds(&items),
            ["block", "block", "activity", "block", "activity", "block"]
        );
        assert_eq!(block_id(&items[1]), "a1");
        assert_eq!(activity_ids(&items[2]), ["a"]);
        assert_eq!(block_id(&items[3]), "a2");
        assert_eq!(activity_ids(&items[4]), ["b"]);
        assert_eq!(block_id(&items[5]), "a3");
    }

    #[test]
    fn keeps_the_trailing_run_of_prose_blocks_out_of_the_stack() {
        let mut done = note("a2", "Done");
        done.streaming = Some(true);
        let items = items(vec![shell("a"), note("a1", "Half"), done]);
        assert_eq!(kinds(&items), ["activity", "block", "block"]);
    }

    #[test]
    fn keeps_prose_standalone_when_the_turn_ends_on_a_tool_call() {
        let items = items(vec![note("a1", "Looking now."), shell("a")]);
        assert_eq!(block_id(&items[0]), "a1");
        assert_eq!(activity_ids(&items[1]), ["a"]);
    }

    #[test]
    fn keeps_thinking_in_the_stack_so_a_long_think_is_visible() {
        let items = items(vec![
            thought("r", "**Checking the config**"),
            shell("a"),
            note("done", "Done."),
        ]);
        assert_eq!(kinds(&items), ["activity", "block"]);
        assert_eq!(activity_ids(&items[0]), ["r", "a"]);
    }

    #[test]
    fn uses_assistant_prose_as_boundaries_between_reasoning_and_tool_groups() {
        let items = items(vec![
            note("a1", "I’ll inspect the config first."),
            thought("r1", "Weighing the options."),
            read("t1", "src/App.tsx"),
            note("a2", "The config is healthy. I’m checking the build next."),
            thought("r2", "Weighing the options."),
            shell("t2"),
        ]);
        assert_eq!(block_id(&items[0]), "a1");
        assert_eq!(activity_ids(&items[1]), ["r1", "t1"]);
        assert_eq!(block_id(&items[2]), "a2");
        assert_eq!(activity_ids(&items[3]), ["r2", "t2"]);
    }

    #[test]
    fn replaces_leading_reasoning_with_the_first_assistant_prose() {
        let items = items(vec![
            user("u", "Investigate it"),
            thought("r1", "I should inspect the current changes."),
            thought("r2", "I need a structured checklist."),
            note("a1", "I’ll investigate the current changes."),
            read("t1", "src/App.tsx"),
        ]);
        assert_eq!(kinds(&items), ["block", "block", "activity"]);
        assert_eq!(block_id(&items[0]), "u");
        assert_eq!(block_id(&items[1]), "a1");
        assert_eq!(activity_ids(&items[2]), ["t1"]);
    }

    #[test]
    fn identifies_leading_reasoning_while_the_first_prose_is_pending() {
        let thinking = items(vec![
            user("u", "Investigate it"),
            thought("r1", "Weighing the options."),
            thought("r2", "Weighing the options."),
        ]);
        assert_eq!(initial_thinking_index(&thinking), Some(1));
        let tool_activity = items(vec![
            user("u", "Investigate it"),
            thought("r1", "Weighing the options."),
            read("t1", "src/App.tsx"),
        ]);
        assert_eq!(initial_thinking_index(&tool_activity), None);
    }
}

mod turn_copy_text {
    use super::*;

    #[test]
    fn joins_assistant_and_plan_markdown_from_the_turn() {
        let mut tasks = Block::new("tasks", BlockRole::Tasks, "[x] inspect\n[~] implement");
        tasks.task_list = tasks_block("t").task_list;
        let text = turn_copy_text(&refs(vec![
            user("u", "fix it"),
            note("a1", "I'll inspect the file."),
            shell("t"),
            Block::new("r", BlockRole::Reasoning, "thinking"),
            tasks,
            Block::new("p", BlockRole::Plan, "## Plan\n\n- edit App.tsx"),
            note("a2", "Done.\n\n```ts\nfixed\n```"),
            Block::new("s", BlockRole::System, "session error"),
        ]));
        assert_eq!(
            text,
            "I'll inspect the file.\n\n[x] inspect\n[~] implement\n\n## Plan\n\n- edit App.tsx\n\nDone.\n\n```ts\nfixed\n```"
        );
    }

    #[test]
    fn returns_empty_when_the_turn_has_no_readable_output() {
        assert_eq!(
            turn_copy_text(&refs(vec![user("u", "go"), shell("t"), note("a", "  ")])),
            ""
        );
    }
}

mod group_turns {
    use super::*;

    fn turn_ids(turns: &[Vec<BlockRef>]) -> Vec<Vec<&str>> {
        turns.iter().map(|turn| ids(turn)).collect()
    }

    #[test]
    fn folds_an_orchestration_turn_the_app_wrote_into_the_turn_above() {
        let mut internal = user("u2", "Worker results are ready.");
        internal.internal = Some(true);
        let turns = group_turns(
            &refs(vec![
                user("u1", "Review the changes"),
                note("a1", "Delegating."),
                internal,
                note("a2", "All three look right."),
                user("u3", "Ship it"),
                note("a3", "Done."),
            ]),
            false,
        );
        assert_eq!(turn_ids(&turns), [vec!["u1", "a1", "a2"], vec!["u3", "a3"]]);
    }

    #[test]
    fn keeps_a_handoff_divider_on_its_own_row_between_providers() {
        let turns = group_turns(
            &refs(vec![
                user("u1", "go"),
                note("a1", "working"),
                handoff("h1"),
                user("u2", "continue"),
            ]),
            false,
        );
        assert_eq!(turn_ids(&turns), [vec!["u1", "a1"], vec!["h1"], vec!["u2"]]);
    }
}

mod build_activity_phases {
    use super::*;

    fn category_calls(count: usize) -> usize {
        let blocks: Vec<Block> = (0..count)
            .map(|i| read(&format!("r{i}"), "src/App.tsx"))
            .collect();
        let blocks = refs(blocks);
        CATEGORY_CALLS.with(|calls| calls.set(0));
        let phases = build_activity_phases(&blocks);
        let calls = CATEGORY_CALLS.with(|calls| calls.get());
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Research);
        assert_eq!(phases[0].steps, blocks);
        calls
    }

    #[test]
    fn does_not_repeatedly_inspect_earlier_calls_as_a_long_tool_run_grows() {
        // Count the work instead of timing the test on a particular CPU.
        assert!((category_calls(400) as f64) < category_calls(200) as f64 * 2.5);
    }

    #[test]
    fn resets_the_dominant_work_tally_when_narration_starts_a_new_group() {
        let phases = phases(vec![
            edit("e1", "src/App.tsx"),
            edit("e2", "src/App.tsx"),
            edit("e3", "src/App.tsx"),
            note("n1", "Checking the result."),
            read("r1", "src/App.tsx"),
            shell("c1"),
        ]);
        assert_eq!(
            phases.iter().map(|phase| phase.kind).collect::<Vec<_>>(),
            [ActivityPhaseKind::Edit, ActivityPhaseKind::Run]
        );
        assert_eq!(
            phases[1].headline.as_ref().map(|b| b.id.as_str()),
            Some("n1")
        );
    }

    #[test]
    fn groups_a_run_of_calls_under_the_line_that_introduced_it() {
        let phases = phases(vec![
            note("n1", "Now I need to find the theme provider."),
            search("s1", "color tokens"),
            read("r1", "src/globals.css"),
            read("r2", "src/layout.tsx"),
            note("n2", "Updating the dark mode tokens."),
            edit("e1", "src/globals.css"),
            edit("e2", "src/theme.ts"),
        ]);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Research);
        assert_eq!(phases[0].headline.as_ref().unwrap().id, "n1");
        assert_eq!(ids(&phases[0].steps), ["s1", "r1", "r2"]);
        assert_eq!(phases[1].kind, ActivityPhaseKind::Edit);
        assert_eq!(phases[1].headline.as_ref().unwrap().id, "n2");
        assert_eq!(ids(&phases[1].steps), ["e1", "e2"]);
    }

    #[test]
    fn keeps_a_run_of_mixed_work_in_one_group() {
        let phases = phases(vec![
            read("r1", "a.ts"),
            read("r2", "b.ts"),
            edit("e1", "a.ts"),
            edit("e2", "b.ts"),
            shell("c1"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Edit);
        assert_eq!(ids(&phases[0].steps), ["r1", "r2", "e1", "e2", "c1"]);
    }

    #[test]
    fn folds_a_lone_uninvited_call_into_the_group_before_it() {
        let phases = phases(vec![
            edit("e1", "a.ts"),
            read("r1", "a.ts"),
            edit("e2", "b.ts"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Edit);
        assert_eq!(ids(&phases[0].steps), ["e1", "r1", "e2"]);
    }

    #[test]
    fn keeps_a_group_the_agent_announced_out_of_that_fold() {
        let phases = phases(vec![
            read("r1", "src/App.tsx"),
            note("n1", "Now the edit."),
            edit("e1", "src/App.tsx"),
        ]);
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[1].kind, ActivityPhaseKind::Edit);
        assert_eq!(phases[1].headline.as_ref().unwrap().id, "n1");
    }

    #[test]
    fn keeps_a_second_paragraph_as_a_step_rather_than_a_group_of_its_own() {
        let phases = phases(vec![
            note("n1", "First."),
            note("n2", "Second."),
            read("r1", "src/App.tsx"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Research);
        assert_eq!(phases[0].headline.as_ref().unwrap().id, "n1");
        assert_eq!(ids(&phases[0].steps), ["n2", "r1"]);
    }

    #[test]
    fn gives_a_turn_that_only_thought_a_group_to_sit_in() {
        let phases = phases(vec![thought("r", "Weighing the options.")]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Think);
        assert!(phases[0].headline.is_none());
        assert_eq!(ids(&phases[0].steps), ["r"]);
    }

    #[test]
    fn keeps_reasoning_inside_the_group_instead_of_titling_it() {
        let phases = phases(vec![
            thought("t1", "Weighing the options."),
            search("s1", "color tokens"),
            thought("t2", "Weighing the options."),
            search("s2", "color tokens"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Research);
        assert!(phases[0].headline.is_none());
        assert_eq!(ids(&phases[0].steps), ["t1", "s1", "t2", "s2"]);
    }

    #[test]
    fn keeps_a_thought_between_two_kinds_of_work_inside_the_group() {
        let phases = phases(vec![
            read("r1", "a.ts"),
            read("r2", "b.ts"),
            thought("t1", "Now to apply the change."),
            edit("e1", "a.ts"),
            edit("e2", "b.ts"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(ids(&phases[0].steps), ["r1", "r2", "t1", "e1", "e2"]);
    }

    #[test]
    fn lets_the_agents_own_words_title_a_group_that_opened_on_a_thought() {
        let phases = phases(vec![
            thought("t1", "Weighing the options."),
            note("n1", "Looking for the theme provider."),
            search("s1", "color tokens"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Research);
        assert_eq!(phases[0].headline.as_ref().unwrap().id, "n1");
        assert_eq!(phases[0].id, "t1");
        assert_eq!(ids(&phases[0].steps), ["t1", "s1"]);
    }
}

mod activity_phase_title {
    use super::*;

    fn title(blocks: Vec<Block>, live: bool) -> String {
        activity_phase_title(&phases(blocks)[0], live)
    }

    #[test]
    fn uses_the_agents_own_line_when_it_wrote_one() {
        assert_eq!(
            title(
                vec![
                    note("n1", "**Found it** — the tokens live in `globals.css`."),
                    read("r1", "src/App.tsx"),
                ],
                false
            ),
            "Found it — the tokens live in globals.css."
        );
    }

    #[test]
    fn says_what_the_calls_add_up_to_in_the_tense_of_the_moment() {
        assert_eq!(
            title(vec![read("r1", "a.ts"), read("r2", "b.ts")], true),
            "Reading 2 files"
        );
        assert_eq!(
            title(vec![read("r1", "a.ts"), read("r2", "b.ts")], false),
            "Read 2 files"
        );
        assert_eq!(
            title(vec![read("r1", "src/index.css")], false),
            "Read index.css"
        );
        assert_eq!(
            title(
                vec![search("s1", "color tokens"), search("s2", "color tokens")],
                false
            ),
            "Searched the project"
        );
        assert_eq!(
            title(
                vec![search("s1", "color tokens"), read("r1", "src/App.tsx")],
                false
            ),
            "Explored the project"
        );
        assert_eq!(
            title(vec![edit("e1", "a.ts"), edit("e2", "b.ts")], false),
            "Edited 2 files"
        );
        assert_eq!(title(vec![shell("a"), shell("b")], false), "Ran 2 commands");
        assert_eq!(title(vec![shell("a")], true), "Running a command");
    }

    #[test]
    fn adds_up_a_group_of_mixed_work_one_clause_per_kind() {
        assert_eq!(
            title(
                vec![
                    shell("c1"),
                    shell("c2"),
                    shell("c3"),
                    search("s1", "color tokens"),
                    edit("e1", "a.ts"),
                    edit("e2", "b.ts"),
                ],
                false
            ),
            "Ran 3 commands · Searched the project · Edited 2 files"
        );
    }

    #[test]
    fn summarises_readable_historical_codex_shell_rows_by_their_inferred_work() {
        let stored = |id: &str, text: &str| {
            let mut block = Block::new(id, BlockRole::Tool, text);
            block.tool = Some(BlockTool {
                kind: Some("execute".into()),
                title: Some(text.into()),
                status: Some("completed".into()),
                ..Default::default()
            });
            block
        };
        assert_eq!(
            title(
                vec![
                    stored(
                        "r1",
                        "/bin/zsh -lc \"sed -n '1,120p' src/lib/paths.ts\nsed -n '330,430p' src/lib/harness/codexProtocol.test.ts\""
                    ),
                    stored(
                        "r2",
                        "/bin/zsh -lc \"nl -ba src/lib/harness/apply.ts | sed -n '520,620p'\""
                    ),
                    stored("run", "/bin/zsh -lc 'npm test'"),
                ],
                false
            ),
            "Read 2 files · Ran a command"
        );
    }

    #[test]
    fn puts_only_the_call_in_flight_in_the_present_tense() {
        assert_eq!(
            title(
                vec![edit("e1", "a.ts"), edit("e2", "b.ts"), shell("c1")],
                true
            ),
            "Edited 2 files · Running a command"
        );
    }

    #[test]
    fn still_names_the_shapes_of_work_on_their_own() {
        assert_eq!(
            title(
                vec![agent("ag", "Explore the auth module", "in_progress")],
                true
            ),
            "Running a subagent"
        );
    }
}

mod running_subagents {
    use super::*;

    fn explore(id: &str, status: &str) -> Block {
        agent(id, "Explore the auth module", status)
    }

    #[test]
    fn counts_a_subagent_in_the_group_it_ran_in() {
        let phases = phases(vec![
            read("r1", "src/App.tsx"),
            explore("ag", "in_progress"),
        ]);
        assert_eq!(phases.len(), 1);
        assert_eq!(
            activity_phase_title(&phases[0], true),
            "Read App.tsx · Running a subagent"
        );
    }

    #[test]
    fn flags_a_live_subagent_until_the_tool_completes() {
        assert!(has_running_subagent(&refs(vec![explore(
            "ag",
            "in_progress"
        )])));
        assert!(activity_still_running(&refs(vec![explore(
            "ag",
            "in_progress"
        )])));
        assert!(!has_running_subagent(&refs(vec![explore(
            "ag",
            "completed"
        )])));
        assert!(!activity_still_running(&refs(vec![explore(
            "ag",
            "completed"
        )])));
    }

    #[test]
    fn surfaces_failed_subagents_in_activity_summaries() {
        assert_eq!(
            subagent_failure_summary(&refs(vec![explore("one", "failed")])).as_deref(),
            Some("Subagent failed")
        );
        assert_eq!(
            subagent_failure_summary(&refs(vec![
                explore("one", "failed"),
                explore("two", "error")
            ]))
            .as_deref(),
            Some("2 subagents failed")
        );
        assert_eq!(
            activity_phase_title(&phases(vec![explore("one", "failed")])[0], false),
            "Subagent failed"
        );
    }
}

mod the_subagent_stack {
    use super::*;

    #[test]
    fn gives_delegated_runs_their_own_item_instead_of_folding_them_into_work() {
        let items = items(vec![
            note("note", "I will run two reviews."),
            agent("a1", "Correctness review", "in_progress"),
            agent("a2", "Quality review", "in_progress"),
            shell("s1"),
        ]);
        assert_eq!(kinds(&items), ["block", "subagents", "activity"]);
        assert_eq!(items[1].blocks().len(), 2);
    }

    #[test]
    fn starts_a_fresh_stack_when_the_agent_narrates_between_spawns() {
        let items = items(vec![
            agent("a1", "Correctness review", "in_progress"),
            note("note", "Adding one more."),
            agent("a2", "Quality review", "in_progress"),
        ]);
        assert_eq!(kinds(&items), ["subagents", "block", "subagents"]);
    }

    #[test]
    fn folds_across_a_stack_so_the_turns_status_line_stays_at_the_top() {
        let items = items(vec![
            note("lead", "Running two reviews."),
            agent("a1", "Correctness review", "in_progress"),
            shell("s1"),
            note("answer", "Both agree."),
        ]);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(fold.start, 0);
        assert_eq!(
            kinds(&items[fold.start..=fold.end]),
            ["block", "subagents", "activity"]
        );
        assert_eq!(ids(&folded_blocks(&items, fold)), ["lead", "s1"]);
    }

    #[test]
    fn shortens_a_run_named_with_its_whole_brief_keeping_the_brief_intact() {
        let briefed = agent(
            "a1",
            "Independently review the current repository's recent changes for correctness and regressions. Inspect the uncommitted diff.",
            "in_progress",
        );
        assert_eq!(
            subagent_name(&briefed),
            "Independently review the current repository's recent\u{2026}"
        );
        assert!(js::len(&subagent_name(&briefed)) <= 57);
        assert!(subagent_brief(&briefed).contains("Inspect the uncommitted diff."));
    }

    #[test]
    fn names_a_run_from_its_description_without_the_tools_own_prefix() {
        assert_eq!(
            subagent_name(&agent("a1", "Task: Correctness review", "in_progress")),
            "Correctness review"
        );
        let mut named = agent("a1", "Explore", "in_progress");
        named.agent_run = Some(crate::block::AgentRunMeta {
            name: "Quality review".into(),
            ..Default::default()
        });
        assert_eq!(subagent_name(&named), "Quality review");
        assert!(is_subagent_block(&agent(
            "a1",
            "Correctness review",
            "in_progress"
        )));
        assert!(!is_subagent_block(&shell("s1")));
    }
}

mod the_settled_work_trail {
    use super::*;

    fn done_agent(id: &str) -> Block {
        agent(id, "Correctness review", "completed")
    }

    #[test]
    fn keeps_a_status_row_inside_the_surrounding_work_live_or_settled() {
        for settled in [false, true] {
            let items = group_turn_items(
                &refs(vec![
                    shell("a"),
                    status("st", "Advisor reviewed this turn"),
                    shell("b"),
                ]),
                settled,
            );
            assert_eq!(items.len(), 1);
            assert_eq!(activity_ids(&items[0]), ["a", "st", "b"]);
        }
    }

    #[test]
    fn keeps_an_interjection_on_its_own_row_while_live_folds_it_in_once_settled() {
        let turn = vec![shell("a"), irc("i1", "new message in #general"), shell("b")];
        assert_eq!(
            kinds(&items(turn.clone())),
            ["activity", "block", "activity"]
        );
        let items = settled_items(turn);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "i1", "b"]);
    }

    #[test]
    fn pins_a_delegated_run_while_live_folds_it_into_the_trail_once_settled() {
        let turn = vec![shell("a"), done_agent("ag"), shell("b")];
        assert_eq!(
            kinds(&items(turn.clone())),
            ["activity", "subagents", "activity"]
        );
        let items = settled_items(turn);
        assert_eq!(items.len(), 1);
        assert_eq!(activity_ids(&items[0]), ["a", "ag", "b"]);
        assert_eq!(
            work_summary_line(items[0].blocks(), false),
            "Ran 2 commands · Ran a subagent"
        );
    }

    #[test]
    fn keeps_a_failed_run_on_its_own_row_once_settled_outside_the_fold() {
        let items = settled_items(vec![
            user("u", "go"),
            shell("t1"),
            done_agent("ag"),
            agent("dead", "Quality review", "failed"),
            shell("t2"),
            note("done", "It could not finish."),
        ]);
        assert_eq!(
            kinds(&items),
            ["block", "activity", "subagents", "activity", "block"]
        );
        let fold = foldable_work(&items).unwrap();
        assert_eq!(fold, WorkFold { start: 1, end: 3 });
        assert_eq!(ids(&folded_blocks(&items, fold)), ["t1", "ag", "t2"]);
    }

    #[test]
    fn spans_the_whole_trail_once_settled_status_rows_and_notes_included() {
        let items = settled_items(vec![
            user("u", "go"),
            shell("t1"),
            status("st", "Advisor reviewed this turn"),
            shell("t2"),
            irc("i1", "new message in #general"),
            irc("i2", "new message in #general"),
            shell("t3"),
            note("done", "All set."),
        ]);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(fold, WorkFold { start: 1, end: 1 });
        let folded = folded_blocks(&items, fold);
        assert_eq!(ids(&folded), ["t1", "st", "t2", "i1", "i2", "t3"]);
        let summary = work_summary_line(&folded, false);
        assert_eq!(summary, "Ran 3 commands · 2 notes");
        assert!(!summary.contains("Advisor reviewed"));
    }

    #[test]
    fn leaves_notes_that_arrive_after_the_answer_as_their_own_trail_under_it() {
        let items = settled_items(vec![
            user("u", "go"),
            shell("t1"),
            note("done", "All set."),
            irc("i1", "new message in #general"),
            irc("i2", "new message in #general"),
        ]);
        assert_eq!(kinds(&items), ["block", "activity", "block", "activity"]);
        assert_eq!(foldable_work(&items), Some(WorkFold { start: 1, end: 1 }));
        let trailing = items[3].blocks();
        let phases = build_activity_phases(trailing);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].kind, ActivityPhaseKind::Note);
        assert_eq!(activity_phase_title(&phases[0], false), "2 notes");
        assert_eq!(work_kind(trailing), ActivityPhaseKind::Note);
    }

    #[test]
    fn keeps_the_answer_claude_yielded_with_above_what_a_background_task_wakes_it_to_say() {
        let mut background = shell("bg");
        background.tool.as_mut().unwrap().background = Some(true);
        let items = settled_items(vec![
            user("u", "go"),
            shell("t1"),
            note("note", "Updating the state file."),
            shell("t2"),
            note("answer", "Two new findings."),
            background,
            note("late", "Stray command, nothing to do."),
        ]);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(ids(&folded_blocks(&items, fold)), ["t1", "note", "t2"]);
        assert_eq!(
            kinds(&items[fold.end + 1..]),
            ["block", "activity", "block"]
        );
    }

    #[test]
    fn keeps_a_yielded_answer_visible_when_status_precedes_the_background_tool() {
        let mut background = shell("background");
        background.tool = Some(BlockTool {
            kind: Some("shell".into()),
            background: Some(true),
            status: Some("completed".into()),
            ..Default::default()
        });
        let items = settled_items(vec![
            shell("before"),
            note("answer", "The initial answer."),
            status("after-yield", "Advisor reviewed this turn"),
            background,
            note("late", "The follow-up."),
        ]);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(ids(&folded_blocks(&items, fold)), ["before"]);
        assert_eq!(
            kinds(&items[fold.end + 1..]),
            ["block", "activity", "block"]
        );
    }

    #[test]
    fn lets_the_settled_fold_reach_across_an_interjection_that_stops_it_live() {
        let turn = vec![
            user("u", "go"),
            shell("t1"),
            note("mid", "Halfway there."),
            irc("i1", "new message in #general"),
            shell("t2"),
            note("done", "All set."),
        ];
        assert_eq!(
            foldable_work(&items(turn.clone())),
            Some(WorkFold { start: 4, end: 4 })
        );
        let items = settled_items(turn);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(fold, WorkFold { start: 1, end: 3 });
        assert_eq!(ids(&folded_blocks(&items, fold)), ["t1", "mid", "i1", "t2"]);
    }

    #[test]
    fn never_counts_a_status_row_as_running_work() {
        assert!(!activity_still_running(&refs(vec![status(
            "st",
            "Advisor reviewed this turn"
        )])));
        assert!(!activity_still_running(&refs(vec![
            shell("done"),
            status("st", "Advisor reviewed this turn"),
            status("st2", "Working on it"),
        ])));
    }

    #[test]
    fn keeps_an_error_outside_the_trail_after_a_completed_call_live_or_settled() {
        let mut error = Block::new("e1", BlockRole::System, "Provider connection lost");
        error.notice = Some(BlockNotice::Error);
        let turn = vec![
            user("u", "go"),
            shell("t1"),
            error,
            note("done", "It failed."),
        ];
        for settled in [false, true] {
            let items = group_turn_items(&refs(turn.clone()), settled);
            assert_eq!(kinds(&items), ["block", "activity", "block", "block"]);
            assert_eq!(block_id(&items[2]), "e1");
            assert_eq!(
                kinds(&group_turn_items(&refs(turn[..3].to_vec()), settled)),
                ["block", "activity", "block"]
            );
        }
        let items = settled_items(turn);
        let fold = foldable_work(&items).unwrap();
        assert_eq!(ids(&folded_blocks(&items, fold)), ["t1"]);
    }

    #[test]
    fn keeps_a_persisted_interrupt_outside_the_trail_even_without_the_tag() {
        let items = settled_items(vec![
            shell("a"),
            Block::new("int", BlockRole::System, INTERRUPT_MESSAGE),
            shell("b"),
        ]);
        assert_eq!(kinds(&items), ["activity", "block", "activity"]);
    }

    #[test]
    fn labels_a_group_that_only_reported_status() {
        let statuses = refs(vec![
            status("s1", "Advisor reviewed this turn"),
            status("s2", "Working on it"),
        ]);
        assert_eq!(work_summary_line(&statuses, false), "Status update");
        let phases = build_activity_phases(&statuses);
        assert_eq!(activity_phase_title(&phases[0], false), "Status update");
        assert_eq!(work_kind(&statuses), ActivityPhaseKind::Note);
        assert_eq!(
            work_summary_line(
                &refs(vec![
                    status("s1", "Advisor reviewed this turn"),
                    thought("r1", "Weighing the options.")
                ]),
                false
            ),
            "Thought"
        );
    }
}

mod foldable_work {
    use super::*;

    #[test]
    fn folds_the_work_the_agent_has_already_answered_for() {
        let turn = items(vec![
            user("u", "go"),
            shell("c1"),
            note("n1", "Checking the other half now."),
            shell("c2"),
            note("done", "All set."),
        ]);
        let fold = foldable_work(&turn).unwrap();
        assert_eq!(fold, WorkFold { start: 1, end: 3 });
        assert_eq!(ids(&folded_blocks(&turn, fold)), ["c1", "n1", "c2"]);
    }

    #[test]
    fn leaves_work_the_agent_has_not_answered_for_alone() {
        assert_eq!(
            foldable_work(&items(vec![user("u", "go"), shell("c1")])),
            None
        );
        assert_eq!(
            foldable_work(&items(vec![
                user("u", "go"),
                note("a", "On it."),
                shell("c1")
            ])),
            None
        );
    }

    #[test]
    fn keeps_the_live_group_outside_the_fold_while_the_agent_works_on() {
        let turn = items(vec![
            user("u", "go"),
            shell("c1"),
            note("n1", "That worked. Running the tests."),
            shell_status("c2", "pending"),
        ]);
        assert_eq!(foldable_work(&turn), Some(WorkFold { start: 1, end: 1 }));
    }

    #[test]
    fn never_folds_a_plan_or_anything_under_it() {
        let turn = items(vec![
            user("u", "go"),
            shell("c1"),
            Block::new("p", BlockRole::Plan, "## Plan"),
            shell("c2"),
            note("done", "Built it."),
        ]);
        assert_eq!(foldable_work(&turn), Some(WorkFold { start: 3, end: 3 }));
    }

    #[test]
    fn uses_an_interjection_as_a_hard_boundary_between_answered_work_phases() {
        let mut advisor = Block::new("advisor", BlockRole::System, "Check the fallback.");
        advisor.interjection = Some(InterjectionMeta {
            custom_type: "advisor".into(),
            severity: Some(InterjectionSeverity::Nit),
            extra: Default::default(),
        });
        let turn = items(vec![
            shell("before"),
            note("answer", "The complete answer."),
            advisor,
            shell("after"),
            note("ack", "Checked."),
        ]);
        let fold = foldable_work(&turn).unwrap();
        assert_eq!(fold, WorkFold { start: 3, end: 3 });
        assert_eq!(ids(&folded_blocks(&turn, fold)), ["after"]);
    }

    #[test]
    fn leaves_an_approval_attached_to_earlier_work_outside_the_fold() {
        let turn = items(vec![
            with_approval(shell_status("pending", "pending"), 1),
            note("n1", "I need permission to run that command."),
        ]);
        assert_eq!(foldable_work(&turn), None);
    }

    #[test]
    fn does_not_swallow_an_earlier_approval_when_later_work_folds() {
        let turn = items(vec![
            with_approval(shell_status("pending", "pending"), 1),
            note("n1", "Checking something else meanwhile."),
            shell("finished"),
            note("n2", "That check passed."),
        ]);
        let fold = foldable_work(&turn).unwrap();
        assert_eq!(ids(&folded_blocks(&turn, fold)), ["n1", "finished"]);
    }

    #[test]
    fn folds_work_normally_once_its_approval_has_been_resolved() {
        let mut approved = shell("approved");
        approved.approval = Some(BlockApproval {
            request_id: 1,
            decided: Some(ApprovalDecided::Allow),
            extra: Default::default(),
        });
        let turn = items(vec![approved, note("n1", "The command succeeded.")]);
        assert_eq!(foldable_work(&turn), Some(WorkFold { start: 0, end: 0 }));
    }

    #[test]
    fn gives_the_fold_line_a_place_to_sit_before_there_is_a_fold() {
        let turn = items(vec![user("u", "go"), shell("c1")]);
        assert_eq!(foldable_work(&turn), None);
        assert_eq!(first_foldable_index(&turn), Some(1));
        assert_eq!(first_foldable_index(&items(vec![user("u", "go")])), None);
    }

    #[test]
    fn has_nothing_to_fold_in_a_turn_that_only_answered() {
        assert_eq!(
            foldable_work(&items(vec![user("u", "go"), note("a", "Here you go.")])),
            None
        );
    }
}

mod last_activity_index {
    use super::*;

    #[test]
    fn points_at_the_fold_that_sits_under_the_final_answer() {
        let items = items(vec![
            user("u", "go"),
            shell("a"),
            Block::new("p", BlockRole::Plan, "## Plan"),
            shell("b"),
            note("done", "Done."),
        ]);
        assert_eq!(last_activity_index(&items), Some(3));
    }

    #[test]
    fn returns_none_for_a_turn_that_ran_no_tools() {
        assert_eq!(last_activity_index(&items(vec![note("a", "Hi.")])), None);
    }
}

mod tool_call_label {
    use super::*;

    fn tool(id: &str, kind: &str, text: &str) -> Block {
        let mut block = Block::new(id, BlockRole::Tool, text);
        block.tool = Some(BlockTool {
            kind: Some(kind.into()),
            title: Some(text.into()),
            ..Default::default()
        });
        block
    }

    #[test]
    fn shows_the_shell_command_not_the_tool_name() {
        assert_eq!(
            tool_call_label(&tool("a", "execute", "git status -s"), None),
            "git status -s"
        );
        assert_eq!(
            tool_call_label(&tool("b", "skill", "Skill /code-review"), None),
            "Skill /code-review"
        );
    }

    #[test]
    fn hides_codexs_shell_launcher_on_commands_that_stay_commands() {
        assert_eq!(
            tool_call_label(
                &tool(
                    "wrapped",
                    "execute",
                    "/bin/zsh -lc \"npm test -- --run src/lib/app.test.ts\""
                ),
                None
            ),
            "npm test -- --run src/lib/app.test.ts"
        );
    }

    #[test]
    fn renders_file_reading_bash_as_a_read_label() {
        assert_eq!(
            tool_call_label(&tool("c", "execute", "cat src/lib/appearance.ts"), None),
            "Read src/lib/appearance.ts"
        );
        assert_eq!(
            tool_call_label(
                &tool("d", "execute", "cat /Users/me/proj/src/lib/appearance.ts"),
                Some("/Users/me/proj")
            ),
            "Read src/lib/appearance.ts"
        );
    }
}

mod edit_verb {
    use super::*;

    #[test]
    fn canonicalises_past_tense_harness_phrasing() {
        assert_eq!(edit_verb("Edited src/App.tsx"), "Edit");
        assert_eq!(edit_verb("Deleted src/old.ts"), "Delete");
        assert_eq!(edit_verb("Renamed src/a.ts"), "Move");
        assert_eq!(edit_verb("Created src/new.ts"), "Create");
        assert_eq!(edit_verb("Wrote src/new.ts"), "Write");
    }

    #[test]
    fn falls_back_to_edit_for_unknown_phrasing() {
        assert_eq!(edit_verb("Patching src/App.tsx"), "Edit");
        assert_eq!(edit_verb(""), "Edit");
    }
}

mod resolve_tool_call_display {
    use super::*;
    use crate::block::ToolPreview;

    fn preview(kind: ToolPreviewKind, path: &str, file_name: &str) -> ToolPreview {
        let mut preview = ToolPreview::new(kind);
        preview.path = Some(path.into());
        preview.file_name = Some(file_name.into());
        preview
    }

    #[test]
    fn opens_the_exact_path_shown_in_the_label_even_when_preview_path_disagrees() {
        let label = "Read /Users/dev/.codex/skills/zuse/SKILL.md";
        let preview = preview(
            ToolPreviewKind::Read,
            "/Users/dev/project/.claude/skills/custom-skill/SKILL.md",
            "SKILL.md",
        );
        let result = resolve_tool_call_display(label, Some(&preview), Some("/Users/dev/project"));
        assert_eq!(
            result.target.as_deref(),
            Some("/Users/dev/.codex/skills/zuse/SKILL.md")
        );
        assert_eq!(result.file_path, result.target);
        assert_ne!(result.file_path, preview.path);
    }

    #[test]
    fn still_resolves_from_preview_path_when_the_label_carries_no_literal_path() {
        let preview = preview(
            ToolPreviewKind::Read,
            "/Users/dev/project/src/App.tsx",
            "App.tsx",
        );
        let result = resolve_tool_call_display("Read", Some(&preview), Some("/Users/dev/project"));
        assert_eq!(result.target.as_deref(), Some("src/App.tsx"));
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/project/src/App.tsx")
        );
    }

    #[test]
    fn falls_back_to_the_raw_label_when_there_is_no_recognisable_action() {
        let result = resolve_tool_call_display("Thinking", None, Some("/Users/dev/project"));
        assert_eq!(result.action, None);
        assert_eq!(result.target, None);
    }

    #[test]
    fn flags_a_write_preview_whose_own_path_disagrees_with_the_labels_file() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/.claude/skills/custom-skill/SKILL.md",
            "SKILL.md",
        );
        let result = resolve_tool_call_display(
            "Edit /Users/dev/.codex/skills/zuse/SKILL.md",
            Some(&preview),
            Some("/Users/dev/project"),
        );
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/.codex/skills/zuse/SKILL.md")
        );
        assert!(!result.preview_matches_file);
    }

    #[test]
    fn keeps_the_write_preview_when_its_path_agrees_with_the_labels_file() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/src/App.tsx",
            "App.tsx",
        );
        let result = resolve_tool_call_display(
            "Edit src/App.tsx",
            Some(&preview),
            Some("/Users/dev/project"),
        );
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/project/src/App.tsx")
        );
        assert!(result.preview_matches_file);
    }

    #[test]
    fn keeps_the_write_preview_when_the_label_carries_no_literal_path_of_its_own() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/src/App.tsx",
            "App.tsx",
        );
        let result = resolve_tool_call_display("Write", Some(&preview), Some("/Users/dev/project"));
        assert!(result.preview_matches_file);
    }

    #[test]
    fn falls_back_to_the_write_previews_path_when_the_labels_target_is_plain_english() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/package.json",
            "package.json",
        );
        let result = resolve_tool_call_display(
            "Edit dependency versions",
            Some(&preview),
            Some("/Users/dev/project"),
        );
        assert_eq!(result.target.as_deref(), Some("package.json"));
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/project/package.json")
        );
        assert!(result.preview_matches_file);
    }

    #[test]
    fn still_trusts_a_labels_own_path_over_the_write_preview_when_it_looks_like_a_file() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/.claude/skills/custom-skill/SKILL.md",
            "SKILL.md",
        );
        let result = resolve_tool_call_display(
            "Edit /Users/dev/.codex/skills/zuse/SKILL.md",
            Some(&preview),
            Some("/Users/dev/project"),
        );
        assert_eq!(
            result.target.as_deref(),
            Some("/Users/dev/.codex/skills/zuse/SKILL.md")
        );
    }

    #[test]
    fn does_not_treat_an_unresolved_write_preview_path_as_a_confirmed_match() {
        let preview = preview(ToolPreviewKind::Write, "src/App.tsx", "App.tsx");
        let result =
            resolve_tool_call_display("Edit /Users/dev/project/src/App.tsx", Some(&preview), None);
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/project/src/App.tsx")
        );
        assert!(!result.preview_matches_file);
    }

    #[test]
    fn trusts_a_label_target_with_a_line_column_suffix_over_a_disagreeing_write_preview() {
        let preview = preview(
            ToolPreviewKind::Write,
            "/Users/dev/project/other.ts",
            "other.ts",
        );
        let result = resolve_tool_call_display(
            "Edit src/main.ts:12",
            Some(&preview),
            Some("/Users/dev/project"),
        );
        assert_eq!(result.target.as_deref(), Some("src/main.ts:12"));
        assert_eq!(
            result.file_path.as_deref(),
            Some("/Users/dev/project/src/main.ts")
        );
        assert!(!result.preview_matches_file);
    }

    #[test]
    fn trusts_a_windows_style_label_target_ending_in_an_extensionless_filename() {
        let result =
            resolve_tool_call_display("Read C:\\repo\\docker\\Dockerfile", None, Some("C:/repo"));
        assert_eq!(
            result.target.as_deref(),
            Some("C:\\repo\\docker\\Dockerfile")
        );
        assert_eq!(
            result.file_path.as_deref(),
            Some("C:/repo/docker/Dockerfile")
        );
    }
}

mod nested_scroll_absorbs_wheel {
    use super::*;

    const OVERFLOWING: ScrollMetrics = ScrollMetrics {
        scroll_top: 40.,
        scroll_height: 200.,
        client_height: 80.,
    };

    #[test]
    fn lets_the_parent_handle_the_wheel_when_the_list_does_not_overflow() {
        let fits = ScrollMetrics {
            scroll_top: 0.,
            scroll_height: 80.,
            client_height: 80.,
        };
        assert!(!nested_scroll_absorbs_wheel(fits, -20.));
    }

    #[test]
    fn consumes_scrolling_that_still_has_room_inside_the_list() {
        assert!(nested_scroll_absorbs_wheel(OVERFLOWING, -20.));
        assert!(nested_scroll_absorbs_wheel(OVERFLOWING, 20.));
    }

    #[test]
    fn releases_the_wheel_at_the_edges_so_the_transcript_can_take_over() {
        let top = ScrollMetrics {
            scroll_top: 0.,
            ..OVERFLOWING
        };
        let bottom = ScrollMetrics {
            scroll_top: 120.,
            ..OVERFLOWING
        };
        assert!(!nested_scroll_absorbs_wheel(top, -20.));
        assert!(!nested_scroll_absorbs_wheel(bottom, 20.));
    }
}

mod prose_summary {
    use super::*;

    #[test]
    fn reduces_a_paragraph_to_one_plain_line() {
        assert_eq!(
            prose_summary("**Full checks pass** — `cargo fmt` and 134 tests.\n\nBumping:"),
            "Full checks pass — cargo fmt and 134 tests."
        );
    }

    #[test]
    fn skips_fenced_code_and_list_markers() {
        assert_eq!(
            prose_summary("```ts\nconst a = 1;\n```\n\n- Ran [checks](x.md)"),
            "Ran checks"
        );
    }
}

mod subagent_model_labels {
    use super::*;

    #[test]
    fn keeps_unknown_model_ids_and_leaves_unspecified_models_blank() {
        let catalog = ModelCatalog::new();
        let row = |model: Option<&str>| {
            let mut block = Block::new("agent", BlockRole::Tool, "Review");
            block.agent_run = Some(crate::block::AgentRunMeta {
                name: "Review".into(),
                model: model.map(str::to_string),
                ..Default::default()
            });
            block
        };
        assert_eq!(
            subagent_model_name(&row(Some("claude-haiku-4-5")), &catalog).as_deref(),
            Some("Haiku 4.5")
        );
        assert_eq!(
            subagent_model_name(&row(Some("custom-model-v2")), &catalog).as_deref(),
            Some("custom-model-v2")
        );
        for model in [
            None,
            Some(""),
            Some("auto"),
            Some("inherit"),
            Some("default"),
        ] {
            assert_eq!(subagent_model_name(&row(model), &catalog), None);
        }
    }
}
