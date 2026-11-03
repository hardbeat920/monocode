//! Display-path helpers ported from paths.ts and fileTree.ts. Paths use `/`;
//! Windows drive and UNC paths compare without case.
use super::name::path_segments;

fn is_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn starts_with_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn windows_path(path: &str) -> bool {
    (starts_with_drive(path) && matches!(path.as_bytes().get(2), Some(b'\\' | b'/')))
        || path.starts_with("\\\\")
        || path.starts_with("//")
}

fn slash(path: &str) -> String {
    if windows_path(path) || (cfg!(windows) && !path.starts_with('/')) {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

fn trim_slash(path: &str) -> String {
    let path = slash(path);
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.into()
    }
}

/// Stable comparison key for Windows paths without changing their display case.
pub fn path_key(path: &str) -> String {
    if let Some(remote) = crate::remote::parse_remote_path(path) {
        return crate::remote::remote_path(&remote.environment_id, &path_key(&remote.host_path));
    }
    let normalized = trim_slash(path);
    let drive =
        starts_with_drive(&normalized) && matches!(normalized.as_bytes().get(2), None | Some(b'/'));
    if drive || normalized.starts_with("//") {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

/// Text after the first `chars` characters.
fn skip_chars(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars)
        .map_or("", |(index, _)| &text[index..])
}

pub fn parent_path(path: &str) -> String {
    if let Some(remote) = crate::remote::parse_remote_path(path) {
        return crate::remote::remote_path(&remote.environment_id, &parent_path(&remote.host_path));
    }
    let trimmed = trim_slash(path);
    if let Some(rest) = trimmed.strip_prefix("//") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|part| !part.is_empty()) {
            return trimmed;
        }
    }
    if is_drive(&trimmed) {
        return format!("{trimmed}/");
    }
    let Some(i) = trimmed.rfind('/').filter(|i| *i > 0) else {
        return "/".into();
    };
    let parent = &trimmed[..i];
    if is_drive(parent) {
        return format!("{parent}/");
    }
    parent.to_string()
}

/// `path` with its `from` prefix replaced by `to`; unrelated paths are unchanged.
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
        return format!("{dest}{}", skip_chars(&normalized, source.chars().count()));
    }
    slash(path)
}

pub fn is_equal_or_inside(path: &str, root: &str) -> bool {
    let key = path_key(path);
    let base_key = path_key(root);
    key == base_key || key.starts_with(&format!("{base_key}/"))
}

pub fn join_path(parent: &str, relative: &str) -> String {
    if let Some(remote) = crate::remote::parse_remote_path(parent) {
        return crate::remote::remote_path(
            &remote.environment_id,
            &join_path(&remote.host_path, relative),
        );
    }
    let mut out = trim_slash(parent);
    let windows = windows_path(parent);
    let parts = relative
        .split(|ch| ch == '/' || (windows && ch == '\\'))
        .filter(|part| !part.is_empty() && *part != ".");
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

/// Path relative to `cwd` when it lives under the project, otherwise unchanged.
pub fn display_path(path: &str, cwd: Option<&str>) -> String {
    let normalized = trim_slash(path);
    if let Some(base) = cwd.map(trim_slash).filter(|base| base != "~") {
        let key = path_key(&normalized);
        let base_key = path_key(&base);
        if key == base_key {
            return normalized
                .split('/')
                .rfind(|part| !part.is_empty())
                .map_or_else(|| normalized.clone(), str::to_string);
        }
        if key.starts_with(&format!("{base_key}/")) {
            return skip_chars(&normalized, base.chars().count() + 1).to_string();
        }
    }
    normalized
}

/// Folder to create into, given the explorer selection. `is_dir_at` reports
/// whether a path is known to be a folder.
pub fn create_parent_of(
    cwd: &str,
    at_path: Option<&str>,
    is_dir_at: impl Fn(&str) -> bool,
) -> String {
    match at_path {
        None => cwd.to_string(),
        Some(selected) if selected == cwd => cwd.to_string(),
        Some(selected) if is_dir_at(selected) => selected.to_string(),
        Some(selected) => parent_path(selected),
    }
}

/// Directories whose children change when creating `name` under `parent`.
pub fn dirs_touched_by_create(parent: &str, name: &str) -> Vec<String> {
    let segments = path_segments(name);
    let mut out = vec![parent.to_string()];
    let mut cur = parent.to_string();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        cur = join_path(&cur, segment);
        out.push(cur.clone());
    }
    out
}

/// Directories whose children change when moving `from` to `to`.
pub fn dirs_touched_by_move(from: &str, to: &str) -> Vec<String> {
    let from_parent = parent_path(from);
    let to_parent = parent_path(to);
    if from_parent == to_parent {
        vec![from_parent]
    } else {
        vec![from_parent, to_parent]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_backslashes_in_absolute_unix_filenames() {
        assert_eq!(slash("/tmp/a\\b.txt"), "/tmp/a\\b.txt");
        assert_eq!(join_path("/tmp", "a\\b.txt"), "/tmp/a\\b.txt");
        assert_eq!(parent_path("/tmp/a\\b.txt"), "/tmp");
    }

    #[test]
    fn normalizes_windows_separators() {
        assert_eq!(slash("C:\\Users\\me\\code"), "C:/Users/me/code");
    }

    #[test]
    fn preserves_windows_filesystem_roots() {
        assert_eq!(parent_path("//server/share"), "//server/share");
        assert_eq!(rebase_path("C:/old", "C:/old", "D:/"), "D:/");
    }

    #[test]
    fn walks_windows_drive_paths() {
        assert_eq!(parent_path("C:/Users/me/code"), "C:/Users/me");
        assert_eq!(parent_path("C:/Users"), "C:/");
        assert_eq!(parent_path("C:/"), "C:/");
        assert_eq!(join_path("C:/Users/me", "code/app"), "C:/Users/me/code/app");
        assert_eq!(join_path("C:/Users/me/code", ".."), "C:/Users/me");
        assert_eq!(join_path("C:/", "Users"), "C:/Users");
    }

    #[test]
    fn walks_unix_paths() {
        assert_eq!(parent_path("/a/b/"), "/a");
        assert_eq!(parent_path("/a"), "/");
        assert_eq!(parent_path("/"), "/");
        assert_eq!(join_path("/", "a/./b"), "/a/b");
        assert_eq!(join_path("/a/b", "../c"), "/a/c");
    }

    #[test]
    fn treats_backslash_and_slash_as_the_same_path() {
        assert!(is_equal_or_inside(
            "C:\\Users\\me\\app\\src",
            "C:/Users/me/app"
        ));
        assert!(is_equal_or_inside("C:/Users/me", "C:/"));
        assert_eq!(
            rebase_path("C:\\Users\\me\\app\\src\\a.ts", "C:/Users/me/app", "D:/x"),
            "D:/x/src/a.ts"
        );
        assert_eq!(
            display_path("C:\\Users\\me\\app\\src\\a.ts", Some("C:/Users/me/app")),
            "src/a.ts"
        );
    }

    #[test]
    fn compares_windows_paths_without_case() {
        assert!(is_equal_or_inside("c:/USERS/me/App/src", "C:/Users/ME/app"));
        assert_eq!(
            rebase_path("c:/USERS/me/App/src/a.ts", "C:/Users/ME/app", "D:/x"),
            "D:/x/src/a.ts"
        );
        assert_eq!(
            display_path("c:/USERS/me/App/src/a.ts", Some("C:/Users/ME/app")),
            "src/a.ts"
        );
    }

    #[test]
    fn rebases_and_displays_unix_paths() {
        assert_eq!(
            rebase_path("/r/old/a.ts", "/r/old", "/r/new"),
            "/r/new/a.ts"
        );
        assert_eq!(rebase_path("/r/old", "/r/old/", "/r/new"), "/r/new");
        assert_eq!(rebase_path("/r/older/a", "/r/old", "/r/new"), "/r/older/a");
        assert_eq!(display_path("/repo/src/a.ts", Some("/repo")), "src/a.ts");
        assert_eq!(display_path("/repo/", Some("/repo")), "repo");
        assert_eq!(display_path("/other/a.ts", Some("/repo")), "/other/a.ts");
        assert_eq!(display_path("/repo/a.ts", Some("~")), "/repo/a.ts");
        assert_eq!(display_path("/repo/a.ts", None), "/repo/a.ts");
    }

    #[test]
    fn remote_paths_preserve_machine_roots_and_windows_semantics() {
        assert_eq!(parent_path("remote://linux/"), "remote://linux/");
        assert_eq!(
            join_path("remote://linux/", "../../file"),
            "remote://linux/file"
        );
        assert_eq!(
            parent_path("remote://windows/C:/code"),
            "remote://windows/C:/"
        );
        assert_eq!(
            join_path("remote://windows/C:/code", "src\\main.rs"),
            "remote://windows/C:/code/src/main.rs"
        );
        assert!(is_equal_or_inside(
            "remote://windows/c:/CODE/src",
            "remote://windows/C:/code"
        ));
        assert!(!is_equal_or_inside(
            "remote://other/c:/code/src",
            "remote://windows/C:/code"
        ));
    }

    #[test]
    fn create_parent_follows_the_selection() {
        let dirs = |path: &str| path == "/repo/src";
        assert_eq!(create_parent_of("/repo", None, dirs), "/repo");
        assert_eq!(create_parent_of("/repo", Some("/repo"), dirs), "/repo");
        assert_eq!(
            create_parent_of("/repo", Some("/repo/src"), dirs),
            "/repo/src"
        );
        assert_eq!(
            create_parent_of("/repo", Some("/repo/src/a.ts"), dirs),
            "/repo/src"
        );
    }

    #[test]
    fn touched_dirs() {
        assert_eq!(dirs_touched_by_create("/repo", "a.ts"), ["/repo"]);
        assert_eq!(
            dirs_touched_by_create("/repo", "src/lib/a.ts"),
            ["/repo", "/repo/src", "/repo/src/lib"]
        );
        assert_eq!(dirs_touched_by_create("/repo", "src/"), ["/repo"]);
        assert_eq!(dirs_touched_by_move("/r/a/x", "/r/a/y"), ["/r/a"]);
        assert_eq!(dirs_touched_by_move("/r/a/x", "/r/b/x"), ["/r/a", "/r/b"]);
    }
}
