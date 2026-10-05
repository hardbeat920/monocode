//! Port of the types and provider table in src/features/sessions/model/btw.ts.
//!
//! `BtwThread` and `BtwMessage` live in `block`, because blocks hold them.
//! The side-thread flow itself stays with the engine.

use crate::block::{Block, BtwThread};
use crate::harness::HarnessId;

/// `BTW_HARNESSES`: harnesses with an isolated text runner suitable for
/// read-only side conversations. fx, Hermes, Droid, and Antigravity do not
/// expose one yet.
pub const BTW_HARNESSES: [HarnessId; 7] = [
    HarnessId::Claude,
    HarnessId::Codex,
    HarnessId::Cursor,
    HarnessId::Grok,
    HarnessId::Opencode,
    HarnessId::Pi,
    HarnessId::Omp,
];

/// `BTW_MAX_BLOCK_CHARS`.
pub const BTW_MAX_BLOCK_CHARS: usize = 8_000;
/// `BTW_MAX_SNAPSHOT_CHARS`.
pub const BTW_MAX_SNAPSHOT_CHARS: usize = 64_000;

/// `supportsBtwHarness`.
pub fn supports_btw_harness(harness: Option<HarnessId>) -> bool {
    harness.is_some_and(|harness| BTW_HARNESSES.contains(&harness))
}

/// `sessionHasBtwThreads`.
pub fn session_has_btw_threads(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| {
        block
            .btw_threads
            .as_ref()
            .is_some_and(|threads| !threads.is_empty())
    })
}

/// `BtwSessionThread`: a side thread together with the turn it was asked about.
#[derive(Debug, Clone, PartialEq)]
pub struct BtwSessionThread {
    pub thread: BtwThread,
    pub turn: Vec<Block>,
}
