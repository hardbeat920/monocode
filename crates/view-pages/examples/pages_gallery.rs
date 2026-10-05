//! Renders the secondary pages to PNGs, offscreen.
//!
//! ```sh
//! cargo run -p monocode-view-pages --example pages_gallery -- target/pages-gallery [scene]
//! ```
//!
//! Each scene opens a headless window with the platform text system and the
//! headless renderer, waits for assets and open animations, and writes
//! `<scene>.png` at 2x through `Window::render_to_image`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, AppContext as _, Context, HeadlessAppContext, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowHandle, div, px, size,
};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_pages::data::{ProjectMark, StaticProjects};

#[path = "gallery/fixtures.rs"]
mod fixtures;

/// A window body that fills the window with one page.
struct Stage {
    view: AnyView,
    padded: bool,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut root = div()
            .size_full()
            .flex()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal));
        if self.padded {
            root = root.p(px(24.)).items_start();
        }
        root.child(self.view.clone())
    }
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> Stage>;
type Act = Box<dyn FnOnce(&mut Window, &mut App)>;

struct Scene {
    name: &'static str,
    width: f32,
    height: f32,
    light: bool,
    build: Build,
    /// Runs after the first frames, to open menus or type.
    act: Option<Act>,
}

fn scene(
    name: &'static str,
    width: f32,
    height: f32,
    build: impl FnOnce(&mut Window, &mut App) -> Stage + 'static,
) -> Scene {
    Scene {
        name,
        width,
        height,
        light: false,
        build: Box::new(build),
        act: None,
    }
}

impl Scene {
    fn light(mut self) -> Self {
        self.light = true;
        self
    }

    fn act(mut self, act: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        self.act = Some(Box::new(act));
        self
    }
}

fn stage(view: impl Into<AnyView>) -> Stage {
    Stage {
        view: view.into(),
        padded: false,
    }
}

fn padded(view: impl Into<AnyView>) -> Stage {
    Stage {
        view: view.into(),
        padded: true,
    }
}

pub fn projects() -> Rc<StaticProjects> {
    Rc::new(
        StaticProjects::new([
            "/Users/me/code/monocode",
            "/Users/me/code/edefyn",
            "/Users/me/code/portognjeeen",
            "/Users/me/code/website",
        ])
        .with_mark(
            "/Users/me/code/monocode",
            ProjectMark {
                label: "MonoCode".into(),
                color: Some(gpui::hsla(0.95, 0.75, 0.70, 1.)),
                ..ProjectMark::plain("/Users/me/code/monocode")
            },
        )
        .with_mark(
            "/Users/me/code/edefyn",
            ProjectMark {
                color: Some(gpui::hsla(0.40, 0.60, 0.60, 1.)),
                ..ProjectMark::plain("/Users/me/code/edefyn")
            },
        ),
    )
}

fn scenes() -> Vec<Scene> {
    let mut list = Vec::new();
    list.extend(fixtures::notes_scenes());
    list.extend(fixtures::date_scenes());
    list.extend(fixtures::search_scenes());
    list.extend(fixtures::settings_scenes());
    list.extend(fixtures::automation_scenes());
    list
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
    let build = scene.build;
    let window: WindowHandle<Stage> = cx
        .open_window(
            size(px(scene.width), px(scene.height)),
            move |window, cx| {
                monocode_ui::sync_window(window, cx);
                let stage = build(window, cx);
                cx.new(|_| stage)
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
    let settle = |cx: &mut HeadlessAppContext| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            draw(cx);
            std::thread::sleep(Duration::from_millis(16));
        }
    };
    settle(cx);
    if let Some(act) = scene.act {
        cx.update_window(window.into(), |_, window, cx| act(window, cx))
            .expect("act");
        settle(cx);
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
        .unwrap_or_else(|| PathBuf::from("target/pages-gallery"));
    let only = std::env::args().nth(2);
    std::fs::create_dir_all(&out).expect("create output dir");
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(monocode_ui::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        monocode_view_pages::init(cx);
    });
    for scene in scenes() {
        if only
            .as_deref()
            .is_some_and(|only| !scene.name.starts_with(only))
        {
            continue;
        }
        capture(&mut cx, scene, &out);
    }
}
