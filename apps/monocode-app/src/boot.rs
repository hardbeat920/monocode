//! App startup without a window: settings, the store, the harness bridge
//! with every provider, and the engine packages, wired the way App.tsx and
//! main.tsx wired them. The window and the headless live test both call
//! [`boot`] and then [`restore_workspace`].

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow};
use futures::future::BoxFuture;
use gpui::{App, AppContext as _, Global, Task};
use monocode_core::platform::Platform;
use monocode_core::settings::AppSettings;
use monocode_engine::attention::{
    Attention, AttentionConfig, AttentionPlatform, KvLocalStore, NativePlatform,
    NativeRateLimitFetcher, system_clock,
};
use monocode_engine::automations::{AutomationsConfig, AutomationsPackage};
use monocode_engine::history::notes_backend::StoreNotesBackend;
use monocode_engine::history::{HistoryConfig, HistoryPackage};
use monocode_engine::inbox::backend::LiveInboxBackend;
use monocode_engine::inbox::client::InboxClient;
use monocode_engine::inbox::inbox::Inbox;
use monocode_engine::orchestration::{Orchestration, OrchestrationConfig, start_control_server};
use monocode_engine::projects::ProjectsGlobal;
use monocode_engine::remote::{RemoteGlobal, peers::RemoteFs};
use monocode_engine::runtime::backend::NoStoreEvents;
use monocode_engine::runtime::session_store::SessionSummary;
use monocode_engine::runtime::sessions::bind_resumed_sessions;
use monocode_engine::runtime::{Engine, EngineConfig, StoreBackend};
use monocode_engine::side_threads::{SideThreads, SideThreadsConfig};
use monocode_engine::submit::attention_glue::install_attention_submit;
use monocode_engine::submit::{Submit, SubmitConfig};
use monocode_engine::workspace::files::LocalFs;
use monocode_engine::workspace::terminals::Terminals;
use monocode_engine::workspace::terminals::pty::HostPty;
use monocode_engine::workspace::{self, WorkspaceConfig, WorkspaceSetup};
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::core::child::HostChildOptions;
use monocode_harness::core::provider_binary_paths::ProviderBinaryPaths;
use monocode_harness::core::registry::{ControlTurns, RegistryOptions};
use monocode_harness::core::task::SharedSpawner;
use monocode_harness::providers::{claude, codex, cursor, grok, opencode};
use monocode_harness::{
    BridgeLease, Children, HarnessAvailabilityProbe, HarnessAvailabilityStore, HarnessContext,
    HarnessRegistry, register_builtin_harnesses,
};
use monocode_process::control::ControlHost;
use monocode_process::harness::HarnessHost;
use monocode_settings::{APP_IDENTIFIER, ImportStatus, Kv, WebviewData};
use monocode_store::session_store::SessionStore;
use monocode_terminal::pty::PtyHost;

use crate::attention_platform::{UnbundledPlatform, running_in_bundle};
use crate::bridge::{self, AppApprovalRouter, AppHarnessHooks};
use crate::data_dir::DataDir;
use crate::provider_hooks::{CursorSessionStore, GeneratedImageStore, MonoGit};
use crate::session_factory::AppSessionFactory;

/// The control owner the app's turns are granted to. One process holds
/// every window's sessions, so the app needs one owner, not one per window
/// label as the Tauri app had.
pub const CONTROL_OWNER: &str = "main";

/// How to start.
#[derive(Debug, Clone)]
pub struct BootOptions {
    pub data_dir: DataDir,
    /// Copy the Tauri app's WebKit localStorage into the settings store the
    /// first time. The import only reads the WebKit files.
    pub import_webkit: bool,
    /// Play attention cues.
    pub sounds: bool,
    /// Kill agent processes a crashed earlier run left behind. Only for the
    /// real data dir, since a development run may share the machine with
    /// another MonoCode.
    pub reap_orphans: bool,
    /// Run due automations and reminders. Only for the real data dir by
    /// default: a development run on a copy of the user's data must not
    /// start the user's scheduled agents. `MONOCODE_RUN_SCHEDULES=1` turns
    /// it on anyway.
    pub run_schedules: bool,
    /// Start the loopback control server, so agents get the `app` and
    /// `control` CLI.
    pub control_server: bool,
}

impl BootOptions {
    /// The defaults for a window on `data_dir`.
    pub fn app(data_dir: DataDir) -> Self {
        let real = data_dir.is_default();
        let forced = std::env::var("MONOCODE_RUN_SCHEDULES").is_ok_and(|value| value == "1");
        Self {
            data_dir,
            import_webkit: true,
            sounds: true,
            reap_orphans: real,
            run_schedules: real || forced,
            control_server: true,
        }
    }
}

/// The services the engine packages share, as a GPUI global.
pub struct AppServices {
    pub data_dir: DataDir,
    pub kv: Kv,
    pub registry: HarnessRegistry,
    pub children: Children,
    pub host: HarnessHost,
    pub catalog: SharedCatalog,
    pub availability: HarnessAvailabilityStore,
    pub factory: Rc<AppSessionFactory>,
    /// `monocode.db`, shared with the packages that open their own tables.
    pub store: Arc<SessionStore>,
    /// The loopback control server, when it started.
    pub control: Option<Arc<ControlHost>>,
    /// The terminals' PTY host, for the "folder in use" checks.
    pub pty: PtyHost,
    /// Settings as they were at boot.
    pub settings: AppSettings,
    /// `startHarnessBridge`: the child router keeps its routes while held.
    _bridge: Rc<BridgeLease>,
}

impl Global for AppServices {}

impl AppServices {
    pub fn global(cx: &App) -> &AppServices {
        cx.global::<AppServices>()
    }

    pub fn try_global(cx: &App) -> Option<&AppServices> {
        cx.try_global::<AppServices>()
    }
}

/// A spawner over GPUI's background executor, for the harness.
pub fn background_spawner(cx: &App) -> SharedSpawner {
    let executor = cx.background_executor().clone();
    Arc::new(move |future: BoxFuture<'static, ()>| executor.spawn(future).detach())
}

/// Open the settings store and run the WebKit import once.
pub fn open_settings(data_dir: &Path, import_webkit: bool) -> Result<Kv> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;
    let kv = Kv::open(data_dir)
        .with_context(|| format!("opening settings in {}", data_dir.display()))?;
    if import_webkit
        && let Some(webview) = WebviewData::for_platform(Platform::current(), APP_IDENTIFIER)
    {
        let report = monocode_settings::import_webkit_local_storage(&kv, &webview);
        if report.status != ImportStatus::AlreadyImported {
            log::info!(
                "[monocode] settings import: {:?}, {} items from {:?}",
                report.status,
                report.imported,
                report.origin
            );
        }
        for error in &report.errors {
            log::warn!("[monocode] settings import: {error}");
        }
    }
    Ok(kv)
}

/// Register every provider. The ones whose features need the app (git
/// context, generated images, Cursor's stores) go through `register_with`
/// first, in the TypeScript order; `register_builtin_harnesses` then adds
/// the rest and keeps the ones already registered.
fn register_providers(ctx: &HarnessContext, kv: &Kv, data_dir: &Path) {
    let claude_kv = kv.clone();
    claude::register_with(
        ctx,
        claude::ClaudeAppHooks {
            claude_hooks: Some(Arc::new(move || {
                monocode_settings::settings_store::load_claude_hooks(&claude_kv)
            })),
            git: Some(Arc::new(MonoGit)),
        },
    );
    cursor::register_with(
        ctx,
        cursor::CursorAppHooks {
            store: Some(Arc::new(CursorSessionStore)),
            git: Some(Arc::new(MonoGit)),
            session_options: Default::default(),
        },
    );
    codex::register_with(
        ctx,
        codex::CodexHost {
            images: Some(Arc::new(GeneratedImageStore {
                data_dir: data_dir.to_path_buf(),
            })),
            git: Some(Arc::new(MonoGit)),
        },
    );
    grok::register_with(
        ctx,
        grok::GrokHost {
            git: Some(Arc::new(MonoGit)),
        },
    );
    opencode::register_with_git(ctx, Some(Arc::new(MonoGit)));
    register_builtin_harnesses(ctx);
}

fn initialize_provider_binary_paths(host: &HarnessHost, kv: &Kv) {
    let store = KvLocalStore(kv.clone());
    ProviderBinaryPaths::from_store(&store).initialize(host, &store);
}

/// Start the engine on `options.data_dir`. Call once, before any window.
pub fn boot(options: BootOptions, cx: &mut App) -> Result<()> {
    let data_dir = options.data_dir.path.clone();
    let kv = open_settings(&data_dir, options.import_webkit)?;
    let settings = monocode_settings::load_app_settings(&kv, Platform::current());
    if options.reap_orphans {
        monocode_process::harness::reap_orphaned_harness_processes();
    }

    let backend = Arc::new(
        StoreBackend::open(data_dir.clone(), cx.background_executor().clone())
            .map_err(|error| anyhow!("opening monocode.db in {}: {error}", data_dir.display()))?,
    );
    let store = backend.session_store().clone();
    let mut config = EngineConfig::with_backend(backend);

    // The control server: agents reach the app through the `app` and
    // `control` CLI it grants per turn (`initControl` in the Tauri app).
    let control = if options.control_server {
        match start_control_server(cx) {
            Ok(host) => Some(host),
            Err(error) => {
                log::error!("[monocode] control server did not start: {error}");
                None
            }
        }
    } else {
        None
    };

    // The harness bridge.
    let spawner = background_spawner(cx);
    let (children, host) = Children::for_host(
        HostChildOptions {
            data_dir: data_dir.clone(),
            control: control.clone(),
            updater: None,
        },
        spawner.clone(),
    );
    initialize_provider_binary_paths(&host, &kv);
    let bridge = children.start_harness_bridge();
    let catalog = SharedCatalog::new();
    let registry = HarnessRegistry::new(
        spawner.clone(),
        RegistryOptions {
            turn_control: control.clone().map(|host| {
                Arc::new(ControlTurns {
                    host,
                    owner: CONTROL_OWNER.into(),
                }) as _
            }),
            ..RegistryOptions::default()
        },
    );
    let ctx = HarnessContext::new(registry.clone(), children.clone(), catalog.clone());
    register_providers(&ctx, &kv, &data_dir);
    let availability = HarnessAvailabilityStore::new();
    let probe =
        HarnessAvailabilityProbe::new(registry.clone(), children.clone(), availability.clone());
    config.hooks.harness = Rc::new(AppHarnessHooks {
        registry: registry.clone(),
        catalog: catalog.clone(),
        host: host.clone(),
        probe,
        cursor_store: Arc::new(CursorSessionStore),
    });
    Engine::init(config, cx);

    // Attention, the way `Attention::init_native` builds it, with
    // notifications only inside the app bundle.
    let (clicks, clicked) = async_channel::unbounded::<String>();
    let on_click: monocode_platform::notifications::ClickHandler =
        Arc::new(move |session_id: &str| {
            let _ = clicks.try_send(session_id.to_string());
        });
    let native = NativePlatform::new(APP_IDENTIFIER, "main", on_click);
    let platform: Arc<dyn AttentionPlatform> = if running_in_bundle() {
        native.install();
        Arc::new(native)
    } else {
        Arc::new(UnbundledPlatform {
            native,
            sounds: options.sounds,
        })
    };
    let fetcher = Arc::new(NativeRateLimitFetcher::new(
        data_dir.clone(),
        cx.background_executor().clone(),
        Some(children.clone()),
    ));
    Attention::init(
        AttentionConfig {
            kv: kv.clone(),
            platform: platform.clone(),
            fetcher,
            clock: system_clock(),
        },
        cx,
    );
    cx.spawn(async move |cx| {
        while let Ok(identifier) = clicked.recv().await {
            cx.update(|cx| bridge::notification_clicked(&identifier, cx));
        }
    })
    .detach();
    Attention::set_approval_router(
        cx,
        Rc::new(AppApprovalRouter {
            registry: registry.clone(),
        }),
    );

    // Submit.
    let mut submit = SubmitConfig::new(registry.clone(), catalog.clone(), kv.clone(), spawner);
    submit.mcp_settings = Some(
        monocode_engine::submit::mcp_settings_cache::McpSettingsCache::new(
            Arc::new(
                monocode_engine::submit::mcp_settings_cache::ProcessMcpSources {
                    host: host.clone(),
                },
            ),
            registry.spawner().clone(),
        ),
    );
    let available = availability.clone();
    submit.is_harness_available = Arc::new(move |id| available.is_harness_available(id));
    Submit::init(submit, cx);
    install_attention_submit(cx);
    // Side threads (`/btw`, second opinions, handoffs) share Submit's
    // registry, catalog, and settings.
    if let Some(config) = SideThreadsConfig::from_submit(cx) {
        SideThreads::init(config, cx);
    }

    RemoteGlobal::init_native(kv.clone(), data_dir.clone(), cx);

    // Workspace, with the terminals' PTY host kept for the projects
    // package's "folder in use" checks.
    let factory = Rc::new(AppSessionFactory::new(
        kv.clone(),
        catalog.clone(),
        availability.clone(),
    ));
    workspace::init(
        WorkspaceSetup {
            fs: Arc::new(RemoteFs::new(
                Arc::new(LocalFs),
                RemoteGlobal::global(cx).client.clone(),
            )),
            sessions: factory.clone(),
            delegate: Rc::new(bridge::AppWorkspaceDelegate),
            terminals: false,
        },
        cx,
    );
    let mut pty = None;
    Terminals::init_with(
        |events| {
            let host = PtyHost::new(events);
            pty = Some(host.clone());
            Arc::new(HostPty(host))
        },
        cx,
    );
    let pty = pty.expect("Terminals::init_with builds the backend at once");

    // Projects: the rail, project settings, and git status.
    let terminals_in_use = {
        let pty = pty.clone();
        Arc::new(move |path: &Path| pty.has_working_dir(path))
    };
    let in_use = {
        let pty = pty.clone();
        let agents = host.clone();
        Arc::new(move |path: &Path| pty.has_working_dir(path) || agents.has_working_dir(path))
    };
    let local_projects = Arc::new(
        monocode_engine::projects::backend::LocalProjectsBackend::new(
            data_dir.clone(),
            store.clone(),
            terminals_in_use,
            in_use,
        ),
    );
    let project_backend = Arc::new(crate::bridge::projects::AppProjectsBackend::new(
        local_projects,
        RemoteGlobal::global(cx).client.clone(),
    ));
    ProjectsGlobal::init(
        monocode_engine::projects::ProjectsConfig {
            kv: kv.clone(),
            backend: project_backend,
            clock: monocode_engine::projects::system_clock(),
        },
        cx,
    );

    // History: the sidebar list, search, and notes.
    let notes = Arc::new(StoreNotesBackend::new(
        store.clone(),
        data_dir.clone(),
        cx.background_executor().clone(),
    ));
    HistoryPackage::init(HistoryConfig::new(kv.clone(), notes, cx), cx);

    // Inbox.
    let client = InboxClient::new(
        LiveInboxBackend::new(data_dir.clone()),
        kv.clone(),
        cx.background_executor().clone(),
    );
    Inbox::init(client, cx);

    // Automations, reminders, and quick launches.
    AutomationsPackage::init(
        AutomationsConfig::from_store(
            kv.clone(),
            store.clone(),
            Arc::new(NoStoreEvents),
            Some(platform),
            cx,
        ),
        cx,
    );

    // Orchestration and the agent app API behind the control server.
    if let Some(control) = control.clone() {
        let mut config = OrchestrationConfig::native(
            store.clone(),
            control,
            Some(host.clone()),
            CONTROL_OWNER,
            kv.clone(),
        );
        config.peers = Rc::new(bridge::AppOrchestrationPeers);
        Orchestration::init(config, cx);
    }

    cx.set_global(AppServices {
        data_dir: options.data_dir,
        kv,
        registry,
        children,
        host,
        catalog,
        availability,
        factory,
        store,
        control,
        pty,
        settings,
        _bridge: Rc::new(bridge),
    });

    // The calls between packages.
    bridge::install_peers(cx);
    start_schedules(options.run_schedules, cx);
    Ok(())
}

/// Load the automation and reminder lists. With `run`, also start the
/// scheduler, the reminder poller, and the quick composer shortcut
/// (`AutomationsPackage::start`).
fn start_schedules(run: bool, cx: &mut App) {
    if run {
        AutomationsPackage::start(cx);
        return;
    }
    let package = AutomationsPackage::global(cx);
    let (automations, reminders) = (package.automations.clone(), package.reminders.clone());
    automations.update(cx, |automations, cx| automations.refresh(cx).detach());
    reminders.update(cx, |reminders, cx| reminders.refresh(cx).detach());
}

/// What a window starts with.
pub struct RestoredWorkspace {
    pub config: WorkspaceConfig,
    /// Sidebar rows listed before first paint.
    pub history: Vec<SessionSummary>,
    pub history_cwd: Option<String>,
}

/// `loadBootWorkspace` and the mount effects around it: restore the
/// workspace the last quit saved, put its sessions in `Sessions` as already
/// saved, bind their provider threads, and refresh the model catalogs.
pub fn restore_workspace(cx: &mut App) -> Task<RestoredWorkspace> {
    let kv = AppServices::global(cx).kv.clone();
    let hinted = monocode_engine::projects::recents::last_project_path(&kv);
    let lifecycle = Engine::lifecycle(cx);
    let boot = lifecycle.update(cx, |lifecycle, cx| {
        lifecycle.load_boot_workspace(hinted.clone(), cx)
    });
    cx.spawn(async move |cx| {
        let boot = boot.await;
        cx.update(|cx| {
            let sessions = Engine::sessions(cx);
            let mut config = match &boot.resumed {
                Some(resumed) => {
                    sessions.update(cx, |sessions, cx| {
                        sessions.adopt_restored(&resumed.sessions);
                        sessions.set_all(resumed.sessions.clone(), cx);
                    });
                    bind_resumed_sessions(&resumed.sessions, &Engine::hooks(cx), cx);
                    WorkspaceConfig::resumed(resumed)
                }
                None => WorkspaceConfig::fresh(hinted.as_deref()),
            };
            config.kv = Some(kv);
            lifecycle.update(cx, |lifecycle, _| lifecycle.attach_workspace());
            sessions.update(cx, |sessions, cx| sessions.refresh_models(cx));
            RestoredWorkspace {
                config,
                history: boot.history,
                history_cwd: boot.history_cwd,
            }
        })
    })
}

/// Settle the store before the process exits: flush pending session
/// writes and the settings file.
pub fn flush(cx: &App) -> Task<()> {
    let writer = Engine::writer(cx);
    let kv = AppServices::try_global(cx).map(|services| services.kv.clone());
    let flush = writer.flush_session_writes();
    cx.background_spawn(async move {
        flush.await;
        if let Some(kv) = kv {
            let _ = kv.flush();
        }
    })
}

/// What the app does on its way out: settle the store and the settings,
/// then stop every agent CLI it started.
pub fn shutdown(cx: &App) -> Task<()> {
    let flush = flush(cx);
    let host = AppServices::try_global(cx).map(|services| services.host.clone());
    cx.background_spawn(async move {
        flush.await;
        if let Some(host) = host {
            smol::unblock(move || host.kill_all()).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_harness::core::provider_binary_paths::STORAGE_KEY;

    #[test]
    fn provider_paths_take_effect_on_the_next_app_launch() {
        let kv = Kv::in_memory();
        kv.set_item(STORAGE_KEY, r#"{"codex":"/configured/codex"}"#);
        let first = HarnessHost::default();
        initialize_provider_binary_paths(&first, &kv);
        assert_eq!(
            first.runtime_binary_path("codex").as_deref(),
            Some("/configured/codex")
        );

        kv.set_item(STORAGE_KEY, r#"{"codex":"/changed/codex"}"#);
        initialize_provider_binary_paths(&first, &kv);
        assert_eq!(
            first.runtime_binary_path("codex").as_deref(),
            Some("/configured/codex")
        );
        let second = HarnessHost::default();
        initialize_provider_binary_paths(&second, &kv);
        assert_eq!(
            second.runtime_binary_path("codex").as_deref(),
            Some("/changed/codex")
        );
    }
}
