//! Views that `--view <name>` can put in the window. Add an entry here to
//! check a new view with `--screenshot`.

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div,
};
use monocode_ui::{Theme, u};

use crate::gallery::{IconsGallery, ModalDemo, WidgetsGallery};
use crate::shell::{self, ShellOptions};

pub struct ViewEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub build: fn(&mut Window, &mut App) -> AnyView,
}

pub const VIEWS: &[ViewEntry] = &[
    ViewEntry {
        name: "shell",
        description: "The app shell with mock data: project rail, sidebar, main pane",
        build: |window, cx| shell::build(ShellOptions::full(), window, cx),
    },
    ViewEntry {
        name: "shell-compact",
        description: "The shell with the 48px compact project rail",
        build: |window, cx| {
            shell::build(
                ShellOptions {
                    project_rail_open: false,
                    compact_rail: true,
                    ..ShellOptions::full()
                },
                window,
                cx,
            )
        },
    },
    ViewEntry {
        name: "shell-no-rail",
        description: "The shell with the project rail closed",
        build: |window, cx| {
            shell::build(
                ShellOptions {
                    project_rail_open: false,
                    ..ShellOptions::full()
                },
                window,
                cx,
            )
        },
    },
    ViewEntry {
        name: "shell-menu",
        description: "The shell with the session context menu open",
        build: |window, cx| {
            shell::build(
                ShellOptions {
                    demo_menu: Some((330.0, 300.0)),
                    ..ShellOptions::full()
                },
                window,
                cx,
            )
        },
    },
    ViewEntry {
        name: "widgets",
        description: "Every monocode-ui widget, with a toast",
        build: WidgetsGallery::build,
    },
    ViewEntry {
        name: "modal",
        description: "A modal over the shell",
        build: ModalDemo::build,
    },
    ViewEntry {
        name: "icons",
        description: "Chrome icons, provider logos, and file-type icons",
        build: IconsGallery::build,
    },
    ViewEntry {
        name: "blank",
        description: "An empty themed window, for checking the harness",
        build: |_, cx| cx.new(|_| Blank).into(),
    },
];

pub fn find(name: &str) -> Option<&'static ViewEntry> {
    VIEWS.iter().find(|view| view.name == name)
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .bg(theme.colors.root_background)
            .text_color(theme.colors.content)
            .text_size(u(theme.text.ui))
            .p(u(40.))
            .child("MonoCode")
    }
}
