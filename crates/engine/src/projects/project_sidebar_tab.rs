//! Port of src/features/settings/model/projectSidebarTab.ts: which Workspace
//! tab (sessions, files, or changes) each project last showed.

use monocode_core::appearance::{SIDEBAR_TAB_ORDER_KEY, SidebarTabId, parse_sidebar_tab_order};
use monocode_core::paths::path_key;
use monocode_layout::tab_groups::JsRecord;
use monocode_settings::Kv;
use serde_json::Value;

use super::js_object::{parse_object, stringify};

pub const KEY: &str = "monocode.projectSidebarTabs.v1";

/// `isProjectSidebarTab`: every sidebar tab but Inbox.
pub fn is_project_sidebar_tab(tab: SidebarTabId) -> bool {
    matches!(
        tab,
        SidebarTabId::Sessions | SidebarTabId::Files | SidebarTabId::Changes
    )
}

fn stored_tab(value: &Value) -> Option<SidebarTabId> {
    value
        .as_str()
        .and_then(SidebarTabId::from_str_opt)
        .filter(|tab| is_project_sidebar_tab(*tab))
}

/// `readAll`.
fn read_all(kv: &Kv) -> JsRecord<String> {
    let Some(raw) = kv.get_item(KEY).filter(|raw| !raw.is_empty()) else {
        return JsRecord::new();
    };
    let Some(parsed) = parse_object(&raw) else {
        return JsRecord::new();
    };
    let mut out = JsRecord::new();
    for (key, value) in parsed.iter() {
        if let Some(tab) = stored_tab(value) {
            out.insert(key, tab.as_str().to_string());
        }
    }
    out
}

/// `writeAll`.
fn write_all(kv: &Kv, tabs: &JsRecord<String>) {
    kv.set_item(KEY, &stringify(tabs));
}

/// `loadProjectSidebarTab`: the project's saved tab, else the first visible
/// tab in the global order.
pub fn load_project_sidebar_tab(kv: &Kv, project: &str) -> SidebarTabId {
    if let Some(saved) = read_all(kv)
        .get(&path_key(project))
        .and_then(|tab| SidebarTabId::from_str_opt(tab))
    {
        return saved;
    }
    parse_sidebar_tab_order(kv.get_item(SIDEBAR_TAB_ORDER_KEY).as_deref())
        .into_iter()
        .find(|tab| is_project_sidebar_tab(*tab))
        .unwrap_or(SidebarTabId::Sessions)
}

/// `saveProjectSidebarTab`. Home and Inbox are never saved.
pub fn save_project_sidebar_tab(kv: &Kv, project: &str, tab: SidebarTabId) {
    if project == "~" || !is_project_sidebar_tab(tab) {
        return;
    }
    let mut tabs = read_all(kv);
    tabs.insert(path_key(project), tab.as_str().to_string());
    write_all(kv, &tabs);
}

/// `clearProjectSidebarTab`.
pub fn clear_project_sidebar_tab(kv: &Kv, project: &str) {
    let mut tabs = read_all(kv);
    if tabs.remove(&path_key(project)).is_none() {
        return;
    }
    write_all(kv, &tabs);
}

/// `rebaseProjectSidebarTab`: follow a project rename.
pub fn rebase_project_sidebar_tab(kv: &Kv, from: &str, to: &str) {
    let old_key = path_key(from);
    let new_key = path_key(to);
    if old_key == new_key {
        return;
    }
    let mut tabs = read_all(kv);
    let Some(tab) = tabs.get(&old_key).cloned() else {
        return;
    };
    tabs.insert(new_key, tab);
    tabs.remove(&old_key);
    write_all(kv, &tabs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_each_projects_tab_across_path_spelling_changes() {
        let kv = Kv::in_memory();
        save_project_sidebar_tab(&kv, "/work/one/", SidebarTabId::Files);
        save_project_sidebar_tab(&kv, "/work/two", SidebarTabId::Changes);

        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/one"),
            SidebarTabId::Files
        );
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/two/"),
            SidebarTabId::Changes
        );
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/three"),
            SidebarTabId::Sessions
        );
    }

    #[test]
    fn ignores_invalid_saved_tabs_and_never_saves_inbox_as_a_workspace_tab() {
        let kv = Kv::in_memory();
        kv.set_item(KEY, r#"{"/work/one":"unknown"}"#);
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/one"),
            SidebarTabId::Sessions
        );

        save_project_sidebar_tab(&kv, "/work/one", SidebarTabId::Inbox);
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/one"),
            SidebarTabId::Sessions
        );
    }

    #[test]
    fn uses_the_first_visible_tab_in_the_global_order_for_a_new_project() {
        let kv = Kv::in_memory();
        kv.set_item(
            SIDEBAR_TAB_ORDER_KEY,
            r#"["inbox","files","sessions","changes"]"#,
        );
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/new"),
            SidebarTabId::Files
        );
    }

    #[test]
    fn follows_a_project_rename_and_clears_a_deleted_projects_choice() {
        let kv = Kv::in_memory();
        save_project_sidebar_tab(&kv, "/work/one", SidebarTabId::Files);
        rebase_project_sidebar_tab(&kv, "/work/one", "/work/renamed");

        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/renamed"),
            SidebarTabId::Files
        );
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/one"),
            SidebarTabId::Sessions
        );

        clear_project_sidebar_tab(&kv, "/work/renamed");
        assert_eq!(
            load_project_sidebar_tab(&kv, "/work/renamed"),
            SidebarTabId::Sessions
        );
    }

    #[test]
    fn home_is_never_saved() {
        let kv = Kv::in_memory();
        save_project_sidebar_tab(&kv, "~", SidebarTabId::Files);
        assert!(kv.get_item(KEY).is_none());
    }
}
