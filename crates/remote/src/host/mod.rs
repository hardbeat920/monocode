//! MonoCode Host: the server a desktop pairs with to run agents on another
//! machine. Ported from the Node program in `host/`.
//!
//! The server, its SQLite store, pairing, TLS identity, network settings,
//! service installation, and the CLI commands live here. The host's engine
//! (agent turns, the transcript reducer, file and Git commands) plugs in
//! through [`HostBackend`].

pub mod attachments;
pub mod backend;
pub mod changes;
pub mod cli;
#[cfg(test)]
mod client_tests;
pub mod connect;
pub mod control;
pub mod exec;
pub mod http;
pub mod js;
pub mod listener;
pub mod network;
#[cfg(test)]
mod node_compat_tests;
pub mod owner;
pub mod protocol;
pub mod runtime;
pub mod server;
pub mod service;
pub mod store;
pub mod sync_transfer;
#[cfg(any(test, feature = "test-backend"))]
pub mod test_backend;
pub mod tls;
pub mod windows;
mod without_key;

pub use backend::HostBackend;
pub use changes::ChangeFeed;
pub use cli::{HostHandle, HostOptions, serve};
pub use store::HostStore;
pub use tls::HostIdentity;
