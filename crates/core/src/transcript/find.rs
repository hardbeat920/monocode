//! Port of src/features/sessions/model/transcriptFind.ts: which blocks a
//! transcript search matches, in reading order.

use crate::js;
use crate::{Block, BlockRole};

use super::activity::BlockRef;

/// `transcriptBlockText`: everything in a block a search can match.
pub fn transcript_block_text(block: &Block) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !js::trim(&block.text).is_empty() {
        parts.push(&block.text);
    }
    let tool = block.tool.as_ref();
    let preview = tool.and_then(|tool| tool.preview.as_ref());
    let image = block.image.as_ref();
    let optional = [
        tool.and_then(|tool| tool.title.as_deref()),
        image.map(|image| image.name.as_str()),
        image.and_then(|image| image.alt.as_deref()),
        tool.and_then(|tool| tool.detail.as_deref()),
        preview.and_then(|preview| preview.query.as_deref()),
        preview.and_then(|preview| preview.path.as_deref()),
        preview.and_then(|preview| preview.output.as_deref()),
        preview.and_then(|preview| preview.title.as_deref()),
    ];
    parts.extend(
        optional
            .into_iter()
            .flatten()
            .filter(|value| !value.is_empty()),
    );
    parts.join("\n")
}

/// `findTranscriptBlocks`: ids of the blocks whose text contains `query`,
/// ignoring case. Reasoning and chrome rows are not searched.
pub fn find_transcript_blocks(blocks: &[BlockRef], query: &str) -> Vec<String> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.role,
                BlockRole::User
                    | BlockRole::Assistant
                    | BlockRole::Tool
                    | BlockRole::Tasks
                    | BlockRole::Plan
                    | BlockRole::Image
            ) && transcript_block_text(block)
                .to_lowercase()
                .contains(&needle)
        })
        .map(|block| block.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::fixtures::{note, refs, search, thought, user};

    fn blocks() -> Vec<BlockRef> {
        let mut tool = search("tool", "sidebar chip");
        tool.text.clear();
        refs(vec![
            user("user", "Find the sidebar chip"),
            tool,
            note("assistant", "The sidebar chip is fixed."),
            thought("reasoning", "sidebar chip"),
        ])
    }

    #[test]
    fn finds_matching_transcript_blocks_in_reading_order_including_tool_previews() {
        assert_eq!(
            find_transcript_blocks(&blocks(), "SIDEBAR chip"),
            ["user", "tool", "assistant"]
        );
    }

    #[test]
    fn ignores_empty_queries_and_hidden_reasoning() {
        assert!(find_transcript_blocks(&blocks(), "  ").is_empty());
        assert!(find_transcript_blocks(&blocks(), "reasoning only").is_empty());
    }
}
