//! The native menu bar, after the Tauri app's `menu.rs`. Items dispatch GPUI
//! actions, so their shortcuts follow the live keymap and items the focused
//! view cannot handle are disabled.
use crate::{
    Quit,
    file_editor::{Find, Format, ReloadFile, Replace, Save, ToggleAutosave, ToggleWordWrap},
    input,
    shell::*,
};
use gpui::{App, KeyBinding, Menu, MenuItem, actions};

actions!(application, [Hide, HideOthers, ShowAll]);

/// Opens a Help menu link in the browser.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = application, no_json)]
pub(crate) struct OpenUrl(pub &'static str);

const REPO: &str = "https://github.com/hardbeat920/monocode";

pub(crate) fn bind(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|action: &OpenUrl, cx| cx.open_url(action.0));
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
        ]);
    }
}

/// Set once the first window has presented a frame.
struct MenuReady;
impl gpui::Global for MenuReady {}

/// Installs the menu bar, called after the first frame is on screen. Setting
/// the main menu makes AppKit check for Writing Tools, which loads a framework
/// on the main thread (about 70 ms), so launch must not wait for it.
pub(crate) fn ready(cx: &mut App) {
    cx.set_global(MenuReady);
    if let Some(prefs) = cx.try_global::<crate::settings::Preferences>().cloned() {
        install(&prefs, cx);
    }
}

pub(crate) fn install(prefs: &crate::settings::Preferences, cx: &mut App) {
    if !cx.has_global::<MenuReady>() {
        return;
    }
    cx.set_menus(vec![
        Menu::new("Monocode Native").items([
            MenuItem::action("About Monocode Native", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Hide Monocode Native", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Monocode Native", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Project…", OpenProject),
            MenuItem::action("Search…", FocusSearch),
            MenuItem::action("Go to File…", GoToFile),
            MenuItem::action("Command Palette…", OpenCommandPalette),
            MenuItem::action("Find in Files…", SearchFiles),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Reload File", ReloadFile),
            MenuItem::action("Autosave", ToggleAutosave).checked(prefs.autosave),
            MenuItem::separator(),
            MenuItem::action("New Tab", NewCodex),
            MenuItem::action("New Claude Tab", NewClaude),
            MenuItem::action("New Terminal", NewTerminal),
            MenuItem::action("Split Pane Right", SplitRight),
            MenuItem::action("Split Pane Down", SplitDown),
            MenuItem::action("Close Pane", CloseTab),
            MenuItem::action("Close Other Tabs", CloseOtherTabs),
            MenuItem::action("Close All Tabs", CloseAllTabs),
            MenuItem::separator(),
            MenuItem::action("Previous Tab", PreviousTab),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Back", VisitBack),
            MenuItem::action("Forward", VisitForward),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", input::Undo),
            MenuItem::action("Redo", input::Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", input::Cut),
            MenuItem::action("Copy", input::Copy),
            MenuItem::action("Paste", input::Paste),
            MenuItem::action("Select All", input::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find", Find),
            MenuItem::action("Replace", Replace),
            MenuItem::action("Format Document", Format),
            MenuItem::action("Word Wrap", ToggleWordWrap).checked(prefs.file_word_wrap),
            MenuItem::separator(),
            MenuItem::action("Fold", input::FoldCode),
            MenuItem::action("Unfold", input::UnfoldCode),
            MenuItem::action("Fold All", input::FoldAll),
            MenuItem::action("Unfold All", input::UnfoldAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Session Sidebar", ToggleRightPanel),
            MenuItem::action("Toggle Layout", ToggleLayout),
            MenuItem::action("Toggle Workspace Style", ToggleWorkspaceStyle),
            MenuItem::action("Inbox", OpenInbox),
            MenuItem::action("Notes", OpenNotes),
            MenuItem::action("Toggle Terminal", ToggleTerminal),
            MenuItem::action("Switch Model…", SwitchModel),
            MenuItem::separator(),
            MenuItem::action("Previous Session", PreviousSession),
            MenuItem::action("Next Session", NextSession),
            MenuItem::action("Previous Project", PreviousProject),
            MenuItem::action("Next Project", NextProject),
            MenuItem::separator(),
            MenuItem::action("Focus Left", FocusPaneLeft),
            MenuItem::action("Focus Right", FocusPaneRight),
            MenuItem::action("Focus Up", FocusPaneUp),
            MenuItem::action("Focus Down", FocusPaneDown),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Reset Zoom", ResetZoom),
            MenuItem::action("Reload", Reload),
            MenuItem::separator(),
            MenuItem::action("Sidebar Appearance…", SidebarAppearance),
        ]),
        // AppKit adds its window list here once the menu is named "Window".
        Menu::new("Window").items([
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
        ]),
        Menu::new("Help").items([
            MenuItem::action("MonoCode Website", OpenUrl("https://usemono.dev")),
            MenuItem::action("View on GitHub", OpenUrl(REPO)),
            MenuItem::separator(),
            MenuItem::action(
                "Report a Bug…",
                OpenUrl(concat!(
                    "https://github.com/hardbeat920/monocode",
                    "/issues/new?template=bug_report.yml"
                )),
            ),
            MenuItem::action(
                "Request a Feature…",
                OpenUrl(concat!(
                    "https://github.com/hardbeat920/monocode",
                    "/issues/new?template=feature_request.yml"
                )),
            ),
        ]),
    ]);
}
