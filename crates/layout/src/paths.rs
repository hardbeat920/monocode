//! Path helpers the layout model calls that `monocode_core::paths` does not
//! have yet. Ports of `prettyCwd`, `projectName`, and `projectKey` in
//! src/shared/lib/paths.ts, `normalizeProjectPath`, `sameProjectPath`, and
//! `isRemoteProjectPath` in src/features/projects/model/recents.ts, and
//! `remotePath` and `parseRemotePath` in
//! src/features/connections/model/remoteProjects.ts.

use monocode_core::paths::{path_key, slash};

/// `REMOTE_PROJECT_PREFIX`: paths on a connected machine use this scheme.
pub const REMOTE_PROJECT_PREFIX: &str = "remote://";

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

/// `projectName`: folder name for tab labels, `~` when the cwd is home.
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

/// `projectKey`: identity for a project's saved appearance and data.
pub fn project_key(cwd: &str) -> String {
    path_key(cwd)
}

/// `normalizeProjectPath`.
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

/// `slashed` in remoteProjects.ts: every backslash becomes a slash.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// `remotePath`: how this app addresses a path on another machine.
pub fn remote_path(environment_id: &str, host_path: &str) -> String {
    let path = slashed(host_path);
    // Keep the second leading slash of a Windows UNC path.
    let rest = if path.starts_with("//") {
        &path[1..]
    } else {
        path.trim_start_matches('/')
    };
    format!("{REMOTE_PROJECT_PREFIX}{environment_id}/{rest}")
}

/// The machine and host path behind a `remote://` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePathParts {
    pub environment_id: String,
    pub host_path: String,
}

/// `parseRemotePath`.
pub fn parse_remote_path(path: &str) -> Option<RemotePathParts> {
    if !is_remote_project_path(path) {
        return None;
    }
    let all = slashed(path);
    let rest = all.get(REMOTE_PROJECT_PREFIX.len()..)?;
    let slash_at = rest.find('/')?;
    if slash_at == 0 {
        return None;
    }
    let host_path = &rest[slash_at + 1..];
    // Windows hosts keep their drive letter and UNC prefix; POSIX paths regain their root.
    let bytes = host_path.as_bytes();
    let windows = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/');
    Some(RemotePathParts {
        environment_id: rest[..slash_at].to_string(),
        host_path: if windows {
            host_path.to_string()
        } else {
            format!("/{host_path}")
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names_come_from_the_folder() {
        assert_eq!(project_name("/Users/me/agent-terminal"), "agent-terminal");
        assert_eq!(project_name("/Users/me"), "~");
        assert_eq!(project_name("~"), "~");
        assert_eq!(project_name(""), "~");
        assert_eq!(project_name("/tmp/beta/"), "beta");
    }

    #[test]
    fn pretty_cwd_folds_home() {
        assert_eq!(pretty_cwd("/Users/me/repo"), "~/repo");
        assert_eq!(pretty_cwd("/home/me"), "~");
        assert_eq!(pretty_cwd("C:/Users/me/repo"), "~/repo");
        assert_eq!(pretty_cwd("/opt/repo/"), "/opt/repo");
    }

    #[test]
    fn remote_paths_round_trip() {
        assert_eq!(remote_path("env", "/repo/a.ts"), "remote://env/repo/a.ts");
        assert_eq!(
            remote_path("env", "//server/share"),
            "remote://env//server/share"
        );
        assert_eq!(
            parse_remote_path("remote://env/repo"),
            Some(RemotePathParts {
                environment_id: "env".into(),
                host_path: "/repo".into(),
            })
        );
        assert_eq!(
            parse_remote_path("remote://env/C:/repo").map(|p| p.host_path),
            Some("C:/repo".into())
        );
        assert_eq!(parse_remote_path("remote:///repo"), None);
        assert_eq!(parse_remote_path("/repo"), None);
    }

    #[test]
    fn normalizes_project_paths() {
        assert_eq!(normalize_project_path("/tmp/a///"), "/tmp/a");
        assert_eq!(normalize_project_path("/"), "/");
        assert!(same_project_path("/tmp/beta/", "/tmp/beta"));
    }
}
