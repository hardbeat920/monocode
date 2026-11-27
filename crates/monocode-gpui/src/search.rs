//! Global search, as the original full page (SearchView.tsx) or, when the
//! `search_panel` preference is on, a floating command panel.
use crate::{
    input::{self, Input},
    runtime::{Database, locked},
    theme::*,
};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, HighlightStyle,
    MouseButton, MouseMoveEvent, Pixels, Point, Render, Role, ScrollHandle, StyledText,
    Subscription, Task, Window, canvas, div, fill, point, prelude::*, px, relative, size,
};
use monocode_core::{
    search::{self, Hit, Scope, Target},
    store::Session,
    transcript::Row,
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(crate) enum Event {
    Open(Target, String),
    Close,
}
#[derive(Default)]
pub(crate) struct Sources {
    pub summaries: Vec<Session>,
    pub live: HashMap<i64, Arc<Vec<Row>>>,
    pub projects: Vec<String>,
    pub recent: Vec<String>,
    pub logos: HashMap<String, String>,
    pub classic: bool,
    pub panel: bool,
}
pub(crate) struct SearchView {
    pub(crate) input: Entity<Input>,
    cwd: String,
    store: Database,
    summaries: Vec<Session>,
    live: HashMap<i64, Arc<Vec<Row>>>,
    projects: Vec<String>,
    recent: Vec<String>,
    logos: HashMap<String, String>,
    classic: bool,
    original_header: Option<f32>,
    /// Floating command panel; otherwise the original full page.
    panel: bool,
    files: Vec<monocode_core::files::ProjectFile>,
    scope: Scope,
    query: String,
    hits: Vec<Hit>,
    remote: Vec<Hit>,
    local: Vec<Hit>,
    live_task: Option<Task<()>>,
    live_revision: u64,
    content: Vec<Hit>,
    active: usize,
    scroll: ScrollHandle,
    pointer: Option<Point<Pixels>>,
    sessions_loading: bool,
    files_loading: bool,
    truncated: (bool, bool),
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    listing_cancel: Arc<AtomicBool>,
    debounce: Option<Task<()>>,
    _changed: Subscription,
}
impl EventEmitter<Event> for SearchView {}
impl Focusable for SearchView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}
impl Drop for SearchView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.listing_cancel.store(true, Ordering::Release);
    }
}
impl SearchView {
    pub(crate) fn new(
        cwd: String,
        store: Database,
        sources: Sources,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            if sources.panel {
                Input::command_field("Search", "Search conversations, files, projects…", cx)
            } else {
                Input::field("Search", "Search everything...", content(0.4), cx)
            }
        });
        let changed = cx.subscribe(&input, |this, _, _: &input::Changed, cx| {
            this.query = this.input.read(cx).content.trim().to_owned();
            this.active = 0;
            this.refresh(cx);
        });
        let listing_cancel = Arc::new(AtomicBool::new(false));
        let flag = listing_cancel.clone();
        let root = cwd.clone();
        let work = cx.background_executor().spawn(async move {
            monocode_core::files::list_project_files_sync_cancellable(&root, Some(&flag))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(files) = result {
                    this.files = files;
                    this.rank(cx);
                }
            });
        })
        .detach();
        let mut view = Self {
            input,
            cwd,
            store,
            summaries: sources.summaries,
            live: sources.live,
            projects: sources.projects,
            recent: sources.recent,
            logos: sources.logos,
            classic: sources.classic,
            original_header: None,
            panel: sources.panel,
            files: vec![],
            scope: Scope::All,
            query: String::new(),
            hits: vec![],
            remote: vec![],
            local: vec![],
            live_task: None,
            live_revision: 0,
            content: vec![],
            active: 0,
            scroll: ScrollHandle::new(),
            pointer: None,
            sessions_loading: false,
            files_loading: false,
            truncated: (false, false),
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
            listing_cancel,
            debounce: None,
            _changed: changed,
        };
        // Recent conversations show at once; recent files join once listed.
        if view.panel {
            view.hits = view.suggestions();
        }
        view
    }
    pub(crate) fn set_classic(&mut self, classic: bool, cx: &mut Context<Self>) {
        self.classic = classic;
        cx.notify();
    }
    /// Original SearchView owns a native title strip and reserves the leading
    /// space occupied by the shared window navigation above its compact rail.
    pub(crate) fn set_original_header(&mut self, inset: Option<f32>, cx: &mut Context<Self>) {
        if self.original_header == inset {
            return;
        }
        self.original_header = inset;
        cx.notify();
    }
    pub(crate) fn draft(&self, cx: &App) -> (String, Scope) {
        (self.input.read(cx).content.to_string(), self.scope)
    }
    pub(crate) fn restore_draft(&mut self, draft: (String, Scope), cx: &mut Context<Self>) {
        self.scope = draft.1;
        self.input
            .update(cx, |input, cx| input.set_text(draft.0, cx));
    }
    pub(crate) fn select_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.active = 0;
        self.refresh(cx);
    }
    pub(crate) fn update_session(
        &mut self,
        summary: Session,
        rows: Arc<Vec<Row>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = self.summaries.iter_mut().find(|s| s.id == summary.id) {
            *old = summary.clone();
        } else {
            self.summaries.push(summary.clone());
        }
        self.live.insert(summary.id, rows);
        self.live_revision += 1;
        if !self.query.is_empty() {
            self.rank(cx);
            self.refresh_live(cx);
        }
    }
    fn refresh_live(&mut self, cx: &mut Context<Self>) {
        if self.query.is_empty() || !self.scope.sessions() || self.live_task.is_some() {
            return;
        }
        let check = self.cancel.clone();
        self.live_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            let Ok((work, revision)) = this.update(cx, |this, cx| {
                let live = this.live.clone();
                let summaries = this.summaries.clone();
                let query = this.query.clone();
                let flag = this.cancel.clone();
                (
                    cx.background_executor().spawn(async move {
                        summaries
                            .into_iter()
                            .flat_map(|s| {
                                live.get(&s.id)
                                    .map(|rows| search::messages(&s, rows, &query, &flag))
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                    }),
                    this.live_revision,
                )
            }) else {
                return;
            };
            let hits = work.await;
            if check.load(Ordering::Acquire) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.local = hits;
                this.live_task = None;
                this.rank(cx);
                if this.live_revision != revision {
                    this.refresh_live(cx);
                }
            });
        }));
    }
    fn rank(&mut self, cx: &mut Context<Self>) {
        self.hits = if self.query.is_empty() {
            if self.panel {
                self.suggestions()
            } else {
                vec![]
            }
        } else {
            search::grouped(
                search::titles(&self.summaries, &self.query)
                    .into_iter()
                    .chain(self.local.clone())
                    .chain(self.remote.clone())
                    .chain(search::filenames(&self.files, &self.query, &self.recent))
                    .chain(self.content.clone())
                    .chain(search::projects(&self.projects, &self.query)),
                self.scope,
            )
        };
        if self.active >= self.hits.len() {
            self.active = 0;
        }
        self.pointer = None;
        cx.notify();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.debounce = None;
        self.live_task = None;
        self.local.clear();
        self.remote.clear();
        self.content.clear();
        self.error = None;
        self.truncated = (false, false);
        self.sessions_loading = !self.query.is_empty() && self.scope.sessions();
        self.files_loading = !self.query.is_empty() && self.scope.files();
        self.rank(cx);
        self.scroll.set_offset(point(px(0.), px(0.)));
        if self.query.is_empty() {
            return;
        }
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = this.update(cx, |this, cx| this.start(cx));
        }));
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.scope.sessions() {
            let store = self.store.clone();
            let query = self.query.clone();
            let flag = self.cancel.clone();
            let check = flag.clone();
            let live = self.live.clone();
            let summaries = self.summaries.clone();
            let revision = self.live_revision;
            let work = cx.background_executor().spawn(async move {
                let mut local = vec![];
                for s in summaries {
                    if let Some(rows) = live.get(&s.id) {
                        local.extend(search::messages(&s, rows, &query, &flag));
                    }
                }
                let saved = locked(&store)
                    .and_then(|db| db.search_reader())
                    .and_then(|reader| match reader {
                        Some(reader) => reader.search_sessions(&query, flag.clone()),
                        None => {
                            locked(&store).and_then(|db| db.search_sessions(&query, flag.clone()))
                        }
                    });
                (local, saved)
            });
            cx.spawn(async move |this, cx| {
                let (local, saved) = work.await;
                if check.load(Ordering::Acquire) {
                    return;
                }
                let _ = this.update(cx, |this, cx| {
                    this.sessions_loading = false;
                    match saved {
                        Ok((hits, truncated)) => {
                            this.remote = hits;
                            this.truncated.0 = truncated;
                        }
                        Err(e) => this.error = Some(e),
                    }
                    if this.live_revision == revision {
                        this.local = local;
                    } else {
                        this.refresh_live(cx);
                    }
                    this.rank(cx);
                });
            })
            .detach();
        }
        if self.scope.files() {
            let cwd = self.cwd.clone();
            let query = self.query.clone();
            let flag = self.cancel.clone();
            let check = flag.clone();
            let work = cx.background_executor().spawn(async move {
                search::project::search_project(
                    &std::path::PathBuf::from(&cwd),
                    &search::project::SearchOptions {
                        cwd: cwd.clone(),
                        query: query.clone(),
                        case_sensitive: false,
                        whole_word: false,
                        regex: false,
                        include: None,
                        exclude: None,
                        search_id: String::new(),
                    },
                    &flag,
                )
                .map(|r| (search::content(r.matches, &query), r.truncated))
            });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                if check.load(Ordering::Acquire) {
                    return;
                }
                let _ = this.update(cx, |this, cx| {
                    this.files_loading = false;
                    match result {
                        Ok((hits, truncated)) => {
                            this.content = hits;
                            this.truncated.1 = truncated;
                        }
                        Err(e) => this.error = Some(e),
                    }
                    this.rank(cx);
                });
            })
            .detach();
        }
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() || self.hits.is_empty() {
            return;
        }
        self.active = (self.active as isize + delta).rem_euclid(self.hits.len() as isize) as usize;
        self.pointer = None;
        self.scroll.scroll_to_item(self.child_index(self.active));
        cx.notify();
    }
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        if let Some(hit) = self.hits.get(ix) {
            cx.emit(Event::Open(hit.target.clone(), self.query.clone()));
        }
    }
}
fn empty_state() -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .px_6()
                .pb(px(96.))
                .child(
                    div()
                        .relative()
                        .w(px(288.))
                        .h(px(192.))
                        .mb_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            canvas(
                                |_, _, _| (),
                                |bounds, _, window, _| {
                                    for row in 0..19 {
                                        for col in 0..27 {
                                            let x = (col as f32 - 13.) * 10.;
                                            let y = (row as f32 - 9.) * 10.;
                                            let radius =
                                                ((x / 144.).powi(2) + (y / 96.).powi(2)).sqrt();
                                            let alpha =
                                                0.14 * ((0.76 - radius) / 0.58).clamp(0., 1.);
                                            window.paint_quad(
                                                fill(
                                                    Bounds::new(
                                                        bounds.center()
                                                            + point(px(x - 1.5), px(y - 1.5)),
                                                        size(px(3.), px(3.)),
                                                    ),
                                                    content(alpha),
                                                )
                                                .corner_radii(px(1.5)),
                                            );
                                        }
                                    }
                                },
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(
                            div()
                                .size(px(56.))
                                .rounded(px(16.))
                                .relative()
                                .child(
                                    canvas(
                                        |_, _, _| (),
                                        |bounds, _, window, _| {
                                            let quad = fill(bounds, content(0.06));
                                            #[cfg(any(target_os = "macos", target_os = "windows"))]
                                            let quad =
                                                quad.border_color(gpui::backdrop_blur_marker(
                                                    4. * window.scale_factor(),
                                                ));
                                            window.paint_quad(quad.corner_radii(px(16.)));
                                        },
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("search", 24.).text_color(content(0.5))),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(19.5))
                        .max_w(px(320.))
                        .text_center()
                        .text_color(content(0.45))
                        .child("Find files, conversations, messages, and projects."),
                ),
        )
        .into_any_element()
}
/// The heading a result sits under; consecutive hits share one.
fn section(target: &Target, suggesting: bool) -> &'static str {
    match (target, suggesting) {
        (Target::Conversation(_), true) => "Recent conversations",
        (Target::Conversation(_), false) => "Conversations",
        (Target::Message { .. }, _) => "Messages",
        (Target::File { location: None, .. }, true) => "Recent files",
        (Target::File { location: None, .. }, false) => "Files",
        (Target::File { .. }, _) => "In file contents",
        (Target::Project(_), _) => "Projects",
    }
}
fn keycap(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(content(0.06))
        .border_1()
        .border_color(content(0.08))
        .text_size(px(10.5))
        .text_color(content(0.55))
        .child(text.into())
}
fn hint(keys: &[&'static str], label: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .children(keys.iter().map(|k| keycap(*k)))
        .child(div().pl(px(2.)).child(label))
}
fn empty_message(text: String) -> AnyElement {
    div()
        .py(px(36.))
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(40.))
                .rounded(px(12.))
                .bg(content(0.05))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("search", 18.).text_color(content(0.4))),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(content(0.5))
                .child(text),
        )
        .into_any_element()
}
impl SearchView {
    /// What the panel offers before anything is typed: where you were last.
    fn suggestions(&self) -> Vec<Hit> {
        let mut hits = vec![];
        let (conversations, files, projects) = match self.scope {
            Scope::All => (5, 4, 0),
            Scope::Conversations => (10, 0, 0),
            Scope::Files => (0, 10, 0),
            Scope::Projects => (0, 0, 12),
        };
        let mut recent: Vec<_> = self
            .summaries
            .iter()
            .filter(|s| !s.archived && s.has_messages)
            .collect();
        recent.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        hits.extend(recent.into_iter().take(conversations).map(|s| Hit {
            id: format!("conversation:{}", s.id),
            target: Target::Conversation(s.id),
            title: monocode_core::session_title::display_title(&s.title, &s.provider).into(),
            meta: search::project_name(&s.cwd),
            provider: s.provider.clone(),
            highlights: vec![],
            score: 0,
            updated_at: s.updated_at,
        }));
        hits.extend(
            self.recent
                .iter()
                .filter_map(|path| self.files.iter().find(|f| &f.path == path))
                .take(files)
                .map(|f| Hit {
                    id: format!("file:{}", f.path),
                    target: Target::File {
                        path: f.path.clone(),
                        location: None,
                    },
                    title: f.name.clone(),
                    meta: f.relative.clone(),
                    provider: String::new(),
                    highlights: vec![],
                    score: 0,
                    updated_at: 0,
                }),
        );
        hits.extend(self.projects.iter().take(projects).map(|path| Hit {
            id: format!("project:{path}"),
            target: Target::Project(path.clone()),
            title: search::project_name(path),
            meta: path.clone(),
            provider: String::new(),
            highlights: vec![],
            score: 0,
            updated_at: 0,
        }));
        hits
    }
    /// The list child that holds hit `ix`, counting the notice and headings.
    fn child_index(&self, ix: usize) -> usize {
        let suggesting = self.query.is_empty();
        let mut child = usize::from(self.truncated.0 || self.truncated.1);
        if !self.panel {
            return child + ix;
        }
        let mut last = None;
        for (i, hit) in self.hits.iter().enumerate().take(ix + 1) {
            let heading = section(&hit.target, suggesting);
            if last != Some(heading) {
                child += 1;
                last = Some(heading);
            }
            if i < ix {
                child += 1;
            }
        }
        child
    }
    fn cycle_scope(&mut self, delta: isize, cx: &mut Context<Self>) {
        let ix = Scope::ALL
            .iter()
            .position(|(s, _)| *s == self.scope)
            .unwrap_or(0) as isize;
        let next = (ix + delta).rem_euclid(Scope::ALL.len() as isize) as usize;
        self.select_scope(Scope::ALL[next].0, cx);
    }
    fn row(&self, ix: usize, hit: &Hit, cx: &mut Context<Self>) -> AnyElement {
        let active = self.active == ix;
        let mark = match &hit.target {
            Target::Conversation(_) => provider_icon(&hit.provider, 14.).into_any_element(),
            Target::Message { .. } => icon("message-square", 14.)
                .text_color(content(0.6))
                .into_any_element(),
            Target::File { path, .. } => {
                crate::file_icons::file_type_icon(&search::project_name(path), false, false, 15.)
            }
            Target::Project(path) => self
                .logos
                .get(path)
                .map(|logo| {
                    crate::image_memory::thumbnail(std::path::PathBuf::from(logo))
                        .size(px(16.))
                        .rounded_sm()
                        .into_any_element()
                })
                .unwrap_or_else(|| {
                    icon("folder", 14.)
                        .text_color(content(0.6))
                        .into_any_element()
                }),
        };
        let text = StyledText::new(hit.title.clone()).with_highlights(
            hit.highlights.iter().cloned().map(|range| {
                (
                    range,
                    HighlightStyle {
                        font_weight: Some(gpui::FontWeight::SEMIBOLD),
                        color: Some(accent_color()),
                        ..Default::default()
                    },
                )
            }),
        );
        let selection = if is_light() { 0.07 } else { 0.09 };
        div()
            .id(("app-search-row", ix))
            .debug_selector(move || format!("app-search-row-{ix}"))
            .role(Role::ListItem)
            .aria_selected(active)
            .aria_label(format!("{} {}", hit.title, hit.meta))
            .h(px(38.))
            .w_full()
            .flex_shrink_0()
            .relative()
            .rounded(px(8.))
            .pl(px(8.))
            .pr(px(10.))
            .flex()
            .gap(px(10.))
            .items_center()
            .text_size(px(13.5))
            .cursor_pointer()
            .when(active, |d| d.bg(content(selection)))
            .when(active, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(10.))
                        .w(px(2.5))
                        .h(px(18.))
                        .rounded_full()
                        .bg(accent_color()),
                )
            })
            .child(
                div()
                    .size(px(26.))
                    .flex_shrink_0()
                    .rounded(px(7.))
                    .bg(content(if active { 0.08 } else { 0.045 }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(mark),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_color(content(if active { 1. } else { 0.86 }))
                    .child(text),
            )
            .child(
                div()
                    .min_w_0()
                    .max_w(relative(0.45))
                    .truncate()
                    .text_size(px(12.))
                    .text_color(content(if active { 0.5 } else { 0.38 }))
                    .child(hit.meta.clone()),
            )
            .when(active, |d| d.child(keycap("↵")))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if this.pointer.is_some_and(|old| old != event.position) && this.active != ix {
                    this.active = ix;
                    cx.notify();
                }
                this.pointer = Some(event.position);
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
            .into_any_element()
    }
}
impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.panel {
            self.render_panel(cx).into_any_element()
        } else {
            self.render_page(window, cx).into_any_element()
        }
    }
}
impl SearchView {
    /// The original page: a title-strip field, scope tabs and full-height rows.
    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .hits
            .iter()
            .enumerate()
            .map(|(ix, hit)| {
                let mark = match &hit.target {
                    Target::Conversation(_) => provider_icon(&hit.provider, 14.).into_any_element(),
                    Target::Message { .. } => icon("message-square", 14.)
                        .text_color(content(0.55))
                        .into_any_element(),
                    Target::File { path, .. } => crate::file_icons::file_type_icon(
                        &search::project_name(path),
                        false,
                        false,
                        16.,
                    ),
                    Target::Project(path) => self
                        .logos
                        .get(path)
                        .map(|logo| {
                            crate::image_memory::thumbnail(std::path::PathBuf::from(logo))
                                .size(px(14.))
                                .rounded_sm()
                                .into_any_element()
                        })
                        .unwrap_or_else(|| icon("folder", 14.).into_any_element()),
                };
                let text = StyledText::new(hit.title.clone()).with_highlights(
                    hit.highlights.iter().cloned().map(|range| {
                        (
                            range,
                            HighlightStyle {
                                font_weight: None,
                                color: Some(accent_color()),
                                ..Default::default()
                            },
                        )
                    }),
                );
                div()
                    .id(("app-search-row", ix))
                    .debug_selector(move || format!("app-search-row-{ix}"))
                    .role(Role::ListItem)
                    .aria_selected(self.active == ix)
                    .aria_label(format!("{} {}", hit.title, hit.meta))
                    .h(px(32.))
                    .w_full()
                    .flex_shrink_0()
                    .rounded_md()
                    .px_2()
                    .flex()
                    .gap_2()
                    .items_center()
                    .text_size(px(13.))
                    .when(self.active == ix, |d| {
                        d.bg(content(if content(1.).l < 0.5 { 0.06 } else { 0.10 }))
                    })
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(mark),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(text))
                    .child(
                        div()
                            .min_w_0()
                            .max_w(relative(0.45))
                            .truncate()
                            .font_family("monospace")
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(hit.meta.clone()),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if this.pointer.is_some_and(|old| old != event.position)
                            && this.active != ix
                        {
                            this.active = ix;
                            cx.notify();
                        }
                        this.pointer = Some(event.position);
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                    .into_any_element()
            })
            .collect();
        let header = if self.original_header.is_some() {
            crate::shell::titlebar_drag_area()
        } else {
            div()
        };
        let loading = self.sessions_loading || self.files_loading;
        let stroke = content(if self.classic { 0.07 } else { 0.04 });
        let selection = if content(1.).l < 0.5 { 0.06 } else { 0.10 };
        div()
            .id("app-search")
            .key_context("Workspace")
            .debug_selector(|| "app-search".into())
            .aria_label("Search")
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.active, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &crate::shell::DismissMenu, _, cx| {
                if !this.input.read(cx).is_composing() {
                    cx.emit(Event::Close);
                }
                cx.stop_propagation();
            }))
            .child(
                header
                    .id("app-search-header")
                    .debug_selector(|| "app-search-header".into())
                    .h(px(40.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(stroke)
                    .pl(px(self.original_header.unwrap_or(12.)))
                    .pr(px(12.
                        + if self.original_header.is_some() {
                            crate::shell::client_controls_width(window)
                        } else {
                            0.
                        }))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("search", 14.).text_color(content(0.5)))
                    .child(
                        div()
                            .debug_selector(|| "app-search-field".into())
                            .flex_1()
                            .min_w_0()
                            .when(self.original_header.is_some(), |d| {
                                d.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            })
                            .child(self.input.clone()),
                    )
                    .when(loading, |d| {
                        d.child(crate::right_panel::spinner(
                            "app-search-loading",
                            "loader",
                            14.,
                        ))
                    }),
            )
            .child(
                div()
                    .id("app-search-scopes")
                    .debug_selector(|| "app-search-scopes".into())
                    .h(px(36.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(1.))
                    .px_3()
                    .border_b_1()
                    .border_color(stroke)
                    .children(
                        Scope::ALL
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (scope, label))| {
                                div()
                                    .id(("app-search-scope", ix))
                                    .debug_selector(move || format!("app-search-scope-{ix}"))
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .child(label)
                                    .aria_selected(self.scope == scope)
                                    .rounded_md()
                                    .px_2()
                                    .py_1()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .bg(content(if self.scope == scope { selection } else { 0. }))
                                    .text_color(content(if self.scope == scope { 1. } else { 0.5 }))
                                    .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_scope(scope, cx);
                                    }))
                            }),
                    ),
            )
            .child(if self.query.is_empty() {
                empty_state()
            } else {
                div()
                    .id("app-search-results")
                    .role(Role::ListBox)
                    .aria_label("Search results")
                    .flex_1()
                    .min_h_0()
                    .p_1p5()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .when(self.truncated.0 || self.truncated.1, |d| {
                        d.child(
                            div()
                                .px_2p5()
                                .py_1()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child("Results limited to the first matches"),
                        )
                    })
                    .children(rows)
                    .when(self.hits.is_empty() && !loading, |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_1p5()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child(self.error.clone().unwrap_or_else(|| "No results".into())),
                        )
                    })
                    .into_any_element()
            })
    }
    fn render_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let suggesting = self.query.is_empty();
        let mut list: Vec<AnyElement> = vec![];
        if self.truncated.0 || self.truncated.1 {
            list.push(
                div()
                    .px(px(10.))
                    .pt_1()
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .child("Showing the first matches")
                    .into_any_element(),
            );
        }
        let mut last = None;
        let hits = std::mem::take(&mut self.hits);
        for (ix, hit) in hits.iter().enumerate() {
            let heading = section(&hit.target, suggesting);
            if last != Some(heading) {
                last = Some(heading);
                list.push(
                    div()
                        .flex_shrink_0()
                        .px(px(10.))
                        .pt(px(if ix == 0 { 6. } else { 12. }))
                        .pb(px(5.))
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(content(0.42))
                        .child(heading)
                        .into_any_element(),
                );
            }
            list.push(self.row(ix, hit, cx));
        }
        self.hits = hits;
        let loading = self.sessions_loading || self.files_loading;
        let count = self.hits.len();
        div()
            .id("app-search")
            .key_context("Workspace")
            .debug_selector(|| "app-search".into())
            .role(Role::Dialog)
            .aria_label("Search")
            .w_full()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.active, cx);
                cx.stop_propagation();
            }))
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let k = &event.keystroke;
                if k.key == "tab" && !k.modifiers.platform && !k.modifiers.control {
                    this.cycle_scope(if k.modifiers.shift { -1 } else { 1 }, cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::shell::DismissMenu, _, cx| {
                if !this.input.read(cx).is_composing() {
                    cx.emit(Event::Close);
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id("app-search-header")
                    .debug_selector(|| "app-search-header".into())
                    .h(px(56.))
                    .flex_shrink_0()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(icon("search", 18.).text_color(content(0.45)))
                    .child(
                        div()
                            .debug_selector(|| "app-search-field".into())
                            .flex_1()
                            .min_w_0()
                            .child(self.input.clone()),
                    )
                    .when(loading, |d| {
                        d.child(crate::right_panel::spinner(
                            "app-search-loading",
                            "loader",
                            14.,
                        ))
                    })
                    .child(keycap("esc")),
            )
            .child(
                div()
                    .id("app-search-scopes")
                    .debug_selector(|| "app-search-scopes".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(12.))
                    .pb(px(10.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .children(
                        Scope::ALL
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (scope, label))| {
                                let selected = self.scope == scope;
                                let glyph = match scope {
                                    Scope::All => "sparkles",
                                    Scope::Conversations => "message-square",
                                    Scope::Files => "file",
                                    Scope::Projects => "folder",
                                };
                                div()
                                    .id(("app-search-scope", ix))
                                    .debug_selector(move || format!("app-search-scope-{ix}"))
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .aria_selected(selected)
                                    .cursor_pointer()
                                    .h(px(26.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if selected {
                                        accent_color().opacity(0.45)
                                    } else {
                                        content(0.08)
                                    })
                                    .bg(if selected {
                                        accent_color().opacity(0.14)
                                    } else {
                                        content(0.)
                                    })
                                    .text_size(px(12.))
                                    .text_color(content(if selected { 1. } else { 0.55 }))
                                    .hover(|s| s.bg(content(0.05)).text_color(content(0.95)))
                                    .child(icon(glyph, 12.).text_color(if selected {
                                        accent_color()
                                    } else {
                                        content(0.45)
                                    }))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_scope(scope, cx);
                                    }))
                            }),
                    ),
            )
            .child(
                div()
                    .id("app-search-results")
                    .role(Role::ListBox)
                    .aria_label("Search results")
                    .h(px(400.))
                    .px(px(8.))
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(list)
                    .when(self.hits.is_empty() && !(loading && !suggesting), |d| {
                        d.child(empty_message(if suggesting {
                            match self.scope {
                                Scope::Projects => "No projects yet".into(),
                                _ => "Type to search conversations, files and projects".into(),
                            }
                        } else {
                            self.error
                                .clone()
                                .unwrap_or_else(|| format!("No results for “{}”", self.query))
                        }))
                    }),
            )
            .child(
                div()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px(px(14.))
                    .border_t_1()
                    .border_color(content(0.07))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child(hint(&["↑", "↓"], "Navigate"))
                    .child(hint(&["↵"], "Open"))
                    .child(hint(&["⇥"], "Switch scope"))
                    .child(div().flex_1())
                    .when(!suggesting && count > 0, |d| {
                        d.child(format!(
                            "{count} result{}",
                            if count == 1 { "" } else { "s" }
                        ))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::store::Store;
    use std::sync::Mutex;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "monocode-search-ui-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[gpui::test]
    fn app_search_debounce_scope_keyboard_and_stale_results(cx: &mut TestAppContext) {
        let dir = TempDir::new();
        std::fs::write(
            dir.path().join("needle.ts"),
            "const value = 'needle content';\n",
        )
        .unwrap();
        let mut store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut session = store.create("codex", "/another-project").unwrap();
        store.title(session.id, "Needle conversation").unwrap();
        session.title = "Needle conversation".into();
        store
            .write_rows(
                session.id,
                0,
                &[Row {
                    role: "user".into(),
                    text: "saved needle message".into(),
                    first: true,
                    sent_at: None,
                }],
            )
            .unwrap();
        let cwd = dir.path().to_string_lossy().into_owned();
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::shell::bind_keys(cx);
            cx.open_window(Default::default(), |w, cx| {
                let view = cx.new(|cx| {
                    SearchView::new(
                        cwd,
                        Arc::new(Mutex::new(store)),
                        Sources {
                            summaries: vec![session],
                            projects: vec![dir.path().to_string_lossy().into_owned()],
                            ..Default::default()
                        },
                        cx,
                    )
                });
                w.focus(&view.focus_handle(cx), cx);
                view
            })
            .unwrap()
        });
        cx.run_until_parked();
        {
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            visual.run_until_parked();
            assert_eq!(
                visual
                    .debug_bounds("app-search-header")
                    .unwrap()
                    .size
                    .height,
                px(40.)
            );
            assert_eq!(
                visual
                    .debug_bounds("app-search-scopes")
                    .unwrap()
                    .size
                    .height,
                px(36.)
            );
            for (selector, min_width) in [
                ("app-search-scope-0", 28.),
                ("app-search-scope-1", 70.),
                ("app-search-scope-2", 32.),
                ("app-search-scope-3", 45.),
            ] {
                assert!(
                    visual.debug_bounds(selector).unwrap().size.width > px(min_width),
                    "scope buttons must render their labels"
                );
            }
            let files = visual.debug_bounds("app-search-scope-2").unwrap();
            visual.simulate_click(files.center(), Default::default());
            visual.run_until_parked();
            window
                .update(visual, |view, _, cx| {
                    assert_eq!(view.scope, Scope::Files);
                    view.select_scope(Scope::All, cx);
                })
                .unwrap();
        }
        cx.simulate_input(window.into(), "needle");
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.hits.len(), 2); // Title and file name are immediate.
                assert!(view.sessions_loading && view.files_loading);
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(!view.sessions_loading && !view.files_loading);
                assert!(
                    view.hits
                        .iter()
                        .any(|h| matches!(h.target, Target::Message { .. }))
                );
                assert!(view.hits.iter().any(|h| matches!(
                    h.target,
                    Target::File {
                        location: Some((1, 16)),
                        ..
                    }
                )));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "up");
        window
            .update(cx, |view, _, _| {
                assert_eq!(view.active, view.hits.len() - 1)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "down");
        window
            .update(cx, |view, _, _| assert_eq!(view.active, 0))
            .unwrap();
        window
            .update(cx, |view, _, cx| view.select_scope(Scope::Files, cx))
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(
                    view.hits
                        .iter()
                        .all(|h| matches!(h.target, Target::File { .. }))
                );
                let stale = view.cancel.clone();
                view.input
                    .update(cx, |i, cx| i.set_text("absent".into(), cx));
                // Changed is delivered after this update.
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1))
                        .await;
                    assert!(stale.load(Ordering::Acquire));
                })
                .detach();
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| assert!(view.hits.is_empty()))
            .unwrap();
    }
    #[gpui::test]
    fn search_panel_suggests_recent_work_and_tabs_through_scopes(cx: &mut TestAppContext) {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut session = store.create("codex", "/project").unwrap();
        session.title = "Recent work".into();
        session.has_messages = true;
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::shell::bind_keys(cx);
            cx.open_window(Default::default(), |w, cx| {
                let view = cx.new(|cx| {
                    SearchView::new(
                        "/nonexistent-search-panel".into(),
                        Arc::new(Mutex::new(store)),
                        Sources {
                            summaries: vec![session],
                            panel: true,
                            ..Default::default()
                        },
                        cx,
                    )
                });
                w.focus(&view.focus_handle(cx), cx);
                view
            })
            .unwrap()
        });
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("app-search-header")
                .unwrap()
                .size
                .height,
            px(56.)
        );
        window
            .update(visual, |view, _, _| {
                assert!(matches!(
                    view.hits.as_slice(),
                    [Hit {
                        target: Target::Conversation(_),
                        ..
                    }]
                ));
            })
            .unwrap();
        visual.simulate_keystrokes("tab");
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.scope, Scope::Conversations)
            })
            .unwrap();
        visual.simulate_keystrokes("shift-tab");
        window
            .update(visual, |view, _, _| assert_eq!(view.scope, Scope::All))
            .unwrap();
    }
}
