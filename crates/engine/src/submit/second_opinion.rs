//! Port of src/features/sessions/model/secondOpinion.ts: ask another
//! provider to review the work one turn did, in the same working copy.

use monocode_core::block::{SecondOpinionKind, SecondOpinionMeta};
use monocode_core::paths::display_path;
use monocode_core::reducer::is_edit_tool;
use monocode_core::{Block, BlockRole, Extra, HARNESSES, HarnessId, js};
use monocode_harness::core::json_text::limit_section;

use super::ci_repair::compact_ci_repair_context;
use super::text::{collapse_space, normalize_newlines};

const USER_LIMIT: usize = 400;
const REPORT_LIMIT: usize = 900;
const PROMPT_LIMIT: usize = 1_800;
const CI_BASE_LIMIT: usize = 900;

/// `SECOND_OPINION_TITLE`.
pub const SECOND_OPINION_TITLE: &str = "Second opinion";

/// `harnessForTurn`: which provider produced this turn, walking back through
/// handoff dividers.
pub fn harness_for_turn(blocks: &[Block], turn: &[Block], session_harness: HarnessId) -> HarnessId {
    if let Some(recorded) = turn
        .iter()
        .find(|block| block.role == BlockRole::User)
        .and_then(|block| block.turn_model.as_ref())
    {
        return recorded.harness;
    }
    let start = turn
        .first()
        .and_then(|first| blocks.iter().position(|block| block.id == first.id));
    if let Some(start) = start.filter(|start| *start > 0) {
        for block in blocks[..start].iter().rev() {
            if let Some(handoff) = &block.handoff {
                return handoff.to;
            }
        }
    }
    blocks
        .iter()
        .find_map(|block| block.handoff.as_ref())
        .map_or(session_harness, |handoff| handoff.from)
}

/// `turnUserRequest`.
pub fn turn_user_request(blocks: &[Block]) -> String {
    blocks
        .iter()
        .find(|block| block.role == BlockRole::User)
        .map(|block| js::trim(&normalize_newlines(&block.text)).to_string())
        .unwrap_or_default()
}

/// `turnReport`.
pub fn turn_report(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.role,
                BlockRole::Assistant | BlockRole::Tasks | BlockRole::Plan
            )
        })
        .map(|block| js::trim(&normalize_newlines(&block.text)).to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `block.text || block.tool?.title`, the title an edit check reads.
pub fn tool_title(block: &Block) -> Option<&str> {
    if !block.text.is_empty() {
        return Some(&block.text);
    }
    block.tool.as_ref().and_then(|tool| tool.title.as_deref())
}

/// `isEditTool` over a transcript block.
pub fn is_edit_block(block: &Block) -> bool {
    let tool = block.tool.as_ref();
    is_edit_tool(
        tool.and_then(|tool| tool.kind.as_deref()),
        tool_title(block),
        tool.and_then(|tool| tool.preview.as_ref()),
    )
}

/// An insertion-ordered map keyed by lowercase label, like the TypeScript
/// `Map` it replaces: a repeated key keeps its place and takes the new value.
pub(crate) fn set_label(files: &mut Vec<(String, String)>, label: String) {
    let key = label.to_lowercase();
    match files.iter_mut().find(|(existing, _)| *existing == key) {
        Some(entry) => entry.1 = label,
        None => files.push((key, label)),
    }
}

/// `turnEditedFiles`.
pub fn turn_edited_files(blocks: &[Block], cwd: Option<&str>) -> Vec<String> {
    let mut files: Vec<(String, String)> = Vec::new();
    for block in blocks {
        if block.role != BlockRole::Tool && block.role != BlockRole::Approval {
            continue;
        }
        if !is_edit_block(block) {
            continue;
        }
        let preview = block.tool.as_ref().and_then(|tool| tool.preview.as_ref());
        let path = match preview {
            Some(preview) if preview.path.as_deref().is_some_and(|path| !path.is_empty()) => Some(
                display_path(preview.path.as_deref().unwrap_or_default(), cwd),
            ),
            Some(preview) => preview.file_name.clone(),
            None => None,
        };
        let label = path.as_deref().map(js::trim).unwrap_or_default();
        if !label.is_empty() {
            set_label(&mut files, label.to_string());
        }
    }
    files.into_iter().take(40).map(|(_, label)| label).collect()
}

/// `secondOpinionTargets` options.
pub struct SecondOpinionTargetOptions<'a> {
    pub installed: &'a dyn Fn(HarnessId) -> bool,
    pub visible: &'a dyn Fn(HarnessId) -> bool,
    pub probed: bool,
    pub include_current: bool,
}

/// `secondOpinionTargets`.
pub fn second_opinion_targets(
    from: HarnessId,
    options: &SecondOpinionTargetOptions<'_>,
) -> Vec<HarnessId> {
    let others: Vec<HarnessId> = HARNESSES
        .into_iter()
        .filter(|id| {
            if *id == from || !(options.visible)(*id) {
                return false;
            }
            !options.probed || (options.installed)(*id)
        })
        .collect();
    if options.include_current && (!options.probed || (options.installed)(from)) {
        return std::iter::once(from).chain(others).collect();
    }
    others
}

/// `buildSecondOpinionPrompt` input.
pub struct SecondOpinionPromptInput<'a> {
    pub from: HarnessId,
    pub user_request: &'a str,
    pub report: &'a str,
    pub files: &'a [String],
    pub ci_context: Option<&'a str>,
}

/// `buildSecondOpinionPrompt`.
pub fn build_second_opinion_prompt(input: &SecondOpinionPromptInput<'_>) -> String {
    let from_title = input.from.title();
    let request = js::trim(input.user_request);
    let report = js::trim(input.report);
    let files: Vec<&str> = input
        .files
        .iter()
        .map(|path| js::trim(path))
        .filter(|path| !path.is_empty())
        .collect();

    let mut sections = vec![
        format!(
            "Give a second opinion on work {from_title} just finished in this same working copy. The files are already on disk."
        ),
        "Review that work: what is wrong, what is missing, and what you would have done differently. Fix anything you agree is broken or incomplete. If you would leave it, say so and stop. Do not redo the task from scratch unless the work is actually wrong. Read the listed files before changing anything.".to_string(),
        format!(
            "## User request\n{}",
            if request.is_empty() {
                "(no user message on this turn)".to_string()
            } else {
                limit_section(request, USER_LIMIT)
            }
        ),
    ];
    if report.is_empty() {
        sections.push(format!(
            "## What {from_title} reported\n(no written summary — inspect the files)"
        ));
    } else {
        sections.push(format!(
            "## What {from_title} reported\n{}",
            limit_section(report, REPORT_LIMIT)
        ));
    }
    if files.is_empty() {
        sections.push("## Files it edited\n(none recorded on this turn)".to_string());
    } else {
        let list: Vec<String> = files.iter().map(|path| format!("- {path}")).collect();
        sections.push(format!("## Files it edited\n{}", list.join("\n")));
    }

    let base = sections.join("\n\n");
    let Some(ci_context) = input.ci_context.filter(|context| !context.is_empty()) else {
        return limit_section(&base, PROMPT_LIMIT);
    };
    let suffix = "\n\n## CI context\n";
    let truncated = "\n\n[truncated]";
    let base_prefix = if js::len(&base) <= CI_BASE_LIMIT {
        base
    } else {
        format!(
            "{}{truncated}",
            js::slice_prefix(&base, CI_BASE_LIMIT - js::len(truncated))
        )
    };
    let budget = PROMPT_LIMIT as i64 - js::len(&base_prefix) as i64 - js::len(suffix) as i64;
    format!(
        "{base_prefix}{suffix}{}",
        compact_ci_repair_context(ci_context, budget)
    )
}

/// `buildSecondOpinionCard`.
pub fn build_second_opinion_card(
    from: HarnessId,
    to: HarnessId,
    user_request: &str,
    files: &[String],
    kind: Option<SecondOpinionKind>,
) -> SecondOpinionMeta {
    let request = js::trim(&collapse_space(user_request)).to_string();
    SecondOpinionMeta {
        from,
        to,
        request: (!request.is_empty()).then(|| js::slice_prefix(&request, 240).to_string()),
        files: (!files.is_empty()).then_some(files.len() as i64),
        kind,
        extra: Extra::new(),
    }
}

/// `buildSecondOpinionRequest`: the prompt and the metadata for its saved
/// user turn.
#[derive(Debug, Clone, PartialEq)]
pub struct SecondOpinionRequest {
    pub prompt: String,
    pub ci_context: Option<String>,
    pub second_opinion: SecondOpinionMeta,
}

/// `buildSecondOpinionRequest`.
pub fn build_second_opinion_request(
    from: HarnessId,
    to: HarnessId,
    turn: &[Block],
    cwd: &str,
) -> SecondOpinionRequest {
    let user_request = turn_user_request(turn);
    let files = turn_edited_files(turn, Some(cwd));
    let ci_context = turn
        .iter()
        .find(|block| block.role == BlockRole::User)
        .and_then(|block| block.ci_context.clone())
        .filter(|context| !context.is_empty());
    SecondOpinionRequest {
        prompt: build_second_opinion_prompt(&SecondOpinionPromptInput {
            from,
            user_request: &user_request,
            report: &turn_report(turn),
            files: &files,
            ci_context: ci_context.as_deref(),
        }),
        second_opinion: build_second_opinion_card(from, to, &user_request, &files, None),
        ci_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::{
        BlockTool, HandoffMeta, HandoffStatus, ToolPreview, ToolPreviewKind, TurnModel,
    };

    fn user(id: &str, text: &str) -> Block {
        Block::new(id, BlockRole::User, text)
    }

    fn assistant(id: &str, text: &str) -> Block {
        Block::new(id, BlockRole::Assistant, text)
    }

    fn edit(id: &str, path: &str) -> Block {
        Block {
            tool: Some(BlockTool {
                kind: Some("edit".into()),
                title: Some(format!("Edited {path}")),
                status: Some("completed".into()),
                preview: Some(ToolPreview {
                    path: Some(path.into()),
                    file_name: path.split('/').next_back().map(str::to_string),
                    ..ToolPreview::new(ToolPreviewKind::Write)
                }),
                ..BlockTool::default()
            }),
            ..Block::new(id, BlockRole::Tool, format!("Edited {path}"))
        }
    }

    fn handoff(from: HarnessId, to: HarnessId) -> Block {
        Block {
            handoff: Some(HandoffMeta {
                from,
                to,
                status: HandoffStatus::Ready,
                pending: None,
                extra: Extra::new(),
            }),
            ..Block::new("h", BlockRole::Handoff, "")
        }
    }

    // harnessForTurn
    #[test]
    fn prefers_provider_provenance_recorded_on_the_turn() {
        let turn = vec![
            Block {
                turn_model: Some(TurnModel {
                    harness: HarnessId::Claude,
                    id: "claude:opus-5".into(),
                    name: "Claude Opus 5".into(),
                    extra: Extra::new(),
                }),
                ..user("u", "go")
            },
            assistant("a", "done"),
        ];
        assert_eq!(
            harness_for_turn(&turn, &turn, HarnessId::Codex),
            HarnessId::Claude
        );
    }

    #[test]
    fn uses_the_session_harness_when_there_was_no_handoff() {
        let turn = vec![user("u", "go"), assistant("a", "done")];
        assert_eq!(
            harness_for_turn(&turn, &turn, HarnessId::Claude),
            HarnessId::Claude
        );
    }

    #[test]
    fn attributes_a_turn_before_a_handoff_to_the_outgoing_provider() {
        let first = vec![user("u1", "go"), assistant("a1", "working")];
        let mut blocks = first.clone();
        blocks.push(handoff(HarnessId::Claude, HarnessId::Codex));
        blocks.push(user("u2", "keep going"));
        assert_eq!(
            harness_for_turn(&blocks, &first, HarnessId::Codex),
            HarnessId::Claude
        );
    }

    #[test]
    fn attributes_a_turn_after_a_handoff_to_the_incoming_provider() {
        let second = vec![user("u2", "keep going"), assistant("a2", "ok")];
        let mut blocks = vec![
            user("u1", "go"),
            handoff(HarnessId::Claude, HarnessId::Codex),
        ];
        blocks.extend(second.clone());
        assert_eq!(
            harness_for_turn(&blocks, &second, HarnessId::Codex),
            HarnessId::Codex
        );
    }

    // turn extracts
    #[test]
    fn reads_the_user_request_report_and_edited_files() {
        let turn = vec![
            user("u", "fix the footer"),
            assistant("a1", "I'll patch UsageFooter."),
            edit("e", "/repo/src/chrome/UsageFooter.tsx"),
            Block::new("p", BlockRole::Plan, "## Plan\n\n- edit the chip"),
            assistant("a2", "Done."),
        ];
        assert_eq!(turn_user_request(&turn), "fix the footer");
        assert_eq!(
            turn_report(&turn),
            "I'll patch UsageFooter.\n\n## Plan\n\n- edit the chip\n\nDone."
        );
        assert_eq!(
            turn_edited_files(&turn, Some("/repo")),
            ["src/chrome/UsageFooter.tsx"]
        );
    }

    #[test]
    fn dedupes_edited_paths() {
        assert_eq!(
            turn_edited_files(
                &[edit("a", "src/App.tsx"), edit("b", "src/App.tsx")],
                Some("/repo")
            ),
            ["src/App.tsx"]
        );
    }

    // secondOpinionTargets
    fn listed(id: HarnessId) -> bool {
        matches!(
            id,
            HarnessId::Claude
                | HarnessId::Codex
                | HarnessId::Cursor
                | HarnessId::Opencode
                | HarnessId::Pi
                | HarnessId::Omp
                | HarnessId::Fx
                | HarnessId::Grok
        )
    }

    fn claude_or_codex(id: HarnessId) -> bool {
        id == HarnessId::Claude || id == HarnessId::Codex
    }

    #[test]
    fn drops_the_provider_that_did_the_work() {
        let options = SecondOpinionTargetOptions {
            installed: &claude_or_codex,
            visible: &listed,
            probed: true,
            include_current: false,
        };
        assert_eq!(
            second_opinion_targets(HarnessId::Claude, &options),
            [HarnessId::Codex]
        );
    }

    #[test]
    fn hides_providers_the_user_turned_off_in_the_picker() {
        let options = SecondOpinionTargetOptions {
            installed: &|_| true,
            visible: &|id| id == HarnessId::Codex,
            probed: true,
            include_current: false,
        };
        assert_eq!(
            second_opinion_targets(HarnessId::Claude, &options),
            [HarnessId::Codex]
        );
    }

    #[test]
    fn keeps_unprobed_visible_providers_so_the_menu_can_open() {
        let options = SecondOpinionTargetOptions {
            installed: &|_| false,
            visible: &|id| id == HarnessId::Codex || id == HarnessId::Cursor,
            probed: false,
            include_current: false,
        };
        assert_eq!(
            second_opinion_targets(HarnessId::Claude, &options),
            [HarnessId::Codex, HarnessId::Cursor]
        );
    }

    #[test]
    fn can_include_the_current_provider_for_choosing_another_model() {
        let options = SecondOpinionTargetOptions {
            installed: &claude_or_codex,
            visible: &listed,
            probed: true,
            include_current: true,
        };
        assert_eq!(
            second_opinion_targets(HarnessId::Codex, &options),
            [HarnessId::Codex, HarnessId::Claude]
        );
    }

    // buildSecondOpinionPrompt
    #[test]
    fn asks_the_next_agent_to_review_and_names_the_files() {
        let files = vec!["src/chrome/UsageFooter.tsx".to_string()];
        let prompt = build_second_opinion_prompt(&SecondOpinionPromptInput {
            from: HarnessId::Claude,
            user_request: "fix the footer",
            report: "Updated the usage chip.",
            files: &files,
            ci_context: None,
        });
        assert!(prompt.contains("Claude Code"));
        assert!(prompt.contains("fix the footer"));
        assert!(prompt.contains("Updated the usage chip."));
        assert!(prompt.contains("- src/chrome/UsageFooter.tsx"));
        assert!(prompt.contains("Do not redo the task from scratch"));
    }

    #[test]
    fn still_builds_a_prompt_when_the_turn_left_no_summary_or_edits() {
        let prompt = build_second_opinion_prompt(&SecondOpinionPromptInput {
            from: HarnessId::Codex,
            user_request: "",
            report: "",
            files: &[],
            ci_context: None,
        });
        assert!(prompt.contains("Codex"));
        assert!(prompt.contains("(no user message on this turn)"));
        assert!(prompt.contains("(no written summary"));
        assert!(prompt.contains("(none recorded on this turn)"));
    }

    // buildSecondOpinionCard
    #[test]
    fn keeps_a_short_request_and_the_file_count() {
        let card = build_second_opinion_card(
            HarnessId::Claude,
            HarnessId::Codex,
            "  fix the footer\nplease  ",
            &["a.ts".into(), "b.ts".into()],
            None,
        );
        assert_eq!(card.request.as_deref(), Some("fix the footer please"));
        assert_eq!(card.files, Some(2));
        assert_eq!(card.kind, None);
    }

    #[test]
    fn omits_empty_request_and_file_fields() {
        let card = build_second_opinion_card(HarnessId::Cursor, HarnessId::Pi, "   ", &[], None);
        assert_eq!(
            serde_json::to_value(&card).unwrap(),
            serde_json::json!({ "from": "cursor", "to": "pi" })
        );
    }

    #[test]
    fn marks_a_split_pane_continue_as_a_handoff() {
        let card = build_second_opinion_card(
            HarnessId::Claude,
            HarnessId::Codex,
            "fix the footer",
            &["a.ts".into()],
            Some(SecondOpinionKind::Handoff),
        );
        assert_eq!(
            serde_json::to_value(&card).unwrap(),
            serde_json::json!({ "from": "claude", "to": "codex", "request": "fix the footer", "files": 1, "kind": "handoff" })
        );
    }
}
