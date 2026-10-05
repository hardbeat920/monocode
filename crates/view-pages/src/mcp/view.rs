//! Port of src/features/settings/ui/McpSettings.tsx: MCP connections for a
//! project and the provider accounts, with provider chips, sign in, remove,
//! show config, and the Add MCP server dialog. The settings page embeds it.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _,
};
use monocode_ui::widgets::{ModalSize, modal, tooltip};
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};
use monocode_view_composer::pickers::{SearchableSelect, SearchableSelectOption, SelectVariant};

use super::add_form::AddServerForm;
use super::data::{McpData, McpProvider, McpScope, McpServerRow, McpSettingsSnapshot};
use crate::data::ProjectsData;
use crate::widgets::{ProjectPicker, ProjectPickerAppearance, window_overlay};

/// The provider chip filter: all, or one provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpFilter {
    #[default]
    All,
    Provider(McpProvider),
}

/// Transports that run locally and have no account to sign in to.
const LOCAL_TRANSPORTS: [&str; 3] = ["stdio", "local", "ws"];

/// Whether a row offers Sign in.
pub fn can_sign_in(row: &McpServerRow) -> bool {
    let transport = row.connection.transport.as_str();
    row.connection.provider != McpProvider::ClaudeDesktop
        && !transport.is_empty()
        && !LOCAL_TRANSPORTS.contains(&transport)
}

/// `ProviderIcon`: the harness logo, Claude for Claude Desktop.
fn provider_icon(provider: McpProvider) -> AnyElement {
    match ProviderLogo::from_id(provider.harness_id()) {
        Some(logo) => provider_logo(logo).size(14.).into_any_element(),
        None => div().into_any_element(),
    }
}

pub struct McpSettingsView {
    data: Rc<dyn McpData>,
    /// The project the page shows (`selection.project`).
    cwd: String,
    picker: Entity<ProjectPicker>,
    servers: Vec<McpServerRow>,
    filter: McpFilter,
    show_all_providers: bool,
    loading: bool,
    busy: Option<String>,
    error: String,
    claude_error: String,
    add: Option<Entity<AddServerForm>>,
    /// `removeScopes`: the scope picked for rows without a config path.
    remove_scopes: HashMap<String, McpScope>,
    scope_selects: HashMap<String, Entity<SearchableSelect>>,
    refresh_generation: u64,
    cwd_subscription: Option<Subscription>,
}

impl McpSettingsView {
    pub fn new(
        data: Rc<dyn McpData>,
        projects: Rc<dyn ProjectsData>,
        cwd: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.weak_entity();
        let rail = cwd.to_string();
        let picker = cx.new(|cx| {
            let mut picker = ProjectPicker::new(cwd, projects.clone(), window, cx)
                .appearance(ProjectPickerAppearance::Settings)
                .on_select(move |path, window, cx| {
                    weak.update(cx, |this, cx| this.select_project(path, window, cx))
                        .ok();
                });
            picker.set_rail_cwd(Some(&rail), cx);
            picker
        });
        let mut view = Self {
            data,
            cwd: String::new(),
            picker,
            servers: Vec::new(),
            filter: McpFilter::All,
            show_all_providers: false,
            loading: true,
            busy: None,
            error: String::new(),
            claude_error: String::new(),
            add: None,
            remove_scopes: HashMap::new(),
            scope_selects: HashMap::new(),
            refresh_generation: 0,
            cwd_subscription: None,
        };
        view.show_project(cwd, cx);
        view
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn servers(&self) -> &[McpServerRow] {
        &self.servers
    }

    pub fn filter(&self) -> McpFilter {
        self.filter
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn add_form(&self) -> Option<&Entity<AddServerForm>> {
        self.add.as_ref()
    }

    pub fn picker(&self) -> &Entity<ProjectPicker> {
        &self.picker
    }

    /// The settings page's project changed (the `cwd` prop).
    pub fn set_cwd(&mut self, cwd: &str, window: &mut Window, cx: &mut Context<Self>) {
        let rail = cwd.to_string();
        self.picker
            .update(cx, |picker, cx| picker.set_rail_cwd(Some(&rail), cx));
        self.select_project(cwd, window, cx);
    }

    fn select_project(&mut self, path: &str, _: &mut Window, cx: &mut Context<Self>) {
        if path == self.cwd {
            return;
        }
        self.show_project(path, cx);
    }

    /// `McpConnections key={project}`: a fresh view of `cwd` from the cache,
    /// then a load.
    fn show_project(&mut self, cwd: &str, cx: &mut Context<Self>) {
        self.cwd = cwd.to_string();
        let path = cwd.to_string();
        self.picker
            .update(cx, |picker, cx| picker.set_cwd(&path, cx));
        let cached = self.data.cached(cwd, cx);
        self.servers = cached
            .as_ref()
            .map(|snapshot| snapshot.servers.clone())
            .unwrap_or_default();
        self.error = cached
            .as_ref()
            .map(|snapshot| snapshot.error.clone())
            .unwrap_or_default();
        self.claude_error = cached
            .as_ref()
            .map(|snapshot| snapshot.claude_error.clone())
            .unwrap_or_default();
        self.loading = cached.is_none();
        self.filter = McpFilter::All;
        self.show_all_providers = false;
        self.busy = None;
        self.add = None;
        self.remove_scopes.clear();
        self.scope_selects.clear();
        let weak = cx.weak_entity();
        let listened = cwd.to_string();
        self.cwd_subscription = Some(self.data.subscribe(
            cwd,
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    if this.cwd != listened {
                        return;
                    }
                    if let Some(snapshot) = this.data.cached(&listened, cx) {
                        this.apply(snapshot, cx);
                    }
                })
                .ok();
            }),
            cx,
        ));
        self.refresh(false, cx);
        cx.notify();
    }

    fn apply(&mut self, snapshot: McpSettingsSnapshot, cx: &mut Context<Self>) {
        self.servers = snapshot.servers;
        self.error = snapshot.error;
        self.claude_error = snapshot.claude_error;
        self.loading = false;
        self.sync_filter();
        cx.notify();
    }

    /// `refresh(force)`: a forced load reloads discovery; otherwise the
    /// cache answers. An answer for an older request or project is dropped.
    pub fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        self.refresh_generation += 1;
        let generation = self.refresh_generation;
        let cwd = self.cwd.clone();
        let previous = self.data.cached(&cwd, cx);
        if !force {
            self.servers = previous
                .as_ref()
                .map(|snapshot| snapshot.servers.clone())
                .unwrap_or_default();
            self.error = previous
                .as_ref()
                .map(|snapshot| snapshot.error.clone())
                .unwrap_or_default();
            self.claude_error = previous
                .as_ref()
                .map(|snapshot| snapshot.claude_error.clone())
                .unwrap_or_default();
        }
        self.loading = force || previous.is_none();
        let load = self.data.load(&cwd, force, cx);
        cx.spawn(async move |this, cx| {
            let snapshot = load.await;
            this.update(cx, |this, cx| {
                if generation != this.refresh_generation || this.cwd != cwd {
                    return;
                }
                let latest = this.data.cached(&cwd, cx).unwrap_or(snapshot);
                this.apply(latest, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The servers the filter shows.
    pub fn visible(&self) -> Vec<McpServerRow> {
        match self.filter {
            McpFilter::All => self.servers.clone(),
            McpFilter::Provider(provider) => self
                .servers
                .iter()
                .filter(|row| row.connection.provider == provider)
                .cloned()
                .collect(),
        }
    }

    /// `filterProviders`: providers with servers, or all of them.
    pub fn filter_providers(&self) -> Vec<McpProvider> {
        McpProvider::ALL
            .into_iter()
            .filter(|provider| {
                self.show_all_providers
                    || self
                        .servers
                        .iter()
                        .any(|row| row.connection.provider == *provider)
            })
            .collect()
    }

    fn sync_filter(&mut self) {
        if let McpFilter::Provider(provider) = self.filter
            && !self.filter_providers().contains(&provider)
        {
            self.filter = McpFilter::All;
        }
    }

    pub fn set_filter(&mut self, filter: McpFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        cx.notify();
    }

    pub fn toggle_all_providers(&mut self, cx: &mut Context<Self>) {
        self.show_all_providers = !self.show_all_providers;
        self.sync_filter();
        cx.notify();
    }

    /// Open the Add MCP server dialog, starting on the filtered provider.
    pub fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match self.filter {
            McpFilter::All => McpProvider::Claude,
            McpFilter::Provider(provider) => provider,
        };
        let weak = cx.weak_entity();
        let added = cx.weak_entity();
        let data = self.data.clone();
        let cwd = self.cwd.clone();
        self.add = Some(cx.new(|cx| {
            AddServerForm::new(data, &cwd, initial, window, cx)
                .on_close(move |_, cx| {
                    weak.update(cx, |this, cx| this.close_add(cx)).ok();
                })
                .on_added(move |_, cx| {
                    added.update(cx, |this, cx| this.refresh(true, cx)).ok();
                })
        }));
        cx.notify();
    }

    pub fn close_add(&mut self, cx: &mut Context<Self>) {
        self.add = None;
        cx.notify();
    }

    /// `login`.
    pub fn login(&mut self, row: &McpServerRow, cx: &mut Context<Self>) {
        self.busy = Some(row.connection.name.clone());
        self.error.clear();
        let task = self
            .data
            .login(&self.cwd, row.connection.provider, &row.connection.name, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.refresh(true, cx),
                    Err(error) => this.error = error,
                }
                this.busy = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The scope a Remove uses: the row's when it has a config file, else
    /// the picked one, local by default.
    pub fn remove_scope(&self, row: &McpServerRow) -> McpScope {
        if row.connection.config_path.is_empty() {
            self.remove_scopes
                .get(&row.connection.name)
                .copied()
                .unwrap_or(McpScope::Local)
        } else {
            row.connection.scope
        }
    }

    /// `remove`: confirm, then `claude_mcp_remove` and refresh.
    pub fn remove(&mut self, row: &McpServerRow, window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.remove_scope(row);
        let name = row.connection.name.clone();
        let confirm = self.data.confirm(
            &format!("Remove {name} from {} scope?", scope.as_str()),
            "Remove MCP server",
            window,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            let task = this.update(cx, |this, cx| {
                this.busy = Some(name.clone());
                this.error.clear();
                cx.notify();
                this.data.remove(&this.cwd, &name, scope, cx)
            });
            let Ok(task) = task else {
                return;
            };
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.refresh(true, cx),
                    Err(error) => this.error = error,
                }
                this.busy = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reveal(&mut self, path: &str, cx: &mut Context<Self>) {
        let task = self.data.reveal(path, cx);
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                this.update(cx, |this, cx| {
                    this.error = error;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn scope_select(
        &mut self,
        row: &McpServerRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SearchableSelect> {
        let name = row.connection.name.clone();
        if let Some(select) = self.scope_selects.get(&name) {
            return select.clone();
        }
        let weak = cx.weak_entity();
        let key = name.clone();
        let options = [McpScope::Local, McpScope::Project, McpScope::User]
            .into_iter()
            .map(|scope| SearchableSelectOption::new(scope.as_str(), scope.label()))
            .collect();
        let select = cx.new(|cx| {
            SearchableSelect::new(
                format!("Scope to remove {name} from"),
                "local",
                options,
                window,
                cx,
            )
            .variant(SelectVariant::Pill)
            .searchable(false)
            .on_change(move |value, _, cx| {
                let scope = match value {
                    "project" => McpScope::Project,
                    "user" => McpScope::User,
                    _ => McpScope::Local,
                };
                let key = key.clone();
                weak.update(cx, |this, cx| {
                    this.remove_scopes.insert(key, scope);
                    cx.notify();
                })
                .ok();
            })
        });
        self.scope_selects.insert(name, select.clone());
        select
    }

    fn render_row(
        &mut self,
        row: McpServerRow,
        last: bool,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let connection = row.connection.clone();
        let busy = self.busy.is_some();
        let small_button = |id: SharedString, label: &'static str, theme: &Theme| {
            let hover = theme.content(0.05);
            let selector = format!("{label} {}", connection.name);
            div()
                .id(gpui::ElementId::Name(id))
                .debug_selector(move || selector.clone())
                .flex_none()
                .px(u(8.))
                .py(u(4.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.colors.stroke)
                .text_px(theme.text.label)
                .hover(move |s| s.bg(hover))
                .child(label)
        };
        let key = format!(
            "{}:{}:{}:{}",
            connection.provider.as_str(),
            connection.scope.as_str(),
            connection.config_path,
            connection.name
        );
        let details = format!(
            "{} · {} · {} · {}",
            connection.provider.label(),
            connection.scope.as_str(),
            if connection.transport.is_empty() {
                "MCP"
            } else {
                &connection.transport
            },
            row.status
        );
        let mut text = div()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .text_px(theme.text.body)
                    .medium()
                    .text_color(theme.colors.content)
                    .child(connection.name.clone()),
            )
            .child(
                div()
                    .mt(u(4.))
                    .text_px(theme.text.label)
                    .leading(theme.leading.relaxed)
                    .text_color(theme.content(0.45))
                    .child(details),
            );
        if !connection.config_path.is_empty() {
            text = text.child(
                div()
                    .id(gpui::ElementId::Name(format!("{key}-path").into()))
                    .truncate()
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.35))
                    .tooltip(tooltip(connection.config_path.clone()))
                    .child(connection.config_path.clone()),
            );
        }
        let mut el = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(u(12.))
            .px(u(16.))
            .py(u(14.))
            .when(!last, |el| {
                el.border_b_1().border_color(theme.content(0.05))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(28.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.05))
                    .border_1()
                    .border_color(theme.content(0.06))
                    .child(provider_icon(connection.provider)),
            )
            .child(text);
        if can_sign_in(&row) {
            let target = row.clone();
            el = el.child(
                small_button(format!("{key}-sign-in").into(), "Sign in", theme)
                    .when(busy, |button| button.opacity(0.5))
                    .when(!busy, |button| {
                        button.on_click(cx.listener(move |this, _, _, cx| this.login(&target, cx)))
                    }),
            );
        }
        if connection.provider == McpProvider::Claude {
            if connection.config_path.is_empty() {
                let select = self.scope_select(&row, window, cx);
                el = el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.55))
                        .child("Scope")
                        .child(select),
                );
            }
            let target = row.clone();
            el =
                el.child(
                    small_button(format!("{key}-remove").into(), "Remove", theme)
                        .when(busy, |button| button.opacity(0.5))
                        .when(!busy, |button| {
                            button.on_click(cx.listener(move |this, _, window, cx| {
                                this.remove(&target, window, cx)
                            }))
                        }),
                );
        } else {
            let path = connection.config_path.clone();
            el = el.child(
                small_button(format!("{key}-show-config").into(), "Show config", theme)
                    .on_click(cx.listener(move |this, _, _, cx| this.reveal(&path, cx))),
            );
        }
        el
    }
}

impl Render for McpSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let busy = self.busy.is_some();
        let loading = self.loading;
        let hover = theme.content(0.05);
        let refresh = div()
            .id("mcp-refresh")
            .debug_selector(|| "mcp-refresh".into())
            .flex()
            .items_center()
            .gap(u(6.))
            .px(u(12.))
            .py(u(6.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.colors.stroke)
            .text_px(theme.text.label)
            .map(|button| {
                if loading || busy {
                    button.opacity(0.5)
                } else {
                    button
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx)))
                }
            })
            .child(
                icon(IconName::RefreshCw)
                    .size(u(14.))
                    .text_color(theme.colors.content),
            )
            .child("Refresh");
        let show_all = self.show_all_providers;
        let filter_toggle = div()
            .id("mcp-show-all")
            .debug_selector(move || {
                if show_all {
                    "Show available providers".into()
                } else {
                    "Show all providers".into()
                }
            })
            .flex()
            .size(u(28.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .hover(move |s| s.bg(hover))
            .when(show_all, |el| el.bg(theme.colors.selection))
            .tooltip(tooltip(if show_all {
                "Showing all providers"
            } else {
                "Showing available providers"
            }))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_all_providers(cx)))
            .child(
                icon(IconName::ListFilter)
                    .size(u(14.))
                    .text_color(if show_all {
                        theme.colors.content
                    } else {
                        theme.content(0.55)
                    }),
            );
        let add_button = div()
            .id("mcp-add")
            .debug_selector(|| "Add MCP server".into())
            .flex()
            .size(u(28.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.colors.stroke)
            .hover(move |s| s.bg(hover))
            .tooltip(tooltip("Add MCP server"))
            .on_click(cx.listener(|this, _, window, cx| this.open_add(window, cx)))
            .child(
                icon(IconName::Plus)
                    .size(u(14.))
                    .text_color(theme.colors.content),
            );
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .child(
                div()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(u(12.))
                            .child(
                                div()
                                    .text_px(theme.text.ui)
                                    .semibold()
                                    .child("MCP connections"),
                            )
                            .child(self.picker.clone()),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.55))
                            .child(
                                "Configured servers for the selected project and your provider accounts.",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(refresh)
                    .child(filter_toggle)
                    .child(add_button),
            );
        let mut chips = div()
            .flex()
            .flex_wrap()
            .max_w_full()
            .gap(u(2.))
            .p(u(2.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .text_px(theme.text.label)
            .debug_selector(|| "mcp-chips".into());
        let mut options = vec![McpFilter::All];
        options.extend(self.filter_providers().into_iter().map(McpFilter::Provider));
        for option in options {
            let selected = self.filter == option;
            let (glyph, label, count) = match option {
                McpFilter::All => (
                    icon(IconName::Globe)
                        .size(u(14.))
                        .text_color(if selected {
                            theme.colors.content
                        } else {
                            theme.content(0.50)
                        })
                        .into_any_element(),
                    "All",
                    self.servers.len(),
                ),
                McpFilter::Provider(provider) => (
                    provider_icon(provider),
                    provider.label(),
                    self.servers
                        .iter()
                        .filter(|row| row.connection.provider == provider)
                        .count(),
                ),
            };
            let ink = theme.colors.content;
            chips = chips.child(
                div()
                    .id(gpui::ElementId::Name(format!("mcp-chip-{label}").into()))
                    .debug_selector(move || format!("mcp-chip {label}"))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(6.))
                    .px(u(10.))
                    .py(u(4.))
                    .rounded(u(5.))
                    .map(|chip| {
                        if selected {
                            chip.bg(theme.colors.selection).text_color(ink)
                        } else {
                            chip.text_color(theme.content(0.50))
                                .hover(move |s| s.text_color(ink))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_filter(option, cx)))
                    .child(glyph)
                    .child(label)
                    .child(div().opacity(0.6).child(count.to_string())),
            );
        }
        let mut page = div()
            .id("setting-mcp-servers")
            .flex()
            .flex_col()
            .gap(u(24.))
            .text_color(theme.colors.content)
            .child(header)
            .child(div().flex().child(chips));
        if !self.error.is_empty() {
            page = page.child(
                div()
                    .p(u(12.))
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(monocode_ui::color::with_alpha(
                        theme.colors.danger_fill,
                        0.3,
                    ))
                    .bg(monocode_ui::color::with_alpha(
                        theme.colors.danger_fill,
                        0.1,
                    ))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .debug_selector(|| "mcp-error".into())
                    .child(self.error.clone()),
            );
        }
        if !self.claude_error.is_empty()
            && matches!(
                self.filter,
                McpFilter::All | McpFilter::Provider(McpProvider::Claude)
            )
        {
            page = page.child(
                div()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.55))
                    .child(format!(
                        "Claude connection status unavailable: {}",
                        self.claude_error
                    )),
            );
        }
        let visible = self.visible();
        let note = |text: &'static str| {
            div()
                .text_px(theme.text.ui)
                .text_color(theme.content(0.55))
                .child(text)
        };
        if self.loading {
            page = page.child(note("Checking servers…"));
        } else if visible.is_empty() {
            page = page.child(note("No MCP servers configured for this provider."));
        } else {
            let count = visible.len();
            let mut list = div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(u(theme.radius.xl))
                .border_1()
                .border_color(theme.content(0.10))
                .bg(theme.content(0.03));
            for (index, row) in visible.into_iter().enumerate() {
                list = list.child(self.render_row(row, index + 1 == count, &theme, window, cx));
            }
            page = page.child(list);
        }
        page = page.child(
            div()
                .text_px(theme.text.label)
                .text_color(theme.content(0.45))
                .child(
                    "Claude Code status comes from its CLI. Other providers show configured entries. Sign in opens your browser when supported.",
                ),
        );
        if let Some(form) = self.add.clone() {
            page = page.child(window_overlay(
                window,
                modal("mcp-add-modal", "Add MCP server")
                    .description("Paste a server configuration and choose where to add it.")
                    .size(ModalSize::Md)
                    .on_close({
                        let weak = cx.weak_entity();
                        move |_, cx| {
                            weak.update(cx, |this, cx| this.close_add(cx)).ok();
                        }
                    })
                    .child(form),
            ));
        }
        page
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::mcp::data::McpConnection;

    fn row(provider: McpProvider, transport: &str) -> McpServerRow {
        McpServerRow {
            connection: McpConnection {
                provider,
                name: "x".into(),
                scope: McpScope::User,
                config_path: String::new(),
                transport: transport.into(),
                enabled: None,
            },
            status: String::new(),
        }
    }

    #[test]
    fn sign_in_needs_a_remote_transport() {
        assert!(can_sign_in(&row(McpProvider::Claude, "http")));
        assert!(can_sign_in(&row(McpProvider::Codex, "sse")));
        assert!(!can_sign_in(&row(McpProvider::Claude, "")));
        assert!(!can_sign_in(&row(McpProvider::Cursor, "stdio")));
        assert!(!can_sign_in(&row(McpProvider::ClaudeDesktop, "http")));
    }
}
