//! Port of `looksLikeProject` and `isLocalProject` from
//! src/features/projects/model/recents.ts, which notes, search, and history
//! need. The projects package owns the rest of recents.ts.

use monocode_core::paths::slash;
use monocode_layout::paths::pretty_cwd;

use crate::runtime::util::project_path::is_remote_project_path;

/// `looksLikeProject`: not empty, a root, a drive, home itself, or inside an
/// app bundle.
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
    // misses it. Indexing it walks `~/Library`, which trips the OS consent
    // prompt.
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
    fn rejects_roots_drives_home_and_bundles() {
        for path in [
            "",
            "/",
            "~",
            "///",
            "C:",
            "C:\\",
            "/Applications/X.app/Contents",
        ] {
            assert!(!looks_like_project(path), "{path}");
        }
        assert!(looks_like_project("/Users/me/code/agent-terminal"));
        assert!(is_local_project("/tmp/project"));
        assert!(!is_local_project("remote://env/home/me/repo"));
    }
}
