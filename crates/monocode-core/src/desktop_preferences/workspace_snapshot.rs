//! Translate supported persisted desktop viewport state without source writes.
use super::{normalize_path, workspace_preferences::valid_hex, DesktopPreferences};
use crate::session_groups::{Folder, Groups};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

impl DesktopPreferences {
    /// A native groups sidecar, when present, is the deliberate native override.
    /// The callback must resolve existing database IDs, never invent sessions for
    /// stale folder members or reinterpret numeric-looking canonical strings.
    pub fn groups(
        &self,
        cwd: &str,
        mut resolve: impl FnMut(&str) -> Option<i64>,
    ) -> Option<Groups> {
        if !self.is_available() {
            return None;
        }
        let key = crate::files::path_key(&normalize_path(cwd));
        let project = |value: Value| {
            value.as_object().and_then(|values| {
                values
                    .iter()
                    .find(|(path, _)| crate::files::path_key(&normalize_path(path)) == key)
                    .map(|(_, value)| value.clone())
            })
        };
        let folders = self.json("monocode.sessionFolders").and_then(project);
        let mut groups = Groups::default();
        let mut seen = HashSet::new();
        let mut ids = HashMap::new();
        for folder in folders
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(id), Some(name), Some(members)) = (
                folder["id"].as_str().filter(|s| !s.is_empty()),
                folder["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
                folder["sessionIds"].as_array(),
            ) else {
                continue;
            };
            if !seen.insert(id.to_owned()) {
                continue;
            }
            let mut session_ids = Vec::new();
            for original in members.iter().filter_map(Value::as_str) {
                let id = *ids
                    .entry(original.to_owned())
                    .or_insert_with(|| resolve(original));
                if let Some(id) = id.filter(|id| !session_ids.contains(id)) {
                    session_ids.push(id);
                }
            }
            let custom_color = folder["customColor"]
                .as_str()
                .filter(|value| valid_hex(value))
                .map(str::to_ascii_lowercase);
            groups.folders.push(Folder {
                id: id.to_owned(),
                name: name.to_owned(),
                session_ids,
                collapsed: folder["collapsed"] == true,
                color_index: folder["colorIndex"]
                    .as_u64()
                    .filter(|n| (1..9).contains(n))
                    .filter(|_| custom_color.is_none())
                    .map(|n| n as usize),
                custom_color,
            });
        }
        groups.pinned_collapsed = self
            .json("monocode.pinnedSessionsCollapsed")
            .and_then(project)
            .is_some_and(|v| v == true);
        groups.reminders_collapsed = self
            .json("monocode.reminderSessionsCollapsed")
            .and_then(project)
            .is_some_and(|v| v == true);
        Some(groups)
    }

    /// Restore persisted chat split trees, title order/focus and project docks.
    /// Keep the full source snapshot alongside supported native fields so file,
    /// terminal-pane and explicit tab-group metadata remains available unchanged.
    /// Only use this when no native workspace override has been saved.
    pub fn workspace_snapshot(
        snapshot: &Value,
        mut resolve: impl FnMut(&str) -> Option<i64>,
    ) -> Value {
        let project = snapshot["projectCwd"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or("~");
        let mut workspace = json!({"project":normalize_path(project), "projects":[normalize_path(project)], "desktop_workspace_snapshot":snapshot});
        let mut tabs = Vec::new();
        let mut layouts = Map::new();
        let mut focused = Map::new();
        let mut tab_keys = HashMap::new();
        let mut leaves = HashMap::new();
        let mut seen = HashSet::new();
        let excluded: HashSet<_> = snapshot["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| !s["inboxAsk"].is_null())
            .filter_map(|s| s["id"].as_str())
            .collect();
        let mut cached_ids = HashMap::new();
        let mut resolve = |original: &str| {
            if excluded.contains(original) {
                return None;
            }
            *cached_ids
                .entry(original.to_owned())
                .or_insert_with(|| resolve(original))
        };
        for tab in snapshot["tabs"].as_array().into_iter().flatten() {
            let Some(layout) = layout(&tab["layout"], &mut resolve, &mut seen, &mut leaves) else {
                continue;
            };
            let key = first_leaf(&layout);
            let focus = tab["focusedId"]
                .as_str()
                .and_then(|id| leaves.get(id))
                .copied()
                .filter(|id| contains(&layout, *id))
                .unwrap_or(key);
            if let Some(id) = tab["id"].as_str() {
                tab_keys.insert(id.to_owned(), (key, focus));
            }
            tabs.push(key);
            if layout["type"] == "split" {
                layouts.insert(key.to_string(), layout);
                focused.insert(key.to_string(), json!(focus));
            }
        }
        workspace["tabs"] = json!(tabs);
        workspace["title_order"] = json!(tabs
            .iter()
            .map(|id| json!({"Session":id}))
            .collect::<Vec<_>>());
        workspace["layouts"] = json!(layouts);
        workspace["focused"] = json!(focused);
        workspace["active"] = json!(snapshot["activeTabId"]
            .as_str()
            .and_then(|id| tab_keys.get(id))
            .map(|(_, focus)| *focus)
            .or_else(|| tabs.first().copied()));
        let mut project_focus = Map::new();
        for memory in snapshot["projectReturnTargets"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(path) = memory["projectPath"].as_str() else {
                continue;
            };
            let id = memory["paneId"]
                .as_str()
                .or_else(|| memory["tabId"].as_str());
            if let Some(focus) = id.and_then(|id| {
                leaves
                    .get(id)
                    .copied()
                    .or_else(|| tab_keys.get(id).map(|(_, focus)| *focus))
            }) {
                project_focus.insert(normalize_path(path), json!(focus));
            }
        }
        workspace["project_focus"] = json!(project_focus);
        let mut docks = Map::new();
        for dock in snapshot["projectTerminals"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(project) = dock["projectPath"].as_str() else {
                continue;
            };
            let files = dock["pane"]["files"].as_array();
            let terminals: Vec<_> = files
                .into_iter()
                .flatten()
                .filter(|file| file["terminal"] == true)
                .filter_map(|file| {
                    file["cwd"]
                        .as_str()
                        .map(|cwd| (file["id"].as_str(), normalize_path(cwd)))
                })
                .collect();
            if terminals.is_empty() {
                continue;
            }
            let active = terminals
                .iter()
                .position(|(id, _)| *id == dock["pane"]["activeFileId"].as_str())
                .unwrap_or(0);
            let side = dock["side"]
                .as_str()
                .filter(|s| ["top", "bottom", "left", "right"].contains(s))
                .unwrap_or("bottom");
            let size = dock["size"]
                .as_f64()
                .filter(|size| size.is_finite())
                .unwrap_or(if ["top", "bottom"].contains(&side) {
                    220.
                } else {
                    360.
                });
            docks.insert(normalize_path(project), json!({"side":side, "size":size, "open":dock["open"] == true, "terminals":terminals.iter().map(|(_, cwd)| cwd).collect::<Vec<_>>(), "active":active}));
        }
        workspace["docks"] = json!(docks);
        workspace
    }
}

fn layout(
    node: &Value,
    resolve: &mut impl FnMut(&str) -> Option<i64>,
    seen: &mut HashSet<i64>,
    leaves: &mut HashMap<String, i64>,
) -> Option<Value> {
    match node["type"].as_str()? {
        "leaf" => {
            let original = node["id"].as_str()?;
            let id = resolve(original)?;
            if !seen.insert(id) {
                return None;
            }
            leaves.insert(original.to_owned(), id);
            Some(json!({"type":"leaf", "id":id}))
        }
        "split" => {
            let dir = node["dir"]
                .as_str()
                .filter(|s| ["right", "down"].contains(s))?;
            let mut children = Vec::new();
            let mut sizes = Vec::new();
            for (ix, child) in node["children"].as_array()?.iter().enumerate() {
                if let Some(child) = layout(child, resolve, seen, leaves) {
                    children.push(child);
                    sizes.push(
                        node["sizes"][ix]
                            .as_f64()
                            .filter(|n| n.is_finite() && *n > 0.)
                            .unwrap_or(1.),
                    );
                }
            }
            if children.len() == 1 {
                return children.pop();
            }
            if children.is_empty() {
                return None;
            }
            let total: f64 = sizes.iter().sum();
            for size in &mut sizes {
                *size /= total;
            }
            Some(json!({"type":"split", "dir":dir, "children":children, "sizes":sizes}))
        }
        _ => None,
    }
}
fn first_leaf(node: &Value) -> i64 {
    node["id"]
        .as_i64()
        .unwrap_or_else(|| first_leaf(&node["children"][0]))
}
fn contains(node: &Value, id: i64) -> bool {
    node["id"] == id
        || node["children"]
            .as_array()
            .is_some_and(|children| children.iter().any(|node| contains(node, id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_restores_only_existing_chat_leaves_with_sizes_focus_and_docks() {
        let source = json!({"projectCwd":"/work", "activeTabId":"tab", "tabs":[{"id":"tab", "focusedId":"b", "groupId":"assigned", "layout":{"type":"split", "dir":"right", "children":[{"type":"leaf", "id":"a"}, {"type":"leaf", "id":"gone"}, {"type":"leaf", "id":"b"}], "sizes":[0.2,0.2,0.6]}}], "projectReturnTargets":[{"projectPath":"/work", "tabId":"tab"}], "projectTerminals":[{"projectPath":"/work", "side":"left", "size":370, "open":false, "pane":{"activeFileId":"t2", "files":[{"id":"t1", "terminal":true, "cwd":"/work"}, {"id":"t2", "terminal":true, "cwd":"/work/sub"}]}}]});
        let mapped = DesktopPreferences::workspace_snapshot(&source, |id| match id {
            "a" => Some(1),
            "b" => Some(2),
            _ => None,
        });
        assert_eq!(mapped["tabs"], json!([1]));
        assert_eq!(mapped["active"], 2);
        assert!((mapped["layouts"]["1"]["sizes"][0].as_f64().unwrap() - 0.25).abs() < 1e-9);
        assert!((mapped["layouts"]["1"]["sizes"][1].as_f64().unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(mapped["project_focus"]["/work"], 2);
        assert_eq!(mapped["docks"]["/work"]["active"], 1);
        assert_eq!(mapped["desktop_workspace_snapshot"], source);
    }
    #[test]
    fn folders_map_string_ids_and_keep_custom_colors_and_collapsed_sections() {
        let source = DesktopPreferences { available:true, values:[("monocode.sessionFolders".into(), json!({"/work/":[{"id":"f", "name":" Work ", "sessionIds":["123", "gone", "123"], "collapsed":true, "colorIndex":2, "customColor":"#AbC123"}]}).to_string()), ("monocode.pinnedSessionsCollapsed".into(), json!({"/work":true}).to_string())].into_iter().collect() };
        let groups = source
            .groups("/work", |id| (id == "123").then_some(42))
            .unwrap();
        assert_eq!(groups.folders[0].session_ids, vec![42]);
        assert_eq!(groups.folders[0].name, "Work");
        assert_eq!(groups.folders[0].custom_color.as_deref(), Some("#abc123"));
        assert_eq!(groups.folders[0].color_index, None);
        assert!(groups.folders[0].collapsed && groups.pinned_collapsed);
    }

    #[test]
    fn unresolved_folder_members_do_not_discard_user_folder_metadata() {
        let source = DesktopPreferences {
            available: true,
            values: [("monocode.sessionFolders".into(), json!({"/work":[
                {"id":"stale", "name":"Keep my folder", "sessionIds":["gone"], "collapsed":true, "customColor":"#abc123"},
                {"id":"empty", "name":"Empty folder", "sessionIds":[], "colorIndex":2}
            ]}).to_string())].into_iter().collect(),
        };
        let groups = source.groups("/work", |_| None).unwrap();
        assert_eq!(groups.folders.len(), 2);
        assert!(groups
            .folders
            .iter()
            .all(|folder| folder.session_ids.is_empty()));
        assert_eq!(groups.folders[0].name, "Keep my folder");
        assert_eq!(groups.folders[0].custom_color.as_deref(), Some("#abc123"));
        assert!(groups.folders[0].collapsed);
        let roundtrip: Groups =
            serde_json::from_value(serde_json::to_value(&groups).unwrap()).unwrap();
        assert_eq!(roundtrip, groups);
    }
}
