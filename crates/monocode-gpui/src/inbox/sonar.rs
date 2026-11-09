//! Sonar connection, project selection and finding actions for the native inbox.
use super::*;
use gpui::{AnyElement, SharedString, div, prelude::*};
use model::sonar as api;

pub(super) enum Manage {
    Load,
    Organization(String),
    Watch(api::SonarProject),
    WatchMany {
        projects: Vec<api::SonarProject>,
        watch: bool,
    },
    Select(String),
}

/// Saved selections remain visible even when they are absent from the loaded page.
fn filtered_projects(
    loaded: &[api::SonarProject],
    watched: &[api::SonarProject],
    query: &str,
) -> Vec<api::SonarProject> {
    let query = query.trim().to_lowercase();
    let mut keys = std::collections::HashSet::new();
    loaded
        .iter()
        .chain(watched)
        .filter(|project| keys.insert(project.key.as_str()))
        .filter(|project| {
            format!("{} {}", project.name, project.key)
                .to_lowercase()
                .contains(&query)
        })
        .cloned()
        .collect()
}

fn button(id: impl Into<SharedString>, text: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    let text = text.into();
    div()
        .id(id.into())
        .role(gpui::Role::Button)
        .aria_label(text.clone())
        .px(px(8.))
        .py(px(5.))
        .rounded(px(5.))
        .text_size(px(12.))
        .text_color(content(0.8))
        .cursor_pointer()
        .hover(|s| s.bg(content(0.1)))
        .child(text)
}

impl Inbox {
    /// Invalidate before starting a switch, so neither list nor detail replies can restore old content.
    pub(super) fn invalidate_sonar(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.loading = false;
        self.refresh_queued = false;
        self.snapshot.items.retain(|item| item.provider != "sonar");
        self.snapshot.sonar_branches.clear();
        self.snapshot.errors.remove("sonar");
        if self.state.source == "sonar" {
            self.deselect(cx);
        }
    }

    pub(super) fn close_sonar_connection(&mut self, cx: &mut Context<Self>) {
        self.sonar_connect_current
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.sonar_connection_generation += 1;
        self.sonar_loading = None;
        self.connect_provider = None;
        self.sonar_edit_connection = false;
        self.connect_token
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        cx.notify();
    }

    fn apply_sonar_status(&mut self, status: api::SonarStatus, cx: &mut Context<Self>) {
        self.snapshot.sonar_selected_project = status.selected_project.clone();
        self.snapshot.sonar_projects = status.projects.clone();
        self.snapshot.connections.insert(
            "sonar".into(),
            model::Status {
                connected: status.connected,
                site: status.site.clone(),
                organization: status.organization.clone(),
                ..Default::default()
            },
        );
        self.sonar_organization_input.update(cx, |input, cx| {
            input.set_text(status.organization.clone(), cx)
        });
        self.sonar_status = Some(status);
    }

    pub(super) fn connect_sonar(&mut self, disconnect: bool, cx: &mut Context<Self>) {
        if self.sonar_loading.is_some() && !disconnect {
            return;
        }
        self.sonar_connect_current
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.sonar_connect_current = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let current = self.sonar_connect_current.clone();
        let token = self.connect_token.read(cx).content.to_string();
        if !disconnect && token.trim().is_empty() {
            self.connect_error = Some("Enter a Sonar personal access token".into());
            cx.notify();
            return;
        }
        self.sonar_connection_generation += 1;
        let generation = self.sonar_connection_generation;
        self.sonar_loading = Some(
            if disconnect {
                "Disconnecting Sonar…"
            } else {
                "Validating Sonar token…"
            }
            .into(),
        );
        self.connect_error = None;
        let dir = self.service.data_dir.clone();
        let site = self.connect_url.read(cx).content.to_string();
        let task = cx.background_executor().spawn(async move {
            if disconnect {
                api::sonar_disconnect(dir)
            } else {
                api::sonar_connect_if_current(dir, site, token, || {
                    current.load(std::sync::atomic::Ordering::SeqCst)
                })
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.sonar_connection_generation {
                    return;
                }
                this.sonar_loading = None;
                match result {
                    Ok(status) => {
                        this.invalidate_sonar(cx);
                        this.apply_sonar_status(status, cx);
                        this.sonar_edit_connection = false;
                        this.connect_token
                            .update(cx, |input, cx| input.set_text(String::new(), cx));
                        this.sonar_projects.clear();
                        this.sonar_organizations.clear();
                        this.sonar_project_page = 0;
                        this.sonar_projects_more = false;
                        if disconnect {
                            this.sonar_content = None;
                            this.close_sonar_connection(cx);
                        } else {
                            this.state.source = "sonar".into();
                            this.discover_sonar_organizations(generation, cx);
                        }
                        this.refresh(cx);
                    }
                    Err(error) => this.connect_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn manage_sonar(&mut self, operation: Manage, cx: &mut Context<Self>) {
        if self.sonar_loading.is_some() {
            return;
        }
        let discover = matches!(operation, Manage::Load | Manage::Organization(_));
        let organizations = matches!(operation, Manage::Load);
        let organization_changed = matches!(operation, Manage::Organization(_));
        let changing = !matches!(operation, Manage::Load);
        if changing {
            self.invalidate_sonar(cx);
        }
        self.sonar_connection_generation += 1;
        let generation = self.sonar_connection_generation;
        self.sonar_loading = Some(
            if changing {
                "Saving Sonar selection…"
            } else {
                "Loading Sonar connection…"
            }
            .into(),
        );
        self.connect_error = None;
        let dir = self.service.data_dir.clone();
        let task = cx.background_executor().spawn(async move {
            match operation {
                Manage::Load => api::sonar_status(dir),
                Manage::Organization(key) => api::sonar_set_organization(dir, key),
                Manage::Select(key) => api::sonar_select_project(dir, key),
                Manage::WatchMany { projects, watch } => {
                    api::sonar_watch_projects(dir, projects, watch)
                }
                Manage::Watch(project) => {
                    let mut projects = api::sonar_status(dir.clone())?.projects;
                    if projects.iter().any(|p| p.key == project.key) {
                        projects.retain(|p| p.key != project.key);
                    } else {
                        projects.push(project);
                    }
                    api::sonar_set_projects(dir, projects)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.sonar_connection_generation {
                    return;
                }
                this.sonar_loading = None;
                match result {
                    Ok(status) => {
                        let connected = status.connected;
                        this.apply_sonar_status(status, cx);
                        if organization_changed {
                            this.sonar_edit_connection = false;
                        }
                        if discover {
                            this.sonar_projects.clear();
                            this.sonar_project_page = 0;
                            this.sonar_projects_more = false;
                            this.sonar_project_search
                                .update(cx, |input, cx| input.set_text(String::new(), cx));
                            if connected {
                                if organizations {
                                    this.discover_sonar_organizations(generation, cx);
                                } else {
                                    this.load_sonar_project_page(generation, 1, cx);
                                }
                            }
                        }
                    }
                    Err(error) => this.connect_error = Some(error),
                }
                if changing {
                    this.invalidate_sonar(cx);
                    this.refresh(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn discover_sonar_organizations(&mut self, generation: u64, cx: &mut Context<Self>) {
        self.sonar_loading = Some("Loading your organizations…".into());
        let dir = self.service.data_dir.clone();
        let task = cx
            .background_executor()
            .spawn(async move { api::sonar_list_organizations(dir) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.complete_sonar_organizations(generation, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn complete_sonar_organizations(
        &mut self,
        generation: u64,
        result: Result<Vec<api::SonarOrganization>, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.sonar_connection_generation {
            return;
        }
        self.sonar_loading = None;
        match result {
            Ok(organizations) => self.sonar_organizations = organizations,
            Err(error) => {
                self.sonar_organizations.clear();
                self.connect_error = Some(format!(
                    "Could not load organizations: {error}. Enter your organization key below, or retry."
                ));
                cx.notify();
                return;
            }
        }
        // No implicit global search, even when discovery succeeds with no memberships.
        if self.sonar_status.as_ref().is_some_and(|status| {
            !api::is_cloud_site(&status.site) || !status.organization.is_empty()
        }) {
            self.load_sonar_project_page(generation, 1, cx);
        }
        cx.notify();
    }

    fn load_sonar_project_page(&mut self, generation: u64, page: u32, cx: &mut Context<Self>) {
        self.sonar_loading = Some(format!("Loading projects · page {page}…"));
        let dir = self.service.data_dir.clone();
        let task = cx
            .background_executor()
            .spawn(async move { api::sonar_project_page(dir, page) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.complete_sonar_project_page(generation, page, result, cx)
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn complete_sonar_project_page(
        &mut self,
        generation: u64,
        page: u32,
        result: Result<api::SonarProjectPage, String>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.sonar_connection_generation {
            return;
        }
        self.sonar_loading = None;
        match result {
            Ok(result) => {
                if page == 1 {
                    self.sonar_projects.clear();
                }
                for project in result.projects {
                    if !self.sonar_projects.iter().any(|old| old.key == project.key) {
                        self.sonar_projects.push(project);
                    }
                }
                self.sonar_project_page = page;
                self.sonar_projects_more = result.has_more;
            }
            Err(error) => self.connect_error = Some(format!("Could not load projects: {error}")),
        }
        cx.notify();
    }

    pub(super) fn sonar_connection_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(status) = &self.sonar_status else {
            return div().into_any_element();
        };
        let mut rows = vec![];
        let busy = self.sonar_loading.is_some();
        let editing = self.sonar_edit_connection
            || (api::is_cloud_site(&status.site) && status.organization.is_empty());
        if editing && (api::is_cloud_site(&status.site) || !self.sonar_organizations.is_empty()) {
            rows.push(
                div()
                    .flex_shrink_0()
                    .text_color(content(0.5))
                    .child("Organization")
                    .into_any_element(),
            );
            if !self.sonar_organizations.is_empty() {
                rows.push(
                    div()
                        .id("sonar-organizations")
                        .max_h(px(84.))
                        .flex_shrink_0()
                        .overflow_y_scroll()
                        .children(self.sonar_organizations.iter().enumerate().map(
                            |(ix, organization)| {
                                let key = organization.key.clone();
                                button(
                                    format!("sonar-organization-{ix}"),
                                    format!(
                                        "{} {}",
                                        if status.organization == key {
                                            "✓"
                                        } else {
                                            "○"
                                        },
                                        organization.name
                                    ),
                                )
                                .w_full()
                                .min_w_0()
                                .truncate()
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.manage_sonar(Manage::Organization(key.clone()), cx)
                                    },
                                ))
                            },
                        ))
                        .into_any_element(),
                );
            }
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_1()
                            .border_color(content(0.1))
                            .rounded(px(6.))
                            .p(px(8.))
                            .child(self.sonar_organization_input.clone()),
                    )
                    .child(
                        button("sonar-use-organization", "Use key").on_click(cx.listener(
                            |this, _, _, cx| {
                                let key = this
                                    .sonar_organization_input
                                    .read(cx)
                                    .content
                                    .trim()
                                    .to_string();
                                if key.is_empty() {
                                    this.connect_error = Some("Enter your organization key".into());
                                    cx.notify();
                                } else {
                                    this.manage_sonar(Manage::Organization(key), cx);
                                }
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        if api::is_cloud_site(&status.site) && status.organization.is_empty() {
            rows.push(div().text_color(content(0.5)).child("Select an organization or enter its key from your SonarCloud organization URL. Projects load only for that organization.").into_any_element());
        } else if !editing {
            rows.push(
                div()
                    .text_color(content(0.5))
                    .child(format!(
                        "Watched projects · {} selected · {} loaded",
                        status.projects.len(),
                        self.sonar_projects.len()
                    ))
                    .into_any_element(),
            );
            if !self.sonar_projects.is_empty() || !status.projects.is_empty() {
                rows.push(
                    div()
                        .flex_shrink_0()
                        .border_1()
                        .border_color(content(0.1))
                        .rounded(px(6.))
                        .p(px(8.))
                        .child(self.sonar_project_search.clone())
                        .into_any_element(),
                );
                let projects = filtered_projects(
                    &self.sonar_projects,
                    &status.projects,
                    &self.sonar_project_search.read(cx).content,
                );
                let select = projects.clone();
                let clear = projects.clone();
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            button("sonar-select-all-projects", "Select all")
                                .debug_selector(|| "sonar-select-all-projects".into())
                                .when(busy || select.is_empty(), |d| d.opacity(0.4))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !select.is_empty() {
                                        this.manage_sonar(
                                            Manage::WatchMany {
                                                projects: select.clone(),
                                                watch: true,
                                            },
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .child(
                            button("sonar-clear-project-selection", "Clear selection")
                                .debug_selector(|| "sonar-clear-project-selection".into())
                                .when(busy || clear.is_empty(), |d| d.opacity(0.4))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !clear.is_empty() {
                                        this.manage_sonar(
                                            Manage::WatchMany {
                                                projects: clear.clone(),
                                                watch: false,
                                            },
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .into_any_element(),
                );
                rows.push(
                    div()
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(
                            "Applies to matching projects listed here, including saved selections.",
                        )
                        .into_any_element(),
                );
                let selected: std::collections::HashSet<_> =
                    status.projects.iter().map(|p| p.key.clone()).collect();
                let entity = cx.entity();
                let count = projects.len();
                rows.push(
                    gpui::uniform_list("sonar-project-results", count, move |range, _, cx| {
                        entity.update(cx, |_, cx| {
                            range
                                .map(|ix| {
                                    let project = projects[ix].clone();
                                    div()
                                        .id(format!("sonar-watch-{}", project.key))
                                        .h(px(52.))
                                        .w_full()
                                        .min_w_0()
                                        .flex_shrink_0()
                                        .px(px(8.))
                                        .py(px(5.))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .cursor_pointer()
                                        .rounded(px(5.))
                                        .hover(|s| s.bg(content(0.1)))
                                        .role(gpui::Role::CheckBox)
                                        .aria_label(format!("{} ({})", project.name, project.key))
                                        .child(if selected.contains(&project.key) {
                                            "✓"
                                        } else {
                                            "○"
                                        })
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_color(content(0.85))
                                                        .child(project.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_size(px(11.))
                                                        .text_color(content(0.45))
                                                        .child(project.key.clone()),
                                                ),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.manage_sonar(Manage::Watch(project.clone()), cx)
                                        }))
                                        .into_any_element()
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                    .track_scroll(&self.sonar_projects_scroll)
                    .debug_selector(|| "sonar-project-results".into())
                    .h(px((count.clamp(1, 6) * 52) as f32))
                    .w_full()
                    .flex_shrink_0()
                    .into_any_element(),
                );
                if count == 0 {
                    rows.push(
                        div()
                            .child("No loaded projects match your search.")
                            .into_any_element(),
                    );
                }
            } else if !busy && self.connect_error.is_none() {
                rows.push(div().child("No projects returned for this organization. Check the key and Browse permission.").into_any_element());
            }
            if self.sonar_projects_more {
                rows.push(
                    button("sonar-more-projects", "Load next 100 projects")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.sonar_loading.is_none() {
                                this.connect_error = None;
                                this.load_sonar_project_page(
                                    this.sonar_connection_generation,
                                    this.sonar_project_page + 1,
                                    cx,
                                );
                            }
                        }))
                        .into_any_element(),
                );
            }
        }
        if editing {
            rows.push(
                button("sonar-retry-discovery", "Reload discovery")
                    .on_click(cx.listener(|this, _, _, cx| this.manage_sonar(Manage::Load, cx)))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(12.))
            .when(busy, |d| d.opacity(0.65))
            .children(rows)
            .into_any_element()
    }

    pub(super) fn sonar_connection_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let connected = self
            .snapshot
            .connections
            .get("sonar")
            .is_some_and(|status| status.connected);
        let replacing = !self.connect_token.read(cx).content.trim().is_empty();
        let busy = self.sonar_loading.is_some();
        let editing = !connected
            || self.sonar_edit_connection
            || self.sonar_status.as_ref().is_some_and(|status| {
                api::is_cloud_site(&status.site) && status.organization.is_empty()
            });
        let summary = self
            .sonar_status
            .as_ref()
            .map(|status| {
                format!(
                    "{}{}{}",
                    status
                        .site
                        .trim_start_matches("https://")
                        .trim_start_matches("http://"),
                    if status.organization.is_empty() {
                        ""
                    } else {
                        " · "
                    },
                    status.organization
                )
            })
            .unwrap_or_default();
        let field = |input: Entity<Input>| {
            div()
                .mb(px(12.))
                .border_1()
                .border_color(content(0.1))
                .rounded(px(6.))
                .px(px(10.))
                .py(px(8.))
                .child(input)
        };
        div().absolute().inset_0().p(px(16.)).flex().items_center().justify_center()
            .child(crate::modal::scrim("sonar-connect-scrim"))
            .child(crate::modal::enter("sonar-connect-card", div().id("sonar-connection-card").debug_selector(|| "sonar-connection-card".into()).relative().w(px(440.)).max_w_full().when(connected, |d| d.h(px(660.))).max_h_full()
                .rounded(px(crate::modal::RADIUS)).border_1().border_color(content(crate::modal::BORDER))
                .child(crate::modal::surface(crate::modal::RADIUS - 1.)).flex().flex_col()
                .child(div().id("sonar-connection-header").debug_selector(|| "sonar-connection-header".into()).flex_shrink_0().p(px(20.)).flex().items_center().gap(px(8.))
                    .child(icon("inbox-sonar", 16.))
                    .child(div().flex_1().text_size(px(18.)).text_color(content(1.)).child(if connected { "Manage Sonar" } else { "Connect Sonar" }))
                    .child(button("sonar-close-connection", "Close").debug_selector(|| "sonar-close-connection".into()).on_click(cx.listener(|this, _, _, cx| this.close_sonar_connection(cx)))))
                .when(!connected || busy, |d| d.child(div().flex_shrink_0().px(px(20.)).pb(px(10.)).text_size(px(12.)).text_color(content(0.65)).id("sonar-connection-progress").debug_selector(|| "sonar-connection-progress".into()).role(gpui::Role::Status)
                    .child(self.sonar_loading.clone().unwrap_or_else(|| if connected { "Connected. Choose your organization and watched projects.".into() } else { "Enter your server URL and personal token to connect.".into() }))))
                .when_some(self.connect_error.clone(), |d, error| d.child(div().id("sonar-connection-error").max_h(px(96.)).overflow_y_scroll().flex_shrink_0().px(px(20.)).pb(px(12.)).text_size(px(12.)).text_color(gpui::rgb(0xf87171)).role(gpui::Role::Alert).child(error)))
                .child(div().id("sonar-connection-body").debug_selector(|| "sonar-connection-body".into()).when(connected, |d| d.flex_1()).min_h_0().overflow_y_scroll().px(px(20.))
                    .when(connected, |d| d.child(div().mb(px(12.)).flex().items_center().gap(px(8.))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(content(0.7)).child(summary))
                        .child(button("sonar-edit-connection", if editing { "Back to projects" } else { "Edit connection" }).debug_selector(|| "sonar-edit-connection".into()).on_click(cx.listener(|this, _, _, cx| {
                            this.sonar_edit_connection = !this.sonar_edit_connection;
                            this.connect_token.update(cx, |input,cx| input.set_text(String::new(),cx));
                            cx.notify();
                        })))))
                    .when(editing, |d| d
                        .child(div().mb(px(12.)).text_size(px(12.)).text_color(content(0.5)).child("Use a personal token with Browse permission. Source access also requires See Source Code permission."))
                        .child(field(self.connect_url.clone()))
                        .when(connected, |d| d.child(div().mb(px(6.)).text_size(px(11.)).text_color(content(0.45)).child("Enter a new token only to replace the saved connection.")))
                        .child(field(self.connect_token.clone())))
                    .when(connected, |d| d.child(self.sonar_connection_controls(cx))))
                .child(div().id("sonar-connection-footer").debug_selector(|| "sonar-connection-footer".into()).flex_shrink_0().p(px(20.)).border_t_1().border_color(content(0.1)).flex().items_center().gap(px(8.))
                    .when(connected, |d| d.child(button("sonar-disconnect", "Disconnect").text_color(gpui::rgb(0xf87171)).on_click(cx.listener(|this, _, _, cx| this.connect_sonar(true, cx)))))
                    .child(div().flex_1())
                    .child(button("sonar-connect-submit", if connected && !replacing { "Done" } else if busy { "Working…" } else if connected { "Update token" } else { "Connect" })
                        .bg(content(0.1)).on_click(cx.listener(move |this, _, _, cx| {
                            if connected && !replacing { this.close_sonar_connection(cx); }
                            else { this.connect_sonar(false, cx); }
                        }))))))
            .into_any_element()
    }

    pub(super) fn load_sonar_detail(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.item() else {
            return;
        };
        self.detail_generation += 1;
        let generation = self.detail_generation;
        let key = item.key();
        self.detail_loading = true;
        self.detail_error = None;
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.sonar_content(&item) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.complete_sonar_detail(&key, generation, result, cx);
            });
        })
        .detach();
    }

    pub(super) fn complete_sonar_detail(
        &mut self,
        key: &String,
        generation: u64,
        result: Result<api::SonarIssueContent, String>,
        cx: &mut Context<Self>,
    ) {
        if self.detail_generation != generation || self.selected.as_ref() != Some(key) {
            return;
        }
        self.detail_loading = false;
        match result {
            Ok(content) => {
                let body = api::body(&content);
                if self.details.as_ref().is_none_or(|old| old.body != body) {
                    self.document = Some(cx.new(|cx| {
                        document::InboxDocument::new(body.clone(), self.document_cwd(), cx)
                    }));
                    self.details = Some(Details {
                        body,
                        ..Default::default()
                    });
                }
                let diff = api::diff(&content);
                let files = diff::models(&diff);
                self.diff_view
                    .update(cx, |view, cx| view.set_files(files, cx));
                self.diff = Some(diff);
                self.sonar_content = Some(content);
                self.activity_revision += 1;
            }
            Err(error) => self.detail_error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn sonar_status_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let transitions = self
            .sonar_content
            .as_ref()
            .and_then(|c| c.issue.get("transitions"))
            .and_then(serde_json::Value::as_array);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .text_size(px(12.))
            .child(
                self.item()
                    .map(|item| api::label(&item.state))
                    .unwrap_or_default(),
            )
            .children(
                transitions
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .enumerate()
                    .map(|(ix, transition)| {
                        let transition = transition.to_string();
                        button(
                            format!("sonar-transition-{ix}"),
                            format!(
                                "{}{}",
                                if self.sonar_transition == transition {
                                    "✓ "
                                } else {
                                    ""
                                },
                                api::label(&transition)
                            ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.acting {
                                return;
                            }
                            this.sonar_transition = transition.clone();
                            this.action_error = None;
                            this.summary_scroll.scroll_to_end();
                            cx.notify();
                        }))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn sonar_comment_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(content(0.65))
            .child("Select an available status above. A comment is optional.")
            .child(
                div()
                    .border_1()
                    .border_color(content(0.1))
                    .rounded(px(6.))
                    .p(px(8.))
                    .child(self.comment.clone()),
            )
            .child(
                button(
                    "sonar-feedback",
                    format!(
                        "{} Share comment as analyzer feedback",
                        if self.sonar_feedback { "✓" } else { "○" }
                    ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.acting {
                        this.sonar_feedback = !this.sonar_feedback;
                        cx.notify();
                    }
                })),
            )
            .child(
                button(
                    "sonar-apply-status",
                    if self.acting {
                        "Updating…".into()
                    } else if self.sonar_transition.is_empty() {
                        "Select a status".into()
                    } else {
                        format!("Apply {}", api::label(&self.sonar_transition))
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| this.transition_sonar(cx))),
            )
            .when_some(self.action_error.clone(), |d, text| {
                d.child(div().text_color(gpui::rgb(0xf87171)).child(text))
            })
            .into_any_element()
    }

    pub(super) fn discuss_sonar(&mut self, cx: &mut Context<Self>) {
        if self.detail_loading {
            return;
        }
        if let (Some(mut item), Some(details)) = (self.item(), self.details.as_ref()) {
            item.project_path = self.start_project.to_string_lossy().into_owned();
            let prompt = item.prompt(&details.body, true);
            cx.emit(Event::StartSession {
                item: Box::new(item),
                prompt,
            });
        }
    }

    pub(super) fn transition_sonar(&mut self, cx: &mut Context<Self>) {
        if self.acting || self.sonar_transition.is_empty() {
            return;
        }
        let Some(item) = self.item().filter(|item| item.provider == "sonar") else {
            return;
        };
        self.acting = true;
        self.action_error = None;
        let key = item.key();
        let generation = self.detail_generation;
        let request = api::SonarTransitionRequest {
            project_key: item.sonar_project,
            branch: item.sonar_branch,
            issue_key: item.id,
            transition: self.sonar_transition.clone(),
            comment: self.comment.read(cx).content.to_string(),
            share_feedback: self.sonar_feedback,
        };
        let dir = self.service.data_dir.clone();
        let projects = self.projects.clone();
        let cwd = self.service.cwd.clone();
        let task = cx.background_executor().spawn(async move {
            api::sonar_transition_issue(dir, request).map(|issue| api::item(issue, &projects, &cwd))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.acting = false;
                if this.selected.as_ref() != Some(&key) || generation != this.detail_generation {
                    return;
                }
                match result {
                    Ok(mut updated) => {
                        if let Some(old) = this.snapshot.items.iter_mut().find(|i| i.key() == key) {
                            updated.repo = old.repo.clone();
                            updated.project_path = old.project_path.clone();
                            *old = updated;
                        }
                        this.comment
                            .update(cx, |input, cx| input.set_text(String::new(), cx));
                        this.sonar_transition.clear();
                        this.sonar_feedback = false;
                        this.load_sonar_detail(cx);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bulk_selection_uses_the_same_filtered_list_and_keeps_saved_projects_visible() {
        let project = |key: &str| api::SonarProject {
            key: key.into(),
            name: key.into(),
        };
        let loaded = vec![project("team-a-one"), project("team-b-one")];
        let watched = vec![
            project("team-b-one"),
            project("team-a-saved-on-another-page"),
        ];
        let visible = filtered_projects(&loaded, &watched, " TEAM-A ");
        assert_eq!(
            visible.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(),
            vec!["team-a-one", "team-a-saved-on-another-page"]
        );
        assert_eq!(filtered_projects(&loaded, &watched, "").len(), 3);
        assert!(filtered_projects(&loaded, &watched, "no matches").is_empty());
    }
}
