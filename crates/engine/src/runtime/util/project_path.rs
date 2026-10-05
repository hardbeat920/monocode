//! Port of the project path helpers in src/features/projects/model/recents.ts
//! that persistence and history need: `normalizeProjectPath`,
//! `sameProjectPath`, and `isRemoteProjectPath`. The `projects` package owns
//! the rest of recents.ts.

use monocode_core::paths::{path_key, slash};

/// `REMOTE_PROJECT_PREFIX`: paths on a connected machine use this scheme.
pub const REMOTE_PROJECT_PREFIX: &str = "remote://";

/// `normalizeProjectPath`: forward slashes, no trailing slash.
pub fn normalize_project_path(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

/// `sameProjectPath`.
pub fn same_project_path(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

/// `isRemoteProjectPath`.
pub fn is_remote_project_path(path: &str) -> bool {
    slash(path).starts_with(REMOTE_PROJECT_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_compares_project_paths() {
        assert_eq!(normalize_project_path("/repo/"), "/repo");
        assert_eq!(normalize_project_path("///"), "/");
        assert_eq!(normalize_project_path("C:\\repo\\"), "C:/repo");
        assert!(same_project_path("C:\\Repo", "c:/repo/"));
        assert!(!same_project_path("/Repo", "/repo"));
        assert!(is_remote_project_path("remote://env/home/me"));
        assert!(!is_remote_project_path("/home/me"));
    }
}
