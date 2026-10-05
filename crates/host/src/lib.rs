//! The remote host's engine: agent turns, files, and Git for MonoCode Host.
//! Port of host/engine.ts, host/workspace.ts, host/workspace-commands.ts,
//! and the modules behind them.
//!
//! `monocode_remote::host` is the server: pairing, TLS, sync, and the CLI.
//! [`HostEngine`] is its backend. It runs provider turns through the harness
//! adapters, applies their events with `monocode_core::reducer`, and saves
//! sessions in the host store. [`run_host_cli`] is `monocode-app host`.
//!
//! The engine calls the harness and the reducer directly instead of running
//! `monocode-engine`'s GPUI entities headless. Those entities keep the local
//! app's state in `monocode.db` and its sessions model; the host keeps
//! different state (revisions, receipts, an event journal) in `host.db`, and
//! engine.ts is small enough to port as it was.

pub mod backend;
pub mod browse;
pub mod child_backend;
pub mod cli;
pub mod commands;
pub mod engine;
pub mod git_branches;
pub mod git_worktrees;
pub mod process;
pub mod providers;
pub mod runtime;
pub mod skills;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod transport_tests;
pub mod workspace;
pub mod workspace_commands;

pub use backend::{HostEngineOptions, HostHarness};
pub use cli::{HOST_VERSION, host_engine, run_host_cli, run_host_cli_with};
pub use engine::{HostEngine, running_sessions_message};
pub use providers::{HostProvider, HostProviders};
