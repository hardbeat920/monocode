//! Inbox views over the engine's list, detail, checks, and session flows.
use super::settings::{AccountsAdapter, convert};
use gpui::{AnyView, App, AppContext as _, ClipboardItem, Context, Entity, Subscription, Window};
use monocode_app::boot::AppServices;
use monocode_core::session::LinkedWorkItem;
use monocode_engine::inbox as engine;
use monocode_engine::inbox::{
    ci_repair_tracking::CiRepairTracker, client::InboxClient, detail::InboxItemDetail,
    inbox::Inbox, list::InboxList, pr_checks::PrChecks,
};
use monocode_engine::{history::HistoryPackage, projects::ProjectsGlobal, workspace::Workspace};
use monocode_view_inbox::data::*;
use monocode_view_settings::accounts::{UsageHost as _, style::parse_css_color};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub struct InboxAdapter {
    pub workspace: Entity<Workspace>,
    client: InboxClient,
    repairs: RefCell<CiRepairTracker>,
    asks: RefCell<HashMap<String, Entity<crate::session_pane::SessionPane>>>,
}
impl InboxAdapter {
    pub fn new(workspace: Entity<Workspace>, cx: &App) -> Self {
        Self {
            workspace,
            client: Inbox::global(cx).read(cx).client().clone(),
            repairs: RefCell::new(CiRepairTracker::new(AppServices::global(cx).kv.clone())),
            asks: RefCell::new(HashMap::new()),
        }
    }
    pub fn list(workspace: Entity<Workspace>, cx: &mut App) -> Rc<dyn InboxListData> {
        Rc::new(ListAdapter(cx.new(|cx| LiveList::new(workspace, cx))))
    }
    fn mark(path: &str, cx: &App) -> ProjectMark {
        let appearance = AccountsAdapter::new().project_appearance(
            &monocode_layout::paths::project_key(path),
            &monocode_layout::paths::project_name(path),
            cx,
        );
        ProjectMark {
            logo_path: appearance.logo,
            mascot_name: appearance.mascot,
            mascot_color: parse_css_color(&appearance.color),
        }
    }
}

struct LiveList {
    list: Entity<InboxList>,
    workspace: Entity<Workspace>,
    cwd: String,
    recents: Vec<String>,
    _subscriptions: Vec<Subscription>,
    list_subscription: Subscription,
}
impl LiveList {
    fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let projects = ProjectsGlobal::projects(cx);
        let cwd = workspace.read(cx).sidebar_cwd(cx);
        let recent: Vec<_> = projects
            .read(cx)
            .recents()
            .iter()
            .map(|v| engine::rail::RecentProject {
                path: v.path.clone(),
                opened_at: v.opened_at,
            })
            .collect();
        let recents = recent.iter().map(|v| v.path.clone()).collect();
        let client = Inbox::global(cx).read(cx).client().clone();
        let list = cx.new(|cx| InboxList::new(client, &recent, &cwd, None, cx));
        let subscriptions = vec![
            cx.observe(&workspace, |this, _, cx| this.sync(cx)),
            cx.observe(&projects, |this, _, cx| this.sync(cx)),
            cx.observe(&Inbox::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&HistoryPackage::history(cx), |_, _, cx| cx.notify()),
        ];
        let list_subscription = cx.observe(&list, |_, _, cx| cx.notify());
        Self {
            list,
            workspace,
            cwd,
            recents,
            _subscriptions: subscriptions,
            list_subscription,
        }
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let cwd = self.workspace.read(cx).sidebar_cwd(cx);
        let recent: Vec<_> = ProjectsGlobal::projects(cx)
            .read(cx)
            .recents()
            .iter()
            .map(|v| engine::rail::RecentProject {
                path: v.path.clone(),
                opened_at: v.opened_at,
            })
            .collect();
        let recents: Vec<_> = recent.iter().map(|v| v.path.clone()).collect();
        if cwd == self.cwd && recents == self.recents {
            cx.notify();
            return;
        }
        let client = Inbox::global(cx).read(cx).client().clone();
        let list = cx.new(|cx| InboxList::new(client, &recent, &cwd, None, cx));
        self.list_subscription = cx.observe(&list, |_, _, cx| cx.notify());
        self.list = list;
        self.cwd = cwd;
        self.recents = recents;
        cx.notify();
    }
}
struct ListAdapter(Entity<LiveList>);
impl InboxListData for ListAdapter {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.0, move |_, cx| listener(cx))
    }
    fn state(&self, cx: &App) -> InboxListState {
        let live = self.0.read(cx);
        let list = live.list.read(cx);
        let mut projects: Vec<_> = list
            .projects()
            .iter()
            .map(|path| InboxProjectOption {
                path: path.clone(),
                name: monocode_layout::paths::project_name(path),
                mark: InboxAdapter::mark(path, cx),
            })
            .collect();
        projects.sort_by(|a, b| engine::text::locale_compare(&a.name, &b.name));
        InboxListState {
            cwd: live.cwd.clone(),
            projects,
            item_count: list.items().len(),
            loading: list.loading(),
            revalidating: list.revalidating(),
            source: convert(list.source()),
            visible_sources: convert(list.visible_sources()),
            connectable_sources: convert(list.connectable_sources()),
            source_error: list.source_error().map(str::to_owned),
            read_status_error: list.read_status_error().map(str::to_owned),
            filters: convert(list.active_filters()),
            filters_active: list.filters_active(),
            source_has_unseen: list.source_has_unseen(),
            linear_projects: list
                .linear_project_options()
                .into_iter()
                .map(|p| LinearProjectOption {
                    id: p.id,
                    name: p.name,
                })
                .collect(),
            linear_teams: list
                .linear_teams()
                .iter()
                .map(|p| TrackerGroup {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    key: p.key.clone(),
                })
                .collect(),
            hidden_linear_team_ids: list.linear_hidden_team_ids().to_vec(),
            jira_projects: list
                .jira_projects()
                .iter()
                .map(|p| TrackerGroup {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    key: p.key.clone(),
                })
                .collect(),
            hidden_jira_project_ids: list.jira_hidden_project_ids().to_vec(),
            target_selection_key: list.target_selection_key(),
        }
    }
    fn visible_items(&self, search: &str, cx: &App) -> Vec<ListedItem> {
        let inbox = Inbox::global(cx);
        let inbox = inbox.read(cx);
        let sessions =
            inbox.inbox_related_sessions(HistoryPackage::history(cx).read(cx).rows(), cx);
        self.0
            .read(cx)
            .list
            .read(cx)
            .visible_items(search)
            .into_iter()
            .map(|item| {
                let related = engine::session_work_item::related_sessions_for_inbox_item(
                    &item,
                    &sessions,
                    |session| session.linked_work_item.as_ref(),
                );
                let related_sessions = related
                    .into_iter()
                    .map(|s| RelatedSession {
                        id: s.id.clone(),
                        title: monocode_core::session::session_display_title(&s.title, s.harness),
                        archived: s.archived == Some(true),
                    })
                    .collect();
                ListedItem {
                    key: engine::github_tasks::inbox_item_key(&item),
                    unseen: inbox.is_item_unseen(&item),
                    project_mark: InboxAdapter::mark(&item.project_path, cx),
                    related_sessions,
                    item: convert(item),
                }
            })
            .collect()
    }
    fn set_source(&self, source: InboxSource, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.set_source(convert(source), cx));
    }
    fn set_filters(&self, filters: InboxFilters, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.set_filters(convert(filters), cx));
    }
    fn set_hidden_linear_team_ids(&self, ids: Vec<String>, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.set_linear_hidden_team_ids(ids, cx));
    }
    fn set_hidden_jira_project_ids(&self, ids: Vec<String>, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.set_jira_hidden_project_ids(ids, cx));
    }
    fn refresh(&self, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.refresh(cx));
    }
    fn mark_source_read(&self, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.mark_source_read(cx));
    }
    fn mark_item_seen(&self, item: &ListedItem, cx: &mut App) {
        Inbox::global(cx).update(cx, |inbox, cx| {
            inbox.mark_item_seen(&convert(&item.item), cx)
        });
    }
    fn update_item(&self, item: InboxItem, cx: &mut App) {
        let list = self.0.read(cx).list.clone();
        list.update(cx, |list, cx| list.update_item(convert(item), cx));
    }
}

struct DetailAdapter {
    detail: Entity<InboxItemDetail>,
    reply_author: RefCell<String>,
}
fn loadable<T: serde::Serialize, U: serde::de::DeserializeOwned>(
    value: &engine::detail::Loadable<T>,
) -> Loadable<U> {
    Loadable {
        value: value.value.as_ref().map(convert),
        loading: value.loading,
        error: value.error.clone(),
    }
}
impl InboxDetailData for DetailAdapter {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.detail, move |_, cx| listener(cx))
    }
    fn state(&self, cx: &App) -> InboxDetailState {
        let d = self.detail.read(cx);
        InboxDetailState {
            details: loadable(d.details()),
            thread: loadable(d.thread()),
            diff: loadable(d.diff()),
            reply_to: d.reply_to().map(|v| InboxReplyTarget {
                id: v.id.clone(),
                thread_id: v.thread_id.clone(),
                author: self.reply_author.borrow().clone(),
            }),
            posting: d.posting(),
            post_error: d.post_error().map(str::to_owned),
            action_busy: d.action_busy(),
            action_error: d.action_error().map(str::to_owned),
            action_notice: d.action_notice().map(str::to_owned),
        }
    }
    fn set_item(&self, item: InboxItem, cx: &mut App) {
        self.detail
            .update(cx, |d, cx| d.set_item(convert(item), cx));
    }
    fn set_revision(&self, revision: u64, cx: &mut App) {
        self.detail.update(cx, |d, cx| d.set_revision(revision, cx));
    }
    fn show_diff(&self, full: bool, cx: &mut App) {
        self.detail.update(cx, |d, cx| d.show_diff(full, cx));
    }
    fn set_reply_to(&self, reply: Option<InboxReplyTarget>, cx: &mut App) {
        *self.reply_author.borrow_mut() =
            reply.as_ref().map(|v| v.author.clone()).unwrap_or_default();
        self.detail.update(cx, |d, cx| {
            d.set_reply_to(
                reply.map(|v| engine::detail::InboxReplyTarget {
                    id: v.id,
                    thread_id: v.thread_id,
                }),
                cx,
            )
        });
    }
    fn post_comment(&self, body: String, cx: &mut App) -> DataTask<()> {
        self.detail.update(cx, |d, cx| d.post_comment(&body, cx))
    }
    fn run_pr_action(&self, action: GithubPrAction, cx: &mut App) -> DataTask<InboxItem> {
        let task = self
            .detail
            .update(cx, |d, cx| d.run_pr_action(convert(action), cx));
        cx.spawn(async move |_| task.await.map(convert))
    }
}
struct ChecksAdapter {
    checks: Entity<PrChecks>,
    generation: Rc<Cell<u64>>,
}
fn checks_params(v: PrChecksParams) -> engine::pr_checks::PrChecksParams {
    engine::pr_checks::PrChecksParams {
        cwd: v.cwd,
        repo: v.repo,
        number: v.number,
        enabled: v.enabled,
        open: v.open,
        poll: v.poll,
        revision: v.revision,
    }
}
impl PrChecksData for ChecksAdapter {
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        let generation = self.generation.clone();
        cx.observe(&self.checks, move |checks, cx| {
            if !checks.read(cx).loading() && !checks.read(cx).refreshing() {
                generation.set(generation.get() + 1);
            }
            listener(cx);
        })
    }
    fn state(&self, cx: &App) -> PrChecksState {
        let c = self.checks.read(cx);
        PrChecksState {
            checks: c.checks().map(convert),
            loading: c.loading(),
            refreshing: c.refreshing(),
            error: c.error().map(str::to_owned),
            stale: c.stale(),
            generation: self.generation.get(),
        }
    }
    fn set_params(&self, params: PrChecksParams, cx: &mut App) {
        self.checks
            .update(cx, |c, cx| c.set_params(checks_params(params), cx));
    }
    fn refresh(&self, cx: &mut App) {
        self.checks.update(cx, |c, cx| c.refresh(cx));
    }
}
fn target_kind(target: &LinkedWorkItem) -> engine::types::WorkItemKind {
    target.kind
}
impl InboxServices for InboxAdapter {
    fn now_ms(&self) -> i64 {
        self.client.now()
    }
    fn open_detail(&self, item: &InboxItem, cx: &mut App) -> Rc<dyn InboxDetailData> {
        Rc::new(DetailAdapter {
            detail: cx.new(|cx| InboxItemDetail::new(self.client.clone(), convert(item), cx)),
            reply_author: RefCell::default(),
        })
    }
    fn open_pr_checks(&self, params: PrChecksParams, cx: &mut App) -> Rc<dyn PrChecksData> {
        Rc::new(ChecksAdapter {
            checks: cx.new(|cx| PrChecks::new(self.client.clone(), checks_params(params), cx)),
            generation: Rc::default(),
        })
    }
    fn peek_github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        _: &App,
    ) -> Option<InboxItem> {
        self.client
            .peek_github_work_item(&target.repo, target_kind(target), target.number)
            .map(|v| {
                convert(engine::types::InboxItem::from_github(
                    &v,
                    cwd,
                    engine::types::InboxProvider::Github,
                ))
            })
    }
    fn github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        cx: &mut App,
    ) -> DataTask<InboxItem> {
        let task = self.client.github_work_item(
            cwd,
            &target.repo,
            target_kind(target),
            target.number,
            false,
        );
        let cwd = cwd.to_string();
        cx.spawn(async move |_| {
            task.await.map(|v| {
                convert(engine::types::InboxItem::from_github(
                    &v,
                    &cwd,
                    engine::types::InboxProvider::Github,
                ))
            })
        })
    }
    fn fetch_check_details(
        &self,
        cwd: &str,
        repo: &str,
        job: &str,
        cx: &mut App,
    ) -> DataTask<GithubCheckDetails> {
        let task = self.client.fetch_github_check_details(cwd, repo, job);
        cx.spawn(async move |_| task.await.map(convert))
    }
    fn commit_file_text(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
        cx: &mut App,
    ) -> DataTask<Option<String>> {
        let cwd = cwd.into();
        let sha = sha.into();
        let relative = relative.into();
        cx.background_executor().spawn(async move {
            monocode_git::fs::git_commit_file_diff(cwd, sha, relative).map(|v| Some(v.current))
        })
    }
    fn ci_repairs(&self, _: &App) -> Vec<TrackedCiRepair> {
        convert(self.repairs.borrow_mut().get_ci_repairs())
    }
    fn subscribe_ci_repairs(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&Inbox::global(cx), move |_, cx| listener(cx))
    }
    fn start_item(&self, item: InboxItem, body: Option<String>, cx: &mut App) -> DataTask<()> {
        let task =
            Inbox::global(cx).update(cx, |i, cx| i.start_inbox_item(convert(item), body, cx));
        cx.spawn(async move |_| task.await.map(|_| ()))
    }
    fn repair_checks(
        &self,
        item: &InboxItem,
        start: CiRepairStart,
        session: Option<String>,
        cx: &mut App,
    ) -> DataTask<()> {
        let evidence: Vec<_> = start
            .evidence
            .into_iter()
            .map(|v| engine::ci_repair::CiRepairEvidence {
                check: convert(v.check),
                details: v.details.map(|v| match v {
                    CiEvidenceDetails::Full(v) => {
                        engine::ci_repair::CiEvidenceDetails::Full(convert(v))
                    }
                    CiEvidenceDetails::Notice(v) => engine::ci_repair::CiEvidenceDetails::Notice(v),
                }),
            })
            .collect();
        let request = engine::ci_repair::build_ci_repair_request(
            &start.repo,
            start.number,
            &start.head_oid,
            &evidence,
        );
        Inbox::global(cx).update(cx, |inbox, cx| {
            inbox.repair_checks(convert(item), request, session, cx)
        })
    }
    fn repair_sessions(&self, item: &InboxItem, cx: &App) -> Vec<RelatedSession> {
        Inbox::global(cx)
            .read(cx)
            .repair_sessions(HistoryPackage::history(cx).read(cx).rows(), cx)
            .into_iter()
            .filter(|s| monocode_layout::paths::same_project_path(&s.cwd, &item.project_path))
            .map(|s| RelatedSession {
                id: s.id,
                title: monocode_core::session::session_display_title(&s.title, s.harness),
                archived: s.archived == Some(true),
            })
            .collect()
    }
    fn ask(&self, item: &InboxItem, cx: &mut App) -> DataTask<String> {
        let task = Inbox::global(cx).update(cx, |i, cx| i.ask_inbox_item(convert(item), cx));
        cx.spawn(async move |_| task.await)
    }
    fn ask_restart(&self, item: &InboxItem, cx: &mut App) -> DataTask<String> {
        Inbox::global(cx).update(cx, |i, cx| i.restart_inbox_ask(convert(item), cx))
    }
    fn ask_pane(&self, id: &str, window: &mut Window, cx: &mut App) -> Option<AnyView> {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_inbox_session(Some(id.to_owned()), cx);
            workspace.set_composer_focused(true, cx);
        });
        if let Some(view) = self.asks.borrow().get(id) {
            view.update(cx, |pane, cx| pane.set_focused(true, window, cx));
            return Some(view.clone().into());
        }
        let id = id.to_string();
        let view = cx.new(|cx| {
            crate::session_pane::SessionPane::new(
                id.clone(),
                self.workspace.downgrade(),
                window,
                cx,
            )
        });
        view.update(cx, |pane, cx| pane.set_focused(true, window, cx));
        self.asks.borrow_mut().insert(id, view.clone());
        Some(view.into())
    }
    fn ask_unmounted(&self, cx: &mut App) {
        self.workspace
            .update(cx, |workspace, cx| workspace.set_inbox_session(None, cx));
        Inbox::global(cx).update(cx, |i, cx| i.set_ask_session(None, cx));
        self.asks.borrow_mut().clear();
    }
    fn open_url(&self, url: &str, cx: &mut App) {
        cx.open_url(url);
    }
    fn fetch_media(&self, url: &str, cx: &mut App) -> DataTask<InboxMedia> {
        let task = self.client.fetch_inbox_media(url);
        cx.spawn(async move |_| {
            let bytes = task.await?;
            let media =
                engine::inbox_media::sniff_inbox_media(&bytes).ok_or("Unsupported media type")?;
            Ok(InboxMedia {
                kind: match media.kind {
                    engine::inbox_media::InboxMediaKind::Image => InboxMediaKind::Image,
                    engine::inbox_media::InboxMediaKind::Video => InboxMediaKind::Video,
                },
                mime: media.mime.into(),
                bytes,
            })
        })
    }
    fn copy_text(&self, text: &str, cx: &mut App) -> bool {
        cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
        true
    }
    fn play_copy_cue(&self, cx: &mut App) {
        let n = monocode_engine::attention::Attention::global(cx)
            .notifier
            .clone();
        n.update(cx, |n, _| {
            n.play_cue(monocode_engine::attention::sounds::SoundCue::Copy, None);
        });
    }
}
