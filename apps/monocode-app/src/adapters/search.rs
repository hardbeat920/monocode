//! Search results and navigation over the real history and file index.
use gpui::{App, AppContext as _, Entity, Global, Subscription, Task, Window};
use monocode_app::bridge::shell::{ShellPage, ShellRequest, ShellRequests};
use monocode_engine::history::app_search as engine;
use monocode_engine::history::search::SearchFiles;
use monocode_engine::history::{HistoryPackage, Search};
use monocode_engine::workspace::paths::FileNavigation;
use monocode_engine::workspace::workspace::FileOpenOptions;
use monocode_engine::workspace::{Files, Workspace};
use monocode_view_pages::{
    Listener,
    search::{SearchData, model as view},
};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct SearchTarget {
    pub block_id: String,
    pub query: String,
}
#[derive(Default)]
pub struct SearchReveals {
    targets: HashMap<String, SearchTarget>,
}
struct SearchRevealsGlobal(Entity<SearchReveals>);
impl Global for SearchRevealsGlobal {}
impl SearchReveals {
    pub fn init(cx: &mut App) {
        if !cx.has_global::<SearchRevealsGlobal>() {
            let entity = cx.new(|_| Self::default());
            cx.set_global(SearchRevealsGlobal(entity));
        }
    }
    pub fn entity(cx: &App) -> Entity<Self> {
        cx.global::<SearchRevealsGlobal>().0.clone()
    }
    pub fn current(session_id: &str, cx: &App) -> Option<SearchTarget> {
        cx.try_global::<SearchRevealsGlobal>()?
            .0
            .read(cx)
            .targets
            .get(session_id)
            .cloned()
    }
    fn reveal(session_id: &str, block_id: &str, query: &str, cx: &mut App) {
        Self::init(cx);
        Self::entity(cx).update(cx, |this, cx| {
            this.targets.insert(
                session_id.to_owned(),
                SearchTarget {
                    block_id: block_id.to_owned(),
                    query: query.to_owned(),
                },
            );
            cx.notify();
        });
    }
}

pub struct AppSearchData {
    pub search: Entity<Search>,
    pub workspace: Entity<Workspace>,
}
fn scope(value: view::SearchScope) -> engine::SearchScope {
    match value {
        view::SearchScope::All => engine::SearchScope::All,
        view::SearchScope::Files => engine::SearchScope::Files,
        view::SearchScope::Projects => engine::SearchScope::Projects,
        view::SearchScope::Conversations => engine::SearchScope::Conversations,
    }
}
fn hit(value: engine::AppSearchHit) -> view::AppSearchHit {
    match value {
        engine::AppSearchHit::Conversation(value) => {
            view::AppSearchHit::Conversation(view::ConversationHit {
                id: value.id,
                session_id: value.session_id,
                cwd: value.cwd,
                harness: value.harness,
                title: value.title,
                updated_at: value.updated_at,
                score: value.score,
                positions: value.positions,
            })
        }
        engine::AppSearchHit::Message(value) => view::AppSearchHit::Message(view::MessageHit {
            id: value.id,
            session_id: value.session_id,
            cwd: value.cwd,
            harness: value.harness,
            title: value.title,
            updated_at: value.updated_at,
            block_id: value.block_id,
            role: value.role,
            preview: value.preview,
            score: value.score,
        }),
        engine::AppSearchHit::File(value) => view::AppSearchHit::File(view::FileHit {
            id: value.id,
            path: value.path,
            relative: value.relative,
            name: value.name,
            score: value.score,
            positions: value.positions,
        }),
        engine::AppSearchHit::Content(value) => view::AppSearchHit::Content(view::ContentHit {
            id: value.id,
            path: value.path,
            relative: value.relative,
            name: value.name,
            line: value.line,
            column: value.column,
            preview: value.preview,
        }),
        engine::AppSearchHit::Project(value) => view::AppSearchHit::Project(view::ProjectHit {
            id: value.id,
            path: value.path,
            name: value.name,
            score: value.score,
            positions: value.positions,
        }),
    }
}
impl SearchData for AppSearchData {
    fn state(&self, cx: &App) -> view::SearchState {
        let search = self.search.read(cx);
        let current = match search.scope() {
            engine::SearchScope::All => view::SearchScope::All,
            engine::SearchScope::Files => view::SearchScope::Files,
            engine::SearchScope::Projects => view::SearchScope::Projects,
            engine::SearchScope::Conversations => view::SearchScope::Conversations,
        };
        view::SearchState {
            query: search.query().to_owned(),
            scope: current,
            active: search.active_index(),
            hits: search.hits(cx).into_iter().map(hit).collect(),
            loading: search.is_loading(),
            error: search.error().map(str::to_owned),
            truncated: search.truncated(),
        }
    }
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.search, move |_, cx| listener(cx))
    }
    fn open(&self, cwd: &str, recents: Vec<String>, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.open(cwd, recents, cx));
    }
    fn close(&self, cx: &mut App) {
        self.search.update(cx, |search, cx| search.close(cx));
    }
    fn set_query(&self, query: &str, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.set_query(query, cx));
    }
    fn set_scope(&self, value: view::SearchScope, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.set_scope(scope(value), cx));
    }
    fn move_active(&self, delta: i64, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.move_active(delta, cx));
    }
    fn set_active(&self, index: usize, cx: &mut App) {
        self.search
            .update(cx, |search, cx| search.set_active(index, cx));
    }
    fn open_hit(&self, hit: &view::AppSearchHit, query: &str, _window: &mut Window, cx: &mut App) {
        match hit {
            view::AppSearchHit::File(hit) => self
                .workspace
                .update(cx, |workspace, cx| {
                    workspace.open_file(
                        &hit.path,
                        None,
                        FileOpenOptions {
                            exact: true,
                            pin: false,
                        },
                        cx,
                    )
                })
                .detach(),
            view::AppSearchHit::Content(hit) => self
                .workspace
                .update(cx, |workspace, cx| {
                    workspace.open_file(
                        &hit.path,
                        Some(FileNavigation {
                            line: hit.line,
                            column: Some(hit.column),
                        }),
                        FileOpenOptions {
                            exact: true,
                            pin: false,
                        },
                        cx,
                    )
                })
                .detach(),
            view::AppSearchHit::Conversation(hit) => self
                .workspace
                .update(cx, |workspace, cx| {
                    workspace.open_session(&hit.session_id, cx)
                })
                .detach(),
            view::AppSearchHit::Message(hit) => {
                SearchReveals::reveal(&hit.session_id, &hit.block_id, query, cx);
                self.workspace
                    .update(cx, |workspace, cx| {
                        workspace.open_session(&hit.session_id, cx)
                    })
                    .detach();
            }
            view::AppSearchHit::Project(hit) => {
                monocode_engine::projects::actions::on_select_project(&hit.path, cx)
            }
        }
        self.close(cx);
        ShellRequests::send(ShellRequest::ClosePage(ShellPage::Search), cx);
    }
}

struct AppSearchFiles;
impl SearchFiles for AppSearchFiles {
    fn rank(&self, cwd: &str, query: &str, limit: usize, cx: &App) -> Vec<engine::FileRank> {
        let Some(files) = Files::try_global(cx) else {
            return Vec::new();
        };
        let index = files.index.read(cx);
        let Some(list) = index.peek_project_files(cwd) else {
            return Vec::new();
        };
        monocode_engine::workspace::files::file_index::rank_project_files_limit(
            &list,
            query,
            &index.recent_opened_files(cwd),
            limit,
        )
        .into_iter()
        .map(|rank| engine::FileRank {
            path: rank.file.path,
            relative: rank.file.relative,
            name: rank.file.name,
            score: rank.score,
            positions: rank.positions,
        })
        .collect()
    }
    fn load(&self, cwd: &str, cx: &mut App) -> Task<()> {
        let Some(files) = Files::try_global(cx) else {
            return Task::ready(());
        };
        let task = files
            .index
            .clone()
            .update(cx, |index, cx| index.load_project_files(cwd, false, cx));
        cx.spawn(async move |_| {
            let _ = task.await;
        })
    }
}
pub fn install_files(cx: &mut App) {
    if let Some(history) = HistoryPackage::try_global(cx) {
        let search = history.search.clone();
        search.update(cx, |search, _| search.set_files(Rc::new(AppSearchFiles)));
    }
}
