//! Engine package `side_threads`: a turn's side conversations. Ports the
//! second-opinion and split-pane handoff flows and the "by the way" side
//! threads from src/app/App.tsx (lines 2434-2443, 4135-4149, 7661-8385),
//! with the flow half of src/features/sessions/model/btw.ts.
//!
//! Start with `SideThreads::init` after `Engine::init`, then call the
//! flows on `SideThreads::global(cx)`. Results land in `Sessions`.
//!
//! The handoff and second-opinion models are submit's (`submit::handoff`,
//! `submit::second_opinion`, `submit::handoff_turn`), and the live agents
//! panel rows are attention's (`attention::live_agents`). This package
//! reuses them.

pub mod btw;
mod flows;
pub mod live_agents;
pub mod peers;
mod requests;

#[cfg(test)]
mod tests;

pub use flows::{BtwSubmit, SideThreads, SideThreadsConfig};
pub use live_agents::{LiveAgent, live_agents};
pub use peers::{DefaultSideThreadPeers, SideThreadPeers};
pub use requests::request_key;
