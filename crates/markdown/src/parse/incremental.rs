//! Streaming parser: an append reparses only from the last stable top-level
//! block boundary and keeps every block before it shared.
//!
//! Text before the second-to-last top-level block cannot change when text is
//! appended. Reparsing the last two blocks, not just the last, covers merges
//! such as a trailing `3` becoming `3.` and joining the loose list above it.
//! A block's separation from the one before it is decided by its own first
//! bytes, which have already arrived, so merges cannot reach further back.
//! The parity tests stream corpora through both paths and compare.
//!
//! Link reference definitions (`[label]: url`) act at a distance, so a source
//! that contains one falls back to full reparses.
//!
//! The boundary strategy follows zeronsh/comet's `markdown/parser.rs` (MIT,
//! Copyright (c) 2026 Wing).

use std::sync::Arc;

use super::{Block, Document, TopBlock, fence, mend, parse, parse_at};

#[derive(Debug, Default)]
pub struct IncrementalParser {
    source: String,
    tree: Document,
    /// Display replacement for the last top-level block when it needs
    /// mending. `None` means the display tree equals the canonical tree.
    display_tail: Option<Vec<Arc<TopBlock>>>,
    full_only: bool,
    last_parse_bytes: usize,
    stable_prefix: usize,
}

impl IncrementalParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The parse of the source as written.
    pub fn tree(&self) -> &Document {
        &self.tree
    }

    /// The tree to render while text streams: the canonical tree with its last
    /// block mended (see [`mend`]) and a half-typed closing fence hidden.
    /// Shares every other block with [`Self::tree`].
    pub fn display_tree(&self) -> Document {
        let Some(tail) = &self.display_tail else {
            return self.tree.clone();
        };
        let stable = &self.tree.blocks[..self.tree.blocks.len().saturating_sub(1)];
        let mut blocks = Vec::with_capacity(stable.len() + tail.len());
        blocks.extend_from_slice(stable);
        blocks.extend_from_slice(tail);
        Document { blocks }
    }

    /// Bytes parsed by the last update, to check that appends cost O(tail).
    pub fn last_parse_bytes(&self) -> usize {
        self.last_parse_bytes
    }

    /// Leading top-level blocks that the last update left untouched.
    pub fn stable_prefix(&self) -> usize {
        self.stable_prefix
    }

    /// Set the source. Appends take the incremental path; anything else
    /// reparses from scratch.
    pub fn set_text(&mut self, text: &str) {
        if text.len() >= self.source.len() && text.starts_with(self.source.as_str()) {
            let delta = &text[self.source.len()..];
            self.append(delta);
        } else {
            self.reset(text);
        }
    }

    pub fn reset(&mut self, text: &str) {
        self.source.clear();
        self.source.push_str(text);
        self.full_only = has_link_definitions(text);
        self.tree = parse(text);
        self.last_parse_bytes = text.len();
        self.stable_prefix = 0;
        self.remend();
    }

    pub fn append(&mut self, delta: &str) {
        if delta.is_empty() {
            self.last_parse_bytes = 0;
            self.stable_prefix = self.tree.blocks.len();
            return;
        }
        // The delta may finish a line that started earlier.
        let scan_from = self.source.rfind('\n').map_or(0, |ix| ix + 1);
        self.source.push_str(delta);
        if !self.full_only && has_link_definitions(&self.source[scan_from..]) {
            self.full_only = true;
        }
        if self.full_only {
            self.tree = parse(&self.source);
            self.last_parse_bytes = self.source.len();
            self.stable_prefix = 0;
            self.remend();
            return;
        }

        let boundary = match self.tree.blocks.len() {
            0 | 1 => 0,
            n => self.tree.blocks[n - 2].range.start,
        };
        // Snap back to a line start so indentation context survives.
        let boundary = self.source[..boundary].rfind('\n').map_or(0, |ix| ix + 1);
        let tail = parse_at(&self.source[boundary..], boundary);
        self.last_parse_bytes = self.source.len() - boundary;
        self.tree
            .blocks
            .retain(|block| block.range.start < boundary);
        self.stable_prefix = self.tree.blocks.len();
        self.tree.blocks.extend(tail.blocks);
        self.remend();
    }

    /// Recompute the display tail for the last top-level block.
    fn remend(&mut self) {
        self.display_tail = None;
        let Some(last) = self.tree.blocks.last() else {
            return;
        };
        match &last.block {
            Block::Code(code) if !code.closed => {
                self.display_tail = display_code(last, code, &self.source).map(|top| vec![top]);
            }
            Block::Code(_) | Block::Rule | Block::Table(_) => {}
            _ => {
                let start = last.range.start;
                let Some(mended) = mend::close_hanging(&self.source[start..]) else {
                    return;
                };
                self.last_parse_bytes += mended.len();
                let mut tail = parse_at(&mended, start).blocks;
                for top in &mut tail {
                    let top = Arc::make_mut(top);
                    top.range.end = top.range.end.min(self.source.len());
                }
                self.display_tail = Some(tail);
            }
        }
    }
}

/// An unclosed code block as it should show mid-stream: no language label
/// until the info line is complete, and no half-typed closing fence.
fn display_code(top: &TopBlock, code: &super::CodeBlock, source: &str) -> Option<Arc<TopBlock>> {
    let raw = &source[top.range.clone()];
    let first_line = raw.lines().next().unwrap_or("");
    let info_pending = !raw.contains('\n') && !code.info.is_empty();
    let cut = fence::partial_closing_fence(&code.code, first_line);
    if !info_pending && cut.is_none() {
        return None;
    }
    let mut code = code.clone();
    if info_pending {
        code.info.clear();
    }
    if let Some(cut) = cut {
        code.code.truncate(cut);
    }
    Some(Arc::new(TopBlock {
        range: top.range.clone(),
        block: Block::Code(code),
    }))
}

/// Conservative check for link reference definition lines (`[label]: dest`,
/// up to three leading spaces).
fn has_link_definitions(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        line.len() - trimmed.len() <= 3 && trimmed.starts_with('[') && trimmed.contains("]:")
    })
}
