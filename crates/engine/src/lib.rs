//! App state and behavior as GPUI entities. `runtime` is always on; every other
//! package sits behind a cargo feature of the same name.

#[cfg(feature = "attention")]
pub mod attention;
#[cfg(feature = "automations")]
pub mod automations;
#[cfg(feature = "history")]
pub mod history;
#[cfg(feature = "inbox")]
pub mod inbox;
#[cfg(feature = "orchestration")]
pub mod orchestration;
#[cfg(feature = "projects")]
pub mod projects;
#[cfg(feature = "remote")]
pub mod remote;
pub mod runtime;
#[cfg(feature = "side_threads")]
pub mod side_threads;
#[cfg(feature = "submit")]
pub mod submit;
#[cfg(feature = "workspace")]
pub mod workspace;
