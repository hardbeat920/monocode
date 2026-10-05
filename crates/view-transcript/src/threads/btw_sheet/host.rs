//! What the btw sheet needs from the engine. BtwSheet.tsx took the flows as
//! callback props (`onSubmit`, `onRetry`, `onDelete`, `onStop`,
//! `onModelChange`) and imported the rules from btw.ts. Both live in
//! `monocode_engine::side_threads` now, so the sheet reaches them through
//! one trait the app implements.

use std::rc::Rc;

use gpui::App;
use monocode_core::HarnessId;
use monocode_core::block::{Block, BtwMessage, BtwThread, ModelSettings};
use monocode_core::btw::BtwSessionThread;
use monocode_view_composer::composer::ComposerHost;

/// `btwThreadBlocks` input, as `side_threads::btw::BtwThreadBlocksInput`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BtwThreadBlocksInput<'a> {
    pub messages: &'a [BtwMessage],
    pub pending_blocks: Option<&'a [Block]>,
    pub running: bool,
    /// When the thread last changed, to close a turn that failed unanswered.
    pub updated_at: Option<i64>,
    pub harness: Option<HarnessId>,
    pub model: Option<&'a str>,
}

/// One side question: `onSubmit(turn, threadId, messageId, text, model,
/// modelSettings)`.
#[derive(Clone, Copy, Debug)]
pub struct BtwRequest<'a> {
    /// The `groupTurns` turn the question is about.
    pub turn: &'a [Block],
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub text: &'a str,
    /// `None` means the session default.
    pub model: Option<&'a str>,
    pub model_settings: &'a ModelSettings,
}

/// The engine side of the sheet. Rules first (pure functions of the
/// session), then the flows, then the composer's own host.
pub trait BtwHost: 'static {
    /// `sessionBtwThreads`: every side thread in the session, oldest first.
    fn session_threads(&self, blocks: &[Block], managed: bool) -> Vec<BtwSessionThread>;

    /// `btwOpenTargetTurnId`: the finished turn a new side question reads.
    fn open_target_turn_id(
        &self,
        turns: &[Vec<Block>],
        blocks: &[Block],
        session_harness: HarnessId,
        managed: bool,
    ) -> Option<String>;

    /// `btwSurfaceHarness`: the provider that answers questions about `turn`.
    fn surface_harness(
        &self,
        blocks: &[Block],
        turn: &[Block],
        session_harness: HarnessId,
        threads: Option<&[BtwThread]>,
    ) -> Option<HarnessId>;

    /// `btwThreadBlocks`: a side thread as ordinary transcript blocks.
    fn thread_blocks(&self, input: BtwThreadBlocksInput<'_>) -> Vec<Block>;

    /// `preferredModelSettings(resolveModel(harness, model), current)`.
    fn preferred_model_settings(
        &self,
        harness: HarnessId,
        model: &str,
        current: &ModelSettings,
    ) -> ModelSettings;

    /// `onSubmit`. Return false to reject the question; the sheet then
    /// keeps the text and stays as it was.
    fn submit(&self, request: BtwRequest<'_>, cx: &mut App) -> bool;

    /// `onRetry`.
    fn retry(&self, turn: &[Block], thread_id: &str, cx: &mut App);

    /// `onDelete`.
    fn delete(&self, _turn: &[Block], _thread_id: &str, _cx: &mut App) {}

    /// `onStop`.
    fn stop(&self, _turn: &[Block], _thread_id: &str, _cx: &mut App) {}

    /// `onModelChange`.
    fn set_model(
        &self,
        _turn: &[Block],
        _thread_id: &str,
        _model: &str,
        _model_settings: &ModelSettings,
        _cx: &mut App,
    ) {
    }

    /// The session composer's host: attachments, skills, mentions, and the
    /// model picker's source. The sheet answers submit, stop, and draft
    /// changes itself.
    fn composer_host(&self) -> Rc<dyn ComposerHost>;
}
