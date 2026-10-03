//! Port of the parts of src/features/projects/model/recents.ts that
//! `inboxProjectsForRail` needs: `RecentProject`, `looksLikeProject`, and
//! `collectRailProjects`.
// TODO(port): the projects package owns recents.ts. Switch to its functions
// once it publishes them.

use monocode_core::paths::{path_key, slash};
use monocode_layout::paths::pretty_cwd;

use crate::runtime::util::project_path::{normalize_project_path, same_project_path};

/// `RecentProject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    pub path: String,
    pub opened_at: i64,
}

/// `looksLikeProject`: a folder worth treating as a project. Home, roots,
/// and app bundles are not.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    let bytes = normalized.as_bytes();
    if (bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':') || normalized == "/"
    {
        return false;
    }
    // Home itself arrives expanded (`/Users/me`), so the `~` check above
    // misses it.
    if pretty_cwd(path) == "~" {
        return false;
    }
    if path.contains(".app/") || path.contains(".app\\") {
        return false;
    }
    true
}

/// `collectRailProjects`: recents plus the current folder, keyed by
/// `pathKey`, in insertion order.
pub fn collect_rail_projects(
    recents: &[RecentProject],
    current_cwd: &str,
    now: i64,
) -> Vec<(String, RecentProject)> {
    let mut map: Vec<(String, RecentProject)> = Vec::new();
    let mut set = |key: String, project: RecentProject| match map
        .iter_mut()
        .find(|(existing, _)| *existing == key)
    {
        Some(entry) => entry.1 = project,
        None => map.push((key, project)),
    };
    for item in recents {
        if !looks_like_project(&item.path) {
            continue;
        }
        let path = normalize_project_path(&item.path);
        set(
            path_key(&path),
            RecentProject {
                path,
                opened_at: item.opened_at,
            },
        );
    }
    if !current_cwd.is_empty() && looks_like_project(current_cwd) {
        let path = normalize_project_path(current_cwd);
        let key = path_key(&path);
        if !map.iter().any(|(existing, _)| *existing == key) {
            map.push((
                key,
                RecentProject {
                    path,
                    opened_at: now,
                },
            ));
        }
    }
    map
}

/// `inboxProjectsForRail`: the rail's projects with the current one first.
pub fn inbox_projects_for_rail(
    recents: &[RecentProject],
    cwd: &str,
    now: i64,
) -> Vec<RecentProject> {
    let map = collect_rail_projects(recents, cwd, now);
    // TODO(port): the TypeScript looks the current folder up by its
    // normalized path, but the map is keyed by `pathKey`. On a
    // case-insensitive key the lookup misses and the order is unchanged.
    let current = if cwd.is_empty() {
        None
    } else {
        let wanted = normalize_project_path(cwd);
        map.iter()
            .find(|(key, _)| *key == wanted)
            .map(|(_, project)| project.clone())
    };
    let rest: Vec<RecentProject> = map
        .into_iter()
        .map(|(_, project)| project)
        .filter(|project| {
            current
                .as_ref()
                .is_none_or(|current| !same_project_path(&project.path, &current.path))
        })
        .collect();
    match current {
        Some(current) => std::iter::once(current).chain(rest).collect(),
        None => rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_the_current_project_first() {
        let recents = [
            RecentProject {
                path: "/tmp/docs".into(),
                opened_at: 1,
            },
            RecentProject {
                path: "/tmp/web".into(),
                opened_at: 2,
            },
        ];
        let paths: Vec<String> = inbox_projects_for_rail(&recents, "/tmp/web", 0)
            .into_iter()
            .map(|project| project.path)
            .collect();
        assert_eq!(paths, ["/tmp/web", "/tmp/docs"]);
    }

    #[test]
    fn skips_roots_and_adds_the_current_folder() {
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("C:"));
        assert!(!looks_like_project("/Applications/Foo.app/Contents"));
        let paths: Vec<String> = inbox_projects_for_rail(&[], "/tmp/new/", 7)
            .into_iter()
            .map(|project| project.path)
            .collect();
        assert_eq!(paths, ["/tmp/new"]);
    }
}
