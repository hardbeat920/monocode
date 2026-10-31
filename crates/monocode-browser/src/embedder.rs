//! What the app embedding the browser module provides: its database, where
//! state changes go, and the few facts only the app's windows know.
//!
//! The Tauri app and the native (GPUI) app each implement [`Embedder`] once
//! and pass it to [`crate::init`].

use std::process::Child;

use rusqlite::Connection;

use crate::protocol::BrowserState;

pub trait Embedder: Send + Sync + 'static {
    /// Run `f` on the app's SQLite database, which holds the audit tables,
    /// the lease rows and the settings. Not calling `f` means "no database
    /// now": rows are skipped, nothing fails.
    fn with_db(&self, f: &mut dyn FnMut(&Connection));

    /// The browser state changed (toggle, connection, grants, tabs,
    /// requests). The app shows it; the Tauri app sends
    /// [`crate::protocol::STATE_EVENT`] to its windows.
    fn state_changed(&self, state: &BrowserState);

    /// Pixel ratio of the display the app's main window is on; the
    /// MonoCode browser starts at that scale.
    fn display_scale(&self) -> Option<f64> {
        None
    }

    /// The MonoCode browser process was just started. On Windows the app
    /// puts it in its job object, so it ends with the app even after a
    /// crash; OFF and app exit stop it either way.
    fn browser_started(&self, _child: &Child) {}
}

/// Tests: an in-memory database and no windows.
#[cfg(test)]
pub(crate) struct MemoryEmbedder(std::sync::Mutex<Connection>);

#[cfg(test)]
impl MemoryEmbedder {
    pub(crate) fn new() -> MemoryEmbedder {
        let conn = Connection::open_in_memory().expect("in-memory db");
        crate::audit::migrate(&conn).expect("migrate");
        MemoryEmbedder(std::sync::Mutex::new(conn))
    }
}

#[cfg(test)]
impl Embedder for MemoryEmbedder {
    fn with_db(&self, f: &mut dyn FnMut(&Connection)) {
        let conn = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn);
    }

    fn state_changed(&self, _state: &BrowserState) {}
}
