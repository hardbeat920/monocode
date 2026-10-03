//! Shared data model for MonoCode: sessions, transcript blocks, harness
//! events, the model catalog, and settings shapes. Pure data and functions,
//! with no IO.
//!
//! JSON shapes match what the TypeScript app wrote, because the native app
//! reads the same `monocode.db` and the same stored settings. Persisted
//! structs keep unknown fields in an `extra` map so a round trip loses
//! nothing.

pub mod appearance;
pub mod attachment;
pub mod block;
pub mod btw;
pub mod context_usage;
pub mod handoff;
pub mod harness;
pub mod harness_event;
pub mod inbox;
pub mod js;
pub mod models;
pub mod notes;
pub mod orchestration;
pub mod paths;
pub mod plan;
pub mod platform;
pub mod project_providers;
pub mod reducer;
pub mod session;
pub mod settings;
pub mod shortcut;
pub mod task_list;
pub mod transcript;
pub mod user_question;

pub use attachment::{Attachment, AttachmentKind};
pub use block::{Block, BlockRole, Extra, ModelSettings};
pub use context_usage::ContextUsage;
pub use harness::{HARNESSES, HarnessId, RUNTIME_MODES, RuntimeMode};
pub use harness_event::HarnessEvent;
pub use models::{AgentModel, HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs};
pub use platform::Platform;
pub use project_providers::{ProjectProviderSettings, ProjectProviders};
pub use session::Session;
pub use settings::{AppSettings, Settings};
