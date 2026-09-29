#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(target_os = "macos")]
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use tauri::menu::{AboutMetadata, Menu, MenuItem, MenuItemBuilder, SubmenuBuilder};
#[cfg(target_os = "macos")]
use tauri::Wry;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
pub struct KeybindingOverride {
    disabled: Option<bool>,
    shortcut: Option<String>,
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
    #[cfg(target_os = "macos")]
    app.set_menu(build(app, &HashMap::new())?)?;
    let _ = app;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn keybindings_set_overrides(
    app: AppHandle,
    overrides: HashMap<String, KeybindingOverride>,
) -> Result<(), String> {
    let menu = build(&app, &overrides).map_err(|error| error.to_string())?;
    // set_menu hands back the previous menu; this command only needs to know
    // whether it succeeded.
    app.set_menu(menu)
        .map(|_| ())
        .map_err(|error| error.to_string())
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
        | "focus_up" | "focus_down" | "sidebar_opacity" | "open_project" | "go_to_file"
        | "open_search" | "open_inbox" | "open_notes" | "find_in_project" | "find"
        | "new_terminal" | "new_terminal_tab" | "toggle_terminal" | "open_model_picker"
        | "open_settings" | "check_for_updates" => {
            let _ = app.emit(id, ());
        }
        // Sidebar, Zoom, Reload, Command Palette, and Close All Tabs target one window: a broadcast would
        // make every window act on a single menu click.
        "toggle_sidebar"
        | "toggle_session_sidebar"
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
    overrides: &HashMap<String, KeybindingOverride>,
) -> tauri::Result<Menu<Wry>> {
    let open_settings = menu_item(
        app,
        "open_settings",
        "设置…",
        "CmdOrCtrl+,",
        "App: Settings",
        overrides,
    )?;
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", "检查更新…").build(app)?;
    let new_window = menu_item(
        app,
        "new_window",
        "新建窗口",
        "CmdOrCtrl+Shift+N",
        "App: New Window",
        overrides,
    )?;
    let open_project = menu_item(
        app,
        "open_project",
        "打开项目…",
        "CmdOrCtrl+O",
        "App: Open Project",
        overrides,
    )?;
    let go_to_file = menu_item(
        app,
        "go_to_file",
        "转到文件…",
        "CmdOrCtrl+P",
        "App: Go to File",
        overrides,
    )?;
    let command_palette = menu_item(
        app,
        "open_command_palette",
        "命令面板…",
        "CmdOrCtrl+Shift+P",
        "App: Command Palette",
        overrides,
    )?;
    let open_search = menu_item(
        app,
        "open_search",
        "搜索…",
        "CmdOrCtrl+K",
        "App: Search",
        overrides,
    )?;
    let open_inbox = MenuItemBuilder::with_id("open_inbox", "收件箱").build(app)?;
    let open_notes = MenuItemBuilder::with_id("open_notes", "随手笔记").build(app)?;
    let new_tab = menu_item(
        app,
        "new_tab",
        "新建标签页",
        "CmdOrCtrl+T",
        "Tab: New",
        overrides,
    )?;
    let new_terminal = menu_item(
        app,
        "new_terminal",
        "新建终端",
        "CmdOrCtrl+`",
        "Terminal: New",
        overrides,
    )?;
    let new_terminal_tab = menu_item(
        app,
        "new_terminal_tab",
        "新建终端标签页",
        "CmdOrCtrl+Shift+`",
        "Terminal: New Tab",
        overrides,
    )?;
    let toggle_terminal = menu_item(
        app,
        "toggle_terminal",
        "切换终端",
        "CmdOrCtrl+J",
        "Terminal: Toggle Dock",
        overrides,
    )?;
    let split_right = menu_item(
        app,
        "split_right",
        "向右拆分窗格",
        "CmdOrCtrl+D",
        "Pane: Split Right",
        overrides,
    )?;
    let split_down = menu_item(
        app,
        "split_down",
        "向下拆分窗格",
        "CmdOrCtrl+Shift+D",
        "Pane: Split Down",
        overrides,
    )?;
    let close_tab = menu_item(
        app,
        "close_tab",
        "关闭窗格",
        "CmdOrCtrl+W",
        "Pane: Close",
        overrides,
    )?;
    let close_other_tabs = menu_item(
        app,
        "close_other_tabs",
        "关闭其他标签页",
        "CmdOrCtrl+Alt+T",
        "Tab: Close Others",
        overrides,
    )?;
    let close_all_tabs = menu_item(
        app,
        "close_all_tabs",
        "关闭所有标签页",
        "CmdOrCtrl+Shift+W",
        "Tab: Close All",
        overrides,
    )?;
    let next_tab = menu_item(
        app,
        "next_tab",
        "下一个标签页",
        "CmdOrCtrl+Shift+]",
        "Tab: Next",
        overrides,
    )?;
    let prev_tab = menu_item(
        app,
        "prev_tab",
        "上一个标签页",
        "CmdOrCtrl+Shift+[",
        "Tab: Previous",
        overrides,
    )?;
    let back_tab = menu_item(
        app,
        "back_tab",
        "返回",
        "CmdOrCtrl+[",
        "Tab: Back",
        overrides,
    )?;
    let forward_tab = menu_item(
        app,
        "forward_tab",
        "前进",
        "CmdOrCtrl+]",
        "Tab: Forward",
        overrides,
    )?;

    let focus_left = menu_item(
        app,
        "focus_left",
        "聚焦左侧窗格",
        "CmdOrCtrl+Alt+Left",
        "Pane: Focus Left",
        overrides,
    )?;
    let focus_right = menu_item(
        app,
        "focus_right",
        "聚焦右侧窗格",
        "CmdOrCtrl+Alt+Right",
        "Pane: Focus Right",
        overrides,
    )?;
    let focus_up = menu_item(
        app,
        "focus_up",
        "聚焦上方窗格",
        "CmdOrCtrl+Alt+Up",
        "Pane: Focus Up",
        overrides,
    )?;
    let focus_down = menu_item(
        app,
        "focus_down",
        "聚焦下方窗格",
        "CmdOrCtrl+Alt+Down",
        "Pane: Focus Down",
        overrides,
    )?;

    let toggle_sidebar = menu_item(
        app,
        "toggle_sidebar",
        "切换侧边栏",
        "CmdOrCtrl+B",
        "App: Toggle Sidebar",
        overrides,
    )?;
    let toggle_session_sidebar = menu_item(
        app,
        "toggle_session_sidebar",
        "切换会话侧边栏",
        "CmdOrCtrl+Shift+B",
        "App: Toggle Session Sidebar",
        overrides,
    )?;
    let open_model_picker = menu_item(
        app,
        "open_model_picker",
        "切换模型…",
        "CmdOrCtrl+.",
        "App: Switch Model",
        overrides,
    )?;
    let sidebar_opacity = MenuItemBuilder::with_id("sidebar_opacity", "侧边栏外观…").build(app)?;
    // No accelerators here on purpose: the webview key handler owns
    // CmdOrCtrl + - 0, and a menu accelerator would fire the same command
    // a second time on top of it.
    let zoom_in = MenuItemBuilder::with_id("zoom_in", "放大").build(app)?;
    let zoom_out = MenuItemBuilder::with_id("zoom_out", "缩小").build(app)?;
    let zoom_reset = MenuItemBuilder::with_id("zoom_reset", "重置缩放").build(app)?;
    let reload = menu_item(
        app,
        "reload",
        "重新加载",
        "CmdOrCtrl+Shift+R",
        "View: Reload",
        overrides,
    )?;
    let find = menu_item(
        app,
        "find",
        "查找",
        "CmdOrCtrl+F",
        "Editor: Find",
        overrides,
    )?;

    let find_in_project = menu_item(
        app,
        "find_in_project",
        "在文件中查找…",
        "CmdOrCtrl+Shift+F",
        "App: Find in Files",
        overrides,
    )?;

    let file = SubmenuBuilder::new(app, "文件")
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

    let view = SubmenuBuilder::new(app, "视图")
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

    let edit = SubmenuBuilder::new(app, "编辑")
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
        let quit = MenuItemBuilder::with_id("quit", "退出 MonoCode")
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
        let window_menu =
            SubmenuBuilder::with_id(app, tauri::menu::WINDOW_SUBMENU_ID, "窗口").build()?;
        return Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window_menu]);
    }

    #[allow(unreachable_code)]
    Menu::with_items(app, &[&file, &edit, &view])
}
