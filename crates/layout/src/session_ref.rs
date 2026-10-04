//! `Pick<Session, "id" | "cwd" | "worktreeCwd">`: the session fields the
//! layout helpers read. The TypeScript passed full sessions or snapshot
//! stubs; both implement this trait.

use monocode_core::Session;

/// A session id, its project working directory, and its working copy.
pub trait SessionRef {
    fn session_id(&self) -> &str;
    fn session_cwd(&self) -> &str;

    /// `worktreeCwd`: the linked worktree the session runs in. `None` for a
    /// session in the project folder, and for refs that do not know it.
    fn session_worktree_cwd(&self) -> Option<&str> {
        None
    }
}

impl SessionRef for Session {
    fn session_id(&self) -> &str {
        &self.id
    }

    fn session_cwd(&self) -> &str {
        &self.cwd
    }

    fn session_worktree_cwd(&self) -> Option<&str> {
        self.worktree_cwd.as_deref()
    }
}

impl<T: SessionRef + ?Sized> SessionRef for &T {
    fn session_id(&self) -> &str {
        (**self).session_id()
    }

    fn session_cwd(&self) -> &str {
        (**self).session_cwd()
    }

    fn session_worktree_cwd(&self) -> Option<&str> {
        (**self).session_worktree_cwd()
    }
}
