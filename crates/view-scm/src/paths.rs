//! Port of the path helpers the source control views use from
//! src/shared/lib/paths.ts (`prettyCwd`, `projectName`, `isEqualOrInside`)
//! and src/features/projects/model/recents.ts (`isRemoteProjectPath`).
//! `pathKey` and `slash` live in monocode-core.

pub use monocode_core::paths::{path_key, slash};

/// `REMOTE_PATH_PREFIX`.
pub const REMOTE_PATH_PREFIX: &str = "remote://";

/// `MOD`: the command key label.
pub const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};

fn trim_slash(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_string()
    }
}

fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `prettyCwd`: a home folder path as `~/...`.
pub fn pretty_cwd(cwd: &str) -> String {
    let trimmed = trim_slash(cwd);
    if trimmed == "~" {
        return "~".into();
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() >= 2 && (parts[0] == "Users" || parts[0] == "home") {
        let rest = parts[2..].join("/");
        return if rest.is_empty() {
            "~".into()
        } else {
            format!("~/{rest}")
        };
    }
    if parts.len() >= 3 && is_drive(parts[0]) && parts[1] == "Users" {
        let rest = parts[3..].join("/");
        return if rest.is_empty() {
            "~".into()
        } else {
            format!("~/{rest}")
        };
    }
    trimmed
}

/// `projectName`: the folder name.
pub fn project_name(cwd: &str) -> String {
    if cwd.is_empty() || pretty_cwd(cwd) == "~" {
        return "~".into();
    }
    let trimmed = trim_slash(cwd);
    if is_drive(&trimmed) {
        return trimmed;
    }
    trimmed
        .split('/')
        .rfind(|part| !part.is_empty())
        .map(str::to_string)
        .unwrap_or(trimmed)
}

/// `isEqualOrInside`.
pub fn is_equal_or_inside(path: &str, root: &str) -> bool {
    let key = path_key(&trim_slash(path));
    let base_key = path_key(&trim_slash(root));
    key == base_key || key.starts_with(&format!("{base_key}/"))
}

/// `isRemoteProjectPath`: a project on a connected machine.
pub fn is_remote_project_path(path: &str) -> bool {
    slash(path).starts_with(REMOTE_PATH_PREFIX)
}

/// `basename` from src/platform/tauri/fs.ts.
pub fn basename(path: &str) -> String {
    monocode_core::paths::basename(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_cwd_shortens_home_folders() {
        assert_eq!(pretty_cwd("/Users/me/code/app/"), "~/code/app");
        assert_eq!(pretty_cwd("/home/me"), "~");
        assert_eq!(pretty_cwd("/opt/repo"), "/opt/repo");
        assert_eq!(pretty_cwd("C:\\Users\\me\\repo"), "~/repo");
    }

    #[test]
    fn project_names_and_containment() {
        assert_eq!(project_name("/repo-worktrees/feature"), "feature");
        assert_eq!(project_name("/Users/me"), "~");
        assert!(is_equal_or_inside("/repo/a", "/repo"));
        assert!(is_equal_or_inside("/repo/", "/repo"));
        assert!(!is_equal_or_inside("/repository", "/repo"));
        assert!(is_remote_project_path("remote://machine/home/user/repo"));
    }
}
