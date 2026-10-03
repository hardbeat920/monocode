//! `InboxList`: the data side of the Inbox page, ported from the state and
//! effects of `InboxView` in src/features/inbox/ui/InboxView.tsx. One list
//! lives while the page is open: it reads the cached list at once, revalidates
//! it, checks which sources are connected, loads the Linear team and Jira
//! project rosters, and applies the filters. Drawing, selection, and search
//! input stay in the view.

use gpui::{Context, Task};
use monocode_core::session::LinkedWorkItem;

use super::client::{InboxClient, InboxSignal, SignalSubscription};
use super::github_tasks::inbox_item_key;
use super::inbox_filters::{
    InboxFilters, InboxSource, InboxSourceConnections, LinearProjectOption, apply_inbox_filters,
    connectable_inbox_sources, has_active_inbox_filters, inbox_fetch_state, linear_project_options,
    load_inbox_connections, load_inbox_filters, load_inbox_source, prune_inbox_filters,
    resolve_inbox_source, save_inbox_connections, save_inbox_filters, save_inbox_source,
    visible_inbox_sources,
};
use super::inbox_seen::{InboxSeenEntry, RememberedInboxItem};
use super::jira::load_hidden_jira_project_ids;
use super::linear::load_hidden_linear_team_ids;
use super::rail::{RecentProject, inbox_projects_for_rail};
use super::session_work_item::{inbox_item_matches_linked_work_item, linked_work_item_inbox_key};
use super::types::{
    InboxItem, InboxProvider, InboxProviderErrors, InboxQuery, JiraProject, LinearTeam,
};

/// The Inbox page's list state.
pub struct InboxList {
    client: InboxClient,
    cwd: String,
    projects: Vec<String>,
    target: Option<LinkedWorkItem>,
    target_item: Option<InboxItem>,
    items: Vec<InboxItem>,
    loading: bool,
    revalidating: bool,
    provider_errors: InboxProviderErrors,
    read_status_error: Option<String>,
    filters: InboxFilters,
    connections: InboxSourceConnections,
    source: InboxSource,
    linear_hidden_team_ids: Vec<String>,
    linear_teams: Vec<LinearTeam>,
    jira_hidden_project_ids: Vec<String>,
    jira_projects: Vec<JiraProject>,
    load_generation: u64,
    connection_generation: u64,
    roster_generation: u64,
    target_generation: u64,
    _signals: SignalSubscription,
    _signal_task: Task<()>,
}

impl InboxList {
    /// Mount the page for the rail's projects. A `target` (a session card's
    /// linked item) switches to the GitHub tab without saving that choice.
    pub fn new(
        client: InboxClient,
        recents: &[RecentProject],
        cwd: &str,
        target: Option<LinkedWorkItem>,
        cx: &mut Context<Self>,
    ) -> Self {
        let projects: Vec<String> = inbox_projects_for_rail(recents, cwd, client.now())
            .into_iter()
            .map(|project| project.path)
            .collect();
        let kv = client.kv().clone();
        let connections = load_inbox_connections(&kv);
        let source = resolve_inbox_source(load_inbox_source(&kv), &connections);
        // Mount only: storage can name a provider this view already left.
        save_inbox_source(&kv, source);
        let (sender, receiver) = async_channel::unbounded::<InboxSignal>();
        let signals = client.subscribe(move |signal| {
            let _ = sender.try_send(signal);
        });
        let signal_task = cx.spawn(async move |this, cx| {
            while let Ok(signal) = receiver.recv().await {
                if this
                    .update(cx, |this, cx| this.handle_signal(signal, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut list = Self {
            cwd: cwd.to_string(),
            projects,
            target: target.clone(),
            target_item: None,
            items: Vec::new(),
            loading: true,
            revalidating: false,
            provider_errors: InboxProviderErrors::new(),
            read_status_error: None,
            filters: load_inbox_filters(&kv),
            connections,
            source: if target.is_some() {
                InboxProvider::Github
            } else {
                source
            },
            linear_hidden_team_ids: load_hidden_linear_team_ids(&kv),
            linear_teams: Vec::new(),
            jira_hidden_project_ids: load_hidden_jira_project_ids(&kv),
            jira_projects: Vec::new(),
            load_generation: 0,
            connection_generation: 0,
            roster_generation: 0,
            target_generation: 0,
            client,
            _signals: signals,
            _signal_task: signal_task,
        };
        if let Some(cached) = list
            .client
            .peek_inbox_list(&list.projects, &list.fetch_query())
        {
            list.items = cached.items;
            list.provider_errors = cached.errors;
            list.loading = false;
        }
        list.read_connections(cx);
        list.load_rosters(cx);
        list.load_list(false, cx);
        list
    }

    fn handle_signal(&mut self, signal: InboxSignal, cx: &mut Context<Self>) {
        let kv = self.client.kv().clone();
        match signal {
            InboxSignal::LinearChange => {
                self.linear_hidden_team_ids = load_hidden_linear_team_ids(&kv);
                self.read_connections(cx);
                self.load_rosters(cx);
                self.load_list(true, cx);
            }
            InboxSignal::JiraChange => {
                self.jira_hidden_project_ids = load_hidden_jira_project_ids(&kv);
                self.read_connections(cx);
                self.load_rosters(cx);
                self.load_list(true, cx);
            }
            InboxSignal::GitlabChange | InboxSignal::AzureDevOpsChange => {
                self.read_connections(cx);
                self.load_list(true, cx);
            }
            InboxSignal::Seen => {}
            InboxSignal::SelfActivity | InboxSignal::LinkedSessionSeen | InboxSignal::CiRepairs => {
                return;
            }
        }
        cx.notify();
    }

    /// The fetch query the filters and hidden rosters imply.
    pub fn fetch_query(&self) -> InboxQuery {
        let filters = self.active_filters();
        InboxQuery {
            assigned_to_me: filters.assigned_to_me,
            state: inbox_fetch_state(&filters),
            search: String::new(),
            linear_hidden_team_ids: Some(self.linear_hidden_team_ids.clone()),
            jira_hidden_project_ids: Some(self.jira_hidden_project_ids.clone()),
        }
    }

    /// The filters with projects that left the rail dropped.
    pub fn active_filters(&self) -> InboxFilters {
        prune_inbox_filters(&self.filters, &self.projects)
    }

    /// The Refresh button: fetch again, past the freshness window.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_list(true, cx);
    }

    fn load_list(&mut self, force: bool, cx: &mut Context<Self>) {
        let query = self.fetch_query();
        let cached = self.client.peek_inbox_list(&self.projects, &query);
        if let Some(cached) = &cached {
            self.items = cached.items.clone();
            self.provider_errors = cached.errors.clone();
            self.loading = false;
            self.after_items_changed(cx);
        }
        let fresh = self
            .client
            .inbox_list_is_fresh(&self.projects, &query, self.client.now());
        if !force && cached.is_some() && fresh {
            cx.notify();
            return;
        }
        self.load_generation += 1;
        let generation = self.load_generation;
        if cached.is_some() {
            self.revalidating = true;
        } else {
            self.loading = true;
            self.provider_errors = InboxProviderErrors::new();
        }
        cx.notify();
        let pending = self.client.list_inbox_items(&self.projects, &query, force);
        let had_cache = cached.is_some();
        cx.spawn(async move |this, cx| {
            let result = pending.await;
            let _ = this.update(cx, |this, cx| {
                if this.load_generation != generation {
                    return;
                }
                match result {
                    Ok(next) => {
                        this.items = next.items;
                        this.provider_errors = next.errors;
                        this.after_items_changed(cx);
                    }
                    Err(message) if !had_cache => {
                        this.items = Vec::new();
                        this.provider_errors = InboxProviderErrors::all(&message);
                        this.after_items_changed(cx);
                    }
                    Err(_) => {}
                }
                this.loading = false;
                this.revalidating = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// The effects keyed on `items`: share the fetched revisions, and look up
    /// a linked target the list omits.
    fn after_items_changed(&mut self, cx: &mut Context<Self>) {
        self.client.remember_inbox_items(
            &self
                .items
                .iter()
                .map(|item| RememberedInboxItem {
                    key: inbox_item_key(item),
                    updated_at: item.updated_at.clone(),
                    project_path: item.project_path.clone(),
                })
                .collect::<Vec<_>>(),
        );
        self.lookup_target(cx);
    }

    fn lookup_target(&mut self, cx: &mut Context<Self>) {
        self.target_generation += 1;
        let Some(target) = self.target.clone() else {
            return;
        };
        if self
            .items
            .iter()
            .any(|item| inbox_item_matches_linked_work_item(item, &target))
        {
            return;
        }
        let generation = self.target_generation;
        let lookup = self.client.github_work_item(
            &self.cwd,
            &target.repo,
            target.kind,
            target.number,
            false,
        );
        let cwd = self.cwd.clone();
        cx.spawn(async move |this, cx| {
            // The Inbox stays usable when the exact lookup fails.
            if let Ok(item) = lookup.await {
                let _ = this.update(cx, |this, cx| {
                    if this.target_generation == generation {
                        this.target_item =
                            Some(InboxItem::from_github(&item, &cwd, InboxProvider::Github));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Ask every provider whether it is connected. Only the newest read may
    /// write, and a failed check keeps the last answer.
    pub fn read_connections(&mut self, cx: &mut Context<Self>) {
        self.connection_generation += 1;
        let generation = self.connection_generation;
        let client = self.client.clone();
        let github = client.github_status();
        let linear = client.linear_connected();
        let jira = client.jira_connected();
        let gitlab = client.gitlab_connected();
        let azure = client.azure_dev_ops_connected();
        cx.spawn(async move |this, cx| {
            let (github, linear, jira, gitlab, azure) =
                futures::join!(github, linear, jira, gitlab, azure);
            let _ = this.update(cx, |this, cx| {
                if this.connection_generation != generation {
                    return;
                }
                let mut next = this.connections;
                if let Ok(status) = github {
                    next.github = Some(status.connected);
                }
                if let Ok(status) = linear {
                    next.linear = Some(status.connected);
                }
                if let Ok(status) = jira {
                    next.jira = Some(status.connected);
                }
                if let Ok(status) = gitlab {
                    next.gitlab = Some(status.connected);
                }
                if let Ok(status) = azure {
                    next.azuredevops = Some(status.connected);
                }
                this.connections = next;
                save_inbox_connections(this.client.kv(), &next);
                // Disconnecting can pull the tab out from under the selection.
                let resolved = resolve_inbox_source(this.source, &next);
                if resolved != this.source {
                    this.source = resolved;
                    save_inbox_source(this.client.kv(), resolved);
                    this.load_rosters(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The Linear team or Jira project roster for the open tab. It comes
    /// from the tracker, not the fetched issues: hiding a team drops its
    /// issues, so a derived list could never offer it back.
    fn load_rosters(&mut self, cx: &mut Context<Self>) {
        self.roster_generation += 1;
        let generation = self.roster_generation;
        match self.source {
            InboxProvider::Linear => {
                let teams = self.client.list_linear_teams();
                cx.spawn(async move |this, cx| {
                    let teams = teams.await.unwrap_or_default();
                    let _ = this.update(cx, |this, cx| {
                        if this.roster_generation == generation {
                            this.linear_teams = teams;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            InboxProvider::Jira => {
                let projects = self.client.list_jira_projects();
                cx.spawn(async move |this, cx| {
                    let projects = projects.await.unwrap_or_default();
                    let _ = this.update(cx, |this, cx| {
                        if this.roster_generation == generation {
                            this.jira_projects = projects;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            _ => {}
        }
    }

    // What the view reads.

    pub fn items(&self) -> &[InboxItem] {
        &self.items
    }

    /// The first load is running and nothing is cached.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// A cached list is on screen while a fresh one loads.
    pub fn revalidating(&self) -> bool {
        self.revalidating
    }

    pub fn provider_errors(&self) -> &InboxProviderErrors {
        &self.provider_errors
    }

    /// The open tab's error.
    pub fn source_error(&self) -> Option<&str> {
        self.provider_errors.get(self.source)
    }

    pub fn read_status_error(&self) -> Option<&str> {
        self.read_status_error.as_deref()
    }

    pub fn filters(&self) -> &InboxFilters {
        &self.filters
    }

    pub fn connections(&self) -> InboxSourceConnections {
        self.connections
    }

    pub fn source(&self) -> InboxSource {
        self.source
    }

    pub fn projects(&self) -> &[String] {
        &self.projects
    }

    pub fn target(&self) -> Option<&LinkedWorkItem> {
        self.target.as_ref()
    }

    /// The selection key the view waits for while the target loads.
    pub fn target_selection_key(&self) -> Option<String> {
        self.target.as_ref().map(linked_work_item_inbox_key)
    }

    pub fn linear_hidden_team_ids(&self) -> &[String] {
        &self.linear_hidden_team_ids
    }

    pub fn linear_teams(&self) -> &[LinearTeam] {
        &self.linear_teams
    }

    pub fn jira_hidden_project_ids(&self) -> &[String] {
        &self.jira_hidden_project_ids
    }

    pub fn jira_projects(&self) -> &[JiraProject] {
        &self.jira_projects
    }

    /// `linearProjectOptions(items)`.
    pub fn linear_project_options(&self) -> Vec<LinearProjectOption> {
        linear_project_options(&self.items)
    }

    pub fn visible_sources(&self) -> Vec<InboxSource> {
        visible_inbox_sources(&self.connections)
    }

    pub fn connectable_sources(&self) -> Vec<InboxSource> {
        connectable_inbox_sources(&self.connections)
    }

    pub fn source_available(&self) -> bool {
        self.visible_sources().contains(&self.source)
    }

    pub fn no_sources_connected(&self) -> bool {
        self.visible_sources().is_empty()
    }

    /// `hasActiveInboxFilters` for the open tab.
    pub fn filters_active(&self) -> bool {
        has_active_inbox_filters(
            &self.active_filters(),
            Some(self.source),
            &self.linear_hidden_team_ids,
            &self.jira_hidden_project_ids,
        )
    }

    /// The open tab's cards after filters and `search`, with a linked target
    /// pinned first on the GitHub tab.
    pub fn visible_items(&self, search: &str) -> Vec<InboxItem> {
        if !self.source_available() {
            return Vec::new();
        }
        let visible = apply_inbox_filters(
            &self.items,
            &self.active_filters(),
            search,
            self.client.now(),
            Some(self.source),
        );
        let Some(target) = self
            .target
            .as_ref()
            .filter(|_| self.source == InboxProvider::Github)
        else {
            return visible;
        };
        let targeted = self
            .items
            .iter()
            .find(|item| inbox_item_matches_linked_work_item(item, target))
            .or_else(|| {
                self.target_item
                    .as_ref()
                    .filter(|item| inbox_item_matches_linked_work_item(item, target))
            });
        match targeted {
            Some(targeted) if !visible.contains(targeted) => {
                std::iter::once(targeted.clone()).chain(visible).collect()
            }
            _ => visible,
        }
    }

    fn source_entries(&self) -> Vec<InboxSeenEntry> {
        if !self.source_available() {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|item| item.provider == self.source)
            .map(|item| InboxSeenEntry::new(inbox_item_key(item), item.updated_at.clone()))
            .collect()
    }

    /// Whether the open tab has an unread card ("Mark all as read" enabled).
    pub fn source_has_unseen(&self) -> bool {
        self.source_entries()
            .iter()
            .any(|entry| self.client.is_inbox_entry_unseen(entry))
    }

    // What the view changes.

    /// "Mark all as read" for the open tab.
    pub fn mark_source_read(&mut self, cx: &mut Context<Self>) {
        self.read_status_error = if self.client.mark_inbox_items_seen(&self.source_entries()) {
            None
        } else {
            Some("Could not save read status. Please try again.".into())
        };
        cx.notify();
    }

    /// Save new filters, dropping projects that left the rail.
    pub fn set_filters(&mut self, next: InboxFilters, cx: &mut Context<Self>) {
        let pruned = prune_inbox_filters(&next, &self.projects);
        save_inbox_filters(self.client.kv(), &pruned);
        let refetch = inbox_fetch_state(&pruned) != inbox_fetch_state(&self.active_filters())
            || pruned.assigned_to_me != self.filters.assigned_to_me;
        self.filters = pruned;
        if refetch {
            self.load_list(false, cx);
        }
        cx.notify();
    }

    /// Switch tabs and remember the choice.
    pub fn set_source(&mut self, next: InboxSource, cx: &mut Context<Self>) {
        self.source = next;
        save_inbox_source(self.client.kv(), next);
        self.load_rosters(cx);
        cx.notify();
    }

    /// Hide Linear teams (shared with Settings), then refetch.
    pub fn set_linear_hidden_team_ids(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.client.save_hidden_linear_team_ids(&ids);
        self.linear_hidden_team_ids = ids;
        self.load_rosters(cx);
        self.load_list(false, cx);
    }

    /// Hide Jira projects (shared with Settings), then refetch.
    pub fn set_jira_hidden_project_ids(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        self.client.save_hidden_jira_project_ids(&ids);
        self.jira_hidden_project_ids = ids;
        self.load_rosters(cx);
        self.load_list(false, cx);
    }

    /// `updateInboxItem`: a card changed in place (a PR action).
    pub fn update_item(&mut self, next: InboxItem, cx: &mut Context<Self>) {
        let key = inbox_item_key(&next);
        for item in &mut self.items {
            if inbox_item_key(item) == key {
                *item = next.clone();
            }
        }
        if let Some(target) = self.target_item.as_mut()
            && inbox_item_key(target) == key
        {
            *target = next;
        }
        cx.notify();
    }
}
