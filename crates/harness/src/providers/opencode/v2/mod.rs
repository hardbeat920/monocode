//! OpenCode 2 HTTP protocol at upstream release 2.0.20.

pub mod adapter;
pub mod catalog;
pub mod client;
pub mod events;
pub mod forms;
pub mod protocol;
pub mod server;

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod transport_tests;
