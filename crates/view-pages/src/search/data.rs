//! What the search page reads and changes.
//!
//! The engine's history package owns the search model in its `Search`
//! entity: the debounced, cancellable session and project searches, the
//! title, transcript, file name, and recent project hits, grouping, and the
//! highlight. [`SearchData`] mirrors that entity's API, plus `open_hit`,
//! which the app answers by opening a file, a session, or a project.

use gpui::{App, AppContext as _, Entity, Subscription, Window};
use monocode_core::js;

use super::model::{AppSearchHit, SearchScope, SearchState};
use crate::data::Listener;

pub trait SearchData: 'static {
    fn state(&self, cx: &App) -> SearchState;
    /// Runs after every change.
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;

    /// Open the page for a project: a fresh query, the All scope, and the
    /// project's file listing.
    fn open(&self, cwd: &str, recents: Vec<String>, cx: &mut App);
    /// Close the page and cancel any search in flight.
    fn close(&self, cx: &mut App);
    fn set_query(&self, query: &str, cx: &mut App);
    fn set_scope(&self, scope: SearchScope, cx: &mut App);
    /// Move the highlight, wrapping at both ends.
    fn move_active(&self, delta: i64, cx: &mut App);
    fn set_active(&self, index: usize, cx: &mut App);
    /// `openHit`: a file at its path, a content match at its line and
    /// column, a conversation, a message with its block and the query, or a
    /// project. The page closes itself afterwards.
    fn open_hit(&self, hit: &AppSearchHit, query: &str, window: &mut Window, cx: &mut App);
}

/// The state behind [`LocalSearch`].
pub struct LocalSearchState {
    corpus: Vec<AppSearchHit>,
    query: String,
    scope: SearchScope,
    active: usize,
    open: bool,
    /// Shown while set, for screenshots.
    pub loading: bool,
    pub truncated: bool,
    pub error: Option<String>,
    /// The hits `open_hit` received.
    pub opened: Vec<AppSearchHit>,
}

/// In-memory search over a fixed list of hits, for the gallery and tests.
/// A hit matches when its title or meta contains the query.
#[derive(Clone)]
pub struct LocalSearch {
    state: Entity<LocalSearchState>,
}

impl LocalSearch {
    pub fn new(corpus: Vec<AppSearchHit>, cx: &mut App) -> Self {
        Self {
            state: cx.new(|_| LocalSearchState {
                corpus,
                query: String::new(),
                scope: SearchScope::All,
                active: 0,
                open: false,
                loading: false,
                truncated: false,
                error: None,
                opened: Vec::new(),
            }),
        }
    }

    pub fn state_entity(&self) -> &Entity<LocalSearchState> {
        &self.state
    }

    pub fn is_open(&self, cx: &App) -> bool {
        self.state.read(cx).open
    }

    pub fn opened(&self, cx: &App) -> Vec<AppSearchHit> {
        self.state.read(cx).opened.clone()
    }

    fn update(&self, cx: &mut App, f: impl FnOnce(&mut LocalSearchState)) {
        self.state.update(cx, |state, cx| {
            f(state);
            cx.notify();
        });
    }
}

fn in_scope(hit: &AppSearchHit, scope: SearchScope) -> bool {
    match scope {
        SearchScope::All => true,
        SearchScope::Conversations => {
            matches!(
                hit,
                AppSearchHit::Conversation(_) | AppSearchHit::Message(_)
            )
        }
        SearchScope::Files => matches!(hit, AppSearchHit::File(_) | AppSearchHit::Content(_)),
        SearchScope::Projects => matches!(hit, AppSearchHit::Project(_)),
    }
}

impl LocalSearchState {
    fn hits(&self) -> Vec<AppSearchHit> {
        let needle = js::trim(&self.query).to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.corpus
            .iter()
            .filter(|hit| in_scope(hit, self.scope))
            .filter(|hit| {
                hit.title().to_lowercase().contains(&needle)
                    || hit.meta().to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }
}

impl SearchData for LocalSearch {
    fn state(&self, cx: &App) -> SearchState {
        let state = self.state.read(cx);
        let hits = state.hits();
        SearchState {
            query: state.query.clone(),
            scope: state.scope,
            active: if state.active < hits.len() {
                state.active
            } else {
                0
            },
            hits,
            loading: state.loading,
            error: state.error.clone(),
            truncated: state.truncated,
        }
    }

    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.state, move |_, cx| listener(cx))
    }

    fn open(&self, _cwd: &str, _recents: Vec<String>, cx: &mut App) {
        self.update(cx, |state| {
            state.open = true;
            state.query.clear();
            state.scope = SearchScope::All;
            state.active = 0;
        });
    }

    fn close(&self, cx: &mut App) {
        self.update(cx, |state| state.open = false);
    }

    fn set_query(&self, query: &str, cx: &mut App) {
        self.update(cx, |state| {
            if js::trim(&state.query) != js::trim(query) {
                state.active = 0;
            }
            state.query = query.to_string();
        });
    }

    fn set_scope(&self, scope: SearchScope, cx: &mut App) {
        self.update(cx, |state| {
            if state.scope != scope {
                state.scope = scope;
                state.active = 0;
            }
        });
    }

    fn move_active(&self, delta: i64, cx: &mut App) {
        self.update(cx, |state| {
            let count = state.hits().len() as i64;
            if count > 0 {
                state.active = (state.active as i64 + delta).rem_euclid(count) as usize;
            }
        });
    }

    fn set_active(&self, index: usize, cx: &mut App) {
        self.update(cx, |state| {
            let count = state.hits().len();
            state.active = if index >= count { 0 } else { index };
        });
    }

    fn open_hit(&self, hit: &AppSearchHit, _query: &str, _window: &mut Window, cx: &mut App) {
        let hit = hit.clone();
        self.update(cx, |state| state.opened.push(hit));
    }
}
