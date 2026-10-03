//! Renders the settings page to PNGs, offscreen.
//!
//! ```sh
//! cargo run -p monocode-view-settings --example settings_gallery -- target/settings-gallery [scene]
//! ```
//!
//! Each scene opens a headless window (it never takes focus) with the
//! platform text system and the headless renderer, waits for images and
//! animations to settle, and writes `<scene>.png` through
//! `Window::render_to_image`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, HeadlessAppContext, IntoElement,
    ParentElement as _, Render, Styled as _, Task, Window, WindowHandle, div, px, size,
};
use image::{Rgba, RgbaImage};
use monocode_core::Platform;
use monocode_core::appearance::{
    CHAT_BACKGROUND_PATH_KEY, NEW_THREAD_BACKGROUND_EFFECT_KEY, NewThreadBackgroundEffect,
};
use monocode_core::harness::HarnessId;
use monocode_core::models::HarnessAvailability;
use monocode_core::settings::SettingsSectionId;
use monocode_settings::Kv;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_settings::settings::page::SectionBody;
use monocode_view_settings::settings::{
    ArchiveHost, ArchivedProject, BinaryInspection, ChatBackground, GeneralHost, GithubStatus,
    HostTask, InboxHost, JiraProject, JiraStatus, KeybindingsHost, LinearTeam,
    ProjectBackgroundDialog, ProjectBackgroundHost, ProjectBackgroundSettings, ProvidersHost,
    SessionSummary, SettingsCallbacks, SettingsHosts, SettingsPage, SettingsProps, UrlStatus,
};

/// One host for every section, with plausible data.
struct GalleryHost {
    background: String,
}

impl GeneralHost for GalleryHost {
    fn app_version(&self, _: &mut App) -> Task<String> {
        Task::ready("0.6.0".into())
    }
}

impl KeybindingsHost for GalleryHost {}

impl monocode_view_settings::settings::AppearanceHost for GalleryHost {}

impl ProvidersHost for GalleryHost {
    fn availability(&self, _: &App) -> HarnessAvailability {
        HarnessAvailability {
            installed: [
                HarnessId::Claude,
                HarnessId::Codex,
                HarnessId::Cursor,
                HarnessId::Opencode,
            ]
            .into_iter()
            .collect(),
            probed: true,
        }
    }

    fn harness_unavailable_hint(&self, harness: HarnessId) -> String {
        format!(
            "{} not found. Install it, or restart MonoCode if it is already installed.",
            harness.title()
        )
    }

    fn inspect_binary(
        &self,
        provider: HarnessId,
        path: Option<&str>,
        _: &mut App,
    ) -> HostTask<BinaryInspection> {
        Task::ready(Ok(BinaryInspection {
            path: path
                .map(str::to_string)
                .unwrap_or_else(|| format!("/opt/homebrew/bin/{}", provider.as_str())),
            version: Some(match provider {
                HarnessId::Codex => "codex-cli 0.156.1".into(),
                HarnessId::Opencode => "opencode 1.18.32".into(),
                _ => "2.1.4".into(),
            }),
            error: None,
        }))
    }
}

impl InboxHost for GalleryHost {
    fn github_status(&self, _: &mut App) -> HostTask<GithubStatus> {
        Task::ready(Ok(GithubStatus {
            installed: true,
            connected: true,
        }))
    }

    fn gitlab_status(&self, _: &mut App) -> HostTask<UrlStatus> {
        Task::ready(Ok(UrlStatus {
            connected: false,
            url: "https://gitlab.com".into(),
        }))
    }

    fn azure_devops_status(&self, _: &mut App) -> HostTask<UrlStatus> {
        Task::ready(Ok(UrlStatus {
            connected: true,
            url: "https://dev.azure.com/monocode".into(),
        }))
    }

    fn linear_connected(&self, _: &mut App) -> HostTask<bool> {
        Task::ready(Ok(true))
    }

    fn list_linear_teams(&self, _: &mut App) -> HostTask<Vec<LinearTeam>> {
        Task::ready(Ok(vec![
            LinearTeam {
                id: "t1".into(),
                name: "Platform".into(),
                key: Some("PLT".into()),
            },
            LinearTeam {
                id: "t2".into(),
                name: "Design".into(),
                key: Some("DES".into()),
            },
        ]))
    }

    fn jira_status(&self, _: &mut App) -> HostTask<JiraStatus> {
        Task::ready(Ok(JiraStatus {
            connected: true,
            site: "acme.atlassian.net".into(),
            email: "ada@example.com".into(),
        }))
    }

    fn list_jira_projects(&self, _: &mut App) -> HostTask<Vec<JiraProject>> {
        Task::ready(Ok(vec![
            JiraProject {
                id: "10000".into(),
                key: "ENG".into(),
                name: "Engineering".into(),
            },
            JiraProject {
                id: "10001".into(),
                key: "OPS".into(),
                name: "Operations".into(),
            },
        ]))
    }
}

impl ArchiveHost for GalleryHost {
    fn archived_projects(&self, _: &App) -> Vec<ArchivedProject> {
        vec![ArchivedProject {
            path: "/Users/ada/src/old-site".into(),
            label: "old-site".into(),
        }]
    }
}

impl ProjectBackgroundHost for GalleryHost {
    fn load_settings(&self, _: &str, _: &App) -> Option<ProjectBackgroundSettings> {
        Some(ProjectBackgroundSettings {
            path: self.background.clone(),
            empty_opacity: 0.4,
            session_opacity: 0.24,
            scope: monocode_core::appearance::ChatBackgroundScope::All,
            effect: NewThreadBackgroundEffect::Halftone,
        })
    }
}

/// The window body: the page on the theme background.
struct Stage {
    view: AnyView,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(self.view.clone())
    }
}

type Build = Box<dyn FnOnce(&Kv, &mut Window, &mut App) -> AnyView>;
type After = Box<dyn FnOnce(&AnyView, &mut Window, &mut App)>;
type PathHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;

struct Scene {
    name: &'static str,
    width: f32,
    height: f32,
    light: bool,
    /// Runs after the first frames, once layout exists.
    after: Option<After>,
    build: Build,
}

fn hosts(background: &str) -> SettingsHosts {
    let host = Rc::new(GalleryHost {
        background: background.to_string(),
    });
    SettingsHosts {
        general: host.clone(),
        keybindings: host.clone(),
        appearance: host.clone(),
        providers: host.clone(),
        inbox: host.clone(),
        archive: host,
        ..Default::default()
    }
}

fn props() -> SettingsProps {
    SettingsProps {
        cwd: "/Users/ada/src/monocode".into(),
        recents: vec![
            "/Users/ada/src/monocode".into(),
            "/Users/ada/src/comet".into(),
        ],
        sessions: vec![
            SessionSummary {
                id: "s1".into(),
                title: "Port the settings page".into(),
                harness: HarnessId::Claude,
                updated_at: 1_790_942_400_000,
                archived: true,
            },
            SessionSummary {
                id: "s2".into(),
                title: "codex · Fix the flaky snapshot test".into(),
                harness: HarnessId::Codex,
                updated_at: 1_790_600_000_000,
                archived: true,
            },
        ],
        ..Default::default()
    }
}

fn page(
    section: SettingsSectionId,
    background: &str,
    kv: &Kv,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SettingsPage> {
    let (kv, hosts) = (kv.clone(), hosts(background));
    let noop: Option<PathHandler> = Some(Rc::new(|_, _, _| {}));
    let callbacks = SettingsCallbacks {
        on_restore_project: noop.clone(),
        on_delete_project: noop,
        ..Default::default()
    };
    cx.new(|cx| {
        SettingsPage::new(
            kv,
            Platform::Mac,
            hosts,
            section,
            props(),
            callbacks,
            window,
            cx,
        )
    })
}

fn scene(
    name: &'static str,
    section: SettingsSectionId,
    background: Option<NewThreadBackgroundEffect>,
    path: &str,
) -> Scene {
    let path = path.to_string();
    Scene {
        name,
        width: 1100.,
        height: 900.,
        light: false,
        after: None,
        build: Box::new(move |kv, window, cx| {
            if let Some(effect) = background {
                kv.set_item(CHAT_BACKGROUND_PATH_KEY, &path);
                kv.set_item(NEW_THREAD_BACKGROUND_EFFECT_KEY, effect.as_str());
                ChatBackground::apply_chat_background(Some(&path), effect, false, cx);
            }
            page(section, &path, kv, window, cx).into()
        }),
    }
}

fn with_after(
    mut scene: Scene,
    after: impl FnOnce(&Entity<SettingsPage>, &mut Window, &mut App) + 'static,
) -> Scene {
    scene.after = Some(Box::new(move |view, window, cx| {
        let page = view
            .clone()
            .downcast::<SettingsPage>()
            .expect("a settings page");
        after(&page, window, cx);
    }));
    scene
}

fn scenes(background: &str) -> Vec<Scene> {
    use SettingsSectionId as S;
    let mut list = vec![
        scene("general", S::General, None, background),
        scene("connections", S::Connections, None, background),
        scene("appearance", S::Appearance, None, background),
        scene("keybindings", S::Keybindings, None, background),
        scene("chat", S::Chat, None, background),
        scene("providers", S::Providers, None, background),
        scene("mcp", S::Mcp, None, background),
        scene("skills", S::Skills, None, background),
        scene("inbox", S::Inbox, None, background),
        scene("worktrees", S::Worktrees, None, background),
        scene("archive", S::Archive, None, background),
    ];
    for (name, effect) in [
        ("appearance-background", NewThreadBackgroundEffect::None),
        ("appearance-dither", NewThreadBackgroundEffect::Dither),
        ("appearance-ascii", NewThreadBackgroundEffect::Ascii),
        ("appearance-halftone", NewThreadBackgroundEffect::Halftone),
        ("appearance-scanlines", NewThreadBackgroundEffect::Scanlines),
        ("appearance-haze", NewThreadBackgroundEffect::GradientBlur),
    ] {
        list.push(with_after(
            scene(name, S::Appearance, Some(effect), background),
            |page, _, cx| page.update(cx, |page, cx| page.scroll_to("chat-background", cx)),
        ));
    }
    list.push(with_after(
        scene("search", S::General, None, background),
        |page, window, cx| {
            let search = page.read(cx).search().clone();
            search.update(cx, |search, cx| {
                search.set_query("notification", window, cx)
            });
        },
    ));
    list.push(with_after(
        scene("appearance-scale-menu", S::Appearance, None, background),
        |page, window, cx| {
            if let SectionBody::Appearance(section) = page.read(cx).body().clone() {
                let select = section.read(cx).ui_scale_select().clone();
                select.update(cx, |select, cx| select.toggle(window, cx));
            }
            page.update(cx, |page, cx| page.scroll_to("interface-scale", cx));
        },
    ));
    list.push(with_after(
        scene("keybindings-recording", S::Keybindings, None, background),
        |page, window, cx| {
            if let SectionBody::Keybindings(section) = page.read(cx).body().clone()
                && let Some(editor) = section.read(cx).editor("App: Search").cloned()
            {
                editor.update(cx, |editor, cx| editor.begin_recording(window, cx));
            }
        },
    ));
    list.push(with_after(
        scene("providers-binary", S::Providers, None, background),
        |page, window, cx| {
            if let SectionBody::Providers(section) = page.read(cx).body().clone()
                && let Some(control) = section.read(cx).binary(HarnessId::Codex).cloned()
            {
                control.update(cx, |control, cx| control.toggle(window, cx));
            }
        },
    ));
    list.push(with_after(
        scene("general-revealed", S::General, None, background),
        |page, _, cx| {
            page.update(cx, |page, cx| page.reveal(Some("notes".into()), cx));
        },
    ));
    let dialog_background = background.to_string();
    list.push(Scene {
        name: "project-background-dialog",
        width: 900.,
        height: 760.,
        light: false,
        after: None,
        build: Box::new(move |kv, _, cx| {
            let host = Rc::new(GalleryHost {
                background: dialog_background.clone(),
            });
            let kv = kv.clone();
            cx.new(|cx| {
                ProjectBackgroundDialog::new("/Users/ada/src/monocode", "monocode", kv, host, cx)
            })
            .into()
        }),
    });
    let mut narrow = scene("general-narrow", S::General, None, background);
    narrow.width = 520.;
    list.push(narrow);
    let mut light = scene("appearance-light", S::Appearance, None, background);
    light.light = true;
    list.push(light);
    let mut light_general = scene("general-light", S::General, None, background);
    light_general.light = true;
    list.push(light_general);
    list
}

/// A colorful test card to apply the effects to.
fn write_background(path: &Path) {
    let (width, height) = (960u32, 600u32);
    let image = RgbaImage::from_fn(width, height, |x, y| {
        let fx = x as f32 / width as f32;
        let fy = y as f32 / height as f32;
        let (cx, cy) = (fx - 0.62, fy - 0.42);
        let sun = (1.0 - ((cx * cx + cy * cy).sqrt() / 0.22)).clamp(0.0, 1.0);
        let r = (40.0 + 180.0 * fx + 200.0 * sun).min(255.0);
        let g = (60.0 + 90.0 * (1.0 - fy) + 160.0 * sun).min(255.0);
        let b = (140.0 + 100.0 * fy - 60.0 * sun).clamp(0.0, 255.0);
        let hills = fy > 0.7 + 0.08 * (fx * 9.0).sin();
        if hills {
            Rgba([30, (90.0 + 60.0 * fx) as u8, 70, 255])
        } else {
            Rgba([r as u8, g as u8, b as u8, 255])
        }
    });
    image.save(path).expect("write the background");
}

fn capture(cx: &mut HeadlessAppContext, scene: Scene, out: &Path) {
    let appearance = AppearanceSettings {
        theme_preference: if scene.light {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        },
        ..Default::default()
    };
    cx.update(|cx| monocode_ui::set_appearance(appearance, cx));
    let kv = Kv::in_memory();
    if scene.light {
        kv.set_item(monocode_core::appearance::SCHEME_KEY, "light");
    }
    let build = scene.build;
    let window: WindowHandle<Stage> = cx
        .open_window(
            size(px(scene.width), px(scene.height)),
            move |window, cx| {
                monocode_ui::sync_window(window, cx);
                let view = build(&kv, window, cx);
                cx.new(|_| Stage { view })
            },
        )
        .expect("open window");
    let draw = |cx: &mut HeadlessAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    };
    for _ in 0..3 {
        draw(cx);
    }
    if let Some(after) = scene.after {
        cx.update_window(window.into(), |root, window, cx| {
            let stage = root.downcast::<Stage>().expect("stage");
            let view = stage.read(cx).view.clone();
            after(&view, window, cx);
        })
        .expect("after");
    }
    // Let assets load, background effects render, and animations finish.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(900) {
        draw(cx);
        std::thread::sleep(Duration::from_millis(16));
    }
    draw(cx);
    let image = cx.capture_screenshot(window.into()).expect("capture");
    let path = out.join(format!("{}.png", scene.name));
    image.save(&path).expect("save png");
    eprintln!(
        "wrote {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .ok();
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/settings-gallery"));
    let only = std::env::args().nth(2);
    std::fs::create_dir_all(&out).expect("create output dir");
    let background = out.join("background.png");
    write_background(&background);
    let background = background.to_string_lossy().to_string();
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(monocode_ui::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
    });
    for scene in scenes(&background) {
        if only.as_deref().is_some_and(|only| only != scene.name) {
            continue;
        }
        capture(&mut cx, scene, &out);
    }
}
