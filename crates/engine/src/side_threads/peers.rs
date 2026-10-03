//! What the side-thread flows need from other packages that the runtime
//! hooks do not cover. Every method has a default, so the package runs with
//! none filled in. NEEDS.md lists which package should own each one.
//!
//! The entity calls peers while it is updating, so a peer must not update
//! the `SideThreads` entity.

use gpui::App;

use crate::submit::{Submit, SubmitOptions};

/// Calls from the side-thread flows into other packages.
pub trait SideThreadPeers {
    /// `onSubmit` for the second-opinion chat. The default sends through the
    /// app's `Submit` entity and returns false without one.
    fn submit(&self, session_id: &str, text: &str, options: SubmitOptions, cx: &mut App) -> bool {
        let Some(submit) = Submit::try_global(cx) else {
            return false;
        };
        submit.update(cx, |submit, cx| {
            submit.on_submit(session_id, text, Vec::new(), options, cx)
        })
    }

    /// The tab half of `openSessionBeside`. The new session is already open
    /// in `Sessions`. Split the tab that holds `source_id` to the right with
    /// `session_id` and focus it, or open it in a new tab for `cwd` when no
    /// tab holds the source. Then take focus from the project terminal and
    /// set the composer focus to `focus_composer`.
    fn open_session_beside(
        &self,
        _source_id: &str,
        _session_id: &str,
        _cwd: &str,
        _focus_composer: bool,
        _cx: &mut App,
    ) {
    }

    /// `sessionReminders.dismissDue`: dismiss the session's due reminders.
    fn dismiss_due_reminders(&self, _session_id: &str, _cx: &mut App) {}

    /// `markLinkedSessionUpdateSeen`: remember the remote snapshot the user
    /// acknowledged for this session.
    fn mark_linked_session_update_seen(
        &self,
        _session_id: &str,
        _remote_updated_at: i64,
        _cx: &mut App,
    ) {
    }
}

/// The defaults: submit through the `Submit` global, and nothing else.
pub struct DefaultSideThreadPeers;

impl SideThreadPeers for DefaultSideThreadPeers {}
