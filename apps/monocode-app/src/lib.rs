//! The engine side of the MonoCode app: the data directory, settings, the
//! harness bridge with every provider, and the engine packages, wired
//! together. The window in `main.rs` and the headless live test both start
//! here with [`boot::boot`].

pub mod attention_platform;
pub mod boot;
pub mod bridge;
pub mod data_dir;
pub mod history;
pub mod projects;
pub mod provider_hooks;
pub mod session_factory;
