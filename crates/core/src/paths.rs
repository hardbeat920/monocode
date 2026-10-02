//! Port of the pure path helpers in src/shared/lib/paths.ts and `basename` in
//! src/platform/tauri/fs.ts. `IS_WIN` becomes `cfg!(windows)`.

/// `windowsPath`: a drive path or a UNC path.
fn windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || path.starts_with("\\\\")
        || path.starts_with("//")
}

/// `slash`: forward slashes for Windows paths.
pub fn slash(path: &str) -> String {
    if windows_path(path) || (cfg!(windows) && !path.starts_with('/')) {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

fn trim_slash(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

fn is_drive_root_or_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/')
}

/// `pathKey`: stable comparison key for Windows paths without changing their
/// display case.
pub fn path_key(path: &str) -> String {
    let normalized = trim_slash(path);
    if is_drive_root_or_path(&normalized) || normalized.starts_with("//") {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

/// `basename` from src/platform/tauri/fs.ts.
pub fn basename(path: &str) -> String {
    let trimmed = trim_slash(path);
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return trimmed;
    }
    trimmed
        .split('/')
        .rfind(|part| !part.is_empty())
        .map(str::to_string)
        .unwrap_or(trimmed)
}

/// `displayPath`: `path` relative to `cwd` when it is inside it, the folder
/// name when it is `cwd` itself, and otherwise the whole path.
pub fn display_path(path: &str, cwd: Option<&str>) -> String {
    let normalized = trim_slash(path);
    let base = cwd.filter(|cwd| !cwd.is_empty()).map(trim_slash);
    if let Some(base) = base.filter(|base| base != "~") {
        let key = path_key(&normalized);
        let base_key = path_key(&base);
        if key == base_key {
            return normalized
                .split('/')
                .rfind(|part| !part.is_empty())
                .map(str::to_string)
                .unwrap_or(normalized);
        }
        if key.starts_with(&format!("{base_key}/")) {
            // JavaScript slices by the UTF-16 length of `${base}/`.
            let prefix = crate::js::slice_prefix(&normalized, crate::js::len(&base) + 1);
            return normalized[prefix.len()..].to_string();
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_paths_are_relative_to_the_project() {
        assert_eq!(display_path("/repo/src/a.ts", Some("/repo")), "src/a.ts");
        assert_eq!(display_path("/repo/", Some("/repo")), "repo");
        assert_eq!(display_path("/other/a.ts", Some("/repo")), "/other/a.ts");
        assert_eq!(display_path("/repo/a.ts", Some("~")), "/repo/a.ts");
        assert_eq!(display_path("C:\\Repo\\a.ts", Some("c:/repo")), "a.ts");
        assert_eq!(display_path("/repo/a.ts", None), "/repo/a.ts");
    }

    #[test]
    fn basename_matches_the_fs_helper() {
        assert_eq!(basename("/a/b/c.txt"), "c.txt");
        assert_eq!(basename("/a/b/"), "b");
        assert_eq!(basename("/"), "/");
        assert_eq!(basename("C:\\Users\\me\\x.png"), "x.png");
        assert_eq!(basename("C:"), "C:");
    }

    #[test]
    fn path_keys_fold_windows_case_only() {
        assert_eq!(path_key("/Repo/A/"), "/Repo/A");
        assert_eq!(path_key("C:\\Repo\\A"), "c:/repo/a");
        assert_eq!(path_key("//Server/Share"), "//server/share");
        assert_eq!(path_key("///"), "/");
    }
}
