//! The search page (SearchView.tsx).

pub mod data;
pub mod model;
pub mod view;

#[cfg(test)]
mod tests;

pub use data::{LocalSearch, SearchData};
pub use model::{
    AppSearchHit, ContentHit, ConversationHit, FileHit, MessageHit, ProjectHit, SearchScope,
    SearchState,
};
pub use view::SearchView;
