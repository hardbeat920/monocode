//! Port of src/features/projects/model/projectGroups.ts: named groups on
//! the project rail and which group each project sits in.
//!
//! Storage goes through `Kv`. `notifyProjectPathsChanged` after a save
//! becomes `ProjectsEvent::PathsChanged`, which the `Projects` entity emits.

use std::collections::HashSet;

use monocode_core::js;
use monocode_core::paths::path_key;
use monocode_layout::tab_groups::{JsRecord, TAB_GROUP_COLORS, tab_group_color};
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::js_object::{Parsed, parse, parse_object, stringify};
use super::project_mascots::is_project_mascot;

pub const GROUPS_KEY: &str = "monocode.projectGroups";
pub const ASSIGNMENTS_KEY: &str = "monocode.projectGroupAssignments";

/// `ProjectGroup`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroup {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mascot: Option<String>,
}

impl ProjectGroup {
    pub fn new(id: impl Into<String>, name: impl Into<String>, collapsed: bool) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            collapsed,
            color_index: None,
            custom_color: None,
            mascot: None,
        }
    }
}

/// `HEX_COLOR_RE`: `/^#[0-9a-fA-F]{6}$/`.
fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

/// `normalizeGroup`.
fn normalize_group(value: &Value) -> Option<ProjectGroup> {
    let candidate = value.as_object()?;
    let id = candidate
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !js::trim(id).is_empty())?;
    let name = candidate
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !js::trim(name).is_empty())?;

    let color_index = candidate
        .get("colorIndex")
        .and_then(Value::as_f64)
        .filter(|index| {
            index.fract() == 0.0 && *index >= 0.0 && *index < TAB_GROUP_COLORS.len() as f64
        })
        .map(|index| index as i64);
    let custom_color = candidate
        .get("customColor")
        .and_then(Value::as_str)
        .filter(|color| is_hex_color(color))
        .map(str::to_lowercase);
    let mascot = candidate
        .get("mascot")
        .and_then(Value::as_str)
        .filter(|mascot| is_project_mascot(mascot))
        .map(str::to_string);

    Some(ProjectGroup {
        id: id.to_string(),
        name: js::trim(name).to_string(),
        collapsed: candidate.get("collapsed") == Some(&Value::Bool(true)),
        // A custom color wins over a palette index.
        color_index: if custom_color.is_some() {
            None
        } else {
            color_index
        },
        custom_color,
        mascot,
    })
}

/// `loadProjectGroups`.
pub fn load_project_groups(kv: &Kv) -> Vec<ProjectGroup> {
    let raw = kv.get_item(GROUPS_KEY).unwrap_or_else(|| "[]".into());
    let Some(Parsed::Array(items)) = parse(&raw) else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    let mut seen = HashSet::new();
    for value in &items {
        let Some(group) = normalize_group(value) else {
            continue;
        };
        if !seen.insert(group.id.clone()) {
            continue;
        }
        groups.push(group);
    }
    groups
}

/// `saveProjectGroups`. Returns `false` when the write failed; `Kv` writes
/// cannot fail, so it is always `true`.
pub fn save_project_groups(kv: &Kv, groups: &[ProjectGroup]) -> bool {
    let normalized: Vec<ProjectGroup> = groups
        .iter()
        .filter_map(|group| serde_json::to_value(group).ok())
        .filter_map(|value| normalize_group(&value))
        .collect();
    match serde_json::to_string(&normalized) {
        Ok(json) => {
            kv.set_item(GROUPS_KEY, &json);
            true
        }
        Err(_) => false,
    }
}

/// `loadProjectGroupAssignments`: path key to group id, only for groups that
/// still exist. `groups` defaults to the stored groups.
pub fn load_project_group_assignments(
    kv: &Kv,
    groups: Option<&[ProjectGroup]>,
) -> JsRecord<String> {
    let loaded;
    let groups = match groups {
        Some(groups) => groups,
        None => {
            loaded = load_project_groups(kv);
            &loaded
        }
    };
    let raw = kv.get_item(ASSIGNMENTS_KEY).unwrap_or_else(|| "{}".into());
    let Some(parsed) = parse_object(&raw) else {
        return JsRecord::new();
    };
    let group_ids: HashSet<&str> = groups.iter().map(|group| group.id.as_str()).collect();
    let mut out = JsRecord::new();
    for (key, value) in parsed.iter() {
        if key.is_empty() {
            continue;
        }
        if let Some(group) = value.as_str().filter(|group| group_ids.contains(group)) {
            out.insert(key, group.to_string());
        }
    }
    out
}

/// `saveProjectGroupAssignments`.
pub fn save_project_group_assignments(kv: &Kv, assignments: &JsRecord<String>) -> bool {
    kv.set_item(ASSIGNMENTS_KEY, &stringify(assignments));
    true
}

/// `projectGroupIdForPath`.
pub fn project_group_id_for_path(path: &str, assignments: &JsRecord<String>) -> Option<String> {
    assignments.get(&path_key(path)).cloned()
}

/// `setProjectGroupAssignment`: put a project in a group, or take it out
/// with `None`. An unknown group id changes nothing.
pub fn set_project_group_assignment(
    kv: &Kv,
    path: &str,
    group_id: Option<&str>,
) -> JsRecord<String> {
    let mut next = load_project_group_assignments(kv, None);
    let key = path_key(path);
    match group_id {
        None => {
            next.remove(&key);
        }
        Some(group_id) => {
            if load_project_groups(kv)
                .iter()
                .any(|group| group.id == group_id)
            {
                next.insert(key, group_id.to_string());
            }
        }
    }
    save_project_group_assignments(kv, &next);
    next
}

/// `removeProjectGroupAssignment`.
pub fn remove_project_group_assignment(kv: &Kv, path: &str) {
    set_project_group_assignment(kv, path, None);
}

/// `rebaseProjectGroupAssignment`: follow a project rename.
pub fn rebase_project_group_assignment(kv: &Kv, from: &str, to: &str) {
    let mut next = load_project_group_assignments(kv, None);
    let old_key = path_key(from);
    let new_key = path_key(to);
    let Some(group) = next.get(&old_key).cloned() else {
        return;
    };
    if old_key == new_key {
        return;
    }
    if !next.contains_key(&new_key) {
        next.insert(new_key, group);
    }
    next.remove(&old_key);
    save_project_group_assignments(kv, &next);
}

/// `updateProjectGroup`.
pub fn update_project_group(
    kv: &Kv,
    id: &str,
    update: impl FnOnce(ProjectGroup) -> ProjectGroup,
) -> bool {
    let mut current = load_project_groups(kv);
    // Stored ids are unique, so at most one group matches.
    let Some(index) = current.iter().position(|group| group.id == id) else {
        return false;
    };
    let group = current[index].clone();
    current[index] = update(group);
    save_project_groups(kv, &current)
}

/// `deleteProjectGroup`: remove the group; its projects become ungrouped.
pub fn delete_project_group(kv: &Kv, id: &str) -> bool {
    let next_groups: Vec<ProjectGroup> = load_project_groups(kv)
        .into_iter()
        .filter(|group| group.id != id)
        .collect();
    if !save_project_groups(kv, &next_groups) {
        return false;
    }
    let assignments = load_project_group_assignments(kv, Some(&next_groups));
    save_project_group_assignments(kv, &assignments);
    true
}

/// `projectGroupColor`.
pub fn project_group_color(group: &ProjectGroup) -> String {
    if let Some(color) = &group.custom_color {
        return color.clone();
    }
    if let Some(index) = group
        .color_index
        .filter(|index| *index >= 0 && (*index as usize) < TAB_GROUP_COLORS.len())
    {
        return TAB_GROUP_COLORS[index as usize].to_string();
    }
    tab_group_color(&group.id).to_string()
}

/// `nextProjectGroupName`: "New group", then "New group 2", and so on.
pub fn next_project_group_name(groups: &[ProjectGroup]) -> String {
    let names: HashSet<String> = groups
        .iter()
        .map(|group| group.name.to_lowercase())
        .collect();
    if !names.contains("new group") {
        return "New group".into();
    }
    let mut suffix = 2;
    loop {
        let name = format!("New group {suffix}");
        if !names.contains(&name.to_lowercase()) {
            return name;
        }
        suffix += 1;
    }
}

/// `createProjectGroup`: a new expanded group with the next free name.
pub fn create_project_group(groups: &[ProjectGroup]) -> ProjectGroup {
    ProjectGroup::new(
        uuid::Uuid::new_v4().to_string(),
        next_project_group_name(groups),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_ordered_appearance_and_collapsed_state() {
        let kv = Kv::in_memory();
        let clients = ProjectGroup {
            custom_color: Some("#AABBCC".into()),
            mascot: Some("ghost".into()),
            ..ProjectGroup::new("clients", "Clients", true)
        };
        let personal = ProjectGroup {
            color_index: Some(4),
            ..ProjectGroup::new("personal", "Personal", false)
        };
        assert!(save_project_groups(&kv, &[clients, personal]));

        assert_eq!(
            load_project_groups(&kv),
            [
                ProjectGroup {
                    custom_color: Some("#aabbcc".into()),
                    mascot: Some("ghost".into()),
                    ..ProjectGroup::new("clients", "Clients", true)
                },
                ProjectGroup {
                    color_index: Some(4),
                    ..ProjectGroup::new("personal", "Personal", false)
                },
            ]
        );
        assert_eq!(
            kv.get_item(GROUPS_KEY).unwrap(),
            r##"[{"id":"clients","name":"Clients","collapsed":true,"customColor":"#aabbcc","mascot":"ghost"},{"id":"personal","name":"Personal","collapsed":false,"colorIndex":4}]"##
        );
    }

    #[test]
    fn keeps_assignments_only_for_groups_that_still_exist() {
        let kv = Kv::in_memory();
        save_project_groups(&kv, &[ProjectGroup::new("clients", "Clients", false)]);
        let mut assignments = JsRecord::new();
        assignments.insert(path_key("/work/client"), "clients".to_string());
        assignments.insert(path_key("/work/stale"), "missing".to_string());
        save_project_group_assignments(&kv, &assignments);

        let loaded = load_project_group_assignments(&kv, None);
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded.get("/work/client").map(String::as_str),
            Some("clients")
        );
        assert!(set_project_group_assignment(&kv, "/work/client", None).is_empty());
    }

    #[test]
    fn creates_stable_unique_default_names() {
        let groups = [
            ProjectGroup::new("one", "New group", false),
            ProjectGroup::new("two", "NEW GROUP 2", false),
        ];
        assert_eq!(next_project_group_name(&groups), "New group 3");
        let created = create_project_group(&groups);
        assert_eq!(created.name, "New group 3");
        assert!(!created.collapsed);
        assert_eq!(next_project_group_name(&[]), "New group");
    }

    #[test]
    fn drops_invalid_groups_and_duplicates() {
        let kv = Kv::in_memory();
        kv.set_item(
            GROUPS_KEY,
            r#"[{"id":"a","name":" A "},{"id":"a","name":"Again"},{"id":" ","name":"x"},{"id":"b","name":""},{"id":"c","name":"C","colorIndex":1.5,"mascot":"nope","collapsed":"yes"}]"#,
        );
        assert_eq!(
            load_project_groups(&kv),
            [
                ProjectGroup::new("a", "A", false),
                ProjectGroup::new("c", "C", false)
            ]
        );
        kv.set_item(GROUPS_KEY, "{}");
        assert!(load_project_groups(&kv).is_empty());
    }

    #[test]
    fn rebases_and_deletes_assignments() {
        let kv = Kv::in_memory();
        save_project_groups(
            &kv,
            &[
                ProjectGroup::new("g1", "One", false),
                ProjectGroup::new("g2", "Two", false),
            ],
        );
        set_project_group_assignment(&kv, "/work/a", Some("g1"));
        set_project_group_assignment(&kv, "/work/b", Some("g2"));
        set_project_group_assignment(&kv, "/work/c", Some("unknown"));
        rebase_project_group_assignment(&kv, "/work/a", "/work/renamed");
        let loaded = load_project_group_assignments(&kv, None);
        assert_eq!(
            project_group_id_for_path("/work/renamed/", &loaded).as_deref(),
            Some("g1")
        );
        assert_eq!(project_group_id_for_path("/work/a", &loaded), None);
        assert_eq!(project_group_id_for_path("/work/c", &loaded), None);

        assert!(delete_project_group(&kv, "g2"));
        let loaded = load_project_group_assignments(&kv, None);
        assert_eq!(project_group_id_for_path("/work/b", &loaded), None);
        assert_eq!(load_project_groups(&kv).len(), 1);
    }

    #[test]
    fn updates_one_group_and_picks_colors() {
        let kv = Kv::in_memory();
        save_project_groups(&kv, &[ProjectGroup::new("g1", "One", false)]);
        assert!(update_project_group(&kv, "g1", |group| ProjectGroup {
            collapsed: true,
            color_index: Some(2),
            ..group
        }));
        assert!(!update_project_group(&kv, "missing", |group| group));
        let group = &load_project_groups(&kv)[0];
        assert!(group.collapsed);
        assert_eq!(project_group_color(group), TAB_GROUP_COLORS[2]);
        let custom = ProjectGroup {
            custom_color: Some("#112233".into()),
            ..group.clone()
        };
        assert_eq!(project_group_color(&custom), "#112233");
        let plain = ProjectGroup::new("g1", "One", false);
        assert_eq!(project_group_color(&plain), tab_group_color("g1"));
    }
}
