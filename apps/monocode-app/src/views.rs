//! Views that `--view <name>` can put in the window. Add an entry here to
//! check a new view with `--screenshot`.

use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div,
};
use monocode_ui::{Theme, u};

use crate::gallery::{IconsGallery, ModalDemo, WidgetsGallery};
use crate::panes::standalone::{SlotKind, Standalone};
use crate::shell::{self, ShellOptions, SidebarTab};
use crate::slots::Page;

pub struct ViewEntry {
    pub name: &'static str,
    pub description: &'static str,
    /// The view reads the engine, so the app boots it on the data dir.
    pub engine: bool,
    pub build: fn(&mut Window, &mut App) -> AnyView,
}

pub const VIEWS: &[ViewEntry] = &[
    ViewEntry {
        name: "shell",
        description: "The app: project rail, session sidebar, workspace panes",
        engine: true,
        build: |window, cx| shell::build(ShellOptions::saved(cx), window, cx),
    },
    ViewEntry {
        name: "shell-compact",
        engine: true,
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
        engine: true,
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
        engine: true,
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
    // One slot view without the shell, for checking the wiring by
    // screenshot. See src/panes/standalone.rs.
    ViewEntry {
        name: "workspace",
        engine: true,
        description: "The workspace area alone: the active tab's panes",
        build: |window, cx| Standalone::build(SlotKind::Workspace, window, cx),
    },
    ViewEntry {
        name: "page-search",
        engine: true,
        description: "The Search page",
        build: |window, cx| Standalone::build(SlotKind::Page(Page::Search), window, cx),
    },
    ViewEntry {
        name: "page-inbox",
        engine: true,
        description: "The Inbox page",
        build: |window, cx| Standalone::build(SlotKind::Page(Page::Inbox), window, cx),
    },
    ViewEntry {
        name: "page-notes",
        engine: true,
        description: "The Notes page",
        build: |window, cx| Standalone::build(SlotKind::Page(Page::Notes), window, cx),
    },
    ViewEntry {
        name: "page-automations",
        engine: true,
        description: "The Automations page",
        build: |window, cx| Standalone::build(SlotKind::Page(Page::Automations), window, cx),
    },
    ViewEntry {
        name: "page-settings",
        engine: true,
        description: "The Settings page",
        build: |window, cx| Standalone::build(SlotKind::Page(Page::Settings), window, cx),
    },
    ViewEntry {
        name: "tab-inbox",
        engine: true,
        description: "The sidebar's Inbox tab",
        build: |window, cx| Standalone::build(SlotKind::SidebarTab(SidebarTab::Inbox), window, cx),
    },
    ViewEntry {
        name: "tab-explorer",
        engine: true,
        description: "The sidebar's Explorer tab",
        build: |window, cx| Standalone::build(SlotKind::SidebarTab(SidebarTab::Files), window, cx),
    },
    ViewEntry {
        name: "tab-changes",
        engine: true,
        description: "The sidebar's Changes tab",
        build: |window, cx| {
            Standalone::build(SlotKind::SidebarTab(SidebarTab::Changes), window, cx)
        },
    },
    ViewEntry {
        name: "widgets",
        engine: false,
        description: "Every monocode-ui widget, with a toast",
        build: WidgetsGallery::build,
    },
    ViewEntry {
        name: "modal",
        engine: true,
        description: "A modal over the shell",
        build: ModalDemo::build,
    },
    ViewEntry {
        name: "icons",
        engine: false,
        description: "Chrome icons, provider logos, and file-type icons",
        build: IconsGallery::build,
    },
    ViewEntry {
        name: "blank",
        engine: false,
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
