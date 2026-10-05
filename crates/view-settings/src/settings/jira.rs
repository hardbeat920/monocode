//! Port of src/features/settings/ui/JiraSettings.tsx.

use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    Window, div,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_settings::Subscription as KvSubscription;
use monocode_settings::display_prefs::{MASK_EMAILS_KEY, load_mask_emails};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::controls::{plain_input, secondary_button, watch_keys};
use super::host::{InboxHost, JiraProject, JiraStatus};
use super::private_email::private_email;
use super::section::SectionContext;
use super::store::{self, JIRA_HIDDEN_PROJECTS_KEY};

pub const JIRA_API_TOKENS_URL: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";

pub struct JiraSettings {
    ctx: SectionContext,
    host: Rc<dyn InboxHost>,
    status: Option<JiraStatus>,
    site: Entity<InputState>,
    email: Entity<InputState>,
    token: Entity<InputState>,
    checking: bool,
    busy: bool,
    error: Option<String>,
    projects: Vec<JiraProject>,
    hidden_ids: Vec<String>,
    job: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _watch: (Vec<KvSubscription>, Task<()>),
}

impl JiraSettings {
    pub fn new(ctx: SectionContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = |placeholder: &'static str,
                     masked: bool,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked)
            })
        };
        let site = field("yourteam.atlassian.net", false, window, cx);
        let email = field("you@example.com", false, window, cx);
        let token = field("API token", true, window, cx);
        let mut subscriptions = Vec::new();
        for input in [&site, &email, &token] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.connect(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }),
            );
        }
        // `JIRA_CHANGE_EVENT`: the inbox filter menu edits the same list.
        // The email follows the masking setting from any window.
        let watch = watch_keys(
            &ctx.kv,
            &[JIRA_HIDDEN_PROJECTS_KEY, MASK_EMAILS_KEY],
            |this: &mut Self, cx| {
                this.hidden_ids = store::load_hidden_ids(&this.ctx.kv, JIRA_HIDDEN_PROJECTS_KEY);
                cx.notify();
            },
            cx,
        );
        let host = ctx.hosts.inbox.clone();
        let mut this = Self {
            hidden_ids: store::load_hidden_ids(&ctx.kv, JIRA_HIDDEN_PROJECTS_KEY),
            ctx,
            host,
            status: None,
            site,
            email,
            token,
            checking: true,
            busy: false,
            error: None,
            projects: Vec::new(),
            job: None,
            _subscriptions: subscriptions,
            _watch: watch,
        };
        let status = this.host.jira_status(cx);
        this.job = Some(cx.spawn(async move |this, cx| {
            let result = status.await;
            let connected = this
                .update(cx, |this, cx| {
                    let connected = match result {
                        Ok(status) => {
                            let connected = status.connected;
                            this.status = Some(status);
                            connected
                        }
                        Err(error) => {
                            this.error = Some(error);
                            false
                        }
                    };
                    cx.notify();
                    connected
                })
                .unwrap_or(false);
            if connected {
                let Ok(load) = this.update(cx, |this, cx| this.load_projects(cx)) else {
                    return;
                };
                load.await;
            }
            this.update(cx, |this, cx| {
                this.checking = false;
                cx.notify();
            })
            .ok();
        }));
        this
    }

    pub fn status(&self) -> Option<&JiraStatus> {
        self.status.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn projects(&self) -> &[JiraProject] {
        &self.projects
    }

    pub fn hidden_ids(&self) -> &[String] {
        &self.hidden_ids
    }

    pub fn fields(&self) -> [&Entity<InputState>; 3] {
        [&self.site, &self.email, &self.token]
    }

    fn connected(&self) -> bool {
        self.status.as_ref().is_some_and(|status| status.connected)
    }

    /// `loadProjects`.
    fn load_projects(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let list = self.host.list_jira_projects(cx);
        cx.spawn(async move |this, cx| {
            let result = list.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(projects) => this.projects = projects,
                    Err(error) => {
                        this.projects.clear();
                        this.error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
    }

    fn value(input: &Entity<InputState>, cx: &gpui::App) -> String {
        input.read(cx).value().to_string()
    }

    /// `connect`.
    pub fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (site, email, token) = (
            Self::value(&self.site, cx),
            Self::value(&self.email, cx),
            Self::value(&self.token, cx),
        );
        if self.busy
            || self.checking
            || site.trim().is_empty()
            || email.trim().is_empty()
            || token.trim().is_empty()
        {
            return;
        }
        self.busy = true;
        self.error = None;
        let saved = self.host.save_jira_config(&site, &email, &token, cx);
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = saved.await;
            let next = this.update_in(cx, |this, window, cx| match result {
                Ok(status) => {
                    this.status = Some(status);
                    this.token
                        .update(cx, |token, cx| token.set_value("", window, cx));
                    this.host.clear_inbox_cache(cx);
                    store::save_hidden_ids(&this.ctx.kv, JIRA_HIDDEN_PROJECTS_KEY, &[]);
                    this.host.notify_jira_change(cx);
                    Some(this.load_projects(cx))
                }
                Err(error) => {
                    this.error = Some(error);
                    None
                }
            });
            if let Ok(Some(load)) = next {
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

    /// `disconnect`.
    pub fn disconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let saved = self.host.save_jira_config("", "", "", cx);
        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let result = saved.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(status) => {
                        this.status = Some(status);
                        this.projects.clear();
                        this.token
                            .update(cx, |token, cx| token.set_value("", window, cx));
                        this.host.clear_inbox_cache(cx);
                        this.host.notify_jira_change(cx);
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

    pub fn toggle_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut next = self.hidden_ids.clone();
        if next.iter().any(|hidden| hidden == id) {
            next.retain(|hidden| hidden != id);
        } else {
            next.push(id.to_string());
        }
        self.host.clear_inbox_cache(cx);
        store::save_hidden_ids(&self.ctx.kv, JIRA_HIDDEN_PROJECTS_KEY, &next);
        self.host.notify_jira_change(cx);
        self.hidden_ids = next;
        cx.notify();
    }

    pub fn refresh_projects(&mut self, cx: &mut Context<Self>) {
        let load = self.load_projects(cx);
        self.job = Some(cx.spawn(async move |_, _| load.await));
    }

    fn field(
        &self,
        label: &'static str,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let field = plain_input(input, cx);
        div()
            .flex()
            .flex_col()
            .gap(u(4.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.65))
            .child(label)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(u(32.))
                    .w_full()
                    .px(u(8.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .text_color(theme.colors.content)
                    .debug_selector(move || format!("jira-field:{label}"))
                    .child(div().flex_1().min_w_0().child(field)),
            )
            .into_any_element()
    }
}

impl Render for JiraSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut body = div().px(u(16.)).py(u(14.));
        if self.checking {
            body = body.child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.45))
                    .child("Checking Jira connection…"),
            );
        } else if let Some(status) = self.status.clone().filter(|status| status.connected) {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .child(
                        div()
                            .min_w_0()
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.65))
                            .child(div().child(status.site.clone()))
                            .child(div().flex().min_w_0().child(private_email(
                                "jira-email",
                                status.email.clone(),
                                load_mask_emails(&self.ctx.kv),
                            ))),
                    )
                    .child(
                        secondary_button(
                            "jira-disconnect",
                            if self.busy {
                                "Disconnecting"
                            } else {
                                "Disconnect"
                            },
                        )
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.disconnect(window, cx))),
                    ),
            );
        } else {
            let (site, email, token) = (
                Self::value(&self.site, cx),
                Self::value(&self.email, cx),
                Self::value(&self.token, cx),
            );
            let incomplete =
                site.trim().is_empty() || email.trim().is_empty() || token.trim().is_empty();
            let link_hover = theme.colors.content;
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(12.))
                    .debug_selector(|| "jira-form".into())
                    .child(
                        div()
                            .text_px(theme.text.label)
                            .leading(theme.leading.relaxed)
                            .text_color(theme.content(0.45))
                            .child("Connect your Jira Cloud site using your Atlassian email and an API token without scopes. Disconnect deletes the saved credentials."),
                    )
                    .child(self.field("Jira site", &self.site.clone(), cx))
                    .child(self.field("Atlassian email", &self.email.clone(), cx))
                    .child(self.field("Jira API token", &self.token.clone(), cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(12.))
                            .child(
                                secondary_button(
                                    "jira-connect",
                                    if self.busy { "Connecting" } else { "Connect" },
                                )
                                .disabled(self.busy || incomplete)
                                .on_click(cx.listener(|this, _, window, cx| this.connect(window, cx))),
                            )
                            .child(
                                div()
                                    .id("jira-create-token")
                                    .text_px(theme.text.label)
                                    .text_color(theme.content(0.65))
                                    .hover(move |s| s.text_color(link_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.host.open_url(JIRA_API_TOKENS_URL, cx)
                                    }))
                                    .child("Create API token"),
                            ),
                    ),
            );
        }
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .mt(u(12.))
                    .text_px(theme.text.label)
                    .text_color(gpui::Hsla {
                        a: 0.9,
                        ..theme.colors.danger
                    })
                    .debug_selector(|| "jira-alert".into())
                    .child(error),
            );
        }
        if self.connected() {
            let mut list = div()
                .mt(u(16.))
                .flex()
                .flex_col()
                .gap(u(8.))
                .debug_selector(|| "jira-projects".into())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_px(theme.text.body)
                                .medium()
                                .text_color(theme.colors.content)
                                .child("Projects"),
                        )
                        .child(
                            secondary_button("jira-refresh-projects", "Refresh projects")
                                .disabled(self.busy || self.checking)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_projects(cx))),
                        ),
                )
                .child(
                    div()
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.45))
                        .child("Unchecked projects stay out of the inbox."),
                );
            for project in self.projects.clone() {
                let checked = !self.hidden_ids.contains(&project.id);
                let id = project.id.clone();
                let name = project.name.clone();
                let mut row = div()
                    .id(gpui::ElementId::from(gpui::SharedString::from(format!(
                        "jira-project-{}",
                        project.id
                    ))))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .text_px(theme.text.body)
                    .text_color(theme.colors.content)
                    .debug_selector(move || format!("jira-project:{name}"))
                    .child(checkbox(checked, &theme))
                    .child(project.name.clone())
                    .child(
                        div()
                            .text_color(theme.content(0.40))
                            .child(project.key.clone()),
                    );
                if self.busy {
                    row = row.opacity(0.5);
                } else {
                    row = row
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(&id, cx)));
                }
                list = list.child(row);
            }
            body = body.child(list);
        }
        body
    }
}

/// A 14px checkbox in the accent color.
pub fn checkbox(checked: bool, theme: &Theme) -> AnyElement {
    let mut box_ = div()
        .flex()
        .flex_none()
        .size(u(14.))
        .items_center()
        .justify_center()
        .rounded(u(3.))
        .border_1();
    box_ = if checked {
        box_.bg(theme.colors.accent)
            .border_color(theme.colors.accent)
            .child(icon(IconName::Check).size(u(10.)).text_color(gpui::white()))
    } else {
        box_.border_color(theme.content(0.30))
    };
    box_.into_any_element()
}
