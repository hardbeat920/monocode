//! The path helpers the composer reads: `prettyCwd` and `projectName` from
//! src/shared/lib/paths.ts and `looksLikeProject` from
//! src/features/projects/model/recents.ts. Copied from monocode-layout and
//! monocode-engine, which view crates may not depend on yet.

use monocode_core::paths::slash;

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

/// `prettyCwd`: a home-relative display form of a working directory.
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

/// `projectName`: the folder name, `~` when the cwd is home.
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
    if pretty_cwd(path) == "~" {
        return false;
    }
    !(path.contains(".app/") || path.contains(".app\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_home_paths() {
        assert_eq!(pretty_cwd("/Users/me/code/app/"), "~/code/app");
        assert_eq!(pretty_cwd("/home/me"), "~");
        assert_eq!(pretty_cwd("C:\\Users\\me\\src"), "~/src");
        assert_eq!(pretty_cwd("/srv/app"), "/srv/app");
        assert_eq!(project_name("/Users/me/code/app"), "app");
        assert_eq!(project_name("/Users/me"), "~");
    }

    #[test]
    fn only_indexes_real_projects() {
        assert!(looks_like_project("/Users/me/code/app"));
        assert!(!looks_like_project("~"));
        assert!(!looks_like_project("/"));
        assert!(!looks_like_project("/Users/me"));
        assert!(!looks_like_project("C:"));
        assert!(!looks_like_project("/Applications/Foo.app/Contents"));
    }
}
