use tauri::{AppHandle, Emitter, Manager};

pub use crate::menu_language::{Lang, MenuLanguage};
// Only macOS builds a native menu, so only macOS needs the label tables.
#[cfg(target_os = "macos")]
pub use crate::menu_language::labels;

/// The language the menu is currently built in.
pub fn language(app: &AppHandle) -> Lang {
    app.state::<MenuLanguage>().get()
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let lang = language(app);
    #[cfg(target_os = "macos")]
    app.set_menu(build(app, lang)?)?;
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
    app.set_menu(build(app, lang).map_err(|error| error.to_string())?)
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
        // Zoom and Close All Tabs target one window: a broadcast would make
        // every window act on a single menu click.
        "zoom_in" | "zoom_out" | "zoom_reset" | "close_all_tabs" => emit_to_focused(app, id),
        _ => {}
    }
}

/// Emit `id` to the focused window, falling back to a visible one, then any.
fn emit_to_focused(app: &AppHandle, id: &str) {
    let mut windows: Vec<_> = app.webview_windows().into_values().collect();
    windows.sort_by(|a, b| a.label().cmp(b.label()));
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
fn build(app: &AppHandle, lang: Lang) -> tauri::Result<Menu<Wry>> {
    let l = labels(lang);
    let open_settings = MenuItemBuilder::with_id("open_settings", l.settings)
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", l.check_for_updates).build(app)?;
    let new_window = MenuItemBuilder::with_id("new_window", l.new_window)
        .accelerator("CmdOrCtrl+Shift+N")
        .build(app)?;
    let open_project = MenuItemBuilder::with_id("open_project", l.open_project)
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let go_to_file = MenuItemBuilder::with_id("go_to_file", l.go_to_file)
        .accelerator("CmdOrCtrl+P")
        .build(app)?;
    let open_search = MenuItemBuilder::with_id("open_search", l.open_search)
        .accelerator("CmdOrCtrl+K")
        .build(app)?;
    let open_inbox = MenuItemBuilder::with_id("open_inbox", l.open_inbox).build(app)?;
    let open_notes = MenuItemBuilder::with_id("open_notes", l.open_notes).build(app)?;
    let new_tab = MenuItemBuilder::with_id("new_tab", l.new_tab)
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let new_terminal = MenuItemBuilder::with_id("new_terminal", l.new_terminal)
        .accelerator("CmdOrCtrl+`")
        .build(app)?;
    let new_terminal_tab = MenuItemBuilder::with_id("new_terminal_tab", l.new_terminal_tab)
        .accelerator("CmdOrCtrl+Shift+`")
        .build(app)?;
    let toggle_terminal = MenuItemBuilder::with_id("toggle_terminal", l.toggle_terminal)
        .accelerator("CmdOrCtrl+J")
        .build(app)?;
    let split_right = MenuItemBuilder::with_id("split_right", l.split_right)
        .accelerator("CmdOrCtrl+D")
        .build(app)?;
    let split_down = MenuItemBuilder::with_id("split_down", l.split_down)
        .accelerator("CmdOrCtrl+Shift+D")
        .build(app)?;
    let close_tab = MenuItemBuilder::with_id("close_tab", l.close_tab)
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let close_other_tabs = MenuItemBuilder::with_id("close_other_tabs", l.close_other_tabs)
        .accelerator("CmdOrCtrl+Alt+T")
        .build(app)?;
    let close_all_tabs = MenuItemBuilder::with_id("close_all_tabs", l.close_all_tabs)
        .accelerator("CmdOrCtrl+Shift+W")
        .build(app)?;
    let next_tab = MenuItemBuilder::with_id("next_tab", l.next_tab)
        .accelerator("CmdOrCtrl+Shift+]")
        .build(app)?;
    let prev_tab = MenuItemBuilder::with_id("prev_tab", l.prev_tab)
        .accelerator("CmdOrCtrl+Shift+[")
        .build(app)?;
    let back_tab = MenuItemBuilder::with_id("back_tab", l.back_tab)
        .accelerator("CmdOrCtrl+[")
        .build(app)?;
    let forward_tab = MenuItemBuilder::with_id("forward_tab", l.forward_tab)
        .accelerator("CmdOrCtrl+]")
        .build(app)?;

    let focus_left = MenuItemBuilder::with_id("focus_left", l.focus_left)
        .accelerator("CmdOrCtrl+Alt+Left")
        .build(app)?;
    let focus_right = MenuItemBuilder::with_id("focus_right", l.focus_right)
        .accelerator("CmdOrCtrl+Alt+Right")
        .build(app)?;
    let focus_up = MenuItemBuilder::with_id("focus_up", l.focus_up)
        .accelerator("CmdOrCtrl+Alt+Up")
        .build(app)?;
    let focus_down = MenuItemBuilder::with_id("focus_down", l.focus_down)
        .accelerator("CmdOrCtrl+Alt+Down")
        .build(app)?;

    let toggle_sidebar = MenuItemBuilder::with_id("toggle_sidebar", l.toggle_sidebar)
        .accelerator("CmdOrCtrl+B")
        .build(app)?;
    let open_model_picker = MenuItemBuilder::with_id("open_model_picker", l.switch_model)
        .accelerator("CmdOrCtrl+.")
        .build(app)?;
    let sidebar_opacity =
        MenuItemBuilder::with_id("sidebar_opacity", l.sidebar_appearance).build(app)?;
    // No accelerators here on purpose: the webview key handler owns
    // CmdOrCtrl + - 0, and a menu accelerator would fire the same command
    // a second time on top of it.
    let zoom_in = MenuItemBuilder::with_id("zoom_in", l.zoom_in).build(app)?;
    let zoom_out = MenuItemBuilder::with_id("zoom_out", l.zoom_out).build(app)?;
    let zoom_reset = MenuItemBuilder::with_id("zoom_reset", l.zoom_reset).build(app)?;
    let find = MenuItemBuilder::with_id("find", l.find)
        .accelerator("CmdOrCtrl+F")
        .build(app)?;

    let find_in_project = MenuItemBuilder::with_id("find_in_project", l.find_in_project)
        .accelerator("CmdOrCtrl+Shift+F")
        .build(app)?;

    let file = SubmenuBuilder::new(app, l.file)
        .item(&new_window)
        .item(&open_project)
        .item(&open_search)
        .item(&go_to_file)
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

    let window_menu = SubmenuBuilder::new(app, l.window).build()?;
    window_menu.set_as_windows_menu_for_nsapp()?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window_menu])
}
