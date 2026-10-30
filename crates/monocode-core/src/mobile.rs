//! Mobile access: a paired phone browses and drives the desktop workspace over
//! the QUIC link in `monocode-mobile-link`. The host here is app-agnostic; the
//! app answers [`host::Event`]s from its own state.
pub mod catalog;
pub mod host;
mod router;
pub mod watch;

pub use host::{
    Device, Event, Invite, MobileHost, Push, Request, Status, TerminalStart, WatchStart,
};
pub use monocode_mobile_link::Address;
