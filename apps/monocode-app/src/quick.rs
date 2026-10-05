//! The native quick composer and its engine adapter.
use crate::adapters::git::AppGit;
use gpui::{App, AppContext as _, Entity, Global, Task, Window};
use monocode_app::boot::AppServices;
use monocode_core::models::{LastModelChoice, ModelPrefs};
use monocode_core::{Attachment, HarnessId};
use monocode_engine::{
    automations::{AutomationsPackage, quick_composer},
    submit::{
        Submit,
        attachments::{self, PastedFile},
    },
};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_quick::{
    QuickPanels,
    host::{HostTask, NativeClipboard, QuickComposerHost, QuickGitHost, QuickSnapshot},
    model::launch::{GitBranches, QuickLaunchRequest, Worktree},
};
use monocode_view_scm::GitBackend;
use std::{cell::RefCell, rc::Rc, sync::Arc};

struct Panels(Entity<QuickPanels>);
impl Global for Panels {}

pub fn init(cx: &mut App) {
    if !cfg!(target_os = "macos") || AppServices::try_global(cx).is_none() {
        return;
    }
    let panels = match QuickPanels::new(
        Rc::new(QuickHost {
            #[cfg(target_os = "macos")]
            captures: Arc::new(monocode_platform::screenshots::Captures::default()),
        }),
        cx,
    ) {
        Ok(panels) => panels,
        Err(error) => {
            log::error!("Could not start quick composer: {error}");
            return;
        }
    };
    cx.set_global(Panels(panels.clone()));
    let (pressed, presses) = async_channel::unbounded();
    let hotkeys = match monocode_platform::global_hotkey::GlobalHotkeys::new() {
        Ok(hotkeys) => hotkeys,
        Err(error) => {
            log::error!("Could not register quick composer shortcut: {error}");
            return;
        }
    };
    let app = Rc::new(QuickApp {
        panels: panels.clone(),
        hotkeys,
        slot: RefCell::default(),
        pressed,
    });
    let quick = AutomationsPackage::global(cx).quick_launch.clone();
    quick.update(cx, |quick, cx| {
        quick.set_app(app);
        if let Err(error) = quick.apply_shortcut(cx) {
            log::error!("Quick composer shortcut: {error}");
        }
    });
    cx.spawn(async move |cx| {
        while presses.recv().await.is_ok() {
            cx.update(|cx| {
                panels.update(cx, |panels, cx| {
                    if let Err(error) = panels.toggle(cx) {
                        log::error!("Quick composer: {error}");
                    }
                })
            });
        }
    })
    .detach();
}

struct QuickApp {
    panels: Entity<QuickPanels>,
    hotkeys: monocode_platform::global_hotkey::GlobalHotkeys,
    slot: RefCell<monocode_platform::global_hotkey::ShortcutSlot>,
    pressed: async_channel::Sender<()>,
}
impl monocode_engine::automations::quick_launch::QuickLaunchApp for QuickApp {
    fn set_shortcut(&self, enabled: bool, shortcut: &str, _: &mut App) -> Result<(), String> {
        let pressed = self.pressed.clone();
        self.slot
            .borrow_mut()
            .set_enabled(
                &self.hotkeys,
                enabled,
                Some(shortcut),
                Arc::new(move || {
                    let _ = pressed.try_send(());
                }),
            )
            .map(|_| ())
    }
    fn open_session_window(&self, reveal: bool, cx: &mut App) -> Result<String, String> {
        crate::shell::windows::open_workspace_window(reveal, cx)
    }
    fn hide_panel(&self, cx: &mut App) {
        self.panels.update(cx, |panels, cx| {
            let _ = panels.dismiss(cx);
        });
    }
}

struct QuickHost {
    #[cfg(target_os = "macos")]
    captures: Arc<monocode_platform::screenshots::Captures>,
}
impl QuickGitHost for QuickHost {
    fn branches(&self, cwd: &str, cx: &mut App) -> Task<Option<GitBranches>> {
        let args = serde_json::json!({ "cwd": cwd })
            .as_object()
            .cloned()
            .unwrap_or_default();
        if let Some(run) = monocode_engine::remote::invoke_workspace("git_branches", &args, cx) {
            return cx
                .background_spawn(async move { serde_json::from_value(run.await.ok()?).ok() });
        }
        let cwd = cwd.to_string();
        cx.background_spawn(async move {
            monocode_git::fs::git_branches(cwd)
                .ok()
                .and_then(|branches| serde_json::to_value(branches).ok())
                .and_then(|value| serde_json::from_value(value).ok())
        })
    }
    fn worktrees(&self, cwd: &str, cx: &mut App) -> HostTask<Vec<Worktree>> {
        let task = monocode_engine::projects::actions::list_worktrees(cwd, cx);
        cx.spawn(async move |_| {
            let worktrees = task.await?;
            worktrees
                .worktrees
                .into_iter()
                .map(|worktree| {
                    serde_json::to_value(worktree)
                        .and_then(serde_json::from_value)
                        .map_err(|error| error.to_string())
                })
                .collect()
        })
    }
    fn checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        force: bool,
        cx: &mut App,
    ) -> HostTask<()> {
        let (cwd, name, remote) = (
            cwd.to_string(),
            name.to_string(),
            remote.map(str::to_string),
        );
        let git = AppGit::new(cx);
        cx.background_spawn(async move {
            git.git_checkout(&cwd, &name, remote.as_deref(), force)
                .map(|_| ())
        })
    }
    fn create_branch(&self, cwd: &str, name: &str, force: bool, cx: &mut App) -> HostTask<()> {
        let (cwd, name) = (cwd.to_string(), name.to_string());
        let git = AppGit::new(cx);
        cx.background_spawn(async move { git.git_create_branch(&cwd, &name, force).map(|_| ()) })
    }
    fn stash(&self, cwd: &str, message: &str, cx: &mut App) -> HostTask<()> {
        let (cwd, message) = (cwd.to_string(), message.to_string());
        let git = AppGit::new(cx);
        cx.background_spawn(async move { git.git_stash(&cwd, Some(&message)) })
    }
    fn commit_all(&self, cwd: &str, message: &str, cx: &mut App) -> HostTask<()> {
        let (cwd, message) = (cwd.to_string(), message.to_string());
        let git = AppGit::new(cx);
        cx.background_spawn(async move {
            git.git_stage_all(&cwd)?;
            git.git_commit(&cwd, &message, false)
        })
    }
    fn git_changed(&self, cx: &mut App) {
        monocode_engine::projects::notify_git_changed(cx);
    }
}
impl QuickComposerHost for QuickHost {
    fn snapshot(&self, cx: &mut App) -> QuickSnapshot {
        use monocode_engine::workspace::SessionFactory as _;
        let services = AppServices::global(cx);
        let projects = quick_composer::load_quick_projects(&services.kv);
        let initial_project = quick_composer::initial_quick_project(&services.kv, &projects);
        let seed = services
            .factory
            .new_default_session(initial_project.as_deref().unwrap_or("~"), None);
        let mut snapshot = QuickSnapshot::new(LastModelChoice {
            harness: seed.harness,
            model: seed.model,
        });
        snapshot.model_settings = seed.model_settings;
        snapshot.catalog = services.catalog.read().clone();
        snapshot.prefs = ModelPrefs::from_local_storage(|key| services.kv.get_item(key));
        snapshot.available = services
            .availability
            .has_probed_harness_availability()
            .then(|| {
                monocode_core::harness::HARNESSES
                    .iter()
                    .copied()
                    .filter(|id| services.availability.is_harness_available(*id))
                    .collect()
            });
        snapshot.projects = projects;
        snapshot.initial_project = initial_project;
        if let Some(projects) = monocode_engine::projects::ProjectsGlobal::try_global(cx) {
            let projects = projects.projects.clone();
            snapshot.appearance = projects.update(cx, |projects, _| {
                monocode_view_quick::model::appearance::ProjectAppearance {
                    logos: projects.logos(),
                    mascots: projects.mascots(),
                    colors: projects.colors(),
                    custom_colors: projects.custom_colors(),
                }
            });
        }
        snapshot
    }
    fn request_catalog(&self, harness: HarnessId, cx: &mut App) {
        let services = AppServices::global(cx);
        let (registry, catalog) = (services.registry.clone(), services.catalog.clone());
        let panels = cx.try_global::<Panels>().map(|panels| panels.0.clone());
        cx.spawn(async move |cx| {
            registry
                .refresh_harness_catalogs(vec![harness], false, |id| catalog.has_live_catalog(id))
                .await;
            if let Some(panels) = panels {
                cx.update(|cx| {
                    let composer = panels.read(cx).composer().clone();
                    composer.update(cx, |composer, cx| {
                        composer.set_catalog(catalog.read().clone(), None, cx)
                    });
                });
            }
        })
        .detach();
    }
    fn submit(&self, request: QuickLaunchRequest, cx: &mut App) -> HostTask<()> {
        let request = serde_json::to_value(request)
            .and_then(serde_json::from_value)
            .map_err(|error| error.to_string());
        let result = request.and_then(|request| {
            AutomationsPackage::global(cx)
                .quick_launch
                .clone()
                .update(cx, |quick, cx| quick.submit(request, cx))
                .map(|_| ())
        });
        Task::ready(result)
    }
    fn remember(&self, request: &QuickLaunchRequest, cx: &mut App) {
        let kv = &AppServices::global(cx).kv;
        quick_composer::remember_quick_project(kv, &request.cwd);
        if let Some(settings) = &request.model_settings {
            kv.set_item(
                monocode_core::models::LAST_MODEL_SETTINGS_KEY,
                &serde_json::to_string(settings).unwrap_or_default(),
            );
        }
        if let Some(model) = &request.model {
            kv.set_item(
                monocode_core::models::LAST_MODEL_KEY,
                &serde_json::to_string(&LastModelChoice {
                    harness: request.harness,
                    model: model.clone(),
                })
                .unwrap_or_default(),
            );
        }
    }
    fn save_favorites(&self, favorites: &[String], cx: &mut App) {
        AppServices::global(cx).kv.set_item(
            monocode_core::models::FAVORITES_KEY,
            &serde_json::to_string(favorites).unwrap_or_default(),
        );
    }
    fn pick_attachments(&self, _: &mut Window, cx: &mut App) -> HostTask<Vec<Attachment>> {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: None,
        });
        let io = Submit::global(cx).read(cx).config().attachment_io.clone();
        cx.background_spawn(async move {
            let paths = picked
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?
                .unwrap_or_default();
            let paths = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            attachments::attachments_from_paths(io.as_ref(), &paths).await
        })
    }
    fn attachments_from_paths(
        &self,
        paths: Vec<String>,
        cx: &mut App,
    ) -> HostTask<Vec<Attachment>> {
        let io = Submit::global(cx).read(cx).config().attachment_io.clone();
        cx.background_spawn(async move {
            attachments::attachments_from_paths(io.as_ref(), &paths).await
        })
    }
    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> HostTask<Vec<Attachment>> {
        let io = Submit::global(cx).read(cx).config().attachment_io.clone();
        let pasted = files
            .into_iter()
            .map(|file| PastedFile {
                name: file.name,
                mime_type: file.mime_type,
                bytes: file.bytes,
            })
            .collect::<Vec<_>>();
        cx.background_spawn(async move {
            attachments::attachments_from_files(io.as_ref(), &[], &pasted).await
        })
    }
    fn native_clipboard(&self, _: &str, cx: &mut App) -> HostTask<NativeClipboard> {
        let io = Submit::global(cx).read(cx).config().attachment_io.clone();
        cx.background_spawn(async move {
            let paths = monocode_platform::pasteboard::clipboard_file_paths()?;
            if !paths.is_empty() {
                return attachments::attachments_from_paths(io.as_ref(), &paths)
                    .await
                    .map(|files| NativeClipboard {
                        files,
                        warning: None,
                    });
            }
            let Ok(bytes) = monocode_platform::pasteboard::clipboard_image() else {
                return Ok(NativeClipboard::default());
            };
            let file = PastedFile {
                name: "Clipboard.png".into(),
                mime_type: "image/png".into(),
                bytes,
            };
            attachments::attachments_from_files(io.as_ref(), &[], &[file])
                .await
                .map(|files| NativeClipboard {
                    files,
                    warning: None,
                })
        })
    }
    fn store_attachments(&self, files: Vec<Attachment>, cx: &mut App) -> HostTask<Vec<Attachment>> {
        let io = Submit::global(cx).read(cx).config().attachment_io.clone();
        cx.background_spawn(async move {
            quick_composer::store_quick_attachments(io.as_ref(), files).await
        })
    }
    fn release_captures(&self, paths: Vec<String>, _: &mut App) {
        #[cfg(target_os = "macos")]
        let _ = self.captures.release(&paths);
        #[cfg(not(target_os = "macos"))]
        let _ = paths;
    }
    fn capture_screenshot(&self, _: &mut Window, cx: &mut App) -> HostTask<Option<String>> {
        #[cfg(target_os = "macos")]
        {
            let panels = cx.try_global::<Panels>().map(|panels| panels.0.clone());
            if let Some(panels) = &panels
                && let Err(error) = panels.update(cx, |panels, cx| panels.set_capturing(true, cx))
            {
                return Task::ready(Err(error));
            }
            let captures = self.captures.clone();
            let capturing = cx.background_spawn(async move {
                let result = monocode_platform::screenshots::capture_screenshot();
                if let Ok(Some(path)) = &result {
                    captures.register(path);
                }
                result
            });
            cx.spawn(async move |cx| {
                let result = capturing.await;
                if let Some(panels) = panels {
                    cx.update(|cx| {
                        let _ = panels.update(cx, |panels, cx| panels.set_capturing(false, cx));
                    });
                }
                result
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = cx;
            Task::ready(Err("Screenshot capture requires macOS.".into()))
        }
    }
}

pub fn set_shortcut(enabled: bool, shortcut: Option<&str>, cx: &mut App) -> Result<(), String> {
    if let Some(services) = AppServices::try_global(cx) {
        monocode_settings::settings_store::save_quick_composer_enabled(&services.kv, enabled);
        if let Some(shortcut) = shortcut {
            monocode_settings::settings_store::save_quick_composer_shortcut(
                &services.kv,
                shortcut,
                monocode_core::Platform::current(),
            )?;
        }
    }
    if let Some(package) = AutomationsPackage::try_global(cx) {
        let quick = package.quick_launch.clone();
        quick.update(cx, |quick, cx| quick.apply_shortcut(cx))
    } else {
        Ok(())
    }
}
