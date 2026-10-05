//! The `Engine` global: the runtime's entities and services, created once
//! per app (windowed or headless).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext, Entity, Global};

use super::backend::{CheckpointBackend, SessionBackend, StoreBackend};
use super::checkpoint::{Checkpoints, ReviewChanges, new_review_entity};
use super::hooks::EngineHooks;
use super::lifecycle::Lifecycle;
use super::session_store::SessionWriter;
use super::sessions::Sessions;

/// What `Engine::init` needs.
pub struct EngineConfig {
    pub sessions: Arc<dyn SessionBackend>,
    pub checkpoints: Arc<dyn CheckpointBackend>,
    pub hooks: EngineHooks,
}

impl EngineConfig {
    /// Storage in `monocode.db` under the app data directory.
    pub fn open_store(data_dir: PathBuf, cx: &App) -> Result<Self, String> {
        let backend = Arc::new(StoreBackend::open(
            data_dir,
            cx.background_executor().clone(),
        )?);
        Ok(Self {
            sessions: backend.clone(),
            checkpoints: backend,
            hooks: EngineHooks::default(),
        })
    }

    /// Any backend for both stores, such as the fake in `runtime::testing`.
    pub fn with_backend<B: SessionBackend + CheckpointBackend>(backend: Arc<B>) -> Self {
        Self {
            sessions: backend.clone(),
            checkpoints: backend,
            hooks: EngineHooks::default(),
        }
    }
}

/// App state as GPUI entities. Views observe `sessions` and `lifecycle`
/// directly; other packages reach the services through `Engine::global`.
pub struct Engine {
    pub sessions: Entity<Sessions>,
    pub lifecycle: Entity<Lifecycle>,
    /// Emits `ReviewChanged` when a session's reviewable changes move.
    pub review: Entity<ReviewChanges>,
    pub writer: SessionWriter,
    pub checkpoints: Checkpoints,
    pub hooks: EngineHooks,
}

impl Global for Engine {}

impl Engine {
    /// Create the runtime entities and install the global.
    pub fn init(config: EngineConfig, cx: &mut App) {
        let executor = cx.background_executor().clone();
        let writer = SessionWriter::new(config.sessions, executor.clone());
        let checkpoints = Checkpoints::new(config.checkpoints, executor);
        let sessions = cx.new(Sessions::new);
        let lifecycle = cx.new(Lifecycle::new);
        let review = new_review_entity(cx);
        cx.set_global(Engine {
            sessions,
            lifecycle,
            review,
            writer,
            checkpoints,
            hooks: config.hooks,
        });
    }

    pub fn global(cx: &App) -> &Engine {
        cx.global::<Engine>()
    }

    pub fn try_global(cx: &App) -> Option<&Engine> {
        cx.try_global::<Engine>()
    }

    /// The `Sessions` entity.
    pub fn sessions(cx: &App) -> Entity<Sessions> {
        Self::global(cx).sessions.clone()
    }

    /// The `Lifecycle` entity.
    pub fn lifecycle(cx: &App) -> Entity<Lifecycle> {
        Self::global(cx).lifecycle.clone()
    }

    /// The ordered session store writer.
    pub fn writer(cx: &App) -> SessionWriter {
        Self::global(cx).writer.clone()
    }

    /// The checkpoint queue.
    pub fn checkpoints(cx: &App) -> Checkpoints {
        Self::global(cx).checkpoints.clone()
    }

    /// A copy of the hook table. Clone it out before calling a hook so the
    /// global is not borrowed during the call.
    pub fn hooks(cx: &App) -> EngineHooks {
        Self::try_global(cx)
            .map(|engine| engine.hooks.clone())
            .unwrap_or_default()
    }

    /// Fill in hooks. Each package replaces only its own field.
    pub fn set_hooks(cx: &mut App, update: impl FnOnce(&mut EngineHooks)) {
        update(&mut cx.global_mut::<Engine>().hooks);
    }
}
