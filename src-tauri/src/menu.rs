#[cfg(target_os = "macos")]
use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, SubmenuBuilder};
#[cfg(target_os = "macos")]
use tauri::Wry;
use tauri::{AppHandle, Emitter, Manager};

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
        "new_tab" | "close_tab" | "close_other_tabs" | "next_tab" | "prev_tab" | "back_tab"
        | "forward_tab" | "split_right" | "split_down" | "focus_left" | "focus_right"
        | "focus_up" | "focus_down" | "toggle_sidebar" | "sidebar_opacity" | "open_project"
        | "go_to_file" | "open_search" | "open_inbox" | "open_notes" | "find_in_project"
        | "find" | "new_terminal" | "new_terminal_tab" | "toggle_terminal"
        | "open_model_picker" | "open_settings" | "check_for_updates" => {
            let _ = app.emit(id, ());
        }
        "zoom_in" | "zoom_out" | "zoom_reset" => {
            // Zoom targets one window: a broadcast would make every window
            // increment the shared scale setting on a single menu click.
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
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    // Labels are Chinese (hard-zh fork); dispatch matches on item IDs, never labels.
    let open_settings = MenuItemBuilder::with_id("open_settings", "设置…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let check_for_updates =
        MenuItemBuilder::with_id("check_for_updates", "检查更新…").build(app)?;
    let new_window = MenuItemBuilder::with_id("new_window", "新建窗口")
        .accelerator("CmdOrCtrl+Shift+N")
        .build(app)?;
    let open_project = MenuItemBuilder::with_id("open_project", "打开项目…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let go_to_file = MenuItemBuilder::with_id("go_to_file", "跳转到文件…")
        .accelerator("CmdOrCtrl+P")
        .build(app)?;
    let open_search = MenuItemBuilder::with_id("open_search", "搜索…")
        .accelerator("CmdOrCtrl+K")
        .build(app)?;
    let open_inbox = MenuItemBuilder::with_id("open_inbox", "收件箱").build(app)?;
    let open_notes = MenuItemBuilder::with_id("open_notes", "笔记").build(app)?;
    let new_tab = MenuItemBuilder::with_id("new_tab", "新建标签页")
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let new_terminal = MenuItemBuilder::with_id("new_terminal", "新建终端")
        .accelerator("CmdOrCtrl+`")
        .build(app)?;
    let new_terminal_tab = MenuItemBuilder::with_id("new_terminal_tab", "新建终端标签页")
        .accelerator("CmdOrCtrl+Shift+`")
        .build(app)?;
    let toggle_terminal = MenuItemBuilder::with_id("toggle_terminal", "切换终端")
        .accelerator("CmdOrCtrl+J")
        .build(app)?;
    let split_right = MenuItemBuilder::with_id("split_right", "向右拆分窗格")
        .accelerator("CmdOrCtrl+D")
        .build(app)?;
    let split_down = MenuItemBuilder::with_id("split_down", "向下拆分窗格")
        .accelerator("CmdOrCtrl+Shift+D")
        .build(app)?;
    let close_tab = MenuItemBuilder::with_id("close_tab", "关闭窗格")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let close_other_tabs = MenuItemBuilder::with_id("close_other_tabs", "关闭其他标签页")
        .accelerator("CmdOrCtrl+Alt+T")
        .build(app)?;
    let next_tab = MenuItemBuilder::with_id("next_tab", "下一个标签页")
        .accelerator("CmdOrCtrl+Shift+]")
        .build(app)?;
    let prev_tab = MenuItemBuilder::with_id("prev_tab", "上一个标签页")
        .accelerator("CmdOrCtrl+Shift+[")
        .build(app)?;
    let back_tab = MenuItemBuilder::with_id("back_tab", "后退")
        .accelerator("CmdOrCtrl+[")
        .build(app)?;
    let forward_tab = MenuItemBuilder::with_id("forward_tab", "前进")
        .accelerator("CmdOrCtrl+]")
        .build(app)?;

    let focus_left = MenuItemBuilder::with_id("focus_left", "聚焦左侧窗格")
        .accelerator("CmdOrCtrl+Alt+Left")
        .build(app)?;
    let focus_right = MenuItemBuilder::with_id("focus_right", "聚焦右侧窗格")
        .accelerator("CmdOrCtrl+Alt+Right")
        .build(app)?;
    let focus_up = MenuItemBuilder::with_id("focus_up", "聚焦上方窗格")
        .accelerator("CmdOrCtrl+Alt+Up")
        .build(app)?;
    let focus_down = MenuItemBuilder::with_id("focus_down", "聚焦下方窗格")
        .accelerator("CmdOrCtrl+Alt+Down")
        .build(app)?;

    let toggle_sidebar = MenuItemBuilder::with_id("toggle_sidebar", "切换侧边栏")
        .accelerator("CmdOrCtrl+B")
        .build(app)?;
    let open_model_picker = MenuItemBuilder::with_id("open_model_picker", "切换模型…")
        .accelerator("CmdOrCtrl+.")
        .build(app)?;
    let sidebar_opacity = MenuItemBuilder::with_id("sidebar_opacity", "侧边栏外观…").build(app)?;
    // No accelerators here on purpose: the webview key handler owns
    // CmdOrCtrl + - 0, and a menu accelerator would fire the same command
    // a second time on top of it.
    let zoom_in = MenuItemBuilder::with_id("zoom_in", "放大").build(app)?;
    let zoom_out = MenuItemBuilder::with_id("zoom_out", "缩小").build(app)?;
    let zoom_reset = MenuItemBuilder::with_id("zoom_reset", "重置缩放").build(app)?;
    let find = MenuItemBuilder::with_id("find", "查找")
        .accelerator("CmdOrCtrl+F")
        .build(app)?;

    let find_in_project = MenuItemBuilder::with_id("find_in_project", "在文件中查找…")
        .accelerator("CmdOrCtrl+Shift+F")
        .build(app)?;

    let file = SubmenuBuilder::new(app, "文件")
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
        .separator()
        .item(&prev_tab)
        .item(&next_tab)
        .item(&back_tab)
        .item(&forward_tab)
        .build()?;

    let view = SubmenuBuilder::new(app, "查看")
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
        let window_menu = SubmenuBuilder::new(app, "窗口").build()?;
        window_menu.set_as_windows_menu_for_nsapp()?;
        return Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window_menu]);
    }

    #[allow(unreachable_code)]
    Menu::with_items(app, &[&file, &edit, &view])
}
