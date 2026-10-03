//! The project rail's list: a small read-only port of
//! src/features/projects/model/recents.ts (`loadRecents`,
//! `loadProjectRailOrder`, `loadPinnedProjects`, `collectRailProjects`,
//! `syncProjectRailOrder`, `lastProjectPath`) and the rail colors from
//! tabGroups.ts. The engine's `projects` package will replace it.

use monocode_layout::paths::{
    normalize_project_path, pretty_cwd, project_key, project_name, same_project_path,
};
use monocode_layout::tab_groups::{AppearanceStore, TabGroupAppearance, resolve_tab_group_color};
use monocode_settings::Kv;
use serde_json::Value;

const RECENTS_KEY: &str = "monocode.recentProjects";
const RAIL_ORDER_KEY: &str = "monocode.projectRailOrder";
const RAIL_PINNED_KEY: &str = "monocode.projectRailPinned";
const ARCHIVED_KEY: &str = "monocode.archivedProjects";

/// `RecentProject`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentProject {
    pub path: String,
    pub opened_at: i64,
}

/// One project rail card.
#[derive(Debug, Clone, PartialEq)]
pub struct RailProject {
    pub path: String,
    pub name: String,
    /// The tint as `#rrggbb` (`resolveTabGroupColor`).
    pub color: String,
    pub pinned: bool,
}

/// `looksLikeProject`.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let normalized = normalize_project_path(path);
    let bytes = normalized.as_bytes();
    let drive = bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || normalized == "/" {
        return false;
    }
    if pretty_cwd(path) == "~" {
        return false;
    }
    !(path.contains(".app/") || path.contains(".app\\"))
}

fn parse_array(kv: &Kv, key: &str) -> Vec<Value> {
    kv.get_item(key)
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| match value {
            Value::Array(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default()
}

/// `loadRecents`.
pub fn load_recents(kv: &Kv) -> Vec<RecentProject> {
    parse_array(kv, RECENTS_KEY)
        .into_iter()
        .filter_map(|item| {
            let path = item.get("path")?.as_str().filter(|path| !path.is_empty())?;
            let opened_at = item
                .get("openedAt")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or(0.0) as i64;
            Some(RecentProject {
                path: normalize_project_path(path),
                opened_at,
            })
        })
        .collect()
}

/// `readPathList`.
fn read_path_list(kv: &Kv, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in parse_array(kv, key) {
        let Some(path) = item.as_str().filter(|path| !path.is_empty()) else {
            continue;
        };
        let path = normalize_project_path(path);
        if out.iter().any(|seen| same_project_path(seen, &path)) {
            continue;
        }
        out.push(path);
    }
    out
}

/// `lastProjectPath`: the most recently opened project.
pub fn last_project_path(kv: &Kv) -> Option<String> {
    load_recents(kv)
        .into_iter()
        .find(|item| looks_like_project(&item.path))
        .map(|item| item.path)
}

/// `knownProjectPaths`.
fn known_project_paths(kv: &Kv) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let archived = parse_array(kv, ARCHIVED_KEY)
        .into_iter()
        .filter_map(|item| item.get("path")?.as_str().map(str::to_string));
    let paths = load_recents(kv)
        .into_iter()
        .map(|item| item.path)
        .chain(read_path_list(kv, RAIL_ORDER_KEY))
        .chain(read_path_list(kv, RAIL_PINNED_KEY))
        .chain(archived);
    for path in paths {
        if out
            .iter()
            .any(|seen| project_key(seen) == project_key(&path))
        {
            continue;
        }
        out.push(normalize_project_path(&path));
    }
    out
}

/// `AppearanceStore` over `Kv`.
struct KvAppearance<'a>(&'a Kv);

impl AppearanceStore for KvAppearance<'_> {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.get_item(key)
    }

    fn set_item(&mut self, key: &str, value: &str) -> bool {
        self.0.set_item(key, value);
        true
    }

    fn known_project_paths(&self) -> Vec<String> {
        known_project_paths(self.0)
    }
}

/// The rail's projects: pinned ones first in pin order, then the saved
/// order with newcomers appended newest first (`collectRailProjects`,
/// `syncProjectRailOrder`, and `projectRailSections`).
pub fn rail_projects(kv: &Kv, current_cwd: &str) -> Vec<RailProject> {
    let mut projects: Vec<RecentProject> = Vec::new();
    for item in load_recents(kv) {
        if !looks_like_project(&item.path) {
            continue;
        }
        match projects
            .iter_mut()
            .find(|seen| project_key(&seen.path) == project_key(&item.path))
        {
            Some(seen) => *seen = item,
            None => projects.push(item),
        }
    }
    if looks_like_project(current_cwd)
        && !projects
            .iter()
            .any(|seen| same_project_path(&seen.path, current_cwd))
    {
        projects.push(RecentProject {
            path: normalize_project_path(current_cwd),
            opened_at: i64::MAX,
        });
    }

    let order = read_path_list(kv, RAIL_ORDER_KEY);
    let mut ordered: Vec<RecentProject> = Vec::new();
    for path in &order {
        if let Some(project) = projects
            .iter()
            .find(|project| same_project_path(&project.path, path))
            && !ordered
                .iter()
                .any(|seen| same_project_path(&seen.path, &project.path))
        {
            ordered.push(project.clone());
        }
    }
    let mut newcomers: Vec<RecentProject> = projects
        .iter()
        .filter(|project| {
            !ordered
                .iter()
                .any(|seen| same_project_path(&seen.path, &project.path))
        })
        .cloned()
        .collect();
    newcomers.sort_by_key(|project| std::cmp::Reverse(project.opened_at));
    ordered.extend(newcomers);

    let pinned_paths = read_path_list(kv, RAIL_PINNED_KEY);
    let mut store = KvAppearance(kv);
    let mut appearance = TabGroupAppearance::new();
    let colors = appearance.load_tab_group_colors(&mut store);
    let custom = appearance.load_tab_group_custom_colors(&mut store);
    let card = |project: &RecentProject, pinned: bool| {
        let key = project_key(&project.path);
        let seed = project_name(&project.path);
        RailProject {
            name: project_name(&project.path),
            color: resolve_tab_group_color(&key, Some(&colors), Some(&custom), Some(&seed)),
            path: project.path.clone(),
            pinned,
        }
    };
    let mut out: Vec<RailProject> = Vec::new();
    for path in &pinned_paths {
        if let Some(project) = ordered
            .iter()
            .find(|project| same_project_path(&project.path, path))
        {
            out.push(card(project, true));
        }
    }
    for project in &ordered {
        if !pinned_paths
            .iter()
            .any(|path| same_project_path(path, &project.path))
        {
            out.push(card(project, false));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_and_roots_are_not_projects() {
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("C:"));
        assert!(looks_like_project("/repo/app"));
        assert!(!looks_like_project("/Applications/Foo.app/Contents"));
    }

    #[test]
    fn pinned_projects_lead_and_newcomers_follow_the_saved_order() {
        let kv = Kv::in_memory();
        kv.set_item(
            RECENTS_KEY,
            r#"[{"path":"/a/one","openedAt":1},{"path":"/a/two","openedAt":3},{"path":"/a/three","openedAt":2}]"#,
        );
        kv.set_item(RAIL_ORDER_KEY, r#"["/a/one"]"#);
        kv.set_item(RAIL_PINNED_KEY, r#"["/a/three"]"#);
        let names: Vec<String> = rail_projects(&kv, "")
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(names, ["three", "one", "two"]);
        assert_eq!(last_project_path(&kv).as_deref(), Some("/a/one"));
    }
}
