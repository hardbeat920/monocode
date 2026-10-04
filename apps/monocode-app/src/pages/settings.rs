//! The Settings page follows this window's project, history, and live catalogs.
use crate::adapters::settings::{AccountsAdapter, SettingsAdapter};
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    Window, div, px,
};
use monocode_app::boot::AppServices;
use monocode_core::Platform;
use monocode_core::settings::SETTINGS_SECTIONS;
use monocode_engine::{
    history::{History, HistoryPackage},
    projects::{Projects, ProjectsGlobal},
    workspace::Workspace,
};
use monocode_view_settings::settings::{
    SessionSummary, SettingsCallbacks, SettingsHosts, SettingsPage, SettingsProps,
};
use std::rc::Rc;

fn props(
    workspace: &Entity<Workspace>,
    projects: &Entity<Projects>,
    history: &Entity<History>,
    cx: &App,
) -> SettingsProps {
    SettingsProps {
        cwd: workspace.read(cx).sidebar_cwd(cx),
        recents: projects
            .read(cx)
            .recents()
            .iter()
            .map(|v| v.path.clone())
            .collect(),
        sessions: history
            .read(cx)
            .rows()
            .iter()
            .map(|v| SessionSummary {
                id: v.id.clone(),
                title: v.title.clone(),
                harness: v.harness,
                updated_at: v.updated_at,
                archived: v.archived == Some(true),
            })
            .collect(),
        beside_rail: true,
        ..Default::default()
    }
}
pub fn reveal_integration(
    source: monocode_view_inbox::data::InboxSource,
    window: &mut Window,
    cx: &mut App,
) {
    use monocode_view_inbox::data::InboxProvider;
    let id = match source {
        InboxProvider::Github => "github",
        InboxProvider::Gitlab => "gitlab",
        InboxProvider::AzureDevops => "azuredevops",
        InboxProvider::Jira => "jira",
        InboxProvider::Linear => "linear",
    };
    if let Some(page) = page(window, cx).and_then(|v| v.downcast::<SettingsWindow>().ok()) {
        let settings = page.read(cx).page.clone();
        settings.update(cx, |page, cx| {
            page.set_section(
                monocode_core::settings::SettingsSectionId::Inbox,
                window,
                cx,
            );
            page.set_anchor(Some(id), cx);
        });
    }
}

pub fn reveal_section(
    section: monocode_core::settings::SettingsSectionId,
    window: &mut Window,
    cx: &mut App,
) {
    monocode_settings::settings_store::save_settings_section(&AppServices::global(cx).kv, section);
    if let Some(view) = page(window, cx).and_then(|v| v.downcast::<SettingsWindow>().ok()) {
        let settings = view.read(cx).page.clone();
        settings.update(cx, |page, cx| page.set_section(section, window, cx));
    }
}

pub fn reveal_project_notifications(path: &str, window: &mut Window, cx: &mut App) {
    let section = monocode_core::settings::SettingsSectionId::Inbox;
    monocode_settings::settings_store::save_settings_section(&AppServices::global(cx).kv, section);
    if let Some(view) = page(window, cx).and_then(|view| view.downcast::<SettingsWindow>().ok()) {
        view.update(cx, |view, cx| {
            view.notification_project_path = Some(path.to_owned());
            view.notification_settings_request = view.notification_settings_request.wrapping_add(1);
            view.sync(cx);
            view.page.update(cx, |page, cx| {
                page.set_section(section, window, cx);
                page.set_anchor(Some("project-notifications"), cx);
            });
        });
    }
}

struct SettingsWindow {
    page: Entity<SettingsPage>,
    workspace: Entity<Workspace>,
    projects: Entity<Projects>,
    history: Entity<History>,
    notification_project_path: Option<String>,
    notification_settings_request: u64,
    _subscriptions: Vec<Subscription>,
    _store_task: Task<()>,
}
impl SettingsWindow {
    fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let projects = ProjectsGlobal::projects(cx);
        let history = HistoryPackage::history(cx);
        let kv = AppServices::global(cx).kv.clone();
        let host = Rc::new(SettingsAdapter::new(cx));
        let accounts = Rc::new(AccountsAdapter::new());
        let hosts = SettingsHosts {
            general: host.clone(),
            keybindings: host.clone(),
            appearance: host.clone(),
            providers: host.clone(),
            inbox: host.clone(),
            archive: host,
            connections: Some(Rc::new(|_, window, cx| {
                super::connections::build(window, cx)
            })),
            mcp: Some(Rc::new(crate::adapters::mcp::mcp_settings_slot)),
            skills: Some(Rc::new(crate::adapters::skills::skills_slot)),
            worktrees: Some(Rc::new(crate::adapters::scm::worktrees_slot)),
            project_notifications: Some(
                monocode_view_settings::accounts::project_notifications_slot(accounts.clone()),
            ),
            accounts: Some(monocode_view_settings::accounts::accounts_slot(accounts)),
            window_controls: None,
        };
        let open = workspace.clone();
        let h_archive = history.clone();
        let h_delete = history.clone();
        let callbacks = SettingsCallbacks {
            on_close: Some(Rc::new(|_, window, cx| {
                window.dispatch_action(Box::new(crate::shell::keymap::OpenSettings), cx)
            })),
            on_open_session: Some(Rc::new(move |id, window, cx| {
                open.update(cx, |workspace, cx| workspace.open_session(&id, cx))
                    .detach();
                window.dispatch_action(Box::new(crate::shell::keymap::OpenSettings), cx);
            })),
            on_archive_session: Some(Rc::new(move |(id, archived), _, cx| {
                h_archive
                    .update(cx, |history, cx| history.archive_session(&id, archived, cx))
                    .detach();
            })),
            on_delete_session: Some(Rc::new(move |id, _, cx| {
                h_delete
                    .update(cx, |history, cx| history.delete_session(&id, cx))
                    .detach();
            })),
            on_restore_project: Some(Rc::new(|path, _, cx| {
                monocode_engine::projects::actions::on_restore_project(&path, cx)
            })),
            on_delete_project: Some(Rc::new(|path, _, cx| {
                monocode_engine::projects::actions::on_remove_project(&path, true, cx)
            })),
            on_open_whats_new: Some(Rc::new(|version, window, cx| {
                crate::shell::whats_new::open(&version, window, cx)
            })),
            on_collapsed_project_rail_mode_change: Some(Rc::new(|mode, _, cx| {
                monocode_app::bridge::shell::ShellRequests::send(
                    monocode_app::bridge::shell::ShellRequest::SetCompactRail(
                        mode == monocode_core::settings::CollapsedProjectRailMode::Compact,
                    ),
                    cx,
                );
            })),
            ..Default::default()
        };
        let page = cx.new(|cx| {
            SettingsPage::new(
                kv.clone(),
                Platform::current(),
                hosts,
                monocode_settings::settings_store::load_settings_section(&kv),
                props(&workspace, &projects, &history, cx),
                callbacks,
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.observe(&workspace, |this, _, cx| this.sync(cx)),
            cx.observe(&projects, |this, _, cx| this.sync(cx)),
            cx.observe(&history, |this, _, cx| this.sync(cx)),
            cx.observe(&page, |_, _, cx| cx.notify()),
        ];
        let (send, receive) = async_channel::bounded(1);
        let store = kv.subscribe(move |_| {
            let _ = send.try_send(());
        });
        let handle = window.window_handle();
        let weak = cx.entity().downgrade();
        let catalog = AppServices::global(cx).catalog.clone();
        let catalog_send = receive.clone();
        let (catalog_tx, catalog_rx) = async_channel::bounded(1);
        let catalog_id = catalog.subscribe(move |_| {
            let _ = catalog_tx.try_send(());
        });
        let availability = AppServices::global(cx).availability.clone();
        let (availability_tx, availability_rx) = async_channel::bounded(1);
        let availability_id = availability.subscribe_harness_availability(move || {
            let _ = availability_tx.try_send(());
        });
        let task = cx.spawn(async move |_, cx| {
            let _leases = (store, catalog_send);
            loop {
                use futures::{FutureExt as _, select_biased};
                select_biased! { value = receive.recv().fuse() => if value.is_err() { break; }, value = catalog_rx.recv().fuse() => if value.is_err() { break; }, value = availability_rx.recv().fuse() => if value.is_err() { break; } }
                let weak = weak.clone();
                handle.update(cx, |_, window, cx| { if let Some(view) = weak.upgrade() { view.update(cx, |view, cx| {
                    let section = monocode_settings::settings_store::load_settings_section(&AppServices::global(cx).kv);
                    view.page.update(cx, |page, cx| { page.set_section(section, window, cx); page.refresh(cx); }); view.sync(cx);
                }); } }).ok();
            }
        });
        let mut subscriptions = subscriptions;
        subscriptions.push(Subscription::new(move || {
            catalog.unsubscribe(catalog_id);
            availability.unsubscribe_harness_availability(availability_id);
        }));
        Self {
            page,
            workspace,
            projects,
            history,
            notification_project_path: None,
            notification_settings_request: 0,
            _subscriptions: subscriptions,
            _store_task: task,
        }
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let mut props = props(&self.workspace, &self.projects, &self.history, cx);
        props.notification_project_path = self.notification_project_path.clone();
        props.notification_settings_request = self.notification_settings_request;
        self.page.update(cx, |page, cx| page.set_props(props, cx));
        cx.notify();
    }
}
impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = monocode_ui::Theme::of(cx);
        let selected = self.page.read(cx).section();
        let mut nav = div()
            .id("settings-sections")
            .w(px(175.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.));
        for section in SETTINGS_SECTIONS {
            let page = self.page.clone();
            nav = nav.child(
                div()
                    .id(section.id.as_str())
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(if selected == section.id {
                        theme.colors.content
                    } else {
                        theme.content(0.50)
                    })
                    .bg(if selected == section.id {
                        theme.colors.selection
                    } else {
                        gpui::Hsla::transparent_black()
                    })
                    .child(section.label)
                    .on_click(move |_, window, cx| {
                        monocode_settings::settings_store::save_settings_section(
                            &AppServices::global(cx).kv,
                            section.id,
                        );
                        page.update(cx, |page, cx| page.set_section(section.id, window, cx));
                    }),
            );
        }
        let full_rail = AppServices::try_global(cx).is_some_and(|services| {
            monocode_settings::load_app_settings(&services.kv, Platform::current())
                .appearance
                .project_rail_open
        });
        div()
            .size_full()
            .flex()
            .children((!full_rail).then_some(nav))
            .child(div().flex_1().min_w_0().h_full().child(self.page.clone()))
    }
}
pub fn page(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    crate::slots::cached_view("settings", window, cx, |window, cx| {
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        Some(
            cx.new(|cx| SettingsWindow::new(workspace, window, cx))
                .into(),
        )
    })
}
