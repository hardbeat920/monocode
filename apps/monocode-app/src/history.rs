//! The session sidebar's rows for one project: `refreshHistory` and the
//! `history` state from App.tsx, kept small for M1. Stored rows come from
//! `monocode.db` through the session writer; open sessions that were never
//! saved join them through `history_with_live_sessions`. The engine's
//! `history` package will replace this.

use gpui::{App, AppContext as _, Context, Entity, Subscription};
use monocode_engine::runtime::session_history::{
    filter_sessions_by_archive, history_with_live_sessions, merge_project_history_summary,
    replace_project_history,
};
use monocode_engine::runtime::session_store::SessionSummary;
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_engine::runtime::{Engine, Sessions, SessionsEvent};

/// Stored rows for every project visited, and the one the sidebar shows.
pub struct SidebarHistory {
    rows: Vec<SessionSummary>,
    cwd: String,
    /// The first listing of `cwd` has not come back yet.
    loading: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl SidebarHistory {
    pub fn new(rows: Vec<SessionSummary>, cwd: Option<String>, cx: &mut Context<Self>) -> Self {
        let sessions = Engine::sessions(cx);
        let subscriptions = vec![
            cx.subscribe(&sessions, Self::on_sessions_event),
            // Live sessions join the list as they change.
            cx.observe(&sessions, |_, _, cx| cx.notify()),
        ];
        Self {
            rows,
            cwd: cwd.unwrap_or_default(),
            loading: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn on_sessions_event(
        &mut self,
        _: Entity<Sessions>,
        event: &SessionsEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SessionsEvent::Persisted(summary) => {
                self.rows = merge_project_history_summary(&self.rows, (**summary).clone());
                cx.notify();
            }
            SessionsEvent::LoadFailed { .. } => {
                let cwd = self.cwd.clone();
                self.refresh(&cwd, cx);
            }
            _ => {}
        }
    }

    /// Show `cwd`, listing it from the store. Rows already cached for it
    /// paint at once and the listing replaces them.
    pub fn show(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if same_project_path(&self.cwd, cwd) {
            return;
        }
        self.cwd = cwd.to_string();
        self.refresh(cwd, cx);
    }

    /// `refreshHistory(cwd)`.
    pub fn refresh(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let cached = self.rows.iter().any(|row| same_project_path(&row.cwd, cwd));
        self.loading = !cached;
        self.error = None;
        let listing = Engine::writer(cx).list_sessions_by_project(cwd);
        let cwd = cwd.to_string();
        cx.spawn(async move |this, cx| {
            let result = listing.await;
            this.update(cx, |this, cx| {
                if !same_project_path(&this.cwd, &cwd) {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(rows) => this.rows = replace_project_history(&this.rows, &cwd, rows),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The sidebar's rows for the current project: stored rows plus open
    /// sessions that are not saved yet, sorted pinned first, then newest.
    /// Archived rows are left out, as the default filter does.
    pub fn visible_rows(&self, cx: &App) -> Vec<SessionSummary> {
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx).all();
        let rows = history_with_live_sessions(&self.rows, open, &self.cwd, None, &[]);
        filter_sessions_by_archive(&rows, false)
    }
}

/// Create the sidebar history for a window.
pub fn new_history(
    rows: Vec<SessionSummary>,
    cwd: Option<String>,
    cx: &mut App,
) -> Entity<SidebarHistory> {
    cx.new(|cx| SidebarHistory::new(rows, cwd, cx))
}
