//! Inbox: the original provider rail and work-item detail surface, backed by
//! the native adapters and the desktop app's existing credential files.
mod avatar;
mod diff;
mod document;
mod ui;
pub(crate) use avatar::image as avatar_image;
pub(crate) use ui::{provider_mark, relative_time};

use crate::{
    diff_view::{DiffEvent, DiffView},
    input::{self, Input},
    popover::AnchorSlot,
    runtime::{Database, locked},
    theme::*,
};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, Pixels, Subscription, px,
};
use monocode_core::{
    inbox::{self as model, Checks, Details, Diff, Item, Service, Snapshot, Thread, ViewState},
    store::Session,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// The latest Inbox rows, for the composer's `#` picker.
pub(crate) struct Catalog {
    pub items: std::sync::Arc<Vec<Item>>,
    pub at: Instant,
}
impl gpui::Global for Catalog {}

pub(crate) enum Event {
    Close,
    OpenSession(i64),
    StartSession { item: Box<Item>, prompt: String },
    ItemsChanged(Vec<Item>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Summary,
    Code,
    Checks,
}

pub(crate) struct Inbox {
    store: Database,
    service: Service,
    projects: Vec<PathBuf>,
    appearances:
        std::collections::HashMap<String, monocode_core::project_appearance::ProjectAppearance>,
    state: ViewState,
    snapshot: Snapshot,
    sessions: Vec<Session>,
    focus: FocusHandle,
    search: Entity<Input>,
    comment: Entity<Input>,
    summary_scroll: ListState,
    summary_width: std::rc::Rc<std::cell::Cell<Pixels>>,
    activity: Vec<ui::Activity>,
    activity_revision: u64,
    summary_rows: std::cell::RefCell<ui::SummaryRows>,
    focus_comment: bool,
    selected: Option<String>,
    /// The address last copied and the press that copied it, until the tick fades.
    copied_link: Option<(String, u64)>,
    details: Option<Details>,
    thread: Option<Thread>,
    document: Option<Entity<document::InboxDocument>>,
    comments: Vec<(model::Comment, Entity<document::InboxDocument>)>,
    diff: Option<Diff>,
    diff_view: Entity<DiffView>,
    checks: Option<Checks>,
    tab: Tab,
    loading: bool,
    detail_loading: bool,
    extra_loading: bool,
    posting: bool,
    acting: bool,
    error: Option<String>,
    detail_error: Option<String>,
    extra_error: Option<String>,
    post_error: Option<String>,
    thread_error: Option<String>,
    expanded_comments: std::collections::HashSet<String>,
    merge_action: String,
    reply: Option<(String, String)>,
    generation: u64,
    detail_generation: u64,
    extra_request: u64,
    filters_open: bool,
    connections_open: bool,
    filter_anchor: AnchorSlot,
    connection_anchor: AnchorSlot,
    connect_provider: Option<String>,
    connect_url: Entity<Input>,
    connect_email: Entity<Input>,
    connect_token: Entity<Input>,
    connecting: bool,
    connect_error: Option<String>,
    start_project: PathBuf,
    project_picker: bool,
    project_anchor: AnchorSlot,
    action_menu: bool,
    action_anchor: AnchorSlot,
    pr_action_anchors: [AnchorSlot; 4],
    action_error: Option<String>,
    action_notice: Option<String>,
    confirm_action: Option<String>,
    resize_start: Option<(f32, f32)>,
    active: bool,
    refresh_queued: bool,
    notifications: model::NotificationTracker,
    last_refresh: Instant,
    _subscriptions: Vec<Subscription>,
}

/// The inbox opens scoped to the project it was opened from.
fn other_projects(projects: &[PathBuf], cwd: &PathBuf) -> Vec<String> {
    projects
        .iter()
        .filter(|project| *project != cwd)
        .map(|project| project.to_string_lossy().into_owned())
        .collect()
}

impl EventEmitter<Event> for Inbox {}
impl Focusable for Inbox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Inbox {
    fn copy_link(&mut self, url: String, cx: &mut Context<Self>) {
        if url.is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
        let press = self.copied_link.as_ref().map_or(0, |(_, press)| press + 1);
        self.copied_link = Some((url, press));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_link.as_ref().is_some_and(|(_, p)| *p == press) {
                    this.copied_link = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(crate) fn new(
        store: Database,
        projects: Vec<PathBuf>,
        cwd: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_inner(store, projects, cwd, true, cx)
    }

    fn new_inner(
        store: Database,
        mut projects: Vec<PathBuf>,
        cwd: PathBuf,
        poll: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        if !projects.contains(&cwd) {
            projects.push(cwd.clone());
        }
        let (mut state, data_dir) = locked(&store)
            .map(|db| {
                (
                    ViewState::load(&db),
                    db.data_dir()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| cwd.clone()),
                )
            })
            .unwrap_or_else(|_| (ViewState::default(), cwd.clone()));
        state.filters.hidden_projects = other_projects(&projects, &cwd);
        let search = cx.new(|cx| Input::field("Filter inbox", "Filter inbox", content(0.4), cx));
        let comment = cx.new(Input::inbox_comment);
        let connect_url = cx.new(|cx| Input::field("Instance URL", "https://", content(0.4), cx));
        let connect_email = cx.new(|cx| Input::field("Email", "you@example.com", content(0.4), cx));
        let connect_token =
            cx.new(|cx| Input::field("API token", "Personal access token", content(0.4), cx));
        let diff_view = cx.new(DiffView::new);
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, _: &input::Changed, cx| cx.notify()),
            cx.subscribe(&comment, |this, _, _: &input::Changed, cx| {
                let count = this.summary_scroll.item_count();
                this.summary_scroll
                    .remeasure_items(count.saturating_sub(1)..count);
                cx.notify();
            }),
            cx.subscribe(&comment, |this, _, _: &input::Submitted, cx| {
                this.post_comment(cx)
            }),
            cx.subscribe(&diff_view, |this, _, event: &DiffEvent, cx| {
                if let DiffEvent::Comment(text) = event {
                    this.comment
                        .update(cx, |input, cx| input.set_text(text.clone(), cx));
                    this.tab = Tab::Summary;
                    this.summary_scroll.scroll_to_end();
                    this.focus_comment = true;
                    cx.notify();
                }
            }),
        ];
        if poll {
            cx.spawn(async move |this, cx| {
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(30))
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            let interval = Duration::from_secs(if this.active { 120 } else { 300 });
                            if this.last_refresh.elapsed() >= interval {
                                this.refresh(cx);
                            }
                            if this.active
                                && this.selected.is_some()
                                && !this.detail_loading
                                && !this.posting
                                && !this.acting
                            {
                                this.load_detail(false, cx);
                                if this.tab == Tab::Checks && !this.extra_loading {
                                    this.checks = None;
                                    this.set_tab(Tab::Checks, cx);
                                }
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        Self {
            service: Service {
                data_dir,
                cwd: cwd.clone(),
            },
            store,
            projects,
            appearances: Default::default(),
            state,
            snapshot: Snapshot::default(),
            sessions: vec![],
            focus: cx.focus_handle(),
            search,
            comment,
            summary_scroll: ListState::new(0, ListAlignment::Top, px(200.)),
            summary_width: Default::default(),
            activity: vec![],
            activity_revision: 0,
            summary_rows: Default::default(),
            focus_comment: false,
            selected: None,
            copied_link: None,
            details: None,
            thread: None,
            document: None,
            comments: vec![],
            diff: None,
            diff_view,
            checks: None,
            tab: Tab::Summary,
            loading: false,
            detail_loading: false,
            extra_loading: false,
            posting: false,
            acting: false,
            error: None,
            detail_error: None,
            extra_error: None,
            post_error: None,
            thread_error: None,
            expanded_comments: Default::default(),
            merge_action: "merge".into(),
            reply: None,
            generation: 0,
            detail_generation: 0,
            extra_request: 0,
            filters_open: false,
            connections_open: false,
            filter_anchor: AnchorSlot::default(),
            connection_anchor: AnchorSlot::default(),
            connect_provider: None,
            connect_url,
            connect_email,
            connect_token,
            connecting: false,
            connect_error: None,
            start_project: cwd,
            project_picker: false,
            project_anchor: AnchorSlot::default(),
            action_menu: false,
            action_anchor: AnchorSlot::default(),
            pr_action_anchors: std::array::from_fn(|_| AnchorSlot::default()),
            action_error: None,
            action_notice: None,
            confirm_action: None,
            resize_start: None,
            active: false,
            refresh_queued: false,
            notifications: model::NotificationTracker::default(),
            last_refresh: Instant::now(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn set_context(
        &mut self,
        mut projects: Vec<PathBuf>,
        cwd: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if !projects.contains(&cwd) {
            projects.push(cwd.clone());
        }
        if self.projects != projects || self.service.cwd != cwd {
            let switched = self.service.cwd != cwd;
            if switched {
                self.state.filters.hidden_projects = other_projects(&projects, &cwd);
                self.save();
            }
            self.projects = projects;
            self.service.cwd = cwd.clone();
            self.start_project = cwd;
            if switched {
                let visible = self.visible_items(cx);
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|key| !visible.iter().any(|item| &item.key() == key))
                {
                    self.deselect(cx);
                    if self.active
                        && let Some(item) = visible.first()
                    {
                        self.select(item.key(), cx);
                    }
                }
            }
            self.refresh(cx);
        }
    }

    pub(crate) fn set_appearances(
        &mut self,
        appearances: std::collections::HashMap<
            String,
            monocode_core::project_appearance::ProjectAppearance,
        >,
        cx: &mut Context<Self>,
    ) {
        if self.appearances != appearances {
            self.appearances = appearances;
            cx.notify();
        }
    }

    pub(crate) fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            if let Some(item) = self.item() {
                self.state.seen.mark(&item);
                self.save();
            } else if let Some(item) = self.visible_items(cx).first() {
                self.select(item.key(), cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn unread_count(&self) -> usize {
        self.snapshot
            .items
            .iter()
            .filter(|item| self.state.seen.unseen(item))
            .count()
    }

    pub(crate) fn connection_statuses(&self) -> std::collections::HashMap<String, (bool, String)> {
        let mut statuses = self
            .snapshot
            .connections
            .iter()
            .map(|(provider, status)| {
                let description = if !status.connected {
                    "Not connected".into()
                } else if !status.site.is_empty() {
                    status.site.clone()
                } else if !status.url.is_empty() {
                    status.url.clone()
                } else if !status.organization.is_empty() {
                    status.organization.clone()
                } else {
                    "Connected".into()
                };
                (provider.clone(), (status.connected, description))
            })
            .collect::<std::collections::HashMap<_, _>>();
        for (provider, error) in &self.snapshot.errors {
            if !statuses.contains_key(provider) {
                statuses.insert(provider.clone(), (false, error.clone()));
            }
        }
        statuses
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            self.refresh_queued = true;
            return;
        }
        self.loading = true;
        self.last_refresh = Instant::now();
        self.generation += 1;
        let generation = self.generation;
        let service = self.service.clone();
        let state = self.state.clone();
        let projects = self.projects.clone();
        let scope = self.fetch_scope();
        let store = self.store.clone();
        let task = cx.background_executor().spawn(async move {
            let snapshot = service.list(&projects, &state);
            let sessions = locked(&store)
                .and_then(|db| db.sessions())
                .unwrap_or_default();
            (snapshot, sessions)
        });
        cx.spawn(async move |this, cx| {
            let (mut snapshot, sessions) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                if scope != this.fetch_scope() {
                    this.refresh_queued = false;
                    this.refresh(cx);
                    return;
                }
                let appeared = this.notifications.observe(&snapshot, scope);
                // A temporary outage must not erase the last usable provider list.
                for provider in snapshot.errors.keys() {
                    snapshot.items.extend(
                        this.snapshot
                            .items
                            .iter()
                            .filter(|i| &i.provider == provider)
                            .cloned(),
                    );
                }
                snapshot
                    .items
                    .sort_by_key(|item| std::cmp::Reverse(item.updated_ms()));
                this.state.seen.seed(&snapshot.items);
                this.snapshot = snapshot;
                this.sessions = sessions;
                cx.set_global(Catalog {
                    items: this.snapshot.items.clone().into(),
                    at: Instant::now(),
                });
                if this.active
                    && let Some(item) = this.item()
                {
                    this.state.seen.mark(&item);
                }
                if !this
                    .snapshot
                    .connections
                    .get(&this.state.source)
                    .is_some_and(|s| s.connected)
                    && let Some(source) = model::PROVIDERS.into_iter().find(|p| {
                        this.snapshot
                            .connections
                            .get(*p)
                            .is_some_and(|s| s.connected)
                    })
                {
                    this.state.source = source.into();
                }
                this.save();
                cx.emit(Event::ItemsChanged(appeared));
                if this.selected.is_some()
                    && this
                        .item()
                        .is_none_or(|item| item.provider != this.state.source)
                {
                    this.deselect(cx);
                }
                if this.active
                    && this.selected.is_none()
                    && let Some(item) = this.visible_items(cx).first()
                {
                    this.select(item.key(), cx);
                }
                if std::mem::take(&mut this.refresh_queued) {
                    this.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Rows are assigned to the active checkout, so a fetch made for another
    /// one is stale even when the project list is the same.
    fn fetch_scope(&self) -> String {
        format!(
            "{}:{}",
            self.state.fetch_scope(&self.projects),
            monocode_core::files::path_key(&self.service.cwd.to_string_lossy())
        )
    }
    fn save(&mut self) {
        if let Err(error) = locked(&self.store).and_then(|db| self.state.save(&db)) {
            self.error = Some(error);
        }
    }
    fn visible_items(&self, cx: &App) -> Vec<Item> {
        let search = self.search.read(cx).content.as_ref();
        self.snapshot
            .items
            .iter()
            .filter(|item| self.state.filters.matches(item, &self.state.source, search))
            .filter(|item| {
                item.provider != "linear" || !self.state.hidden_linear_teams.contains(&item.team_id)
            })
            .cloned()
            .collect()
    }
    fn item(&self) -> Option<Item> {
        self.snapshot
            .items
            .iter()
            .find(|item| Some(item.key()) == self.selected)
            .cloned()
    }
    fn select(&mut self, key: String, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&key) {
            return;
        }
        self.selected = Some(key);
        self.reset_detail(cx);
        if let Some(item) = self.item() {
            if self.active {
                self.state.seen.mark(&item);
                self.save();
            }
            if !item.project_path.is_empty() {
                self.start_project = PathBuf::from(&item.project_path);
            }
        }
        self.load_detail(true, cx);
        cx.notify();
    }
    fn deselect(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.reset_detail(cx);
        cx.notify();
    }
    /// Drops the detail pane's state and invalidates its in-flight requests.
    fn reset_detail(&mut self, cx: &mut Context<Self>) {
        self.detail_generation += 1;
        self.detail_loading = false;
        self.detail_error = None;
        self.summary_scroll = ListState::new(0, ListAlignment::Top, px(200.));
        self.activity.clear();
        self.activity_revision += 1;
        self.summary_rows = Default::default();
        self.focus_comment = false;
        self.tab = Tab::Summary;
        self.diff = None;
        self.extra_request += 1;
        self.diff_view
            .update(cx, |view, cx| view.set_files(vec![], cx));
        self.checks = None;
        self.extra_loading = false;
        self.extra_error = None;
        self.details = None;
        self.thread = None;
        self.document = None;
        self.comments.clear();
        self.reply = None;
        self.post_error = None;
        self.thread_error = None;
        self.expanded_comments.clear();
        self.merge_action = "merge".into();
        self.confirm_action = None;
        self.action_error = None;
        self.action_notice = None;
        self.action_menu = false;
        self.comment
            .update(cx, |input, cx| input.set_text(String::new(), cx));
    }
    fn change_source(&mut self, source: String, cx: &mut Context<Self>) {
        self.state.source = source;
        self.selected = None;
        self.details = None;
        self.document = None;
        self.comments.clear();
        self.search
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        if let Some(item) = self.visible_items(cx).first() {
            self.select(item.key(), cx);
        }
        self.save();
        cx.notify();
    }
    fn load_detail(&mut self, clear: bool, cx: &mut Context<Self>) {
        let Some(item) = self.item() else {
            return;
        };
        self.detail_generation += 1;
        let generation = self.detail_generation;
        self.detail_loading = true;
        self.detail_error = None;
        self.thread_error = None;
        if clear {
            self.document = None;
            self.comments.clear();
        }
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { (service.details(&item), service.thread(&item)) });
        cx.spawn(async move |this, cx| {
            let (details, thread) = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.detail_generation != generation {
                    return;
                }
                this.detail_loading = false;
                match details {
                    Ok(details) => {
                        if this
                            .details
                            .as_ref()
                            .is_none_or(|old| old.body != details.body)
                        {
                            this.document = Some(cx.new(|cx| {
                                document::InboxDocument::new(
                                    details.body.clone(),
                                    this.document_cwd(),
                                    cx,
                                )
                            }));
                        }
                        this.details = Some(details);
                    }
                    Err(error) => this.detail_error = Some(error),
                }
                match thread {
                    Ok(thread) => {
                        let mut rows = vec![];
                        for comment in &thread.comments {
                            let document = this
                                .comments
                                .iter()
                                .find(|(old, _)| old.id == comment.id && old.body == comment.body)
                                .map(|(_, document)| document.clone())
                                .unwrap_or_else(|| {
                                    cx.new(|cx| {
                                        document::InboxDocument::new(
                                            comment.body.clone(),
                                            this.document_cwd(),
                                            cx,
                                        )
                                    })
                                });
                            rows.push((comment.clone(), document));
                            for reply in &comment.replies {
                                let document = this
                                    .comments
                                    .iter()
                                    .find(|(old, _)| old.id == reply.id && old.body == reply.body)
                                    .map(|(_, document)| document.clone())
                                    .unwrap_or_else(|| {
                                        cx.new(|cx| {
                                            document::InboxDocument::new(
                                                reply.body.clone(),
                                                this.document_cwd(),
                                                cx,
                                            )
                                        })
                                    });
                                rows.push((reply.clone(), document));
                            }
                        }
                        this.comments = rows;
                        this.thread = Some(thread);
                        this.rebuild_activity();
                    }
                    Err(error) => this.thread_error = Some(error),
                }
                this.summary_scroll.remeasure();
                cx.notify();
            });
        })
        .detach();
    }
    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.extra_error = None;
        let Some(item) = self.item() else {
            return;
        };
        if tab == Tab::Summary
            || (tab == Tab::Code && self.diff.is_some())
            || (tab == Tab::Checks && self.checks.is_some())
        {
            self.extra_loading = false;
            cx.notify();
            return;
        }
        self.extra_loading = true;
        self.extra_request += 1;
        let request = self.extra_request;
        let key = item.key();
        let service = self.service.clone();
        let task = cx.background_executor().spawn(async move {
            if tab == Tab::Code {
                service.diff(&item).map(|diff| {
                    let files = diff::models(&diff);
                    (Some((diff, files)), None)
                })
            } else {
                service.checks(&item).map(|checks| (None, Some(checks)))
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected.as_ref() != Some(&key)
                    || this.tab != tab
                    || request != this.extra_request
                {
                    return;
                }
                this.extra_loading = false;
                match result {
                    Ok((diff, checks)) => {
                        if let Some((diff, files)) = diff {
                            this.diff_view.update(cx, |view, cx| {
                                view.file_count = Some(diff.files.len());
                                view.totals = Some((
                                    diff.additions.max(0).min(u32::MAX as i64) as u32,
                                    diff.deletions.max(0).min(u32::MAX as i64) as u32,
                                ));
                                view.set_files(files, cx);
                            });
                            this.diff = Some(diff);
                        }
                        if let Some(checks) = checks {
                            this.checks = Some(checks);
                        }
                    }
                    Err(error) => this.extra_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn post_comment(&mut self, cx: &mut Context<Self>) {
        if self.posting {
            return;
        }
        let Some(item) = self.item() else {
            return;
        };
        let body = self.comment.read(cx).content.trim().to_string();
        if body.is_empty() {
            return;
        }
        self.posting = true;
        self.post_error = None;
        let key = item.key();
        let service = self.service.clone();
        let reply = self.reply.as_ref().map(|r| r.0.clone()).unwrap_or_default();
        let task = cx
            .background_executor()
            .spawn(async move { service.comment(&item, body, reply) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.posting = false;
                if this.selected.as_ref() != Some(&key) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(_) => {
                        this.comment
                            .update(cx, |input, cx| input.set_text(String::new(), cx));
                        this.reply = None;
                        this.load_detail(false, cx);
                        this.refresh(cx);
                    }
                    Err(error) => this.post_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn document_cwd(&self) -> PathBuf {
        self.item()
            .filter(|item| !item.project_path.is_empty())
            .map(|item| PathBuf::from(item.project_path))
            .unwrap_or_else(|| self.start_project.clone())
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if let Some(mut item) = self.item() {
            if item.project_path.is_empty() {
                item.project_path = self.start_project.to_string_lossy().into_owned();
            }
            let body = self.details.as_ref().map(|d| d.body.as_str()).unwrap_or("");
            let discussion = self
                .comments
                .iter()
                .map(|(c, _)| format!("{}: {}", c.author, c.body))
                .collect::<Vec<_>>()
                .join("\n\n");
            let prompt = item.prompt(&format!("{body}\n\n{discussion}"), false);
            cx.emit(Event::StartSession {
                item: Box::new(item),
                prompt,
            });
        }
    }
    pub(crate) fn open_connection(&mut self, provider: String, cx: &mut Context<Self>) {
        self.connections_open = false;
        self.connect_error = None;
        let status = self
            .snapshot
            .connections
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        let url = if !status.site.is_empty() {
            status.site
        } else if !status.url.is_empty() {
            status.url
        } else if !status.organization.is_empty() {
            status.organization
        } else if provider == "gitlab" {
            "https://gitlab.com".into()
        } else {
            String::new()
        };
        self.connect_url
            .update(cx, |input, cx| input.set_text(url, cx));
        self.connect_email
            .update(cx, |input, cx| input.set_text(status.email, cx));
        self.connect_token
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.connect_provider = Some(provider);
        cx.notify();
    }
    fn connect(&mut self, disconnect: bool, cx: &mut Context<Self>) {
        if self.connecting {
            return;
        }
        let Some(provider) = self.connect_provider.clone() else {
            return;
        };
        let url = self.connect_url.read(cx).content.to_string();
        let email = self.connect_email.read(cx).content.to_string();
        let token = if disconnect {
            String::new()
        } else {
            self.connect_token.read(cx).content.to_string()
        };
        if provider != "github" && !disconnect && token.trim().is_empty() {
            self.connect_error = Some("Enter an API token".into());
            cx.notify();
            return;
        }
        self.connecting = true;
        self.connect_error = None;
        let service = self.service.clone();
        let source = provider.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.connect(&source, url, email, token) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.connecting = false;
                match result {
                    Ok(status) => {
                        let connected = status.connected;
                        this.snapshot.connections.insert(provider.clone(), status);
                        this.connect_token.update(cx, |input, cx| input.set_text(String::new(), cx));
                        if connected || disconnect {
                            this.connect_provider = None;
                            if connected { this.change_source(provider, cx); }
                            this.refresh(cx);
                        } else { this.connect_error = Some("GitHub is not connected. Run gh auth login in a terminal, then check again.".into()); }
                    }
                    Err(error) => this.connect_error = Some(error),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    fn run_action(&mut self, cx: &mut Context<Self>) {
        if self.acting {
            return;
        }
        let (Some(item), Some(action)) = (self.item(), self.confirm_action.clone()) else {
            return;
        };
        self.acting = true;
        self.action_error = None;
        let service = self.service.clone();
        let key = item.key();
        let merging = matches!(action.as_str(), "merge" | "squash" | "rebase");
        let task = cx
            .background_executor()
            .spawn(async move { service.action(&item, &action) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.acting = false;
                match result {
                    Ok(item) => {
                        this.action_notice = (merging
                            && !item.state.eq_ignore_ascii_case("merged"))
                        .then(|| "Merge queued or auto-merge enabled.".into());
                        if let Some(old) = this.snapshot.items.iter_mut().find(|i| i.key() == key) {
                            *old = item;
                        }
                        this.confirm_action = None;
                        this.load_detail(false, cx);
                        this.refresh(cx);
                    }
                    Err(error) => this.action_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
