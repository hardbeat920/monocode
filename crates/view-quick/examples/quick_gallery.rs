//! Draws the quick composer and its pickers without showing or focusing a window.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, AsyncApp, Bounds, Context, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::Root;
use monocode_core::models::{LastModelChoice, ModelCatalog};
use monocode_core::{Attachment, HarnessId};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_quick::{
    GitBranchInfo, GitBranches, HostTask, NativeClipboard, Picker, QuickComposer,
    QuickComposerHost, QuickGitAnchor, QuickGitHost, QuickGitKind, QuickGitPopup, QuickGitRequest,
    QuickLaunchRequest, QuickSnapshot, QuickWorkspace, Worktree,
};

struct GalleryHost;
impl QuickGitHost for GalleryHost {
    fn branches(&self, _: &str, _: &mut App) -> Task<Option<GitBranches>> {
        Task::ready(Some(branches()))
    }
    fn worktrees(&self, _: &str, _: &mut App) -> HostTask<Vec<Worktree>> {
        Task::ready(Ok(vec![Worktree::linked(
            "/Users/dev/monocode-search",
            "feature/search",
            "abc1234",
        )]))
    }
    fn checkout(&self, _: &str, _: &str, _: Option<&str>, _: bool, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn create_branch(&self, _: &str, _: &str, _: bool, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn stash(&self, _: &str, _: &str, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
    fn commit_all(&self, _: &str, _: &str, _: &mut App) -> HostTask<()> {
        Task::ready(Ok(()))
    }
}
impl QuickComposerHost for GalleryHost {
    fn snapshot(&self, _: &mut App) -> QuickSnapshot {
        let catalog = ModelCatalog::new();
        let choice = LastModelChoice {
            harness: HarnessId::Claude,
            model: catalog.default_model_id(HarnessId::Claude),
        };
        let mut snapshot = QuickSnapshot::new(choice);
        snapshot.projects = vec![
            "/Users/dev/monocode".into(),
            "/Users/dev/arcade".into(),
            "/Users/dev/website".into(),
        ];
        snapshot.initial_project = snapshot.projects.first().cloned();
        snapshot.catalog = catalog;
        snapshot.available = Some(vec![HarnessId::Claude, HarnessId::Codex, HarnessId::Cursor]);
        snapshot
    }
    fn submit(&self, request: QuickLaunchRequest, _: &mut App) -> HostTask<()> {
        eprintln!("launch {}", request.prompt);
        Task::ready(Ok(()))
    }
    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn attachments_from_paths(&self, _: Vec<String>, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn attachments_from_files(
        &self,
        _: Vec<ClipboardFile>,
        _: &mut App,
    ) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(Vec::new()))
    }
    fn native_clipboard(&self, _: &str, _: &mut App) -> HostTask<NativeClipboard> {
        Task::ready(Ok(NativeClipboard::default()))
    }
    fn store_attachments(&self, files: Vec<Attachment>, _: &mut App) -> HostTask<Vec<Attachment>> {
        Task::ready(Ok(files))
    }
    fn capture_screenshot(&self, _: &mut Window, _: &mut App) -> HostTask<Option<String>> {
        Task::ready(Ok(None))
    }
}
fn branches() -> GitBranches {
    GitBranches {
        current: Some("main".into()),
        detached: false,
        branches: vec![
            GitBranchInfo::local("main", true),
            GitBranchInfo::local("feature/search", false),
            GitBranchInfo::remote("develop", "origin"),
        ],
    }
}
struct Stage {
    view: AnyView,
}
impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .bg(theme.colors.background_base)
            .child(self.view.clone())
    }
}
fn main() {
    let mut args = std::env::args().skip(1);
    let mut scene = "composer".to_string();
    let mut screenshot = None;
    let mut light = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scene" => scene = args.next().expect("--scene needs a value"),
            "--screenshot" => {
                screenshot = Some(PathBuf::from(
                    args.next().expect("--screenshot needs a path"),
                ))
            }
            "--theme" => light = args.next().as_deref() == Some("light"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            monocode_ui::init(
                AppearanceSettings {
                    theme_preference: if light {
                        ThemePreference::Light
                    } else {
                        ThemePreference::Dark
                    },
                    ..Default::default()
                },
                cx,
            );
            monocode_view_composer::composer::init(cx);
            monocode_view_quick::init(cx);
            let git_scene = matches!(
                scene.as_str(),
                "workspace" | "branch" | "base" | "worktrees" | "create"
            );
            let width = if git_scene { 320. } else { 680. };
            let height = if git_scene || scene != "composer" {
                440.
            } else {
                128.
            };
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(width), px(height)),
                            cx,
                        ))),
                        focus: false,
                        show: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let host = Rc::new(GalleryHost);
                        let view: AnyView = if git_scene {
                            cx.new(|cx| {
                                let mut view = QuickGitPopup::new(host, window, cx);
                                let kind = match scene.as_str() {
                                    "workspace" | "worktrees" => QuickGitKind::Workspace,
                                    "base" => QuickGitKind::Base,
                                    _ => QuickGitKind::Branch,
                                };
                                view.open(
                                    QuickGitRequest {
                                        id: "gallery".into(),
                                        kind,
                                        choice: QuickWorkspace::current(Some(
                                            "/Users/dev/monocode",
                                        )),
                                        branches: Some(branches()),
                                        anchor: QuickGitAnchor {
                                            x: 0.,
                                            y: 0.,
                                            width: 100.,
                                            height: 24.,
                                        },
                                    },
                                    window,
                                    cx,
                                );
                                if scene == "worktrees" {
                                    view.open_worktree_menu(cx);
                                }
                                if scene == "create" {
                                    view.pick_branch(3, window, cx);
                                }
                                view
                            })
                            .into()
                        } else {
                            cx.new(|cx| {
                                let mut view = QuickComposer::new(host, window, cx);
                                let text = if scene == "commands" {
                                    "/plan "
                                } else {
                                    "Fix the transcript scroll position after a session reload"
                                };
                                view.prompt().update(cx, |prompt, cx| {
                                    prompt.set_text(text, 0, cx);
                                });
                                match scene.as_str() {
                                    "projects" => view.open_picker(Picker::Project, window, cx),
                                    "models" => view.open_picker(Picker::Model, window, cx),
                                    "attachments" => {
                                        view.open_picker(Picker::Attachments, window, cx)
                                    }
                                    "commands" => {
                                        view.prompt()
                                            .update(cx, |prompt, cx| prompt.set_text("/", 1, cx));
                                    }
                                    "composer" => {}
                                    _ => panic!("unknown scene {scene}"),
                                }
                                view
                            })
                            .into()
                        };
                        let stage = cx.new(|_| Stage { view });
                        cx.new(|cx| Root::new(stage, window, cx))
                    },
                )
                .expect("gallery window");
            if let Some(out) = screenshot.clone() {
                capture_and_quit(window.into(), out, cx);
            }
        });
}
fn capture_and_quit(window: gpui::AnyWindowHandle, out: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for _ in 0..15 {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            window
                .update(cx, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })
                .ok();
        }
        let image = window
            .update(cx, |_, window, cx| {
                window.draw(cx).clear();
                window.render_to_image()
            })
            .expect("window")
            .expect("render");
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).expect("output directory");
        }
        image.save(&out).expect("PNG");
        eprintln!("wrote {}", out.display());
        cx.update(|cx| cx.quit());
    })
    .detach();
}
