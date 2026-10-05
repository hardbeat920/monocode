//! Port of src/features/projects/model/recents.ts: recent projects, the
//! project rail order, pins, and the archive.
//!
//! Storage goes through `Kv` with the same keys and JSON values. The window
//! events the TypeScript dispatched (`monocode:project-paths-changed`,
//! `monocode:archived-projects-changed`) become `ProjectsEvent`s that the
//! `Projects` entity emits after it calls these functions.

use std::collections::HashSet;

use monocode_core::paths::{path_key, slash};
use monocode_layout::paths::pretty_cwd;
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::runtime::util::project_path::{
    REMOTE_PROJECT_PREFIX, is_remote_project_path, normalize_project_path, same_project_path,
};

use super::js_object::{Parsed, finite_i64, parse};
use super::now_ms;

pub const KEY: &str = "monocode.recentProjects";
pub const RAIL_ORDER_KEY: &str = "monocode.projectRailOrder";
pub const RAIL_PINNED_KEY: &str = "monocode.projectRailPinned";
pub const ARCHIVED_KEY: &str = "monocode.archivedProjects";
/// How many recent projects the rail remembers.
pub const MAX: usize = 20;

/// `RecentProject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: String,
    pub opened_at: i64,
}

impl RecentProject {
    pub fn new(path: impl Into<String>, opened_at: i64) -> Self {
        Self {
            path: path.into(),
            opened_at,
        }
    }
}

/// `ArchivedProject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedProject {
    pub path: String,
    pub archived_at: i64,
}

/// The stored array, or nothing when the value is missing or is not an array.
fn read_array(kv: &Kv, key: &str) -> Vec<Value> {
    let Some(raw) = kv.get_item(key).filter(|raw| !raw.is_empty()) else {
        return Vec::new();
    };
    match parse(&raw) {
        Some(Parsed::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `loadRecents`.
pub fn load_recents(kv: &Kv) -> Vec<RecentProject> {
    let mut out = Vec::new();
    for item in read_array(kv, KEY) {
        let Some(record) = item.as_object() else {
            continue;
        };
        let Some(path) = record
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        let opened_at = finite_i64(record.get("openedAt")).unwrap_or(0);
        out.push(RecentProject::new(normalize_project_path(path), opened_at));
    }
    out
}

/// `save`.
fn save(kv: &Kv, next: &[RecentProject]) {
    if let Ok(json) = serde_json::to_string(next) {
        kv.set_item(KEY, &json);
    }
}

/// `rememberProject`: move the project to the front of the recents and take
/// it out of the archive.
pub fn remember_project(kv: &Kv, path: &str) -> Vec<RecentProject> {
    let normalized = normalize_project_path(path);
    if normalized == "~" {
        return load_recents(kv);
    }
    drop_archived(kv, &normalized);
    let prev = load_recents(kv)
        .into_iter()
        .filter(|project| !same_project_path(&project.path, &normalized));
    let next: Vec<RecentProject> =
        std::iter::once(RecentProject::new(normalized.clone(), now_ms()))
            .chain(prev)
            .take(MAX)
            .collect();
    save(kv, &next);
    next
}

/// `replacePath`: rename one entry and drop the duplicates that makes.
fn replace_path(paths: &[String], from: &str, to: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let next = if same_project_path(path, from) {
            to.to_string()
        } else {
            path.clone()
        };
        if seen.insert(path_key(&next)) {
            out.push(next);
        }
    }
    out
}

/// `replaceProjectPath`: replace a renamed project's path everywhere the
/// project rail stores it. The caller emits the paths-changed event.
pub fn replace_project_path(kv: &Kv, from: &str, to: &str) -> Vec<RecentProject> {
    let previous = normalize_project_path(from);
    let next_path = normalize_project_path(to);
    if same_project_path(&previous, &next_path) {
        return load_recents(kv);
    }

    let mut recents = Vec::new();
    let mut seen = HashSet::new();
    for item in load_recents(kv) {
        let path = if same_project_path(&item.path, &previous) {
            next_path.clone()
        } else {
            item.path.clone()
        };
        if !seen.insert(path_key(&path)) {
            continue;
        }
        recents.push(RecentProject { path, ..item });
    }
    save(kv, &recents);
    save_project_rail_order(
        kv,
        &replace_path(&load_project_rail_order(kv), &previous, &next_path),
    );
    save_pinned_projects(
        kv,
        &replace_path(&load_pinned_projects(kv), &previous, &next_path),
    );

    let archived = load_archived_projects(kv);
    if archived
        .iter()
        .any(|item| same_project_path(&item.path, &previous))
    {
        let mut replaced = Vec::new();
        let mut archived_seen = HashSet::new();
        for item in archived {
            let path = if same_project_path(&item.path, &previous) {
                next_path.clone()
            } else {
                item.path.clone()
            };
            if !archived_seen.insert(path_key(&path)) {
                continue;
            }
            replaced.push(ArchivedProject { path, ..item });
        }
        save_archived(kv, &replaced);
    }
    recents
}

/// `dropFromRail`: drop a project's recent entry, saved order slot, and pin.
fn drop_from_rail(kv: &Kv, path: &str) -> Vec<RecentProject> {
    let normalized = normalize_project_path(path);
    let next: Vec<RecentProject> = load_recents(kv)
        .into_iter()
        .filter(|item| !same_project_path(&item.path, &normalized))
        .collect();
    save(kv, &next);
    let order: Vec<String> = load_project_rail_order(kv)
        .into_iter()
        .filter(|entry| !same_project_path(entry, &normalized))
        .collect();
    save_project_rail_order(kv, &order);
    let pinned: Vec<String> = load_pinned_projects(kv)
        .into_iter()
        .filter(|entry| !same_project_path(entry, &normalized))
        .collect();
    save_pinned_projects(kv, &pinned);
    next
}

/// `forgetProject`: remove a project from the rail and from the archive
/// (Delete).
pub fn forget_project(kv: &Kv, path: &str) -> Vec<RecentProject> {
    drop_archived(kv, path);
    drop_from_rail(kv, path)
}

/// `archiveProject`: remove a project from the rail and file it in the
/// archive (Archive).
pub fn archive_project(kv: &Kv, path: &str) -> Vec<RecentProject> {
    let normalized = normalize_project_path(path);
    if !looks_like_project(&normalized) {
        return load_recents(kv);
    }
    let recents = drop_from_rail(kv, &normalized);
    let rest = load_archived_projects(kv)
        .into_iter()
        .filter(|item| !same_project_path(&item.path, &normalized));
    let next: Vec<ArchivedProject> = std::iter::once(ArchivedProject {
        path: normalized.clone(),
        archived_at: now_ms(),
    })
    .chain(rest)
    .collect();
    save_archived(kv, &next);
    recents
}

/// `loadArchivedProjects`.
pub fn load_archived_projects(kv: &Kv) -> Vec<ArchivedProject> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in read_array(kv, ARCHIVED_KEY) {
        let Some(record) = item.as_object() else {
            continue;
        };
        let Some(path) = record
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        let path = normalize_project_path(path);
        let key = path_key(&path);
        if seen.contains(&key) || !looks_like_project(&path) {
            continue;
        }
        seen.insert(key);
        let archived_at = finite_i64(record.get("archivedAt")).unwrap_or(0);
        out.push(ArchivedProject { path, archived_at });
    }
    out
}

/// `saveArchived`. The caller emits the archived-changed event.
fn save_archived(kv: &Kv, next: &[ArchivedProject]) {
    if let Ok(json) = serde_json::to_string(next) {
        kv.set_item(ARCHIVED_KEY, &json);
    }
}

/// `dropArchived`.
fn drop_archived(kv: &Kv, path: &str) {
    let normalized = normalize_project_path(path);
    let prev = load_archived_projects(kv);
    let before = prev.len();
    let next: Vec<ArchivedProject> = prev
        .into_iter()
        .filter(|item| !same_project_path(&item.path, &normalized))
        .collect();
    if next.len() == before {
        return;
    }
    save_archived(kv, &next);
}

/// `lastProjectPath`: the most recently opened project, used to restore the
/// folder on launch.
pub fn last_project_path(kv: &Kv) -> Option<String> {
    load_recents(kv)
        .into_iter()
        .find(|item| looks_like_project(&item.path))
        .map(|item| item.path)
}

/// `ProjectRailSections`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectRailSections {
    pub pinned: Vec<RecentProject>,
    pub projects: Vec<RecentProject>,
}

/// `readPathList`.
fn read_path_list(kv: &Kv, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in read_array(kv, key) {
        let Some(item) = item.as_str().filter(|item| !item.is_empty()) else {
            continue;
        };
        let path = normalize_project_path(item);
        if !seen.insert(path_key(&path)) {
            continue;
        }
        out.push(path);
    }
    out
}

/// `savePathList`.
fn save_path_list(kv: &Kv, key: &str, paths: &[String]) {
    if let Ok(json) = serde_json::to_string(paths) {
        kv.set_item(key, &json);
    }
}

/// `loadProjectRailOrder`.
pub fn load_project_rail_order(kv: &Kv) -> Vec<String> {
    read_path_list(kv, RAIL_ORDER_KEY)
}

/// `saveProjectRailOrder`.
pub fn save_project_rail_order(kv: &Kv, order: &[String]) {
    let order: Vec<String> = order
        .iter()
        .map(|path| normalize_project_path(path))
        .collect();
    save_path_list(kv, RAIL_ORDER_KEY, &order);
}

/// `loadPinnedProjects`.
pub fn load_pinned_projects(kv: &Kv) -> Vec<String> {
    read_path_list(kv, RAIL_PINNED_KEY)
}

/// `savePinnedProjects`. The caller emits the paths-changed event.
pub fn save_pinned_projects(kv: &Kv, pinned: &[String]) {
    let pinned: Vec<String> = pinned
        .iter()
        .map(|path| normalize_project_path(path))
        .collect();
    save_path_list(kv, RAIL_PINNED_KEY, &pinned);
}

/// `toggleProjectPin`.
pub fn toggle_project_pin(kv: &Kv, path: &str) {
    let pinned = load_pinned_projects(kv);
    let next: Vec<String> = if pinned.iter().any(|item| same_project_path(item, path)) {
        pinned
            .into_iter()
            .filter(|item| !same_project_path(item, path))
            .collect()
    } else {
        let mut next = pinned;
        next.push(path.to_string());
        next
    };
    save_pinned_projects(kv, &next);
}

/// `knownProjectPaths`: every project path still remembered: rail, pins,
/// saved order, and archive.
pub fn known_project_paths(kv: &Kv) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let paths = load_recents(kv)
        .into_iter()
        .map(|item| item.path)
        .chain(load_project_rail_order(kv))
        .chain(load_pinned_projects(kv))
        .chain(load_archived_projects(kv).into_iter().map(|item| item.path));
    for path in paths {
        if !seen.insert(path_key(&path)) {
            continue;
        }
        out.push(normalize_project_path(&path));
    }
    out
}

/// The `Map<string, RecentProject>` `collectRailProjects` returns: keyed by
/// path key, in insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RailProjects {
    entries: Vec<(String, RecentProject)>,
}

impl RailProjects {
    pub fn new() -> Self {
        Self::default()
    }

    /// `map.get(key)`.
    pub fn get(&self, key: &str) -> Option<&RecentProject> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, project)| project)
    }

    /// `map.has(key)`.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `map.set(key, project)`: an existing key keeps its place.
    pub fn set(&mut self, key: impl Into<String>, project: RecentProject) {
        let key = key.into();
        match self.entries.iter_mut().find(|(entry, _)| *entry == key) {
            Some(entry) => entry.1 = project,
            None => self.entries.push((key, project)),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `map.entries()`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &RecentProject)> {
        self.entries
            .iter()
            .map(|(key, project)| (key.as_str(), project))
    }
}

impl<K: Into<String>> FromIterator<(K, RecentProject)> for RailProjects {
    fn from_iter<I: IntoIterator<Item = (K, RecentProject)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (key, project) in iter {
            map.set(key, project);
        }
        map
    }
}

/// `collectRailProjects`: every project for the rail, keyed by path key.
pub fn collect_rail_projects(recents: &[RecentProject], current_cwd: &str) -> RailProjects {
    let mut map = RailProjects::new();
    for item in recents {
        if !looks_like_project(&item.path) {
            continue;
        }
        let path = normalize_project_path(&item.path);
        map.set(path_key(&path), RecentProject::new(path, item.opened_at));
    }
    if !current_cwd.is_empty() && looks_like_project(current_cwd) {
        let path = normalize_project_path(current_cwd);
        let key = path_key(&path);
        if !map.has(&key) {
            map.set(key, RecentProject::new(path, now_ms()));
        }
    }
    map
}

/// `syncProjectRailOrder`: append new projects to the saved order without
/// moving existing entries.
pub fn sync_project_rail_order(order: &[String], projects: &RailProjects) -> Vec<String> {
    let mut next = Vec::new();
    let mut seen = HashSet::new();
    for path in order {
        let key = path_key(path);
        let Some(project) = projects.get(&key) else {
            continue;
        };
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        next.push(project.path.clone());
    }
    let mut newcomers: Vec<&RecentProject> = projects
        .iter()
        .filter(|(key, _)| !seen.contains(*key))
        .map(|(_, project)| project)
        .collect();
    newcomers.sort_by_key(|project| std::cmp::Reverse(project.opened_at));
    next.extend(newcomers.into_iter().map(|project| project.path.clone()));
    next
}

/// `projectRailSections`.
pub fn project_rail_sections(
    recents: &[RecentProject],
    current_cwd: &str,
    order: &[String],
    pinned_paths: &[String],
) -> ProjectRailSections {
    let projects = collect_rail_projects(recents, current_cwd);
    let synced_order = sync_project_rail_order(order, &projects);
    let pinned_set: HashSet<String> = pinned_paths.iter().map(|path| path_key(path)).collect();
    let mut sections = ProjectRailSections::default();
    for path in synced_order {
        let key = path_key(&path);
        let Some(item) = projects.get(&key) else {
            continue;
        };
        if pinned_set.contains(&key) {
            sections.pinned.push(item.clone());
        } else {
            sections.projects.push(item.clone());
        }
    }
    sections
}

/// `projectRailItems`: recents plus the current folder when it is a project
/// not yet remembered, pinned first.
pub fn project_rail_items(
    kv: &Kv,
    recents: &[RecentProject],
    current_cwd: &str,
) -> Vec<RecentProject> {
    let projects = collect_rail_projects(recents, current_cwd);
    let order = sync_project_rail_order(&load_project_rail_order(kv), &projects);
    let sections = project_rail_sections(recents, current_cwd, &order, &load_pinned_projects(kv));
    sections
        .pinned
        .into_iter()
        .chain(sections.projects)
        .collect()
}

/// `isLocalProject`: a project folder on this computer, safe to index,
/// search, or run git and terminals in. Use `looks_like_project` where a
/// remote project also counts.
pub fn is_local_project(path: &str) -> bool {
    looks_like_project(path) && !is_remote_project_path(path)
}

/// `/^[A-Za-z]:$/`.
fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `looksLikeProject`: a user project, not an app bundle or system root.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    if is_drive(normalized) || normalized == "/" {
        return false;
    }
    // Home itself arrives expanded (`/Users/me`), so the `~` check above misses
    // it. Indexing it walks `~/Library`, which trips the OS consent prompt.
    if pretty_cwd(path) == "~" {
        return false;
    }
    if path.contains(".app/") || path.contains(".app\\") {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[RecentProject]) -> Vec<&str> {
        items.iter().map(|item| item.path.as_str()).collect()
    }

    // looksLikeProject

    #[test]
    fn rejects_the_home_directory_so_it_is_never_indexed() {
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("/Users/me/"));
        assert!(!looks_like_project("/home/me"));
        assert!(!looks_like_project("C:/Users/me"));
        assert!(!looks_like_project("C:\\Users\\me"));
        assert!(!looks_like_project("~"));
    }

    #[test]
    fn rejects_system_roots_and_app_bundles() {
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project(""));
        assert!(!looks_like_project("C:/"));
        assert!(!looks_like_project("C:"));
        assert!(!looks_like_project("/Applications/Some.app/Contents"));
    }

    #[test]
    fn accepts_real_projects_including_ones_directly_under_home() {
        assert!(looks_like_project("/Users/me/code/app"));
        assert!(looks_like_project("/Users/me/Desktop"));
        assert!(looks_like_project("/tmp/scratch"));
        assert!(looks_like_project("C:/Users/me/code/app"));
    }

    #[test]
    fn local_projects_exclude_remote_ones() {
        assert!(is_local_project("/tmp/scratch"));
        assert!(!is_local_project("remote://env/home/me/app"));
        assert!(looks_like_project("remote://env/home/me/app"));
    }

    // projectRailSections

    #[test]
    fn keeps_saved_order_and_does_not_move_the_current_project_first() {
        let recents = [
            RecentProject::new("/tmp/older", 1),
            RecentProject::new("/tmp/current", 2),
        ];
        let sections = project_rail_sections(
            &recents,
            "/tmp/current/",
            &["/tmp/older".into(), "/tmp/current".into()],
            &[],
        );
        let all: Vec<RecentProject> = sections
            .pinned
            .into_iter()
            .chain(sections.projects)
            .collect();
        assert_eq!(paths(&all), ["/tmp/older", "/tmp/current"]);
    }

    #[test]
    fn places_pinned_projects_before_unpinned_ones() {
        let recents = [
            RecentProject::new("/tmp/a", 1),
            RecentProject::new("/tmp/b", 2),
            RecentProject::new("/tmp/c", 3),
        ];
        let sections = project_rail_sections(
            &recents,
            "/tmp/a",
            &["/tmp/a".into(), "/tmp/b".into(), "/tmp/c".into()],
            &["/tmp/b".into()],
        );
        assert_eq!(paths(&sections.pinned), ["/tmp/b"]);
        assert_eq!(paths(&sections.projects), ["/tmp/a", "/tmp/c"]);
    }

    #[test]
    fn appends_new_projects_without_reordering_existing_entries() {
        let projects: RailProjects = [
            ("/tmp/older", RecentProject::new("/tmp/older", 1)),
            ("/tmp/new", RecentProject::new("/tmp/new", 3)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            sync_project_rail_order(&["/tmp/older".into()], &projects),
            ["/tmp/older", "/tmp/new"]
        );
    }

    #[test]
    fn newcomers_sort_newest_first() {
        let projects: RailProjects = [
            ("/tmp/a", RecentProject::new("/tmp/a", 1)),
            ("/tmp/b", RecentProject::new("/tmp/b", 5)),
            ("/tmp/c", RecentProject::new("/tmp/c", 3)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            sync_project_rail_order(&[], &projects),
            ["/tmp/b", "/tmp/c", "/tmp/a"]
        );
    }

    // projectRailItems

    #[test]
    fn rail_items_ignore_home_as_a_current_folder() {
        let kv = Kv::in_memory();
        let items = project_rail_items(&kv, &[RecentProject::new("/tmp/app", 1)], "/Users/me");
        assert_eq!(paths(&items), ["/tmp/app"]);
    }

    // forgetProject

    #[test]
    fn forget_drops_the_recent_entry_rail_order_slot_and_pin() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/tmp/keep");
        remember_project(&kv, "/tmp/gone");
        save_project_rail_order(&kv, &["/tmp/keep".into(), "/tmp/gone".into()]);
        save_pinned_projects(&kv, &["/tmp/gone".into()]);

        assert_eq!(paths(&forget_project(&kv, "/tmp/gone")), ["/tmp/keep"]);
        assert_eq!(paths(&load_recents(&kv)), ["/tmp/keep"]);
        assert_eq!(load_project_rail_order(&kv), ["/tmp/keep"]);
        assert!(load_pinned_projects(&kv).is_empty());
    }

    #[test]
    fn treats_differently_cased_windows_paths_as_one_project() {
        let kv = Kv::in_memory();
        remember_project(&kv, "C:/Users/me/Code/App");
        remember_project(&kv, "c:/users/ME/code/app");
        assert_eq!(paths(&load_recents(&kv)), ["c:/users/ME/code/app"]);
    }

    #[test]
    fn remember_caps_the_list_and_ignores_home() {
        let kv = Kv::in_memory();
        for index in 0..25 {
            remember_project(&kv, &format!("/tmp/p{index}"));
        }
        let recents = load_recents(&kv);
        assert_eq!(recents.len(), MAX);
        assert_eq!(recents[0].path, "/tmp/p24");
        assert_eq!(remember_project(&kv, "~").len(), MAX);
        assert_eq!(last_project_path(&kv).as_deref(), Some("/tmp/p24"));
    }

    // replaceProjectPath

    #[test]
    fn keeps_rail_order_and_pin_state_under_the_renamed_path() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/work/other");
        remember_project(&kv, "/work/monocode");
        save_project_rail_order(&kv, &["/work/other".into(), "/work/monocode".into()]);
        save_pinned_projects(&kv, &["/work/monocode".into()]);

        assert_eq!(
            paths(&replace_project_path(
                &kv,
                "/work/monocode",
                "/work/monocode-personal"
            )),
            ["/work/monocode-personal", "/work/other"]
        );
        assert_eq!(
            load_project_rail_order(&kv),
            ["/work/other", "/work/monocode-personal"]
        );
        assert_eq!(load_pinned_projects(&kv), ["/work/monocode-personal"]);
    }

    // archiveProject

    #[test]
    fn files_the_project_in_the_archive_and_takes_it_off_the_rail() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/tmp/keep");
        remember_project(&kv, "/tmp/gone");
        save_pinned_projects(&kv, &["/tmp/gone".into()]);

        assert_eq!(paths(&archive_project(&kv, "/tmp/gone")), ["/tmp/keep"]);
        let archived: Vec<String> = load_archived_projects(&kv)
            .into_iter()
            .map(|item| item.path)
            .collect();
        assert_eq!(archived, ["/tmp/gone"]);
        assert!(load_pinned_projects(&kv).is_empty());
        assert_eq!(paths(&load_recents(&kv)), ["/tmp/keep"]);
    }

    #[test]
    fn opening_a_project_again_restores_it_from_the_archive() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/tmp/gone");
        archive_project(&kv, "/tmp/gone");
        assert_eq!(load_archived_projects(&kv).len(), 1);

        remember_project(&kv, "/tmp/gone");
        assert!(load_archived_projects(&kv).is_empty());
        assert_eq!(paths(&load_recents(&kv)), ["/tmp/gone"]);
    }

    #[test]
    fn delete_drops_an_archived_project_instead_of_restoring_it() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/tmp/gone");
        archive_project(&kv, "/tmp/gone");
        forget_project(&kv, "/tmp/gone");
        assert!(load_archived_projects(&kv).is_empty());
        assert!(load_recents(&kv).is_empty());
    }

    #[test]
    fn known_paths_cover_every_list_once() {
        let kv = Kv::in_memory();
        remember_project(&kv, "/tmp/a");
        save_project_rail_order(&kv, &["/tmp/a".into(), "/tmp/b".into()]);
        save_pinned_projects(&kv, &["/tmp/c/".into()]);
        remember_project(&kv, "/tmp/d");
        archive_project(&kv, "/tmp/d");
        assert_eq!(
            known_project_paths(&kv),
            ["/tmp/a", "/tmp/b", "/tmp/c", "/tmp/d"]
        );
    }

    #[test]
    fn malformed_storage_reads_as_empty() {
        let kv = Kv::in_memory();
        kv.set_item(KEY, "{not json");
        kv.set_item(RAIL_ORDER_KEY, r#"{"a":1}"#);
        kv.set_item(
            ARCHIVED_KEY,
            r#"[{"path":"/"},{"path":"/tmp/x","archivedAt":"no"}]"#,
        );
        assert!(load_recents(&kv).is_empty());
        assert!(load_project_rail_order(&kv).is_empty());
        assert_eq!(
            load_archived_projects(&kv),
            [ArchivedProject {
                path: "/tmp/x".into(),
                archived_at: 0
            }]
        );
    }

    #[test]
    fn toggle_pin_adds_and_removes() {
        let kv = Kv::in_memory();
        toggle_project_pin(&kv, "/tmp/a");
        assert_eq!(load_pinned_projects(&kv), ["/tmp/a"]);
        toggle_project_pin(&kv, "/tmp/a/");
        assert!(load_pinned_projects(&kv).is_empty());
    }
}
