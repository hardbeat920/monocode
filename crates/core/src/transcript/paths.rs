//! Port of `resolveWorkspacePath`, `looksLikeFilePath`, `joinPath`, and
//! `parentPath` in src/shared/lib/paths.ts, and `leafName` in
//! src/features/files/model/fileName.ts.
//!
//! `crate::paths` has the display helpers. These are the ones the
//! transcript needs to turn a tool row's label into a file a click opens.

use std::sync::RwLock;

use crate::js;
use crate::paths::slash;

/// `cachedHomeDir`: the real OS home directory, primed once at startup.
static HOME_DIR: RwLock<Option<String>> = RwLock::new(None);

/// `setHomeDir`: record the OS home directory so `~/` references resolve
/// exactly instead of being inferred from `cwd`. `None` clears it.
pub fn set_home_dir(path: Option<&str>) {
    let value = path
        .filter(|path| !path.is_empty())
        .map(|path| trim_slash(&slash(path)));
    if let Ok(mut home) = HOME_DIR.write() {
        *home = value;
    }
}

fn cached_home_dir() -> Option<String> {
    HOME_DIR.read().ok().and_then(|home| home.clone())
}

/// `trimSlash`: forward slashes, no trailing slash, `/` for the root.
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
fn is_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `/^[A-Za-z]:\//`.
fn starts_with_drive_slash(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// `windowsPath`.
fn windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || path.starts_with("\\\\")
        || path.starts_with("//")
}

/// `parentPath`.
pub fn parent_path(path: &str) -> String {
    let trimmed = trim_slash(path);
    // `/^\/\/[^/]+\/[^/]+$/`: a UNC share root has no parent.
    if let Some(rest) = trimmed.strip_prefix("//") {
        let mut parts = rest.split('/');
        if let (Some(server), Some(share), None) = (parts.next(), parts.next(), parts.next())
            && !server.is_empty()
            && !share.is_empty()
        {
            return trimmed;
        }
    }
    if is_drive(&trimmed) {
        return format!("{trimmed}/");
    }
    match trimmed.rfind('/') {
        None | Some(0) => "/".into(),
        Some(i) => {
            let parent = &trimmed[..i];
            if is_drive(parent) {
                format!("{parent}/")
            } else {
                parent.to_string()
            }
        }
    }
}

/// `joinPath`.
pub fn join_path(parent: &str, relative: &str) -> String {
    let base = trim_slash(parent);
    let windows = windows_path(parent);
    let parts = relative
        .split(|c: char| c == '/' || (windows && c == '\\'))
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

/// `/^remote:\/\/[^/]+\//.exec(cwd)?.[0]`.
fn remote_root(cwd: &str) -> Option<&str> {
    let rest = cwd.strip_prefix("remote://")?;
    let slash = rest.find('/')?;
    if slash == 0 {
        return None;
    }
    Some(&cwd[.."remote://".len() + slash + 1])
}

/// `homeDirFromCwd`.
fn home_dir_from_cwd(cwd: &str) -> Option<String> {
    let remote = remote_root(cwd);
    let trimmed = match remote {
        Some(root) => trim_slash(&format!("/{}", &cwd[root.len()..])),
        None => trim_slash(cwd),
    };
    if trimmed == "~" {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() >= 2 && (parts[0] == "Users" || parts[0] == "home") {
        let home = format!("/{}/{}", parts[0], parts[1]);
        return Some(match remote {
            Some(root) => format!("{root}{}", &home[1..]),
            None => home,
        });
    }
    if parts.len() >= 3 && is_drive(parts[0]) && parts[1].to_lowercase() == "users" {
        let home = format!("{}/{}/{}", parts[0], parts[1], parts[2]);
        return Some(match remote {
            Some(root) => format!("{root}{home}"),
            None => home,
        });
    }
    None
}

/// `isExtensionlessFileName`.
pub fn is_extensionless_file_name(value: &str) -> bool {
    ["dockerfile", "makefile", "gemfile", "license"]
        .iter()
        .any(|name| value.eq_ignore_ascii_case(name))
}

/// `/\.[A-Za-z][A-Za-z0-9+]{0,11}$/`.
fn has_extension(value: &str) -> bool {
    let Some(dot) = value.rfind('.') else {
        return false;
    };
    let ext = &value[dot + 1..];
    let bytes = ext.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 12
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'+')
}

/// `looksLikeFilePath`.
pub fn looks_like_file_path(value: &str) -> bool {
    if value.starts_with('/') || starts_with_drive_slash(value) {
        return true;
    }
    if value.contains('/') {
        return true;
    }
    is_extensionless_file_name(value) || has_extension(value)
}

/// The `:line[:column]` or `#Lline[-Lend]` suffix the location regex strips,
/// `/(?::(\d+)(?::(\d+))?|#L(\d+)(?:-L\d+)?)$/`. Returns the value without it.
fn strip_location(value: &str) -> &str {
    let digits_end =
        |s: &str| -> usize { s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len() };
    // `#L12` or `#L12-L20`.
    if let Some(hash) = value.rfind("#L") {
        let tail = &value[hash + 2..];
        let (first, rest) =
            tail.split_at(tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len());
        if !first.is_empty() {
            if rest.is_empty() {
                return &value[..hash];
            }
            if let Some(end) = rest.strip_prefix("-L")
                && !end.is_empty()
                && end.bytes().all(|b| b.is_ascii_digit())
            {
                return &value[..hash];
            }
        }
    }
    // `:12` or `:12:4`.
    let n = digits_end(value);
    if n == 0 {
        return value;
    }
    let before = &value[..value.len() - n];
    let Some(before) = before.strip_suffix(':') else {
        return value;
    };
    let m = digits_end(before);
    if m > 0 {
        let head = &before[..before.len() - m];
        if let Some(head) = head.strip_suffix(':') {
            return head;
        }
    }
    before
}

/// `/^[a-z][a-z0-9+.-]*:/i`: a URL scheme.
fn has_scheme(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for &b in &bytes[1..] {
        if b == b':' {
            return true;
        }
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-') {
            return false;
        }
    }
    false
}

/// `resolveWorkspacePath`: the absolute path for a workspace file reference,
/// local or on a connected machine. `None` when it is not a file path.
pub fn resolve_workspace_path(href: &str, cwd: Option<&str>) -> Option<String> {
    let mut value = js::trim(href).to_string();
    if value.is_empty() {
        return None;
    }
    value = strip_location(&value).to_string();

    let file_url = value.starts_with("file://");
    if file_url {
        value = value["file://".len()..].to_string();
        if value.starts_with("localhost/") {
            value = value["localhost".len()..].to_string();
        }
        value = percent_decode(&value).unwrap_or(value);
    }

    value = slash(&value);
    let remote = cwd.and_then(remote_root);
    if let Some(root) = remote
        && value.starts_with(root)
    {
        return Some(value);
    }
    if file_url {
        let bytes = value.as_bytes();
        if bytes.len() >= 2 && matches!(bytes[0], b'/' | b'\\') && matches!(bytes[1], b'/' | b'\\')
        {
            return None;
        }
    }
    if value == "~" || value.starts_with("~/") {
        let home = match remote {
            Some(_) => cwd.and_then(home_dir_from_cwd),
            None => cached_home_dir().or_else(|| cwd.and_then(home_dir_from_cwd)),
        }?;
        value = if value == "~" {
            home
        } else {
            join_path(&home, &value[2..])
        };
    }
    if let Some(root) = remote
        && value.starts_with(root)
    {
        return Some(value);
    }
    if has_scheme(&value) && !starts_with_drive_slash(&value) {
        return None;
    }
    if value.is_empty()
        || value == "."
        || value.starts_with('#')
        || value.starts_with('?')
        || value.contains("://")
    {
        return None;
    }
    if !looks_like_file_path(&value) {
        return None;
    }
    if starts_with_drive_slash(&value) {
        return Some(match remote {
            Some(root) => format!("{root}{value}"),
            None => value,
        });
    }
    if let Some(root) = remote
        && value.starts_with("//")
    {
        return Some(format!("{root}{}", &value[1..]));
    }
    if let Some(rest) = value.strip_prefix('/') {
        return Some(match remote {
            Some(root) => format!("{root}{}", value.trim_start_matches('/')),
            None if starts_with_drive_slash(rest) => rest.to_string(),
            None => value,
        });
    }
    let cwd = cwd.filter(|cwd| !cwd.is_empty() && *cwd != "~")?;
    Some(join_path(cwd, &value))
}

/// `decodeURIComponent`, `None` on a malformed escape.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let text = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(text, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `wellFormedFileName`: no leading or trailing tabs, no trailing slashes.
fn well_formed_file_name(name: &str) -> &str {
    name.trim_matches('\t').trim_end_matches(['/', '\\'])
}

/// `leafName`: the last path segment.
pub fn leaf_name(raw: &str) -> String {
    well_formed_file_name(raw)
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_against_the_project() {
        assert_eq!(
            resolve_workspace_path("src/App.tsx", Some("/repo")).as_deref(),
            Some("/repo/src/App.tsx")
        );
        assert_eq!(
            resolve_workspace_path("src/main.ts:12", Some("/repo")).as_deref(),
            Some("/repo/src/main.ts")
        );
        assert_eq!(
            resolve_workspace_path("src/main.ts:12:4", Some("/repo")).as_deref(),
            Some("/repo/src/main.ts")
        );
        assert_eq!(
            resolve_workspace_path("src/main.ts#L3-L9", Some("/repo")).as_deref(),
            Some("/repo/src/main.ts")
        );
        assert_eq!(
            resolve_workspace_path("../other/a.ts", Some("/repo/app")).as_deref(),
            Some("/repo/other/a.ts")
        );
        assert_eq!(resolve_workspace_path("App.tsx", None), None);
        assert_eq!(resolve_workspace_path("App.tsx", Some("~")), None);
    }

    #[test]
    fn rejects_prose_and_urls() {
        assert_eq!(
            resolve_workspace_path("dependency versions", Some("/repo")),
            None
        );
        assert_eq!(
            resolve_workspace_path("https://example.com/a.ts", Some("/repo")),
            None
        );
        assert_eq!(resolve_workspace_path("#heading", Some("/repo")), None);
        assert_eq!(resolve_workspace_path("  ", Some("/repo")), None);
    }

    #[test]
    fn keeps_absolute_and_windows_paths() {
        assert_eq!(
            resolve_workspace_path(
                "/Users/dev/.codex/skills/zuse/SKILL.md",
                Some("/Users/dev/project")
            )
            .as_deref(),
            Some("/Users/dev/.codex/skills/zuse/SKILL.md")
        );
        assert_eq!(
            resolve_workspace_path("C:\\repo\\docker\\Dockerfile", Some("C:/repo")).as_deref(),
            Some("C:/repo/docker/Dockerfile")
        );
        assert_eq!(
            resolve_workspace_path("/C:/repo/a.ts", None).as_deref(),
            Some("C:/repo/a.ts")
        );
        assert_eq!(
            resolve_workspace_path("file:///tmp/a%20b.txt", None).as_deref(),
            Some("/tmp/a b.txt")
        );
    }

    #[test]
    fn expands_home_from_the_project_path() {
        assert_eq!(
            resolve_workspace_path("~/notes/a.md", Some("/Users/me/repo")).as_deref(),
            Some("/Users/me/notes/a.md")
        );
        assert_eq!(
            resolve_workspace_path("~/notes/a.md", Some("/opt/repo")),
            None
        );
        assert_eq!(
            resolve_workspace_path("src/a.ts", Some("remote://box/home/me/repo")).as_deref(),
            Some("remote://box/home/me/repo/src/a.ts")
        );
        assert_eq!(
            resolve_workspace_path("/etc/hosts", Some("remote://box/home/me/repo")).as_deref(),
            Some("remote://box/etc/hosts")
        );
    }

    #[test]
    fn leaf_names_skip_trailing_separators() {
        assert_eq!(leaf_name("src/lib/a.ts"), "a.ts");
        assert_eq!(leaf_name("src/lib/"), "lib");
        assert_eq!(leaf_name("C:\\a\\b.txt"), "b.txt");
        assert_eq!(leaf_name(""), "");
    }
}
