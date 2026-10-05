//! Port of `InboxPage`, `GithubSettings`, `GitlabSettings`,
//! `AzureDevOpsSettings`, and `LinearSettings` in SettingsView.tsx. The
//! project notification card at the top is a slot.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_settings::Subscription as KvSubscription;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::chrome::{group, row, row_error};
use super::controls::{secondary_button, text_field, watch_keys};
use super::host::{GithubStatus, InboxHost, LinearTeam, SlotContext};
use super::jira::JiraSettings;
use super::private_email::{InboxProvider, inbox_provider_mark};
use super::section::SectionContext;
use super::store::{self, LINEAR_HIDDEN_TEAMS_KEY};

pub const GITHUB_CLI_URL: &str = "https://cli.github.com/";

/// `GithubSettings`.
pub struct GithubSettings {
    host: Rc<dyn InboxHost>,
    status: Option<GithubStatus>,
    checking: bool,
    error: Option<String>,
    request: u64,
    job: Option<Task<()>>,
}

impl GithubSettings {
    pub fn new(host: Rc<dyn InboxHost>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            host,
            status: None,
            checking: true,
            error: None,
            request: 0,
            job: None,
        };
        this.check_status(cx);
        this
    }

    /// `checkStatus`: a newer check drops an older one's result.
    pub fn check_status(&mut self, cx: &mut Context<Self>) {
        self.request += 1;
        let generation = self.request;
        self.checking = true;
        self.error = None;
        let status = self.host.github_status(cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = status.await;
            this.update(cx, |this, cx| {
                if generation != this.request {
                    return;
                }
                match result {
                    Ok(status) => this.status = Some(status),
                    Err(error) => this.error = Some(error),
                }
                this.checking = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The status label beside the buttons.
    pub fn label(&self) -> &'static str {
        let status = self.status.unwrap_or_default();
        if self.checking {
            "Checking"
        } else if status.connected {
            "Connected"
        } else if status.installed {
            "Sign in required"
        } else {
            "Not installed"
        }
    }

    pub fn description(&self) -> &'static str {
        let status = self.status.unwrap_or_default();
        if status.connected {
            "GitHub CLI is installed and authenticated. MonoCode uses it for GitHub inbox items."
        } else if status.installed {
            "Run gh auth login in a terminal, complete the sign-in flow, then check again."
        } else {
            "Install GitHub CLI from cli.github.com, run gh auth login in a terminal, then check again."
        }
    }
}

impl Render for GithubSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let reveal = super::chrome::Reveal::default();
        let installed = self.status.is_some_and(|status| status.installed);
        let mut connection = row(&reveal, "Connection")
            .selector("row:github-connection")
            .description(self.description())
            .child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(self.label()),
            );
        if !self.checking && !installed {
            connection = connection.child(
                secondary_button("github-install-guide", "Installation guide")
                    .on_click(cx.listener(|this, _, _, cx| this.host.open_url(GITHUB_CLI_URL, cx))),
            );
        }
        connection = connection.child(
            secondary_button(
                "github-check",
                if self.checking {
                    "Checking"
                } else {
                    "Check again"
                },
            )
            .disabled(self.checking)
            .on_click(cx.listener(|this, _, _, cx| this.check_status(cx))),
        );
        let mut el = div().flex().flex_col().child(connection);
        if let Some(error) = self.error.clone() {
            el = el.child(row_error(error, cx));
        }
        el
    }
}

/// GitLab and Azure DevOps: a URL and a personal access token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlIntegration {
    Gitlab,
    AzureDevOps,
}

impl UrlIntegration {
    fn default_url(self) -> &'static str {
        match self {
            Self::Gitlab => "https://gitlab.com",
            Self::AzureDevOps => "https://dev.azure.com/myorg",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Gitlab => {
                "Connect GitLab.com or a self-managed GitLab instance. Use a personal access token with API access; the token is stored locally and Disconnect deletes it."
            }
            Self::AzureDevOps => {
                "Connect your ADO organization with a personal access token (Boards + Repos read & write for comments). The token is stored locally and Disconnect deletes it."
            }
        }
    }

    fn token_placeholder(self) -> &'static str {
        match self {
            Self::Gitlab => "glpat-…",
            Self::AzureDevOps => "PAT…",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Gitlab => "gitlab",
            Self::AzureDevOps => "azuredevops",
        }
    }
}

/// `GitlabSettings` and `AzureDevOpsSettings`.
pub struct UrlSettings {
    kind: UrlIntegration,
    host: Rc<dyn InboxHost>,
    url: Entity<InputState>,
    token: Entity<InputState>,
    connected: bool,
    busy: bool,
    error: Option<String>,
    job: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl UrlSettings {
    pub fn new(
        kind: UrlIntegration,
        host: Rc<dyn InboxHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(kind.default_url())
                .default_value(kind.default_url())
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(kind.token_placeholder())
                .masked(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(&token, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.save(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        let mut this = Self {
            kind,
            host,
            url,
            token,
            connected: false,
            busy: false,
            error: None,
            job: None,
            _subscriptions: subscriptions,
        };
        let status = match kind {
            UrlIntegration::Gitlab => this.host.gitlab_status(cx),
            UrlIntegration::AzureDevOps => this.host.azure_devops_status(cx),
        };
        this.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = status.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(status) => {
                        this.connected = status.connected;
                        // ADO keeps its placeholder org when none is stored.
                        if this.kind == UrlIntegration::Gitlab || !status.url.is_empty() {
                            this.url
                                .update(cx, |url, cx| url.set_value(status.url, window, cx));
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
        this
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn url_input(&self) -> &Entity<InputState> {
        &self.url
    }

    pub fn token_input(&self) -> &Entity<InputState> {
        &self.token
    }

    /// `onSave`.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token.read(cx).value().to_string();
        if token.trim().is_empty() || self.busy {
            return;
        }
        let url = self.url.read(cx).value().to_string();
        self.busy = true;
        self.error = None;
        let saved = match self.kind {
            UrlIntegration::Gitlab => self.host.save_gitlab(&url, &token, cx),
            UrlIntegration::AzureDevOps => self.host.save_azure_devops(&url, &token, cx),
        };
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = saved.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(status) => {
                        this.url
                            .update(cx, |url, cx| url.set_value(status.url, window, cx));
                        this.token
                            .update(cx, |token, cx| token.set_value("", window, cx));
                        this.connected = status.connected;
                        this.host.clear_inbox_cache(cx);
                    }
                    Err(error) => {
                        this.connected = false;
                        this.error = Some(error);
                    }
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `onDisconnect`.
    pub fn disconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let url = self.url.read(cx).value().to_string();
        self.busy = true;
        self.error = None;
        let done = match self.kind {
            UrlIntegration::Gitlab => self.host.disconnect_gitlab(&url, cx),
            UrlIntegration::AzureDevOps => self.host.disconnect_azure_devops(&url, cx),
        };
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = done.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(status) => {
                        this.connected = false;
                        let next =
                            if status.url.is_empty() && this.kind == UrlIntegration::AzureDevOps {
                                url
                            } else {
                                status.url
                            };
                        this.url
                            .update(cx, |input, cx| input.set_value(next, window, cx));
                        this.host.clear_inbox_cache(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for UrlSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let reveal = super::chrome::Reveal::default();
        let id = self.kind.id();
        let controls: AnyElement = if self.connected {
            let url = self.url.read(cx).value().to_string();
            div()
                .flex()
                .flex_wrap()
                .min_w_0()
                .max_w_full()
                .items_center()
                .gap(u(8.))
                .child(
                    div()
                        .max_w(u(224.))
                        .truncate()
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.50))
                        .child(url),
                )
                .child(
                    secondary_button(format!("{id}-disconnect"), "Disconnect")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.disconnect(window, cx))),
                )
                .into_any_element()
        } else {
            let empty = self.token.read(cx).value().trim().is_empty();
            let (url_selector, token_selector) = match self.kind {
                UrlIntegration::Gitlab => ("field:GitLab URL", "field:GitLab access token"),
                UrlIntegration::AzureDevOps => (
                    "field:Azure DevOps organization URL",
                    "field:Azure DevOps personal access token",
                ),
            };
            div()
                .flex()
                .flex_wrap()
                .min_w_0()
                .max_w_full()
                .items_center()
                .gap(u(8.))
                .child(text_field(url_selector, &self.url, None, window, cx))
                .child(text_field(token_selector, &self.token, None, window, cx))
                .child(
                    secondary_button(
                        format!("{id}-connect"),
                        if self.busy { "Saving" } else { "Connect" },
                    )
                    .disabled(self.busy || empty)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                )
                .into_any_element()
        };
        let mut el = div().flex().flex_col().child(
            row(&reveal, "Connection")
                .selector(format!("row:{id}-connection"))
                .description(self.kind.description())
                .child(controls),
        );
        if let Some(error) = self.error.clone() {
            el = el.child(row_error(error, cx));
        }
        el
    }
}

/// `LinearSettings`.
pub struct LinearSettings {
    ctx: SectionContext,
    host: Rc<dyn InboxHost>,
    token: Entity<InputState>,
    connected: bool,
    busy: bool,
    error: Option<String>,
    teams: Vec<LinearTeam>,
    hidden_team_ids: Vec<String>,
    job: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _watch: (Vec<KvSubscription>, Task<()>),
}

impl LinearSettings {
    pub fn new(ctx: SectionContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("lin_api_…")
                .masked(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(&token, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.save(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        // The inbox filter menu writes the same list, so follow it.
        let watch = watch_keys(
            &ctx.kv,
            &[LINEAR_HIDDEN_TEAMS_KEY],
            |this: &mut Self, cx| {
                this.hidden_team_ids =
                    store::load_hidden_ids(&this.ctx.kv, LINEAR_HIDDEN_TEAMS_KEY);
                cx.notify();
            },
            cx,
        );
        let host = ctx.hosts.inbox.clone();
        let mut this = Self {
            hidden_team_ids: store::load_hidden_ids(&ctx.kv, LINEAR_HIDDEN_TEAMS_KEY),
            ctx,
            host,
            token,
            connected: false,
            busy: false,
            error: None,
            teams: Vec::new(),
            job: None,
            _subscriptions: subscriptions,
            _watch: watch,
        };
        let connected = this.host.linear_connected(cx);
        this.job = Some(cx.spawn(async move |this, cx| {
            let connected = connected.await.unwrap_or(false);
            let load = this
                .update(cx, |this, cx| {
                    this.connected = connected;
                    cx.notify();
                    connected.then(|| this.load_teams(cx))
                })
                .ok()
                .flatten();
            if let Some(load) = load {
                load.await;
            }
        }));
        this
    }

    pub fn teams(&self) -> &[LinearTeam] {
        &self.teams
    }

    pub fn hidden_team_ids(&self) -> &[String] {
        &self.hidden_team_ids
    }

    pub fn token_input(&self) -> &Entity<InputState> {
        &self.token
    }

    /// `loadTeams`.
    fn load_teams(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let teams = self.host.list_linear_teams(cx);
        cx.spawn(async move |this, cx| {
            let teams = teams.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.teams = teams;
                cx.notify();
            })
            .ok();
        })
    }

    /// `onSave`.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.token.read(cx).value().to_string();
        if token.trim().is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let saved = self.host.save_linear_token(&token, cx);
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = saved.await;
            let load = this
                .update_in(cx, |this, window, cx| match result {
                    Ok(()) => {
                        this.token
                            .update(cx, |token, cx| token.set_value("", window, cx));
                        this.connected = true;
                        this.host.clear_inbox_cache(cx);
                        this.host.notify_linear_change(cx);
                        Some(this.load_teams(cx))
                    }
                    Err(error) => {
                        this.connected = false;
                        this.error = Some(error);
                        None
                    }
                })
                .ok()
                .flatten();
            if let Some(load) = load {
                load.await;
            }
            this.update(cx, |this, cx| {
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `onDisconnect`.
    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let done = self.host.disconnect_linear(cx);
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = done.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.connected = false;
                        this.teams.clear();
                        this.host.clear_inbox_cache(cx);
                        this.host.notify_linear_change(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `toggleTeam`.
    pub fn toggle_team(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut ids = self.hidden_team_ids.clone();
        if ids.iter().any(|hidden| hidden == id) {
            ids.retain(|hidden| hidden != id);
        } else {
            ids.push(id.to_string());
        }
        self.hidden_team_ids = ids.clone();
        store::save_hidden_ids(&self.ctx.kv, LINEAR_HIDDEN_TEAMS_KEY, &ids);
        self.host.notify_linear_change(cx);
        self.host.clear_inbox_cache(cx);
        cx.notify();
    }
}

impl Render for LinearSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let reveal = super::chrome::Reveal::default();
        let controls: AnyElement = if self.connected {
            secondary_button("linear-disconnect", "Disconnect")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx)))
                .into_any_element()
        } else {
            let empty = self.token.read(cx).value().trim().is_empty();
            div()
                .flex()
                .flex_wrap()
                .max_w_full()
                .items_center()
                .gap(u(8.))
                .child(text_field(
                    "field:Linear API key",
                    &self.token,
                    None,
                    window,
                    cx,
                ))
                .child(
                    secondary_button(
                        "linear-connect",
                        if self.busy { "Saving" } else { "Connect" },
                    )
                    .disabled(self.busy || empty)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                )
                .into_any_element()
        };
        let mut el = div().flex().flex_col().child(
            row(&reveal, "API key")
                .selector("row:linear-api-key")
                .description("Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.")
                .child(controls),
        );
        if let Some(error) = self.error.clone() {
            el = el.child(row_error(error, cx));
        }
        if self.connected && !self.teams.is_empty() {
            let mut list = div().mx(u(-8.)).mt(u(8.)).flex().flex_col().gap(u(2.));
            for team in self.teams.clone() {
                let checked = !self.hidden_team_ids.contains(&team.id);
                let id = team.id.clone();
                let name = team.name.clone();
                let hover = theme.content(0.05);
                list =
                    list.child(
                        div()
                            .id(gpui::ElementId::from(SharedString::from(format!(
                                "linear-team-{}",
                                team.id
                            ))))
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .h(u(28.))
                            .px(u(8.))
                            .rounded(u(theme.radius.md))
                            .text_px(theme.text.body)
                            .text_color(theme.colors.content)
                            .hover(move |s| s.bg(hover))
                            .debug_selector(move || format!("linear-team:{name}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_team(&id, cx)))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .gap(u(6.))
                                    .child(team.name.clone())
                                    .children(team.key.clone().map(|key| {
                                        div().text_color(theme.content(0.40)).child(key)
                                    })),
                            )
                            .children(checked.then(|| {
                                icon(IconName::Check)
                                    .size(u(14.))
                                    .text_color(theme.colors.content)
                            })),
                    );
            }
            el = el.child(
                div()
                    .px(u(16.))
                    .py(u(14.))
                    .border_b_1()
                    .border_color(theme.content(0.05))
                    .child(
                        div()
                            .text_px(theme.text.body)
                            .medium()
                            .text_color(theme.colors.content)
                            .child("Teams"),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(theme.text.label)
                            .leading(theme.leading.relaxed)
                            .text_color(theme.content(0.45))
                            .child("Unchecked teams stay out of the inbox."),
                    )
                    .child(list),
            );
        }
        el
    }
}

/// `InboxPage`.
pub struct InboxSection {
    ctx: SectionContext,
    project_notifications: Option<AnyView>,
    github: Entity<GithubSettings>,
    gitlab: Entity<UrlSettings>,
    azure_devops: Entity<UrlSettings>,
    jira: Entity<JiraSettings>,
    linear: Entity<LinearSettings>,
}

impl InboxSection {
    pub fn new(
        ctx: SectionContext,
        slot: &SlotContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = ctx.hosts.inbox.clone();
        let project_notifications = ctx
            .hosts
            .project_notifications
            .clone()
            .map(|build| build(slot, window, cx));
        let github = cx.new(|cx| GithubSettings::new(host.clone(), cx));
        let gitlab =
            cx.new(|cx| UrlSettings::new(UrlIntegration::Gitlab, host.clone(), window, cx));
        let azure_devops =
            cx.new(|cx| UrlSettings::new(UrlIntegration::AzureDevOps, host.clone(), window, cx));
        let jira_ctx = ctx.clone();
        let jira = cx.new(|cx| JiraSettings::new(jira_ctx, window, cx));
        let linear_ctx = ctx.clone();
        let linear = cx.new(|cx| LinearSettings::new(linear_ctx, window, cx));
        Self {
            ctx,
            project_notifications,
            github,
            gitlab,
            azure_devops,
            jira,
            linear,
        }
    }

    pub fn github(&self) -> &Entity<GithubSettings> {
        &self.github
    }

    pub fn gitlab(&self) -> &Entity<UrlSettings> {
        &self.gitlab
    }

    pub fn azure_devops(&self) -> &Entity<UrlSettings> {
        &self.azure_devops
    }

    pub fn jira(&self) -> &Entity<JiraSettings> {
        &self.jira
    }

    pub fn linear(&self) -> &Entity<LinearSettings> {
        &self.linear
    }

    fn title(provider: InboxProvider, name: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(inbox_provider_mark(provider, cx))
            .child(name)
            .into_any_element()
    }
}

impl Render for InboxSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let reveal = self.ctx.reveal(cx);
        let id: SharedString = "project-notifications".into();
        let flash = reveal.is(&id);
        let notifications = div()
            .relative()
            .debug_selector(|| "setting-id:project-notifications".into())
            .child(reveal.anchors.probe(id))
            .child(match self.project_notifications.clone() {
                Some(view) => view.into_any_element(),
                None => {
                    // Without the notifications view, keep the card's place
                    // and its search highlight.
                    group(&reveal, "Project notifications")
                        .first(true)
                        .description("Mute a project's sounds, banners, and reminders, or pick the categories it notifies about.")
                        .child(
                            div()
                                .px(u(16.))
                                .py(u(14.))
                                .text_px(theme.text.label)
                                .text_color(theme.content(0.45))
                                .when_flash(flash, &theme)
                                .child("Project notification settings appear here."),
                        )
                        .into_any_element()
                }
            });
        let github_title = Self::title(InboxProvider::Github, "GitHub", cx);
        let gitlab_title = Self::title(InboxProvider::Gitlab, "GitLab", cx);
        let ado_title = Self::title(InboxProvider::AzureDevOps, "ADO", cx);
        let jira_title = Self::title(InboxProvider::Jira, "Jira", cx);
        let linear_title = Self::title(InboxProvider::Linear, "Linear", cx);
        div()
            .flex()
            .flex_col()
            .child(notifications)
            .child(
                group(&reveal, github_title)
                    .id("github")
                    .description("Pull requests, reviews, and issues, read through the GitHub CLI.")
                    .child(self.github.clone()),
            )
            .child(
                group(&reveal, gitlab_title)
                    .id("gitlab")
                    .description("Merge requests from GitLab.com or a self-managed instance.")
                    .child(self.gitlab.clone()),
            )
            .child(
                group(&reveal, ado_title)
                    .id("azuredevops")
                    .description("Pull requests and Boards work items from your ADO organization.")
                    .child(self.azure_devops.clone()),
            )
            .child(
                group(&reveal, jira_title)
                    .id("jira")
                    .description("Jira Cloud issues from the projects you pick.")
                    .child(self.jira.clone()),
            )
            .child(
                group(&reveal, linear_title)
                    .id("linear")
                    .description("Issues assigned to you, from the teams you pick.")
                    .child(self.linear.clone()),
            )
    }
}

/// The accent wash a revealed placeholder card shows.
trait WhenFlash {
    fn when_flash(self, flash: bool, theme: &Theme) -> Self;
}

impl WhenFlash for gpui::Div {
    fn when_flash(self, flash: bool, theme: &Theme) -> Self {
        if flash {
            self.bg(theme.accent(0.10))
                .debug_selector(|| "flash-row".into())
        } else {
            self
        }
    }
}
