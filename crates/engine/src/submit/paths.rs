//! Port of the path helpers the submit pipeline needs from
//! src/shared/lib/paths.ts (`isEqualOrInside`, `joinPath`, `parentPath`) and
//! src/features/projects/model/recents.ts (`looksLikeProject`,
//! `isLocalProject`).

use monocode_core::paths::{path_key, slash};
use monocode_layout::paths::pretty_cwd;

use crate::runtime::util::project_path::is_remote_project_path;

fn windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || path.starts_with("\\\\")
        || path.starts_with("//")
}

fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `trimSlash`.
fn trim_slash(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

/// `parentPath`.
pub fn parent_path(path: &str) -> String {
    let trimmed = trim_slash(path);
    // `//server/share` stays as it is.
    if let Some(rest) = trimmed.strip_prefix("//") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|part| !part.is_empty()) {
            return trimmed;
        }
    }
    if is_drive(&trimmed) {
        return format!("{trimmed}/");
    }
    let Some(index) = trimmed.rfind('/').filter(|index| *index > 0) else {
        return "/".into();
    };
    let parent = &trimmed[..index];
    if is_drive(parent) {
        return format!("{parent}/");
    }
    parent.to_string()
}

/// `isEqualOrInside`.
pub fn is_equal_or_inside(path: &str, root: &str) -> bool {
    let key = path_key(&trim_slash(path));
    let base = path_key(&trim_slash(root));
    key == base || key.starts_with(&format!("{base}/"))
}

/// `joinPath`.
pub fn join_path(parent: &str, relative: &str) -> String {
    let base = trim_slash(parent);
    let windows = windows_path(parent);
    let parts = relative
        .split(|c| c == '/' || (windows && c == '\\'))
        .filter(|part| !part.is_empty() && *part != ".");
    let mut out = base;
    for part in parts {
        if part == ".." {
            out = parent_path(&out);
            continue;
        }
        out = if out == "/" {
            format!("/{part}")
        } else {
            format!("{out}/{part}")
        };
    }
    out
}

/// `looksLikeProject`: not empty, a root, a drive, home itself, or inside an
/// app bundle.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let normalized = trim_slash(path);
    if is_drive(&normalized) || normalized == "/" {
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

/// `isLocalProject`.
pub fn is_local_project(path: &str) -> bool {
    looks_like_project(path) && !is_remote_project_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_compares_paths() {
        assert_eq!(
            join_path("/repo/", ".agents/skills/x/SKILL.md"),
            "/repo/.agents/skills/x/SKILL.md"
        );
        assert_eq!(join_path("/repo", "../other"), "/other");
        assert_eq!(join_path("/", "a"), "/a");
        assert_eq!(join_path("C:\\repo", "a\\b"), "C:/repo/a/b");
        assert_eq!(parent_path("C:/repo"), "C:/");
        assert!(is_equal_or_inside("/repo/.worktrees/a", "/repo"));
        assert!(is_equal_or_inside("/repo/", "/repo"));
        assert!(!is_equal_or_inside("/repository", "/repo"));
    }

    #[test]
    fn recognizes_projects() {
        assert!(looks_like_project("/Users/me/code/app"));
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("C:/"));
        assert!(!looks_like_project("/Applications/X.app/Contents"));
        assert!(!is_local_project("remote://env/home/me/app"));
    }
}
