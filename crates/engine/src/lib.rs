//! App state and behavior as GPUI entities. `runtime` is always on; every other
//! package sits behind a cargo feature of the same name.

pub mod runtime;
#[cfg(feature = "submit")]
pub mod submit;
#[cfg(feature = "attention")]
pub mod attention;
#[cfg(feature = "side_threads")]
pub mod side_threads;
#[cfg(feature = "orchestration")]
pub mod orchestration;
#[cfg(feature = "workspace")]
pub mod workspace;
#[cfg(feature = "projects")]
pub mod projects;
#[cfg(feature = "history")]
pub mod history;
#[cfg(feature = "automations")]
pub mod automations;
#[cfg(feature = "inbox")]
pub mod inbox;
#[cfg(feature = "remote")]
pub mod remote;
