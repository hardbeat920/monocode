//! Path helpers the file views call. `basename`, `displayPath`,
//! `parentPath`, and `joinPath` come from monocode-core. `rebasePath` from
//! src/shared/lib/paths.ts and `looksLikeProject` from
//! src/features/projects/model/recents.ts are ported here, because their
//! other copies live in the engine.

pub use monocode_core::paths::{basename, display_path, path_key, slash};
pub use monocode_core::transcript::paths::{join_path, parent_path};
pub use monocode_layout::paths::{REMOTE_PROJECT_PREFIX, is_remote_project_path};

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

/// `/^[A-Za-z]:$/`.
fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `rebasePath`: `path` moved from under `from` to under `to`.
pub fn rebase_path(path: &str, from: &str, to: &str) -> String {
    let normalized = trim_slash(path);
    let source = trim_slash(from);
    let dest = trim_slash(to);
    let key = path_key(&normalized);
    let source_key = path_key(&source);
    if key == source_key {
        return if is_drive(&dest) {
            format!("{dest}/")
        } else {
            dest
        };
    }
    if key.starts_with(&format!("{source_key}/")) {
        return format!("{dest}{}", &normalized[source.len()..]);
    }
    slash(path)
}

/// `looksLikeProject`: a folder worth indexing.
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
    // Home itself arrives expanded (`/Users/me`), so the `~` check above
    // misses it. Indexing it walks `~/Library`, which trips the OS consent
    // prompt.
    if monocode_layout::paths::pretty_cwd(path) == "~" {
        return false;
    }
    !(path.contains(".app/") || path.contains(".app\\"))
}

/// `normalizeEditorPath`.
pub fn normalize_editor_path(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        path.to_string()
    } else {
        trimmed.to_string()
    }
}

/// `editorPathsEqual`.
pub fn editor_paths_equal(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

/// `true` when `path` is `root` or inside it (`path === root ||
/// path.startsWith(`${root}/`)`).
pub fn is_same_or_inside(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebases_paths_under_a_moved_folder() {
        assert_eq!(rebase_path("/p/a/b.ts", "/p/a", "/p/c"), "/p/c/b.ts");
        assert_eq!(rebase_path("/p/a", "/p/a", "/p/c"), "/p/c");
        assert_eq!(rebase_path("/p/ab", "/p/a", "/p/c"), "/p/ab");
    }

    #[test]
    fn skips_home_and_roots_as_projects() {
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("/Applications/Foo.app/Contents"));
        assert!(looks_like_project("/Users/me/repo"));
        assert!(looks_like_project("remote://env/home/me/repo"));
    }

    #[test]
    fn checks_containment_by_segment() {
        assert!(is_same_or_inside("/p/a", "/p/a"));
        assert!(is_same_or_inside("/p/a/b", "/p/a"));
        assert!(!is_same_or_inside("/p/ab", "/p/a"));
    }
}
