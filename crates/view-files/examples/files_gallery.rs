//! The file views on this repository: the explorer tree, quick open, the
//! command palette, the explorer menu, and a file pane with a Markdown
//! preview and its find bar.
//!
//! ```text
//! cargo run -p monocode-view-files --example files_gallery -- [options]
//!   --view <name>        workspace (default), tree, tree-menu, picker,
//!                        palette, find, editor
//!   --light              light theme
//!   --screenshot <png>   write what the window draws, then quit
//! ```

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Window, WindowBounds, WindowOptions, div,
    point, px, size,
};
use gpui_component::Root;
use monocode_layout::{EditorPane, new_editor_pane, new_file_tab};
use monocode_ui::{AppearanceSettings, IconName, Theme, ThemePreference, UiStyled as _, icon, u};
use monocode_view_files::{
    FileEditorSurface, FilePane, FilePicker, FileTree, FilesData, GitStatusMap, LocalFiles,
    file_pane::Surface as PaneSurface,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Workspace,
    Tree,
    TreeMenu,
    Picker,
    Palette,
    Find,
    Editor,
}

struct Options {
    view: View,
    light: bool,
    screenshot: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        view: View::Workspace,
        light: false,
        screenshot: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => options.light = true,
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            "--view" => {
                options.view = match args.next().as_deref() {
                    Some("tree") => View::Tree,
                    Some("tree-menu") => View::TreeMenu,
                    Some("picker") => View::Picker,
                    Some("palette") => View::Palette,
                    Some("find") => View::Find,
                    Some("editor") => View::Editor,
                    Some("workspace") | None => View::Workspace,
                    Some(other) => {
                        eprintln!("unknown view {other}");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    options
}

/// The repository root, two folders above this crate.
fn repo_root() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(root)
        .expect("repo root")
        .to_string_lossy()
        .into_owned()
}

struct Gallery {
    view: View,
    tree: Entity<FileTree>,
    pane: Option<Entity<FilePane>>,
    picker: Option<Entity<FilePicker>>,
}

/// A stand-in for `SurfaceTabs`, which another crate ports: the active
/// tab's icon, name, and close button.
fn tab_strip(pane: &EditorPane, _: &mut Window, cx: &mut App) -> AnyElement {
    let theme = Theme::of(cx).clone();
    let mut strip = div()
        .flex()
        .h(u(36.))
        .flex_none()
        .items_center()
        .border_b_1()
        .border_color(theme.colors.stroke);
    for file in &pane.files {
        let name = monocode_view_files::paths::basename(&file.path);
        let active = file.id == pane.active_file_id;
        strip = strip.child(
            div()
                .flex()
                .h_full()
                .items_center()
                .gap(u(8.))
                .px(u(12.))
                .border_r_1()
                .border_color(theme.colors.stroke)
                .text_px(theme.text.body)
                .text_color(if active {
                    theme.colors.content
                } else {
                    theme.content(0.50)
                })
                .child(
                    icon(IconName::GripVertical)
                        .size(u(14.))
                        .text_color(theme.content(0.35)),
                )
                .child(monocode_ui::file_type_icon(name.clone()))
                .child(name)
                .child(
                    icon(IconName::X)
                        .size(u(14.))
                        .text_color(theme.content(0.45)),
                ),
        );
    }
    strip.into_any_element()
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let sidebar = div()
            .flex()
            .flex_col()
            .w(u(300.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.colors.stroke)
            .bg(theme.colors.sidebar_pane)
            .child(self.tree.clone());
        let mut root = div()
            .id("gallery")
            .flex()
            .size_full()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .child(sidebar);
        if self.view != View::Tree && self.view != View::TreeMenu {
            root = root.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(self.pane.clone()),
            );
        }
        root.children(self.picker.clone())
    }
}

fn main() {
    let options = parse_options();
    let cwd = repo_root();
    let markdown = format!("{cwd}/docs/gpui-port-conventions.md");
    let rust = format!("{cwd}/crates/view-files/src/paths.rs");

    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            monocode_ui::init(
                AppearanceSettings {
                    theme_preference: if options.light {
                        ThemePreference::Light
                    } else {
                        ThemePreference::Dark
                    },
                    ..AppearanceSettings::default()
                },
                cx,
            );
            monocode_editor::init(cx);
            monocode_markdown::init(cx);
            monocode_view_files::init(cx);

            let files = LocalFiles::new();
            let data: Rc<dyn FilesData> = Rc::new(files.clone());
            // Open the folders this crate lives in, and select a file.
            let expanded = [
                cwd.clone(),
                format!("{cwd}/crates"),
                format!("{cwd}/crates/view-files"),
                format!("{cwd}/crates/view-files/src"),
            ];
            files
                .explorer()
                .save_expanded(&cwd, expanded.into_iter().collect());
            files.explorer().save_selected(
                &cwd,
                Some(format!("{cwd}/crates/view-files/src/file_tree.rs")),
            );
            let statuses = GitStatusMap::from_changed_files(
                &monocode_git::fs::git_diff_files(cwd.clone()).files,
                &cwd,
            );

            let (width, height) = match options.view {
                View::Tree | View::TreeMenu => (300., 760.),
                _ => (1180., 760.),
            };
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            let view = options.view;
            let screenshot = options.screenshot.clone();
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        // A screenshot run must not steal focus from the
                        // user's other windows.
                        focus: options.screenshot.is_none(),
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let tree = cx.new(|cx| {
                            let mut tree = FileTree::new(data.clone(), cwd.clone(), window, cx);
                            tree.set_git_statuses(statuses.clone(), cx);
                            tree.set_search_enabled(true, cx);
                            tree.set_open_terminal_enabled(true, cx);
                            tree.set_animate_menus(false);
                            tree
                        });
                        let pane = (!matches!(view, View::Tree | View::TreeMenu)).then(|| {
                            let path = if view == View::Editor {
                                &rust
                            } else {
                                &markdown
                            };
                            let mut pane =
                                new_editor_pane(new_file_tab(path, &cwd, false, None, None));
                            if view == View::Workspace {
                                pane.files
                                    .push(new_file_tab(&rust, &cwd, false, None, None));
                            }
                            cx.new(|cx| {
                                let mut pane = FilePane::new(data.clone(), pane, None, window, cx);
                                pane.set_tab_strip(Some(Rc::new(tab_strip)), cx);
                                pane.set_focused(true, window, cx);
                                pane
                            })
                        });
                        let picker = matches!(view, View::Picker | View::Palette).then(|| {
                            let query = if view == View::Palette { ">" } else { "tree" };
                            cx.new(|cx| {
                                FilePicker::new(
                                    data.clone(),
                                    cwd.clone(),
                                    Vec::new(),
                                    query,
                                    window,
                                    cx,
                                )
                            })
                        });
                        let gallery = cx.new(|_| Gallery {
                            view,
                            tree,
                            pane,
                            picker,
                        });
                        cx.new(|cx| Root::new(gallery, window, cx))
                    },
                )
                .expect("open window");

            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1200))
                    .await;
                let _ = window.update(cx, |root, window, cx| {
                    let Ok(gallery) = root.view().clone().downcast::<Gallery>() else {
                        return;
                    };
                    let (tree, pane) = {
                        let gallery = gallery.read(cx);
                        (gallery.tree.clone(), gallery.pane.clone())
                    };
                    if view == View::TreeMenu {
                        tree.update(cx, |tree, cx| {
                            let target = format!("{}/crates/view-files/src/data.rs", tree.cwd());
                            tree.open_menu_for(&target, point(px(150.), px(330.)), window, cx);
                        });
                    }
                    if view == View::Find
                        && let Some(pane) = pane
                    {
                        let id = pane.read(cx).pane().active_file_id.clone();
                        let surface: Option<Entity<FileEditorSurface>> =
                            match pane.read(cx).surface(&id) {
                                Some(PaneSurface::Editor(editor)) => Some(editor.clone()),
                                _ => None,
                            };
                        if let Some(search) =
                            surface.and_then(|surface| surface.read(cx).preview_search().cloned())
                        {
                            search.update(cx, |search, cx| {
                                search.open(window, cx);
                                let input = search.query_input().clone();
                                input.update(cx, |input, cx| input.set_value("crate", window, cx));
                                search.refresh(cx);
                                search.step(1, cx);
                            });
                        }
                    }
                });
                let Some(screenshot) = screenshot else {
                    return;
                };
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                let any_window: gpui::AnyWindowHandle = window.into();
                let result = any_window.update(cx, |_, window, cx| {
                    // Keep hover styles out of the capture.
                    window.dispatch_event(
                        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position: point(px(-1000.), px(-1000.)),
                            ..Default::default()
                        }),
                        cx,
                    );
                    window.draw(cx).clear();
                    window
                        .render_to_image()
                        .map(|image| image.save(&screenshot))
                });
                match result {
                    Ok(Ok(Ok(()))) => eprintln!("wrote {}", screenshot.display()),
                    other => eprintln!("screenshot failed: {other:?}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
            if options.screenshot.is_none() {
                cx.activate(true);
            }
        });
}
