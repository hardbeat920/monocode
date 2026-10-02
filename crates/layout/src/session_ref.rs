//! `Pick<Session, "id" | "cwd">`: the session fields the layout helpers read.
//! The TypeScript passed full sessions or snapshot stubs; both implement
//! this trait.

use monocode_core::Session;

/// A session id and its project working directory.
pub trait SessionRef {
    fn session_id(&self) -> &str;
    fn session_cwd(&self) -> &str;
}

impl SessionRef for Session {
    fn session_id(&self) -> &str {
        &self.id
    }

    fn session_cwd(&self) -> &str {
        &self.cwd
    }
}

impl<T: SessionRef + ?Sized> SessionRef for &T {
    fn session_id(&self) -> &str {
        (**self).session_id()
    }

    fn session_cwd(&self) -> &str {
        (**self).session_cwd()
    }
}
