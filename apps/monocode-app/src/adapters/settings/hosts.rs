//! The native calls for each Settings section.
use gpui::{AnyElement, App, Task, Window};
use monocode_app::boot::AppServices;
use monocode_core::HarnessId;
use monocode_core::models::{HarnessAvailability, ModelCatalog};
use monocode_core::settings::KeybindingOverrides;
use monocode_engine::attention::{Attention, sounds::SoundCue};
use monocode_engine::inbox::{client::InboxClient, inbox::Inbox};
use monocode_engine::projects::{ProjectsGlobal, chat_background};
use monocode_harness::core::{availability::HarnessAvailabilityProbe, child::BinaryPathChoice};
use monocode_view_settings::settings::host::UpdateReporter;
use monocode_view_settings::settings::*;

pub struct SettingsAdapter {
    host: monocode_process::harness::HarnessHost,
}
impl SettingsAdapter {
    pub fn new(cx: &App) -> Self {
        Self {
            host: AppServices::global(cx).host.clone(),
        }
    }
}
fn permission(
    value: monocode_engine::attention::notifications::NotificationPermission,
) -> NotificationPermission {
    match value {
        monocode_engine::attention::notifications::NotificationPermission::Prompt => {
            NotificationPermission::Prompt
        }
        monocode_engine::attention::notifications::NotificationPermission::Granted => {
            NotificationPermission::Granted
        }
        monocode_engine::attention::notifications::NotificationPermission::Denied => {
            NotificationPermission::Denied
        }
        monocode_engine::attention::notifications::NotificationPermission::Unsupported => {
            NotificationPermission::Unsupported
        }
    }
}
impl GeneralHost for SettingsAdapter {
    fn play_switch_cue(&self, cx: &mut App) {
        let notifier = Attention::global(cx).notifier.clone();
        notifier.update(cx, |n, _| {
            n.play_cue(SoundCue::Switch, None);
        });
    }
    fn cached_notification_permission(&self, cx: &App) -> NotificationPermission {
        permission(Attention::global(cx).notifier.read(cx).permission())
    }
    fn probe_notification_permission(&self, cx: &mut App) -> Task<NotificationPermission> {
        let notifier = Attention::global(cx).notifier.clone();
        let task = notifier.update(cx, |n, cx| n.probe_permission(cx));
        cx.spawn(async move |_| permission(task.await))
    }
    fn request_notification_permission(&self, cx: &mut App) -> Task<NotificationPermission> {
        let notifier = Attention::global(cx).notifier.clone();
        let task = notifier.update(cx, |n, cx| n.request_permission(cx));
        cx.spawn(async move |_| permission(task.await))
    }
    fn open_notification_settings(&self, cx: &mut App) {
        let notifier = Attention::global(cx).notifier.clone();
        let task = notifier.update(cx, |n, cx| n.open_notification_settings(cx));
        cx.spawn(async move |cx| {
            if let Err(error) = task.await {
                cx.update(|cx| monocode_app::bridge::dialogs::alert(&error, true, cx));
            }
        })
        .detach();
    }
    fn run_update_flow(&self, manual: bool, report: UpdateReporter, cx: &mut App) {
        super::updater::run(manual, report, cx).detach();
    }
    fn install_pending_update(&self, report: UpdateReporter, cx: &mut App) {
        super::updater::install(report, cx).detach();
    }
}
impl KeybindingsHost for SettingsAdapter {
    fn set_quick_composer_shortcut(
        &self,
        enabled: bool,
        shortcut: Option<&str>,
        cx: &mut App,
    ) -> HostTask<()> {
        Task::ready(crate::quick::set_shortcut(enabled, shortcut, cx))
    }
    fn set_keybinding_overrides(
        &self,
        overrides: &KeybindingOverrides,
        cx: &mut App,
    ) -> HostTask<()> {
        crate::shell::keymap::rebind(overrides, cx);
        crate::shell::menus::init(cx);
        Task::ready(Ok(()))
    }
}
fn pick_background(project: Option<&str>, cx: &mut App) -> HostTask<Option<String>> {
    let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose chat background".into()),
    });
    let backend = ProjectsGlobal::projects(cx).read(cx).backend().clone();
    let project = project.map(str::to_owned);
    cx.background_executor().spawn(async move {
        let picked = picked
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let Some(source) = picked.and_then(|v| v.into_iter().next()) else {
            return Ok(None);
        };
        let source = source.to_string_lossy();
        if let Some(project) = project {
            backend
                .save_project_chat_background(&project, &source)
                .map(Some)
        } else {
            backend.save_chat_background(&source).map(Some)
        }
    })
}
impl AppearanceHost for SettingsAdapter {
    fn set_window_background_blur(&self, _: i64, cx: &mut App) {
        for handle in cx.windows() {
            handle
                .update(cx, |_, window, cx| crate::glass::sync_window(window, cx))
                .ok();
        }
    }
    fn pick_and_save_chat_background(&self, cx: &mut App) -> HostTask<Option<String>> {
        pick_background(None, cx)
    }
    fn remove_chat_background(&self, cx: &mut App) -> HostTask<()> {
        chat_background::remove_chat_background(
            ProjectsGlobal::projects(cx).read(cx).backend(),
            cx.background_executor(),
        )
    }
}
impl ProvidersHost for SettingsAdapter {
    fn catalog(&self, cx: &App) -> ModelCatalog {
        AppServices::global(cx).catalog.snapshot()
    }
    fn availability(&self, cx: &App) -> HarnessAvailability {
        let store = &AppServices::global(cx).availability;
        HarnessAvailability {
            probed: store.has_probed_harness_availability(),
            installed: monocode_core::harness::HARNESSES
                .iter()
                .copied()
                .filter(|id| store.is_harness_available(*id))
                .collect(),
        }
    }
    fn harness_unavailable_hint(&self, id: HarnessId) -> String {
        monocode_harness::core::availability::harness_unavailable_hint(id)
    }
    fn probe_harness_availability(&self, cx: &mut App) {
        let services = AppServices::global(cx);
        let probe = HarnessAvailabilityProbe::new(
            services.registry.clone(),
            services.children.clone(),
            services.availability.clone(),
        );
        cx.background_executor()
            .spawn(probe.probe_harness_availability(true))
            .detach();
    }
    fn refresh_harness_catalog(&self, id: HarnessId, cx: &mut App) {
        let services = AppServices::global(cx);
        let registry = services.registry.clone();
        let catalog = services.catalog.clone();
        cx.background_executor()
            .spawn(async move {
                registry
                    .refresh_harness_catalogs([id], true, move |id| catalog.has_live_catalog(id))
                    .await
            })
            .detach();
    }
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        self.host.runtime_binary_path(provider.as_str())
    }
    fn inspect_binary(
        &self,
        provider: HarnessId,
        path: Option<&str>,
        cx: &mut App,
    ) -> HostTask<BinaryInspection> {
        let children = AppServices::global(cx).children.clone();
        let path = BinaryPathChoice::Given(path.map(str::to_owned));
        cx.background_executor().spawn(async move {
            children
                .inspect_harness_binary(provider, path)
                .await
                .map(|v| BinaryInspection {
                    path: v.path,
                    version: v.version,
                    error: v.error,
                })
                .map_err(|e| e.to_string())
        })
    }
    fn reveal_path(&self, path: &str, cx: &mut App) -> HostTask<()> {
        cx.reveal_path(std::path::Path::new(path));
        Task::ready(Ok(()))
    }
    fn project_icon(&self, path: &str, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
        use gpui::{IntoElement as _, Styled as _, StyledImage as _};
        use monocode_view_settings::accounts::UsageHost as _;
        let seed = monocode_layout::paths::project_name(path);
        let appearance = super::AccountsAdapter::new().project_appearance(
            &monocode_layout::paths::project_key(path),
            &seed,
            cx,
        );
        if let Some(logo) = appearance.logo {
            return Some(
                gpui::img(std::path::PathBuf::from(logo))
                    .size(gpui::px(14.))
                    .object_fit(gpui::ObjectFit::Contain)
                    .into_any_element(),
            );
        }
        let color = monocode_view_quick::model::appearance::hex_color(&appearance.color);
        Some(
            monocode_view_settings::accounts::mascot::project_mascot_icon(
                &seed,
                appearance.mascot.as_deref(),
                monocode_ui::color::parse_hex(&color)
                    .unwrap_or(monocode_ui::Theme::of(cx).colors.accent),
                14.,
            ),
        )
    }
}
fn client(cx: &App) -> InboxClient {
    Inbox::global(cx).read(cx).client().clone()
}
impl InboxHost for SettingsAdapter {
    fn open_url(&self, url: &str, cx: &mut App) {
        cx.open_url(url);
    }
    fn clear_inbox_cache(&self, cx: &mut App) {
        client(cx).clear_inbox_cache();
    }
    fn github_status(&self, cx: &mut App) -> HostTask<GithubStatus> {
        let task = client(cx).github_status();
        cx.spawn(async move |_| {
            task.await.map(|v| GithubStatus {
                installed: v.installed,
                connected: v.connected,
            })
        })
    }
    fn gitlab_status(&self, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).gitlab_connected();
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn save_gitlab(&self, url: &str, token: &str, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).save_gitlab_config(url, token);
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn disconnect_gitlab(&self, url: &str, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).disconnect_gitlab(url);
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn azure_devops_status(&self, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).azure_dev_ops_connected();
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn save_azure_devops(&self, url: &str, token: &str, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).save_azure_dev_ops_config(url, token);
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn disconnect_azure_devops(&self, url: &str, cx: &mut App) -> HostTask<UrlStatus> {
        let task = client(cx).disconnect_azure_dev_ops(url);
        cx.spawn(async move |_| {
            task.await.map(|v| UrlStatus {
                connected: v.connected,
                url: v.url,
            })
        })
    }
    fn linear_connected(&self, cx: &mut App) -> HostTask<bool> {
        let task = client(cx).linear_connected();
        cx.spawn(async move |_| task.await.map(|v| v.connected))
    }
    fn list_linear_teams(&self, cx: &mut App) -> HostTask<Vec<LinearTeam>> {
        let task = client(cx).list_linear_teams();
        cx.spawn(async move |_| {
            task.await.map(|v| {
                v.into_iter()
                    .map(|v| LinearTeam {
                        id: v.id,
                        name: v.name,
                        key: Some(v.key),
                    })
                    .collect()
            })
        })
    }
    fn save_linear_token(&self, token: &str, cx: &mut App) -> HostTask<()> {
        let task = client(cx).save_linear_token(token);
        cx.spawn(async move |_| task.await.map(|_| ()))
    }
    fn disconnect_linear(&self, cx: &mut App) -> HostTask<()> {
        let task = client(cx).disconnect_linear();
        cx.spawn(async move |_| task.await.map(|_| ()))
    }
    fn notify_linear_change(&self, cx: &mut App) {
        client(cx).notify_linear_change();
    }
    fn jira_status(&self, cx: &mut App) -> HostTask<JiraStatus> {
        let task = client(cx).jira_connected();
        cx.spawn(async move |_| {
            task.await.map(|v| JiraStatus {
                connected: v.connected,
                site: v.site,
                email: v.email,
            })
        })
    }
    fn save_jira_config(
        &self,
        site: &str,
        email: &str,
        token: &str,
        cx: &mut App,
    ) -> HostTask<JiraStatus> {
        let task = client(cx).save_jira_config(site, email, token);
        cx.spawn(async move |_| {
            task.await.map(|v| JiraStatus {
                connected: v.connected,
                site: v.site,
                email: v.email,
            })
        })
    }
    fn list_jira_projects(&self, cx: &mut App) -> HostTask<Vec<JiraProject>> {
        let task = client(cx).list_jira_projects();
        cx.spawn(async move |_| {
            task.await.map(|v| {
                v.into_iter()
                    .map(|v| JiraProject {
                        id: v.id,
                        key: v.key,
                        name: v.name,
                    })
                    .collect()
            })
        })
    }
    fn notify_jira_change(&self, cx: &mut App) {
        client(cx).notify_jira_change();
    }
}
impl ArchiveHost for SettingsAdapter {
    fn archived_projects(&self, cx: &App) -> Vec<ArchivedProject> {
        ProjectsGlobal::projects(cx)
            .read(cx)
            .archived()
            .iter()
            .map(|v| ArchivedProject {
                path: v.path.clone(),
                label: monocode_layout::paths::project_name(&v.path),
            })
            .collect()
    }
    fn project_session_count(&self, path: &str, cx: &mut App) -> Task<Option<i64>> {
        let task = monocode_engine::projects::project_data::project_session_count(path, cx);
        cx.spawn(async move |_| Some(task.await as i64))
    }
}
impl ProjectBackgroundHost for SettingsAdapter {
    fn load_settings(&self, project: &str, cx: &App) -> Option<ProjectBackgroundSettings> {
        ProjectsGlobal::projects(cx)
            .read(cx)
            .chat_background_settings(project)
            .map(|v| ProjectBackgroundSettings {
                path: v.path,
                empty_opacity: v.empty_opacity,
                session_opacity: v.session_opacity,
                scope: v.scope,
                effect: v.effect,
            })
    }
    fn save_settings(
        &self,
        project: &str,
        v: &ProjectBackgroundSettings,
        changed: bool,
        cx: &mut App,
    ) {
        ProjectsGlobal::projects(cx).update(cx, |projects, cx| projects.save_chat_background_settings(project, &monocode_engine::projects::project_chat_background::ProjectChatBackgroundSettings { path: v.path.clone(), empty_opacity: v.empty_opacity, session_opacity: v.session_opacity, scope: v.scope, effect: v.effect }, changed, cx));
    }
    fn clear_setting(&self, project: &str, cx: &mut App) {
        ProjectsGlobal::projects(cx).update(cx, |projects, cx| {
            projects.clear_chat_background_setting(project, cx)
        });
    }
    fn image_revision(&self, cx: &App) -> i64 {
        ProjectsGlobal::projects(cx)
            .read(cx)
            .chat_background_image_revision()
    }
    fn pick_and_save(&self, project: &str, cx: &mut App) -> HostTask<Option<String>> {
        pick_background(Some(project), cx)
    }
    fn clear_image(&self, project: &str, cx: &mut App) -> HostTask<()> {
        chat_background::clear_project_chat_background(
            ProjectsGlobal::projects(cx).read(cx).backend(),
            project,
            cx.background_executor(),
        )
    }
}
