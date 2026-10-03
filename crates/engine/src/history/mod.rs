//! Engine package `history`: the sidebar session list (filters, folders,
//! pins, archive, delete, selection), the app-wide search page, and notes.
//!
//! `History` holds the history rows and actions plus the sidebar list
//! state. Calls into other packages go through `HistoryHost` (host.rs).

pub mod app_search;
pub mod archive_shortcut;
#[allow(clippy::module_inception)]
pub mod history;
pub mod host;
pub mod note_images;
pub mod notes;
pub mod notes_backend;
pub mod notes_entity;
pub mod paths;
pub mod search;
pub mod session_filters;
pub mod session_folders;
pub mod session_removal;
pub mod session_selection;
pub mod session_workspace_lifecycle;
pub mod sidebar;

pub use history::History;
pub use host::{HistoryHost, NoHistoryHost};

use std::sync::Arc;

use gpui::{App, AppContext, Entity, Global};
use monocode_settings::Kv;

pub use notes_entity::{Notes, NotesEvent};
pub use search::Search;

use notes_backend::NotesBackend;
use search::{GitProjectSearch, ProjectSearchBackend};

/// What `history::init` needs.
pub struct HistoryConfig {
    /// The settings store (folders, collapsed groups, sidebar filters).
    pub kv: Kv,
    /// Note storage, usually `StoreNotesBackend` over the engine's store.
    pub notes: Arc<dyn NotesBackend>,
    /// Project text search, usually `GitProjectSearch`.
    pub project_search: Arc<dyn ProjectSearchBackend>,
}

impl HistoryConfig {
    /// A config with project search over `monocode-git`.
    pub fn new(kv: Kv, notes: Arc<dyn NotesBackend>, cx: &App) -> Self {
        Self {
            kv,
            notes,
            project_search: Arc::new(GitProjectSearch::new(cx.background_executor().clone())),
        }
    }
}

/// The package's entities. Views observe them; install hosts with
/// `History::set_host` and `Search::set_files`.
pub struct HistoryPackage {
    pub history: Entity<History>,
    pub search: Entity<Search>,
    pub notes: Entity<Notes>,
}

impl Global for HistoryPackage {}

impl HistoryPackage {
    /// Create the entities and install the global. Call after `Engine::init`.
    pub fn init(config: HistoryConfig, cx: &mut App) {
        let kv = config.kv;
        let history = cx.new(|cx| History::new(kv, cx));
        let project_search = config.project_search;
        let search_history = history.clone();
        let search = cx.new(|cx| Search::new(search_history, project_search, cx));
        let notes_backend = config.notes;
        let notes = cx.new(|cx| Notes::new(notes_backend, cx));
        cx.set_global(HistoryPackage {
            history,
            search,
            notes,
        });
    }

    pub fn global(cx: &App) -> &HistoryPackage {
        cx.global::<HistoryPackage>()
    }

    pub fn try_global(cx: &App) -> Option<&HistoryPackage> {
        cx.try_global::<HistoryPackage>()
    }

    /// The `History` entity.
    pub fn history(cx: &App) -> Entity<History> {
        Self::global(cx).history.clone()
    }

    /// The `Search` entity.
    pub fn search(cx: &App) -> Entity<Search> {
        Self::global(cx).search.clone()
    }

    /// The `Notes` entity.
    pub fn notes(cx: &App) -> Entity<Notes> {
        Self::global(cx).notes.clone()
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::notes_backend::fake::FakeNotes;
    use super::*;
    use crate::runtime::testing::init_test_engine;

    #[gpui::test]
    fn installs_the_package_entities(cx: &mut TestAppContext) {
        init_test_engine(cx);
        cx.update(|cx| {
            let config = HistoryConfig::new(Kv::in_memory(), FakeNotes::with(Vec::new()), cx);
            HistoryPackage::init(config, cx);
            assert!(HistoryPackage::try_global(cx).is_some());
            assert!(!HistoryPackage::search(cx).read(cx).is_open());
            assert!(!HistoryPackage::notes(cx).read(cx).is_open());
            assert!(HistoryPackage::history(cx).read(cx).rows().is_empty());
        });
    }
}
