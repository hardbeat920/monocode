use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use tauri::menu::{
    AboutMetadata, Menu, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
#[cfg(target_os = "macos")]
use tauri::Wry;
use tauri::{AppHandle, Emitter, Manager};

pub use crate::menu_language::{Lang, MenuLanguage};
// Only macOS builds a native menu, so only macOS needs the label tables.
#[cfg(target_os = "macos")]
pub use crate::menu_language::labels;

/// The language the menu is currently built in.
pub fn language(app: &AppHandle) -> Lang {
    app.state::<MenuLanguage>().get()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Deserialize)]
pub struct KeybindingOverride {
    disabled: Option<bool>,
    shortcut: Option<String>,
}

/// Managed state: the keybinding overrides the native menu was last built with.
///
/// The webview owns them (they live in localStorage) and pushes them across.
/// The language command rebuilds the same menu, so the Rust side has to keep
/// the last set: without it a language flip would drop custom accelerators.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Default)]
pub struct MenuKeybindings(Mutex<HashMap<String, KeybindingOverride>>);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl MenuKeybindings {
    fn get(&self) -> HashMap<String, KeybindingOverride> {
        self.0
            .lock()
            .map(|current| current.clone())
            .unwrap_or_default()
    }

    fn set(&self, next: HashMap<String, KeybindingOverride>) -> Result<(), String> {
        *self.0.lock().map_err(|error| error.to_string())? = next;
        Ok(())
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn custom_accelerator(shortcut: &str) -> String {
    let mut parts = shortcut.split('+');
    let key = parts.next_back().unwrap_or_default();
    let key = match key {
        value if value.starts_with("Key") => &value[3..],
        value if value.starts_with("Digit") => &value[5..],
        "Equal" => "=",
        "Minus" => "-",
        "Backquote" => "`",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        other => other,
    };
    parts
        .map(|part| match part {
            "Command" => "Cmd",
            "Control" => "Ctrl",
            "Option" => "Option",
            "Shift" => "Shift",
            other => other,
        })
        .chain(std::iter::once(key))
        .collect::<Vec<_>>()
        .join("+")
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let lang = language(app);
    #[cfg(target_os = "macos")]
    app.set_menu(build(app, lang, &app.state::<MenuKeybindings>().get())?)?;
    let _ = (app, lang);
    Ok(())
}

/// Rebuilds the native menu in `language`. The webview calls this on boot and
/// whenever the language setting flips. A no-op on platforms whose menu bar is
/// drawn by the webview (Windows/Linux), so the frontend can call it blindly.
#[tauri::command]
pub fn set_menu_language(app: AppHandle, language: String) -> Result<(), String> {
    let next = Lang::from_tag(&language);
    if !app.state::<MenuLanguage>().set(next)? {
        return Ok(());
    }
    apply(&app, next)
}

#[cfg(target_os = "macos")]
fn apply(app: &AppHandle, lang: Lang) -> Result<(), String> {
    app.set_menu(
        build(app, lang, &app.state::<MenuKeybindings>().get())
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    // The dock menu is a separate NSMenu that Tauri does not own, so it has to
    // be rebuilt alongside the app menu.
    crate::macos::install_dock_menu(app, lang);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn apply(_app: &AppHandle, _lang: Lang) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn keybindings_set_overrides(
    app: AppHandle,
    overrides: HashMap<String, KeybindingOverride>,
) -> Result<(), String> {
    app.state::<MenuKeybindings>().set(overrides)?;
    apply(&app, language(&app))
}

#[cfg(target_os = "macos")]
fn menu_item(
    app: &AppHandle,
    id: &str,
    text: &str,
    accelerator: &str,
    command: &str,
    overrides: &HashMap<String, KeybindingOverride>,
) -> tauri::Result<MenuItem<Wry>> {
    let builder = MenuItemBuilder::with_id(id, text);
    let override_ = overrides.get(command);
    let builder = if override_.and_then(|value| value.disabled).unwrap_or(false) {
        builder
    } else if let Some(shortcut) = override_.and_then(|value| value.shortcut.as_deref()) {
        builder.accelerator(custom_accelerator(shortcut))
    } else {
        builder.accelerator(accelerator)
    };
    builder.build(app)
}

pub fn dispatch(app: &AppHandle, id: &str) {
    match id {
        "new_window" => {
            let _ = crate::window::open_new_window(app);
        }
        "quit" => crate::window::request_quit(app),
        "new_tab" | "close_tab" | "close_other_tabs" | "next_tab" | "prev_tab" | "back_tab"
        | "forward_tab" | "split_right" | "split_down" | "focus_left" | "focus_right"
        | "focus_up" | "focus_down" | "toggle_sidebar" | "sidebar_opacity" | "open_project"
        | "go_to_file" | "open_search" | "open_inbox" | "open_notes" | "find_in_project"
        | "find" | "new_terminal" | "new_terminal_tab" | "toggle_terminal"
        | "open_model_picker" | "open_settings" | "check_for_updates" => {
            let _ = app.emit(id, ());
        }
        // Sidebar, Zoom, Reload, Command Palette, and Close All Tabs target one window: a broadcast would
        // make every window act on a single menu click.
        "toggle_session_sidebar"
        | "zoom_in"
        | "zoom_out"
        | "zoom_reset"
        | "reload"
        | "open_command_palette"
        | "close_all_tabs" => emit_to_focused(app, id),
        _ => {}
    }
}

/// Emit `id` to the focused window, falling back to a visible one, then any.
fn emit_to_focused(app: &AppHandle, id: &str) {
    let windows = crate::window::workspace_windows(app);
    let target = windows
        .iter()
        .find(|window| window.is_focused().unwrap_or(false))
        .or_else(|| {
            windows
                .iter()
                .find(|window| window.is_visible().unwrap_or(false))
        })
        .or(windows.first());
    match target {
        Some(window) => {
            let _ = app.emit_to(window.label(), id, ());
        }
        None => {
            let _ = app.emit(id, ());
        }
    }
}

#[cfg(target_os = "macos")]
fn build(
    app: &AppHandle,
    lang: Lang,
    overrides: &HashMap<String, KeybindingOverride>,
) -> tauri::Result<Menu<Wry>> {
    let l = labels(lang);
    let open_settings = menu_item(
        app,
        "open_settings",
        l.settings,
        "CmdOrCtrl+,",
        "App: Settings",
        overrides,
    )?;
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", l.check_for_updates).build(app)?;
    let new_window = menu_item(
        app,
        "new_window",
        l.new_window,
        "CmdOrCtrl+Shift+N",
        "App: New Window",
        overrides,
    )?;
    let open_project = menu_item(
        app,
        "open_project",
        l.open_project,
        "CmdOrCtrl+O",
        "App: Open Project",
        overrides,
    )?;
    let go_to_file = menu_item(
        app,
        "go_to_file",
        l.go_to_file,
        "CmdOrCtrl+P",
        "App: Go to File",
        overrides,
    )?;
    let command_palette = menu_item(
        app,
        "open_command_palette",
        l.command_palette,
        "CmdOrCtrl+Shift+P",
        "App: Command Palette",
        overrides,
    )?;
    let open_search = menu_item(
        app,
        "open_search",
        l.open_search,
        "CmdOrCtrl+K",
        "App: Search",
        overrides,
    )?;
    let open_inbox = MenuItemBuilder::with_id("open_inbox", l.open_inbox).build(app)?;
    let open_notes = MenuItemBuilder::with_id("open_notes", l.open_notes).build(app)?;
    let new_tab = menu_item(
        app,
        "new_tab",
        l.new_tab,
        "CmdOrCtrl+T",
        "Tab: New",
        overrides,
    )?;
    let new_terminal = menu_item(
        app,
        "new_terminal",
        l.new_terminal,
        "CmdOrCtrl+`",
        "Terminal: New",
        overrides,
    )?;
    let new_terminal_tab = menu_item(
        app,
        "new_terminal_tab",
        l.new_terminal_tab,
        "CmdOrCtrl+Shift+`",
        "Terminal: New Tab",
        overrides,
    )?;
    let toggle_terminal = menu_item(
        app,
        "toggle_terminal",
        l.toggle_terminal,
        "CmdOrCtrl+J",
        "Terminal: Toggle Dock",
        overrides,
    )?;
    let split_right = menu_item(
        app,
        "split_right",
        l.split_right,
        "CmdOrCtrl+D",
        "Pane: Split Right",
        overrides,
    )?;
    let split_down = menu_item(
        app,
        "split_down",
        l.split_down,
        "CmdOrCtrl+Shift+D",
        "Pane: Split Down",
        overrides,
    )?;
    let close_tab = menu_item(
        app,
        "close_tab",
        l.close_tab,
        "CmdOrCtrl+W",
        "Pane: Close",
        overrides,
    )?;
    let close_other_tabs = menu_item(
        app,
        "close_other_tabs",
        l.close_other_tabs,
        "CmdOrCtrl+Alt+T",
        "Tab: Close Others",
        overrides,
    )?;
    let close_all_tabs = menu_item(
        app,
        "close_all_tabs",
        l.close_all_tabs,
        "CmdOrCtrl+Shift+W",
        "Tab: Close All",
        overrides,
    )?;
    let next_tab = menu_item(
        app,
        "next_tab",
        l.next_tab,
        "CmdOrCtrl+Shift+]",
        "Tab: Next",
        overrides,
    )?;
    let prev_tab = menu_item(
        app,
        "prev_tab",
        l.prev_tab,
        "CmdOrCtrl+Shift+[",
        "Tab: Previous",
        overrides,
    )?;
    let back_tab = menu_item(
        app,
        "back_tab",
        l.back_tab,
        "CmdOrCtrl+[",
        "Tab: Back",
        overrides,
    )?;
    let forward_tab = menu_item(
        app,
        "forward_tab",
        l.forward_tab,
        "CmdOrCtrl+]",
        "Tab: Forward",
        overrides,
    )?;

    let focus_left = menu_item(
        app,
        "focus_left",
        l.focus_left,
        "CmdOrCtrl+Alt+Left",
        "Pane: Focus Left",
        overrides,
    )?;
    let focus_right = menu_item(
        app,
        "focus_right",
        l.focus_right,
        "CmdOrCtrl+Alt+Right",
        "Pane: Focus Right",
        overrides,
    )?;
    let focus_up = menu_item(
        app,
        "focus_up",
        l.focus_up,
        "CmdOrCtrl+Alt+Up",
        "Pane: Focus Up",
        overrides,
    )?;
    let focus_down = menu_item(
        app,
        "focus_down",
        l.focus_down,
        "CmdOrCtrl+Alt+Down",
        "Pane: Focus Down",
        overrides,
    )?;

    let toggle_sidebar = menu_item(
        app,
        "toggle_sidebar",
        l.toggle_sidebar,
        "CmdOrCtrl+B",
        "App: Toggle Sidebar",
        overrides,
    )?;
    let toggle_session_sidebar = menu_item(
        app,
        "toggle_session_sidebar",
        l.toggle_session_sidebar,
        "CmdOrCtrl+Shift+B",
        "App: Toggle Session Sidebar",
        overrides,
    )?;
    let open_model_picker = menu_item(
        app,
        "open_model_picker",
        l.switch_model,
        "CmdOrCtrl+.",
        "App: Switch Model",
        overrides,
    )?;
    let sidebar_opacity =
        MenuItemBuilder::with_id("sidebar_opacity", l.sidebar_appearance).build(app)?;
    // No accelerators here on purpose: the webview key handler owns
    // CmdOrCtrl + - 0, and a menu accelerator would fire the same command
    // a second time on top of it.
    let zoom_in = MenuItemBuilder::with_id("zoom_in", l.zoom_in).build(app)?;
    let zoom_out = MenuItemBuilder::with_id("zoom_out", l.zoom_out).build(app)?;
    let zoom_reset = MenuItemBuilder::with_id("zoom_reset", l.zoom_reset).build(app)?;
    let reload = menu_item(
        app,
        "reload",
        l.reload,
        "CmdOrCtrl+Shift+R",
        "View: Reload",
        overrides,
    )?;
    let find = menu_item(
        app,
        "find",
        l.find,
        "CmdOrCtrl+F",
        "Editor: Find",
        overrides,
    )?;

    let find_in_project = menu_item(
        app,
        "find_in_project",
        l.find_in_project,
        "CmdOrCtrl+Shift+F",
        "App: Find in Files",
        overrides,
    )?;

    let file = SubmenuBuilder::new(app, l.file)
        .item(&new_window)
        .item(&open_project)
        .item(&open_search)
        .item(&go_to_file)
        .item(&command_palette)
        .item(&find_in_project)
        .separator()
        .item(&new_tab)
        .item(&new_terminal)
        .item(&new_terminal_tab)
        .item(&split_right)
        .item(&split_down)
        .item(&close_tab)
        .item(&close_other_tabs)
        .item(&close_all_tabs)
        .separator()
        .item(&prev_tab)
        .item(&next_tab)
        .item(&back_tab)
        .item(&forward_tab)
        .build()?;

    let view = SubmenuBuilder::new(app, l.view)
        .item(&toggle_sidebar)
        .item(&toggle_session_sidebar)
        .item(&open_inbox)
        .item(&open_notes)
        .item(&toggle_terminal)
        .item(&open_model_picker)
        .separator()
        .item(&focus_left)
        .item(&focus_right)
        .item(&focus_up)
        .item(&focus_down)
        .separator()
        .item(&zoom_in)
        .item(&zoom_out)
        .item(&zoom_reset)
        .item(&reload)
        .separator()
        .item(&sidebar_opacity)
        .build()?;

    // The standard items take explicit text: Tauri's defaults are English, so
    // leaving them alone would put Undo/Cut/Copy in an English menu bar.
    let undo = PredefinedMenuItem::undo(app, Some(l.undo))?;
    let redo = PredefinedMenuItem::redo(app, Some(l.redo))?;
    let cut = PredefinedMenuItem::cut(app, Some(l.cut))?;
    let copy = PredefinedMenuItem::copy(app, Some(l.copy))?;
    let paste = PredefinedMenuItem::paste(app, Some(l.paste))?;
    let select_all = PredefinedMenuItem::select_all(app, Some(l.select_all))?;

    let edit = SubmenuBuilder::new(app, l.edit)
        .item(&undo)
        .item(&redo)
        .separator()
        .item(&cut)
        .item(&copy)
        .item(&paste)
        .item(&select_all)
        .separator()
        .item(&find)
        .build()?;

    let about = PredefinedMenuItem::about(app, Some(l.about), Some(AboutMetadata::default()))?;
    let hide = PredefinedMenuItem::hide(app, Some(l.hide))?;
    let hide_others = PredefinedMenuItem::hide_others(app, Some(l.hide_others))?;
    let show_all = PredefinedMenuItem::show_all(app, Some(l.show_all))?;
    let quit = MenuItemBuilder::with_id("quit", l.quit)
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;
    let app_menu = SubmenuBuilder::new(app, "MonoCode")
        .item(&about)
        .separator()
        .item(&open_settings)
        .item(&check_for_updates)
        .separator()
        .item(&hide)
        .item(&hide_others)
        .item(&show_all)
        .separator()
        .item(&quit)
        .build()?;

    // Use Tauri's reserved id so macOS wires the native Window menu (including
    // system tiling actions) when the menu is attached to the application.
    let window_menu =
        SubmenuBuilder::with_id(app, tauri::menu::WINDOW_SUBMENU_ID, l.window).build()?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window_menu])
}
