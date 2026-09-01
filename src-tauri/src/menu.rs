#[cfg(target_os = "macos")]
use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, SubmenuBuilder};
#[cfg(target_os = "macos")]
use tauri::Wry;
use tauri::{AppHandle, Emitter};

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.set_menu(build(app)?)?;
    let _ = app;
    Ok(())
}

pub fn dispatch(app: &AppHandle, id: &str) {
    match id {
        "new_window" => {
            let _ = crate::window::open_new_window(app);
        }
        "quit" => crate::window::request_quit(app),
        "new_tab" | "close_tab" | "next_tab" | "prev_tab" | "back_tab" | "forward_tab"
        | "split_right" | "split_down" | "focus_left" | "focus_right" | "focus_up"
        | "focus_down" | "toggle_sidebar" | "toggle_zen" | "sidebar_opacity" | "open_project"
        | "go_to_file" | "open_search" | "open_inbox" | "open_notes" | "find_in_project"
        | "find" | "new_terminal" | "new_terminal_tab" | "toggle_terminal"
        | "open_model_picker" | "open_settings" | "check_for_updates" => {
            let _ = app.emit(id, ());
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let open_settings = MenuItemBuilder::with_id("open_settings", "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", "Check for Updates…").build(app)?;
    let new_window = MenuItemBuilder::with_id("new_window", "New Window")
        .accelerator("CmdOrCtrl+Shift+N")
        .build(app)?;
    let open_project = MenuItemBuilder::with_id("open_project", "Open Project…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let go_to_file = MenuItemBuilder::with_id("go_to_file", "Go to File…")
        .accelerator("CmdOrCtrl+P")
        .build(app)?;
    let open_search = MenuItemBuilder::with_id("open_search", "Search…")
        .accelerator("CmdOrCtrl+K")
        .build(app)?;
    let open_inbox = MenuItemBuilder::with_id("open_inbox", "Inbox").build(app)?;
    let open_notes = MenuItemBuilder::with_id("open_notes", "Notes").build(app)?;
    let new_tab = MenuItemBuilder::with_id("new_tab", "New Tab")
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let new_terminal = MenuItemBuilder::with_id("new_terminal", "New Terminal")
        .accelerator("CmdOrCtrl+`")
        .build(app)?;
    let new_terminal_tab = MenuItemBuilder::with_id("new_terminal_tab", "New Terminal Tab")
        .accelerator("CmdOrCtrl+Shift+`")
        .build(app)?;
    let toggle_terminal = MenuItemBuilder::with_id("toggle_terminal", "Toggle Terminal")
        .accelerator("CmdOrCtrl+J")
        .build(app)?;
    let split_right = MenuItemBuilder::with_id("split_right", "Split Pane Right")
        .accelerator("CmdOrCtrl+D")
        .build(app)?;
    let split_down = MenuItemBuilder::with_id("split_down", "Split Pane Down")
        .accelerator("CmdOrCtrl+Shift+D")
        .build(app)?;
    let close_tab = MenuItemBuilder::with_id("close_tab", "Close Pane")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let next_tab = MenuItemBuilder::with_id("next_tab", "Next Tab")
        .accelerator("CmdOrCtrl+Shift+]")
        .build(app)?;
    let prev_tab = MenuItemBuilder::with_id("prev_tab", "Previous Tab")
        .accelerator("CmdOrCtrl+Shift+[")
        .build(app)?;
    let back_tab = MenuItemBuilder::with_id("back_tab", "Go Back")
        .accelerator("CmdOrCtrl+[")
        .build(app)?;
    let forward_tab = MenuItemBuilder::with_id("forward_tab", "Go Forward")
        .accelerator("CmdOrCtrl+]")
        .build(app)?;

    let focus_left = MenuItemBuilder::with_id("focus_left", "Focus Pane Left")
        .accelerator("CmdOrCtrl+Alt+Left")
        .build(app)?;
    let focus_right = MenuItemBuilder::with_id("focus_right", "Focus Pane Right")
        .accelerator("CmdOrCtrl+Alt+Right")
        .build(app)?;
    let focus_up = MenuItemBuilder::with_id("focus_up", "Focus Pane Up")
        .accelerator("CmdOrCtrl+Alt+Up")
        .build(app)?;
    let focus_down = MenuItemBuilder::with_id("focus_down", "Focus Pane Down")
        .accelerator("CmdOrCtrl+Alt+Down")
        .build(app)?;

    let toggle_sidebar = MenuItemBuilder::with_id("toggle_sidebar", "Toggle Sidebar")
        .accelerator("CmdOrCtrl+B")
        .build(app)?;
    let toggle_zen = MenuItemBuilder::with_id("toggle_zen", "Toggle Zen Mode")
        .accelerator("CmdOrCtrl+Alt+Z")
        .build(app)?;
    let open_model_picker = MenuItemBuilder::with_id("open_model_picker", "Switch Model…")
        .accelerator("CmdOrCtrl+.")
        .build(app)?;
    let sidebar_opacity =
        MenuItemBuilder::with_id("sidebar_opacity", "Sidebar Appearance…").build(app)?;
    let find = MenuItemBuilder::with_id("find", "Find")
        .accelerator("CmdOrCtrl+F")
        .build(app)?;

    let find_in_project = MenuItemBuilder::with_id("find_in_project", "Find in Files…")
        .accelerator("CmdOrCtrl+Shift+F")
        .build(app)?;

    let file = SubmenuBuilder::new(app, "File")
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
        .separator()
        .item(&prev_tab)
        .item(&next_tab)
        .item(&back_tab)
        .item(&forward_tab)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&toggle_sidebar)
        .item(&toggle_zen)
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
        .item(&sidebar_opacity)
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&find)
        .build()?;

    #[cfg(target_os = "macos")]
    {
        let quit = MenuItemBuilder::with_id("quit", "Quit MonoCode")
            .accelerator("CmdOrCtrl+Q")
            .build(app)?;
        let app_menu = SubmenuBuilder::new(app, "MonoCode")
            .about(Some(AboutMetadata::default()))
            .separator()
            .item(&open_settings)
            .item(&check_for_updates)
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .item(&quit)
            .build()?;
        let window_menu = SubmenuBuilder::new(app, "Window").build()?;
        window_menu.set_as_windows_menu_for_nsapp()?;
        return Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window_menu]);
    }

    #[allow(unreachable_code)]
    Menu::with_items(app, &[&file, &edit, &view])
}
