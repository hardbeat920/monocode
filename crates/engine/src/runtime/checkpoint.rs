//! Port of src/features/sessions/model/checkpoint.ts: per-session file
//! snapshots behind Keep and Undo, and the review-changed signal.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{App, AppContext, BackgroundExecutor, Context, Entity, EventEmitter, Task};
use monocode_store::checkpoint::{CheckpointApplyResult, CheckpointFileDiff, CheckpointStatus};

use super::backend::CheckpointBackend;
use super::engine::Engine;
use super::queue::SerialQueues;

/// Checkpoint commands, chained per session so a capture cannot overtake
/// the prepare before it.
#[derive(Clone)]
pub struct Checkpoints {
    backend: Arc<dyn CheckpointBackend>,
    queues: SerialQueues<String>,
}

impl Checkpoints {
    pub fn new(backend: Arc<dyn CheckpointBackend>, executor: BackgroundExecutor) -> Self {
        Self {
            backend,
            queues: SerialQueues::new(executor),
        }
    }

    fn enqueue<T: Send + 'static>(
        &self,
        session_id: &str,
        operation: impl FnOnce(&dyn CheckpointBackend) -> super::backend::StoreFuture<T>
        + Send
        + 'static,
    ) -> Task<Result<T, String>> {
        let backend = self.backend.clone();
        self.queues
            .enqueue(session_id.to_string(), move || operation(backend.as_ref()))
    }

    /// `flushSessionCheckpoint`: wait until every queued checkpoint write for
    /// this session is durable.
    pub fn flush_session_checkpoint(&self, session_id: &str) -> BoxFuture<'static, ()> {
        match self.queues.tail(&session_id.to_string()) {
            Some(tail) => tail.boxed(),
            None => futures::future::ready(()).boxed(),
        }
    }

    /// `ensureSessionCheckpoint`.
    pub fn ensure(&self, session_id: &str, cwd: &str) -> Task<Result<(), String>> {
        let (id, cwd) = (session_id.to_string(), cwd.to_string());
        self.enqueue(session_id, move |backend| backend.ensure(id, cwd))
    }

    /// `prepareSessionCheckpoint`: capture files right before a structured
    /// edit starts.
    pub fn prepare(
        &self,
        session_id: &str,
        cwd: &str,
        paths: Vec<String>,
    ) -> Task<Result<(), String>> {
        if paths.is_empty() {
            return Task::ready(Ok(()));
        }
        let (id, cwd) = (session_id.to_string(), cwd.to_string());
        self.enqueue(session_id, move |backend| backend.prepare(id, cwd, paths))
    }

    /// `captureSessionCheckpoint`.
    pub fn capture(
        &self,
        session_id: &str,
        cwd: &str,
        paths: Vec<String>,
    ) -> Task<Result<(), String>> {
        if paths.is_empty() {
            return Task::ready(Ok(()));
        }
        let (id, cwd) = (session_id.to_string(), cwd.to_string());
        self.enqueue(session_id, move |backend| backend.capture(id, cwd, paths))
    }

    /// `sessionCheckpointStatus`.
    pub fn status(&self, session_id: &str, cwd: &str) -> Task<Result<CheckpointStatus, String>> {
        let (id, cwd) = (session_id.to_string(), cwd.to_string());
        self.enqueue(session_id, move |backend| backend.status(id, cwd))
    }

    /// `applySessionCheckpoint`: apply one isolated worker's captured delta
    /// to its lead checkout.
    pub fn apply(
        &self,
        session_id: &str,
        from_cwd: &str,
        to_cwd: &str,
    ) -> Task<Result<CheckpointApplyResult, String>> {
        let (id, from, to) = (
            session_id.to_string(),
            from_cwd.to_string(),
            to_cwd.to_string(),
        );
        self.enqueue(session_id, move |backend| backend.apply(id, from, to))
    }

    /// `sessionCheckpointCleanupSafe`: true only when the checkout still
    /// matches its seeded, pre-worker state.
    pub fn cleanup_safe(&self, session_id: &str, cwd: &str) -> Task<Result<bool, String>> {
        let (id, cwd) = (session_id.to_string(), cwd.to_string());
        self.enqueue(session_id, move |backend| backend.cleanup_safe(id, cwd))
    }

    /// `forgetSessionCheckpoint`.
    pub fn forget(&self, session_id: &str) -> Task<Result<(), String>> {
        let id = session_id.to_string();
        self.enqueue(session_id, move |backend| backend.forget(id))
    }

    /// `sessionCheckpointFileDiff`.
    pub fn file_diff(
        &self,
        session_id: &str,
        cwd: &str,
        relative: &str,
    ) -> Task<Result<CheckpointFileDiff, String>> {
        let (id, cwd, relative) = (
            session_id.to_string(),
            cwd.to_string(),
            relative.to_string(),
        );
        self.enqueue(session_id, move |backend| {
            backend.file_diff(id, cwd, relative)
        })
    }

    /// `undoSessionChanges`.
    pub fn undo(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Task<Result<CheckpointStatus, String>> {
        let (id, cwd, relative) = (
            session_id.to_string(),
            cwd.to_string(),
            relative.map(str::to_string),
        );
        self.enqueue(session_id, move |backend| backend.undo(id, cwd, relative))
    }

    /// `keepSessionChanges`.
    pub fn keep(
        &self,
        session_id: &str,
        cwd: &str,
        relative: Option<&str>,
    ) -> Task<Result<CheckpointStatus, String>> {
        let (id, cwd, relative) = (
            session_id.to_string(),
            cwd.to_string(),
            relative.map(str::to_string),
        );
        self.enqueue(session_id, move |backend| backend.keep(id, cwd, relative))
    }
}

/// `monocode-review-changed`: a session's reviewable changes moved. An
/// empty id means "any session".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewChanged {
    pub session_id: String,
}

/// The entity review views subscribe to (`subscribeReviewChanged`).
#[derive(Default)]
pub struct ReviewChanges;

impl EventEmitter<ReviewChanged> for ReviewChanges {}

impl ReviewChanges {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

/// `notifyReviewChanged`.
pub fn notify_review_changed(session_id: Option<&str>, cx: &mut App) {
    let Some(review) = Engine::try_global(cx).map(|engine| engine.review.clone()) else {
        return;
    };
    let session_id = session_id.unwrap_or("").to_string();
    review.update(cx, |_, cx| cx.emit(ReviewChanged { session_id }));
}

/// `beginSessionTurn`: snapshot the worktree before a live turn so Keep and
/// Undo can target this session.
pub fn begin_session_turn(session_id: &str, cwd: &str, cx: &mut App) -> Task<Result<(), String>> {
    if cwd.is_empty() || cwd == "~" {
        return Task::ready(Ok(()));
    }
    let Some(checkpoints) = Engine::try_global(cx).map(|engine| engine.checkpoints.clone()) else {
        return Task::ready(Ok(()));
    };
    let ensure = checkpoints.ensure(session_id, cwd);
    let session_id = session_id.to_string();
    cx.spawn(async move |cx| {
        ensure.await?;
        cx.update(|cx| notify_review_changed(Some(&session_id), cx));
        Ok(())
    })
}

/// A review signal entity for `Engine::init`.
pub(crate) fn new_review_entity(cx: &mut App) -> Entity<ReviewChanges> {
    cx.new(ReviewChanges::new)
}
