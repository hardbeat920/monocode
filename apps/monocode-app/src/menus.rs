//! Native menu bar and dock menu.
use super::keymap::*;
use gpui::{App, Menu, MenuItem};

pub fn init(cx: &mut App) {
    cx.set_menus(vec![
        Menu::new("MonoCode").items([
            MenuItem::action("Settings...", OpenSettings),
            MenuItem::action("Check for updates...", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Quit MonoCode", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New session", NewSession),
            MenuItem::action("New window", NewWindow),
            MenuItem::action("Open project...", OpenProject),
            MenuItem::separator(),
            MenuItem::action("Close pane", ClosePane),
            MenuItem::action("Close other tabs", CloseOtherTabs),
            MenuItem::action("Close all tabs", CloseAllTabs),
        ]),
        Menu::new("Edit").items([MenuItem::action(
            "Format document",
            monocode_editor::code_editor::FormatDocument,
        )]),
        Menu::new("View").items([
            MenuItem::action("Search", OpenSearch),
            MenuItem::action("Inbox", OpenInbox),
            MenuItem::action("Notes", OpenNotes),
            MenuItem::action("Automations", OpenAutomations),
            MenuItem::separator(),
            MenuItem::action("Toggle sidebar", ToggleSidebar),
            MenuItem::action("Toggle session sidebar", ToggleSessionSidebar),
            MenuItem::action("Toggle terminal", ToggleTerminal),
            MenuItem::separator(),
            MenuItem::action("Zoom in", ZoomIn),
            MenuItem::action("Zoom out", ZoomOut),
            MenuItem::action("Reset zoom", ZoomReset),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Split right", SplitRight),
            MenuItem::action("Split down", SplitDown),
            MenuItem::action("Next tab", NextTab),
            MenuItem::action("Previous tab", PrevTab),
        ]),
        Menu::new("Help").items([
            MenuItem::action("MonoCode website", OpenWebsite),
            MenuItem::action("GitHub", OpenGithub),
            MenuItem::action("Report a bug", ReportBug),
        ]),
    ]);
    #[cfg(target_os = "macos")]
    cx.set_dock_menu(vec![
        MenuItem::action("New window", NewWindow),
        MenuItem::action("New session", NewSession),
    ]);
}
