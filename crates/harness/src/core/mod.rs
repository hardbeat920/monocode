//! Provider-independent harness framework. Port of src/integrations/harness/core,
//! plus the feature models the adapters share (session titles, git text,
//! provider accounts, provider binary paths).
//!
//! Start with [`registry::HarnessAdapter`] (the trait each provider
//! implements), [`register::HarnessContext`] (what a provider gets when it
//! registers), and [`child::Children`] (process I/O).

pub mod abort_text_prompt;
pub mod acp;
pub mod acp_subagents;
pub mod auth;
pub mod auth_support;
pub mod availability;
pub mod availability_state;
pub mod catalog;
pub mod child;
pub mod git_text;
pub mod json_rpc;
pub mod json_text;
pub mod local_store;
pub mod native_commands;
pub mod provider_account_credentials;
pub mod provider_account_identity;
pub mod provider_accounts;
pub mod provider_binary_paths;
pub mod register;
pub mod registry;
pub mod session_title;
pub mod task;
pub mod text_harness;

#[cfg(test)]
pub(crate) mod testing;

pub use catalog::SharedCatalog;
pub use child::{
    BinaryPathChoice, ChildAccount, ChildBackend, ChildEvent, ChildEvents, ChildHandlers,
    ChildRouter, Children, HostChildBackend, HostChildOptions, SseEvent, SseEvents,
};
pub use local_store::{LocalStore, MemoryStore};
pub use register::{HarnessContext, register_builtin_harnesses};
pub use registry::{
    AcceptedHook, AdapterCapabilities, ControlTurns, EventSink, GeneratedPrContent, HarnessAdapter,
    HarnessRegistry, RegistryOptions, TextPromptInput, TitleInput, TurnControl, event_sink,
    ignore_events,
};
pub use task::{AbortSignal, BoxFuture, SharedSpawner, SmolSpawner, Spawner};
