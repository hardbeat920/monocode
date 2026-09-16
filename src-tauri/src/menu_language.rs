//! Language data for the native macOS menu.
//!
//! Deliberately free of Tauri types: it is the part of the menu that can be
//! unit-tested anywhere, so the label tables stay honest on every platform
//! even though only macOS draws them.

use std::sync::Mutex;

/// Language the native menu is drawn in.
///
/// The webview owns the preference (it persists to localStorage and resolves
/// "auto" against the OS), but the macOS menu bar lives outside the webview and
/// cannot read it. The frontend therefore pushes the resolved tag across with
/// `set_menu_language` on boot and on every change; this enum is the Rust-side
/// mirror of that decision.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    /// Maps a BCP-47 tag (`zh-CN`, `en`, …) onto a language we have labels for.
    pub fn from_tag(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
    }
}

/// Managed state: the language the menu was last built in.
#[derive(Default)]
pub struct MenuLanguage(Mutex<Lang>);

impl MenuLanguage {
    /// The language the menu was last built in.
    pub fn get(&self) -> Lang {
        self.0.lock().map(|current| *current).unwrap_or_default()
    }

    /// Stores `next`, reporting whether it actually changed.
    pub fn set(&self, next: Lang) -> Result<bool, String> {
        let mut current = self.0.lock().map_err(|error| error.to_string())?;
        if *current == next {
            return Ok(false);
        }
        *current = next;
        Ok(true)
    }
}

/// Every user-visible string in the native menu and the dock menu.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Labels {
    // Submenu titles.
    pub file: &'static str,
    pub edit: &'static str,
    pub view: &'static str,
    pub window: &'static str,
    // App menu.
    pub about: &'static str,
    pub settings: &'static str,
    pub check_for_updates: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub show_all: &'static str,
    pub quit: &'static str,
    // Edit menu.
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub select_all: &'static str,
    pub find: &'static str,
    // File menu.
    pub new_window: &'static str,
    pub open_project: &'static str,
    pub open_search: &'static str,
    pub go_to_file: &'static str,
    pub find_in_project: &'static str,
    pub new_tab: &'static str,
    pub new_terminal: &'static str,
    pub new_terminal_tab: &'static str,
    pub split_right: &'static str,
    pub split_down: &'static str,
    pub close_tab: &'static str,
    pub close_other_tabs: &'static str,
    pub close_all_tabs: &'static str,
    pub prev_tab: &'static str,
    pub next_tab: &'static str,
    pub back_tab: &'static str,
    pub forward_tab: &'static str,
    // View menu.
    pub toggle_sidebar: &'static str,
    pub open_inbox: &'static str,
    pub open_notes: &'static str,
    pub toggle_terminal: &'static str,
    pub switch_model: &'static str,
    pub focus_left: &'static str,
    pub focus_right: &'static str,
    pub focus_up: &'static str,
    pub focus_down: &'static str,
    pub zoom_in: &'static str,
    pub zoom_out: &'static str,
    pub zoom_reset: &'static str,
    pub sidebar_appearance: &'static str,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const EN: Labels = Labels {
    file: "File",
    edit: "Edit",
    view: "View",
    window: "Window",
    about: "About MonoCode",
    settings: "Settings…",
    check_for_updates: "Check for Updates…",
    hide: "Hide MonoCode",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit MonoCode",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    select_all: "Select All",
    find: "Find",
    new_window: "New Window",
    open_project: "Open Project…",
    open_search: "Search…",
    go_to_file: "Go to File…",
    find_in_project: "Find in Files…",
    new_tab: "New Tab",
    new_terminal: "New Terminal",
    new_terminal_tab: "New Terminal Tab",
    split_right: "Split Pane Right",
    split_down: "Split Pane Down",
    close_tab: "Close Pane",
    close_other_tabs: "Close Other Tabs",
    close_all_tabs: "Close All Tabs",
    prev_tab: "Previous Tab",
    next_tab: "Next Tab",
    back_tab: "Go Back",
    forward_tab: "Go Forward",
    toggle_sidebar: "Toggle Sidebar",
    open_inbox: "Inbox",
    open_notes: "Notes",
    toggle_terminal: "Toggle Terminal",
    switch_model: "Switch Model…",
    focus_left: "Focus Pane Left",
    focus_right: "Focus Pane Right",
    focus_up: "Focus Pane Up",
    focus_down: "Focus Pane Down",
    zoom_in: "Zoom In",
    zoom_out: "Zoom Out",
    zoom_reset: "Reset Zoom",
    sidebar_appearance: "Sidebar Appearance…",
};

/// Kept in step with the ZH table in `src/lib/i18n.ts`: the same English source
/// string maps to the same translation, so the macOS menu bar and the in-app
/// menu bar (Windows/Linux) read alike.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const ZH: Labels = Labels {
    file: "文件",
    edit: "编辑",
    view: "视图",
    window: "窗口",
    about: "关于 MonoCode",
    settings: "设置…",
    check_for_updates: "检查更新…",
    hide: "隐藏 MonoCode",
    hide_others: "隐藏其他",
    show_all: "全部显示",
    quit: "退出 MonoCode",
    undo: "撤销",
    redo: "重做",
    cut: "剪切",
    copy: "拷贝",
    paste: "粘贴",
    select_all: "全选",
    find: "查找",
    new_window: "新建窗口",
    open_project: "打开项目…",
    open_search: "搜索…",
    go_to_file: "前往文件…",
    find_in_project: "在文件中查找…",
    new_tab: "新建标签页",
    new_terminal: "新建终端",
    new_terminal_tab: "新建终端标签页",
    split_right: "向右拆分面板",
    split_down: "向下拆分面板",
    close_tab: "关闭面板",
    close_other_tabs: "关闭其他标签页",
    close_all_tabs: "关闭所有标签页",
    prev_tab: "上一个标签页",
    next_tab: "下一个标签页",
    back_tab: "后退",
    forward_tab: "前进",
    toggle_sidebar: "切换侧边栏",
    open_inbox: "收件箱",
    open_notes: "笔记",
    toggle_terminal: "显示/隐藏终端",
    switch_model: "切换模型…",
    focus_left: "聚焦左侧面板",
    focus_right: "聚焦右侧面板",
    focus_up: "聚焦上方面板",
    focus_down: "聚焦下方面板",
    zoom_in: "放大",
    zoom_out: "缩小",
    zoom_reset: "重置缩放",
    sidebar_appearance: "侧边栏外观…",
};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn labels(lang: Lang) -> &'static Labels {
    match lang {
        Lang::En => &EN,
        Lang::Zh => &ZH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_language_tags() {
        assert_eq!(Lang::from_tag("zh"), Lang::Zh);
        assert_eq!(Lang::from_tag("zh-CN"), Lang::Zh);
        assert_eq!(Lang::from_tag("ZH-Hant"), Lang::Zh);
        assert_eq!(Lang::from_tag(" zh-CN "), Lang::Zh);
        assert_eq!(Lang::from_tag("en"), Lang::En);
        assert_eq!(Lang::from_tag("en-US"), Lang::En);
        // An unsupported language falls back to the source language rather
        // than showing a half-translated menu.
        assert_eq!(Lang::from_tag("de-DE"), Lang::En);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }

    #[test]
    fn remembers_which_language_the_menu_was_built_in() {
        let state = MenuLanguage::default();
        // The default matches the language `install` builds at startup, so the
        // first push of "en" does not rebuild the menu for nothing.
        assert!(!state.set(Lang::En).unwrap());
        assert!(state.set(Lang::Zh).unwrap());
        assert!(!state.set(Lang::Zh).unwrap());
        assert!(state.set(Lang::En).unwrap());
    }

    #[test]
    fn translates_every_label() {
        // Destructuring is exhaustive: adding a field to `Labels` breaks this
        // test's compilation, so a new menu string cannot ship untranslated.
        let Labels {
            file,
            edit,
            view,
            window,
            about,
            settings,
            check_for_updates,
            hide,
            hide_others,
            show_all,
            quit,
            undo,
            redo,
            cut,
            copy,
            paste,
            select_all,
            find,
            new_window,
            open_project,
            open_search,
            go_to_file,
            find_in_project,
            new_tab,
            new_terminal,
            new_terminal_tab,
            split_right,
            split_down,
            close_tab,
            close_other_tabs,
            close_all_tabs,
            prev_tab,
            next_tab,
            back_tab,
            forward_tab,
            toggle_sidebar,
            open_inbox,
            open_notes,
            toggle_terminal,
            switch_model,
            focus_left,
            focus_right,
            focus_up,
            focus_down,
            zoom_in,
            zoom_out,
            zoom_reset,
            sidebar_appearance,
        } = ZH;

        for (english, chinese) in [
            (EN.file, file),
            (EN.edit, edit),
            (EN.view, view),
            (EN.window, window),
            (EN.about, about),
            (EN.settings, settings),
            (EN.check_for_updates, check_for_updates),
            (EN.hide, hide),
            (EN.hide_others, hide_others),
            (EN.show_all, show_all),
            (EN.quit, quit),
            (EN.undo, undo),
            (EN.redo, redo),
            (EN.cut, cut),
            (EN.copy, copy),
            (EN.paste, paste),
            (EN.select_all, select_all),
            (EN.find, find),
            (EN.new_window, new_window),
            (EN.open_project, open_project),
            (EN.open_search, open_search),
            (EN.go_to_file, go_to_file),
            (EN.find_in_project, find_in_project),
            (EN.new_tab, new_tab),
            (EN.new_terminal, new_terminal),
            (EN.new_terminal_tab, new_terminal_tab),
            (EN.split_right, split_right),
            (EN.split_down, split_down),
            (EN.close_tab, close_tab),
            (EN.close_other_tabs, close_other_tabs),
            (EN.close_all_tabs, close_all_tabs),
            (EN.prev_tab, prev_tab),
            (EN.next_tab, next_tab),
            (EN.back_tab, back_tab),
            (EN.forward_tab, forward_tab),
            (EN.toggle_sidebar, toggle_sidebar),
            (EN.open_inbox, open_inbox),
            (EN.open_notes, open_notes),
            (EN.toggle_terminal, toggle_terminal),
            (EN.switch_model, switch_model),
            (EN.focus_left, focus_left),
            (EN.focus_right, focus_right),
            (EN.focus_up, focus_up),
            (EN.focus_down, focus_down),
            (EN.zoom_in, zoom_in),
            (EN.zoom_out, zoom_out),
            (EN.zoom_reset, zoom_reset),
            (EN.sidebar_appearance, sidebar_appearance),
        ] {
            assert_ne!(english, chinese, "{english} has no translation");
            assert!(!chinese.is_empty(), "{english} translated to nothing");
        }
    }

    #[test]
    fn selects_the_matching_table() {
        assert_eq!(labels(Lang::En).settings, EN.settings);
        assert_eq!(labels(Lang::Zh).settings, ZH.settings);
        assert_eq!(labels(Lang::Zh).new_window, "新建窗口");
    }
}
