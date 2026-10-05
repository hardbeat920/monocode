//! MonoCode Connect views. Port of src/features/connections/ui.
//!
//! - [`connections`]: the Connections page in Settings (`ConnectionsSettings`).
//! - [`add_project`]: the dialog that opens a folder on a machine as a
//!   project (`AddRemoteProjectDialog`).
//! - [`session`]: the pane for a session that runs on a host (`RemoteSession`).
//!
//! The settings page and the dialog call the app through [`RemoteHost`]. The
//! session pane takes its state as [`session::RemoteSessionProps`], its
//! composer actions through [`session::RemoteSessionHost`], and reports the
//! rest as [`session::RemoteSessionEvent`]s.

pub mod add_project;
pub mod connections;
pub mod host;
pub mod machines;
pub mod session;
pub mod style;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod test_support;

pub use add_project::{AddRemoteProjectDialog, AddRemoteProjectEvent};
pub use connections::{ConnectionsEvent, ConnectionsSettings};
pub use host::{HostTask, RemoteHost, SshBegin, remote_project_key};
pub use session::{
    RemoteMachineState, RemoteSessionEvent, RemoteSessionHost, RemoteSessionPane,
    RemoteSessionProps,
};
